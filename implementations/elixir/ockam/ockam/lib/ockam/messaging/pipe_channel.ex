defmodule Ockam.Messaging.PipeChannel do
  @moduledoc """
  Ockam channel using pipes to deliver messages

  Can be used with different pipe implementations to get different delivery properties

  See `Ockam.Messaging.PipeChannel.Initiator` and `Ockam.Messaging.PipeChannel.Responder` for usage
  """

  alias Ockam.Message
  alias Ockam.Router

  @doc false
  def forward_message(message, state) do
    case inner_message?(message, state) do
      true ->
        ## Inner message is forwarded
        [_me | onward_route] = Message.onward_route(message)
        return_route = Message.return_route(message)
        payload = Message.payload(message)

        Router.route(%{
          onward_route: onward_route,
          return_route: [state.address | return_route],
          payload: payload
        })

      false ->
        ## Outer message is forwarded to channel responder
        channel_route = Map.get(state, :channel_route)

        [_me | onward_route] = Message.onward_route(message)
        return_route = Message.return_route(message)
        payload = Message.payload(message)

        sender = Map.fetch!(state, :sender)

        Router.route(%{
          onward_route: [sender | channel_route ++ onward_route],
          return_route: return_route,
          payload: payload
        })
    end

    {:ok, state}
  end

  @doc false
  def inner_message?(message, state) do
    [me | _] = Message.onward_route(message)
    me == Map.get(state, :inner_address)
  end

  @doc false
  def register_inner_address(state) do
    {:ok, inner_address} = Ockam.Node.register_random_address()
    Map.put(state, :inner_address, inner_address)
  end

  @doc false
  def pipe_mods(options) do
    case Keyword.fetch(options, :pipe_mods) do
      {:ok, {sender_mod, receiver_mod}} ->
        {:ok, {sender_mod, receiver_mod}}

      {:ok, pipe_namespace} when is_atom(pipe_namespace) ->
        {:ok,
         {Module.safe_concat(pipe_namespace, Sender),
          Module.safe_concat(pipe_namespace, Receiver)}}
    end
  end
end

defmodule Ockam.Messaging.PipeChannel.Metadata do
  @moduledoc """
  Encodable data structure for pipechannel handshake metadata
  """

  defstruct [:receiver_route, :channel_route]

  @type t() :: %__MODULE__{}

  ## TODO: use proper address encoding
  @schema {:struct, [receiver_route: {:array, :data}, channel_route: {:array, :data}]}

  @spec encode(t()) :: binary()
  def encode(meta) do
    :bare.encode(meta, @schema)
  end

  @spec decode(binary()) :: t()
  def decode(data) do
    case :bare.decode(data, @schema) do
      {:ok, meta, ""} ->
        struct(__MODULE__, meta)

      other ->
        exit({:meta_decode_error, data, other})
    end
  end
end

defmodule Ockam.Messaging.PipeChannel.Initiator do
  @moduledoc """
  Pipe channel initiator.

  Using two addresses for inner and outer communication.

  Starts a local receiver and sends a handshake message to the remote spawner.
  The handshake message is sent wiht a RECEIVER address in the retourn route.

  In handshake stage:
  buffers all messages received on outer address.
  On handshake response creates a local sender using handshake metadata (receiver route) and spawner route.

  In ready stage:
  forwards messages from outer address to the sender and remote responder
  forwards messages from inner address to the onward route and traces own outer address in the return route

  Options:

  `pipe_mods` - pipe modules to use, either {sender, receiver} or an atom namespace, which has .Sender and .Receiver (e.g. `Ockam.Messaging.Ordering.Monotonic.IndexPipe`)
  `spawner_route` - a route to responder spawner

  """

  use Ockam.Worker

  alias Ockam.Messaging.PipeChannel
  alias Ockam.Messaging.PipeChannel.Metadata

  alias Ockam.Message
  alias Ockam.Router

  @impl true
  def setup(options, state) do
    spawner_route = Keyword.fetch!(options, :spawner_route)

    {:ok, {sender_mod, receiver_mod}} = PipeChannel.pipe_mods(options)

    {:ok, receiver} = receiver_mod.create([])

    state = PipeChannel.register_inner_address(state)

    send_handshake(spawner_route, receiver, state)

    {:ok,
     Map.merge(state, %{
       receiver: receiver,
       spawner_route: spawner_route,
       state: :handshake,
       sender_mod: sender_mod,
       receiver_mod: receiver_mod
     })}
  end

  @impl true
  def handle_message(message, %{state: :handshake} = state) do
    ## TODO: find a better solution than buffering
    case PipeChannel.inner_message?(message, state) do
      true ->
        payload = Message.payload(message)

        %Metadata{
          channel_route: channel_route,
          receiver_route: remote_receiver_route
        } = Metadata.decode(payload)

        spawner_route = Map.fetch!(state, :spawner_route)

        receiver_route = make_receiver_route(spawner_route, remote_receiver_route)

        sender_mod = Map.get(state, :sender_mod)
        {:ok, sender} = sender_mod.create(receiver_route: receiver_route)

        process_buffer(
          Map.merge(state, %{
            sender: sender,
            channel_route: channel_route,
            state: :ready
          })
        )

      false ->
        state = buffer_message(message, state)
        {:ok, state}
    end
  end

  def handle_message(message, %{state: :ready} = state) do
    PipeChannel.forward_message(message, state)
  end

  def process_buffer(state) do
    buffer = Map.get(state, :buffer, [])

    Enum.reduce(buffer, {:ok, state}, fn message, {:ok, state} ->
      handle_message(message, state)
    end)
  end

  def buffer_message(message, state) do
    buffer = Map.get(state, :buffer, [])
    Map.put(state, :buffer, buffer ++ [message])
  end

  defp make_receiver_route(spawner_route, remote_receiver_route) do
    Enum.take(spawner_route, Enum.count(spawner_route) - 1) ++ remote_receiver_route
  end

  defp send_handshake(spawner_route, receiver, state) do
    msg = %{
      onward_route: spawner_route,
      return_route: [receiver],
      payload:
        Metadata.encode(%Metadata{
          channel_route: [state.inner_address],
          receiver_route: [receiver]
        })
    }

    Router.route(msg)
  end
