use crate::{PortalMessage, TcpPortalRecvProcessor};
use async_trait::async_trait;
use ockam_core::{
    Address, Any, LocalMessage, Message, Result, Route, Routed, TransportMessage, Worker,
};
use ockam_node::Context;
use ockam_transport_core::TransportError;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tokio::{io::AsyncWriteExt, net::tcp::OwnedWriteHalf};
use tracing::{debug, warn};

enum State {
    SendPing {
        ping_route: Route,
        receiver: TcpPortalRecvProcessor,
    },
    ReceivePing {
        receiver: TcpPortalRecvProcessor,
    },
    Initialized {
        onward_route: Route,
    },
}

/// A TCP sending message worker
///
/// Create this worker type by calling
/// [`start_tcp_worker`](crate::start_tcp_worker)!
///
/// This half of the worker is created when spawning a new connection
/// worker pair, and listens for messages from the node message system
/// to dispatch to a remote peer.
pub(crate) struct TcpPortalWorker {
    state: Option<State>,
    tx: OwnedWriteHalf,
    peer: SocketAddr,
    internal_address: Address,
    remote_address: Address,
}

impl TcpPortalWorker {
    pub(crate) async fn new_inlet(
        ctx: &Context,
        stream: TcpStream,
        peer: SocketAddr,
        ping_route: Route,
    ) -> Result<()> {
        debug!("Creating new portal worker pair from stream");

        let tx_internal_addr = Address::random(0);
        let tx_remote_addr = Address::random(0);

        // Create two workers based on the split TCP I/O streams
        let (rx, tx) = stream.into_split();
        let receiver = TcpPortalRecvProcessor::new(rx, tx_internal_addr.clone());
        let sender = TcpPortalWorker::new(
            ping_route,
            tx,
            peer,
            tx_internal_addr.clone(),
            tx_remote_addr.clone(),
            receiver,
        );

        // Derive local worker addresses, and start them
        ctx.start_worker(vec![tx_internal_addr, tx_remote_addr], sender)
            .await?;

        // Return a handle to the worker pair
        Ok(())
    }

    pub(crate) async fn new_outlet(
        ctx: &Context,
        peer: SocketAddr,
        ping_route: Route,
    ) -> Result<Address> {
        debug!("Starting worker connection to remote {}", peer);

        // TODO: make i/o errors into ockam_error
        let stream = TcpStream::connect(peer)
            .await
            .map_err(TransportError::from)?;

        debug!("Creating new portal worker pair from stream");

        let tx_internal_addr = Address::random(0);
        let tx_remote_addr = Address::random(0);

        // Create two workers based on the split TCP I/O streams
        let (rx, tx) = stream.into_split();
        let receiver = TcpPortalRecvProcessor::new(rx, tx_internal_addr.clone());
        let sender = TcpPortalWorker::new(
            ping_route,
            tx,
            peer,
            tx_internal_addr.clone(),
            tx_remote_addr.clone(),
            receiver,
        );

        // Derive local worker addresses, and start them
        ctx.start_worker(vec![tx_internal_addr, tx_remote_addr.clone()], sender)
            .await?;

        // Return a handle to the worker pair
        Ok(tx_remote_addr)
    }

    fn new(
        ping_route: Route,
        tx: OwnedWriteHalf,
        peer: SocketAddr,
        internal_address: Address,
        remote_address: Address,
        receiver: TcpPortalRecvProcessor,
    ) -> Self {
        Self {
            state: Some(State::SendPing {
                ping_route,
                receiver,
            }),
            tx,
            peer,
            internal_address,
            remote_address,
        }
    }
}

impl TcpPortalWorker {
    fn take_state(&mut self) -> Result<State> {
        let state;
        if let Some(s) = self.state.take() {
            state = s;
        } else {
            return Err(TransportError::PortalInvalidState.into());
        }

        Ok(state)
    }
}

impl TcpPortalWorker {
    fn prepare_message(&mut self, msg: &Vec<u8>) -> Result<Vec<u8>> {
        let msg = PortalMessage::decode(&msg)?;

        Ok(msg.binary)
    }
}

#[async_trait]
impl Worker for TcpPortalWorker {
    type Context = Context;
    type Message = Any;

    async fn initialize(&mut self, ctx: &mut Self::Context) -> Result<()> {
        let state = self.take_state()?;

        match state {
            State::SendPing {
                ping_route,
                receiver,
            } => {
                let empty_payload: Vec<u8> = vec![];
                // Force creation of Outlet on the other side
                ctx.send_from_address(ping_route, empty_payload, self.remote_address.clone())
                    .await?;

                self.state = Some(State::ReceivePing { receiver });
            }
            _ => return Err(TransportError::PortalInvalidState.into()),
        }

        Ok(())
    }

    // TcpSendWorker will receive messages from the TcpRouter to send
    // across the TcpStream to our friend
    async fn handle_message(&mut self, ctx: &mut Context, msg: Routed<Any>) -> Result<()> {
        // Remove our own address from the route so the other end
        // knows what to do with the incoming message
        let mut onward_route = msg.onward_route();
        let recipient = onward_route.step()?;

        if onward_route.next().is_ok() {
            return Err(TransportError::UnknownRoute.into());
        }

        let state = self.take_state()?;

        match state {
            State::ReceivePing { receiver } => {
                if recipient == self.internal_address {
                    return Err(TransportError::PortalInvalidState.into());
                }

                // TODO: This should be serialized empty vec
                if msg.payload().len() != 1 {
                    return Err(TransportError::Protocol.into());
                }
                // Update route
                let onward_route = msg.return_route();
                ctx.start_processor(Address::random(0), receiver).await?;
                self.state = Some(State::Initialized { onward_route });
            }
            State::Initialized { onward_route } => {
                if recipient == self.internal_address {
                    // Forward message
                    let payload = msg.payload().clone();
                    let msg = TransportMessage::v1(
                        onward_route.clone(),
                        self.remote_address.clone(),
                        payload,
                    );
                    ctx.forward(LocalMessage::new(msg, Vec::new())).await?;
                } else {
                    // Send to Tcp stream
                    // Create a message buffer with pre-pended length
                    let msg = self.prepare_message(msg.payload())?;

                    if self.tx.write(&msg).await.is_err() {
                        warn!("Failed to send message to peer {}", self.peer);
                        ctx.stop_worker(ctx.address()).await?;
                    }
                }

                self.state = Some(State::Initialized {onward_route})
            }
            _ => return Err(TransportError::PortalInvalidState.into()),
        };

        Ok(())
    }
}
