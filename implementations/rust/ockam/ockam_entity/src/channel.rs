use crate::{Identity, ProfileIdentifier};
use ockam_core::async_trait;
use ockam_core::compat::boxed::Box;
use ockam_core::{Address, Decodable, Message, Result, Route, Routed};
use ockam_node::Context;
use ockam_vault_sync_core::VaultSync;

mod secure_channel_worker;
pub(crate) use secure_channel_worker::*;
mod listener;
pub(crate) use listener::*;
mod messages;
pub(crate) use messages::*;
mod trust_policy;
pub use trust_policy::*;
mod local_info;
pub use local_info::*;

#[async_trait]
pub trait SecureChannelTrait {
    async fn create_secure_channel_async(
        self,
        ctx: &Context,
        route: Route,
        trust_policy: impl TrustPolicy,
        vault: &Address,
    ) -> Result<Address>;

    async fn create_secure_channel_listener_async(
        self,
        ctx: &Context,
        address: Address,
        trust_policy: impl TrustPolicy,
        vault: &Address,
    ) -> Result<()>;
}

#[async_trait]
impl<P: Identity> SecureChannelTrait for P {
    /// Create mutually authenticated secure channel
    async fn create_secure_channel_async(
        self,
        ctx: &Context,
        route: Route,
        trust_policy: impl TrustPolicy,
        vault: &Address,
    ) -> Result<Address> {
        let vault = VaultSync::create_with_worker(ctx, vault).await?;
        SecureChannelWorker::create_initiator(ctx, route, self, trust_policy, vault).await
    }

    /// Create mutually authenticated secure channel listener
    async fn create_secure_channel_listener_async(
        self,
        ctx: &Context,
        address: Address,
        trust_policy: impl TrustPolicy,
        vault: &Address,
    ) -> Result<()> {
        let vault = VaultSync::create_with_worker(ctx, vault).await?;
        let listener = ProfileChannelListener::new(trust_policy, self, vault);
        ctx.start_worker(address, listener).await
    }
}

// TODO: rename
pub fn check_message_origin<T: Message>(
    msg: &Routed<T>,
    their_profile_id: &ProfileIdentifier,
) -> Result<bool> {
    let local_msg = msg.local_message();
    let local_info = LocalInfo::decode(local_msg.local_info())?;

    let res = local_info.their_profile_id() == their_profile_id;

    Ok(res)
}