end

defmodule Ockam.Messaging.PipeChannel.Responder do
  @moduledoc """
  Pipe channel responder

  Using two addresses for inner and outer communication.

  Created with remote receiver route and channel route

  On start:
  creates a local receiver
  creates a sender for a remote receiver route
  sends a channel handshake confirmation through the sender
  confirmation contains local receiver address and responder inner address

  forwards messages from outer address through the sender and remote initiator
  forwards messages from inner address and traces own outer address in the return route

  Options:

  `pipe_mods` - pipe modules to use, either {sender, receiver} or an atom namespace, which has .Sender and .Receiver (e.g. `Ockam.Messaging.Ordering.Monotonic.IndexPipe`)
  `receiver_route` - route to the receiver on the initiator side, used to create a sender
  `channel_route` - route from initiator receiver to initiator, used in forwarding
  """

  use Ockam.Worker

  alias Ockam.Messaging.PipeChannel
  alias Ockam.Messaging.PipeChannel.Metadata

  alias Ockam.Router

  require Logger

  @impl true
  def setup(options, state) do
    receiver_route = Keyword.fetch!(options, :receiver_route)
    channel_route = Keyword.fetch!(options, :channel_route)

    {:ok, {sender_mod, receiver_mod}} = PipeChannel.pipe_mods(options)

    state = PipeChannel.register_inner_address(state)

    {:ok, receiver} = receiver_mod.create([])
    {:ok, sender} = sender_mod.create(receiver_route: receiver_route)

    send_handshake(receiver, sender, channel_route, state)

    {:ok,
     Map.merge(state, %{
       receiver: receiver,
       sender: sender,
       channel_route: channel_route,
       sender_mod: sender_mod,
       receiver_mod: receiver_mod
     })}
  end

  @impl true
  def handle_message(message, state) do
    PipeChannel.forward_message(message, state)
  end

  defp send_handshake(receiver, sender, channel_route, state) do
    msg = %{
      onward_route: [sender | channel_route],
      return_route: [state.inner_address],
      payload:
        Metadata.encode(%Metadata{
          channel_route: [state.inner_address],
          receiver_route: [receiver]
        })
    }

    Logger.info("Handshake #{inspect(msg)}")

    Router.route(msg)
  end
end

defmodule Ockam.Messaging.PipeChannel.Spawner do
  @moduledoc """
  Pipe channel receiver spawner

  On message spawns a channel receiver
  with remote route as a remote receiver route
  and channel route taken from the message metadata

  Options:

  `responder_options` - additional options to pass to the responder
  """
  use Ockam.Worker

  alias Ockam.Messaging.PipeChannel.Metadata
  alias Ockam.Messaging.PipeChannel.Responder

  alias Ockam.Message

  require Logger

  @impl true
  def setup(options, state) do
    responder_options = Keyword.fetch!(options, :responder_options)
    {:ok, Map.put(state, :responder_options, responder_options)}
  end

  @impl true
  def handle_message(message, state) do
    return_route = Message.return_route(message)
    payload = Message.payload(message)

    ## We ignore receiver route here and rely on return route tracing
    %Metadata{channel_route: channel_route} = Metadata.decode(payload)

    responder_options = Map.get(state, :responder_options)

    Responder.create(
      Keyword.merge(responder_options, receiver_route: return_route, channel_route: channel_route)
    )

    {:ok, state}
  end
end