// TODO: rename
pub fn get_secure_channel_participant_id<T: Message>(msg: &Routed<T>) -> Result<ProfileIdentifier> {
    let local_msg = msg.local_message();
    let local_info = LocalInfo::decode(local_msg.local_info())?;

    let res = local_info.their_profile_id().clone();

    Ok(res)
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{Entity, SecureChannels};
    use ockam_core::route;
    use ockam_vault_sync_core::Vault;

    #[test]
    fn test_channel() {
        let (mut ctx, mut executor) = ockam_node::start_node();
        executor
            .execute(async move {
                let alice_vault = Vault::create(&ctx).await.expect("failed to create vault");
                let bob_vault = Vault::create(&ctx).await.expect("failed to create vault");

                let mut alice = Entity::create(&ctx, &alice_vault).await.unwrap();
                let mut bob = Entity::create(&ctx, &bob_vault).await.unwrap();

                let alice_trust_policy =
                    TrustIdentifierPolicy::new(bob.identifier().await.unwrap());
                let bob_trust_policy =
                    TrustIdentifierPolicy::new(alice.identifier().await.unwrap());

                bob.create_secure_channel_listener("bob_listener".into(), bob_trust_policy)
                    .await
                    .unwrap();

                let alice_channel = alice
                    .create_secure_channel(route!["bob_listener"], alice_trust_policy)
                    .await
                    .unwrap();

                ctx.send(
                    route![alice_channel, ctx.address()],
                    "Hello, Bob!".to_string(),
                )
                .await
                .unwrap();
                let msg = ctx.receive::<String>().await.unwrap().take();

                let local_info = LocalInfo::decode(msg.local_message().local_info()).unwrap();
                assert_eq!(
                    local_info.their_profile_id(),
                    &alice.identifier().await.unwrap()
                );

                let return_route = msg.return_route();
                assert_eq!("Hello, Bob!", msg.body());

                ctx.send(return_route, "Hello, Alice!".to_string())
                    .await
                    .unwrap();

                let msg = ctx.receive::<String>().await.unwrap().take();

                let local_info = msg.local_message().local_info();

                let local_info = LocalInfo::decode(local_info).unwrap();
                assert_eq!(
                    local_info.their_profile_id(),
                    &bob.identifier().await.unwrap()
                );

                assert_eq!("Hello, Alice!", msg.body());

                ctx.stop().await.unwrap();
            })
            .unwrap();
    }

    #[test]
    fn test_tunneled_secure_channel_works() {
        let (mut ctx, mut executor) = ockam_node::start_node();
        executor
            .execute(async move {
                let vault = Vault::create(&ctx).await.unwrap();

                let mut alice = Entity::create(&ctx, &vault).await.unwrap();
                let mut bob = Entity::create(&ctx, &vault).await.unwrap();

                let alice_trust_policy =
                    TrustIdentifierPolicy::new(bob.identifier().await.unwrap());
                let bob_trust_policy =
                    TrustIdentifierPolicy::new(alice.identifier().await.unwrap());

                bob.create_secure_channel_listener("bob_listener".into(), bob_trust_policy.clone())
                    .await
                    .unwrap();

                let alice_channel = alice
                    .create_secure_channel(route!["bob_listener"], alice_trust_policy.clone())
                    .await
                    .unwrap();

                bob.create_secure_channel_listener("bob_another_listener".into(), bob_trust_policy)
                    .await
                    .unwrap();

                let alice_another_channel = alice
                    .create_secure_channel(
                        route![alice_channel, "bob_another_listener"],
                        alice_trust_policy,
                    )
                    .await
                    .unwrap();

                ctx.send(
                    route![alice_another_channel, ctx.address()],
                    "Hello, Bob!".to_string(),
                )
                .await
                .unwrap();
                let msg = ctx.receive::<String>().await.unwrap().take();
                let return_route = msg.return_route();
                assert_eq!("Hello, Bob!", msg.body());

                ctx.send(return_route, "Hello, Alice!".to_string())
                    .await
                    .unwrap();
                assert_eq!(
                    "Hello, Alice!",
                    ctx.receive::<String>().await.unwrap().take().body()
                );

                ctx.stop().await.unwrap();
            })
            .unwrap();
    }

    #[test]
    fn test_double_tunneled_secure_channel_works() {
        let (mut ctx, mut executor) = ockam_node::start_node();
        executor
            .execute(async move {
                let vault = Vault::create(&ctx).await.unwrap();

                let mut alice = Entity::create(&ctx, &vault).await.unwrap();
                let mut bob = Entity::create(&ctx, &vault).await.unwrap();

                let alice_trust_policy =
                    TrustIdentifierPolicy::new(bob.identifier().await.unwrap());
                let bob_trust_policy =
                    TrustIdentifierPolicy::new(alice.identifier().await.unwrap());

                bob.create_secure_channel_listener("bob_listener".into(), bob_trust_policy.clone())
                    .await
                    .unwrap();

                let alice_channel = alice
                    .create_secure_channel(route!["bob_listener"], alice_trust_policy.clone())
                    .await
                    .unwrap();

                bob.create_secure_channel_listener(
                    "bob_another_listener".into(),
                    bob_trust_policy.clone(),
                )
                .await
                .unwrap();

                let alice_another_channel = alice
                    .create_secure_channel(
                        route![alice_channel, "bob_another_listener"],
                        alice_trust_policy.clone(),
                    )
                    .await
                    .unwrap();

                bob.create_secure_channel_listener(
                    "bob_yet_another_listener".into(),
                    bob_trust_policy,
                )
                .await
                .unwrap();

                let alice_yet_another_channel = alice
                    .create_secure_channel(
                        route![alice_another_channel, "bob_yet_another_listener"],
                        alice_trust_policy,
                    )
                    .await
                    .unwrap();

                ctx.send(
                    route![alice_yet_another_channel, ctx.address()],
                    "Hello, Bob!".to_string(),
                )
                .await
                .unwrap();
                let msg = ctx.receive::<String>().await.unwrap().take();
                let return_route = msg.return_route();
                assert_eq!("Hello, Bob!", msg.body());

                ctx.send(return_route, "Hello, Alice!".to_string())
                    .await
                    .unwrap();
                assert_eq!(
                    "Hello, Alice!",
                    ctx.receive::<String>().await.unwrap().take().body()
                );

                ctx.stop().await.unwrap();
            })
            .unwrap();
    }

    #[test]
    fn test_many_times_tunneled_secure_channel_works() {
        let (mut ctx, mut executor) = ockam_node::start_node();
        executor
            .execute(async move {
                let vault = Vault::create(&ctx).await.unwrap();

                let mut alice = Entity::create(&ctx, &vault).await.unwrap();
                let mut bob = Entity::create(&ctx, &vault).await.unwrap();

                let alice_trust_policy =
                    TrustIdentifierPolicy::new(bob.identifier().await.unwrap());
                let bob_trust_policy =
                    TrustIdentifierPolicy::new(alice.identifier().await.unwrap());

                let n = rand::random::<u8>() % 5 + 4;
                let mut channels = vec![];
                for i in 0..n {
                    bob.create_secure_channel_listener(
                        i.to_string().into(),
                        bob_trust_policy.clone(),
                    )
                    .await
                    .unwrap();
                    let channel_route: Route;
                    if i > 0 {
                        channel_route = route![channels.pop().unwrap(), i.to_string()];
                    } else {
                        channel_route = route![i.to_string()];
                    }
                    let alice_channel = alice
                        .create_secure_channel(channel_route, alice_trust_policy.clone())
                        .await
                        .unwrap();
                    channels.push(alice_channel);
                }

                ctx.send(
                    route![channels.pop().unwrap(), ctx.address()],
                    "Hello, Bob!".to_string(),
                )
                .await
                .unwrap();
                let msg = ctx.receive::<String>().await.unwrap().take();
                let return_route = msg.return_route();
                assert_eq!("Hello, Bob!", msg.body());

                ctx.send(return_route, "Hello, Alice!".to_string())
                    .await
                    .unwrap();
                assert_eq!(
                    "Hello, Alice!",
                    ctx.receive::<String>().await.unwrap().take().body()
                );

                ctx.stop().await.unwrap();
            })
            .unwrap();
    }
}
