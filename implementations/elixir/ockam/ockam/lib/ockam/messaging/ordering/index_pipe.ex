defmodule Ockam.Messaging.Ordering.Monotonic.IndexPipe do
  @moduledoc """
  Monotonicly ordered pipe using indexing to enforce ordering

  See `Ockam.Messaging.Ordering.Monotonic.IndexPipe.Sender` and
  `Ockam.Messaging.Ordering.Monotonic.IndexPipe.Receiver`
  """
end

defmodule Ockam.Messaging.Ordering.Monotonic.IndexPipe.Wrapper do
  @moduledoc """
  Message wrapper for indexed pipes
  """

  @schema {:struct, [index: :uint, message: :data]}

  @doc """
  Encodes message and index into a binary
  """

  @spec wrap_message(integer(), Ockam.Message.t()) :: binary()
  def wrap_message(index, message) do
    {:ok, encoded} = Ockam.Wire.encode(Ockam.Wire.Binary.V2, message)
    :bare.encode(%{index: index, message: encoded}, @schema)
  end

  @doc """
  Decodes message and index from a binary
  """

  @spec unwrap_message(binary()) :: {:ok, integer(), Ockam.Message.t()} | {:error, any()}
  def unwrap_message(payload) do
    with {:ok, %{index: index, message: encoded_message}, ""} <-
           :bare.decode(payload, @schema),
         {:ok, message} <- Ockam.Wire.decode(Ockam.Wire.Binary.V2, encoded_message) do
      {:ok, index, message}
    end
  end
end

defmodule Ockam.Messaging.Ordering.Monotonic.IndexPipe.Sender do
  @moduledoc """
  Sender side of monotonic ordered pipe using indexing to enforce ordering
  Each incoming message is assigned an monotonic index, wrapped and sent to receiver

  Options:

  `receiver_route` - a route to receiver
  """

  use Ockam.Worker

  alias Ockam.Message

  alias Ockam.Messaging.Ordering.Monotonic.IndexPipe.Wrapper

  @impl true
  def setup(options, state) do
    receiver_route = Keyword.fetch!(options, :receiver_route)
    {:ok, Map.put(state, :receiver_route, receiver_route)}
  end

  @impl true
  def handle_message(message, state) do
    {indexed_message, state} = make_indexed_message(message, state)
    Ockam.Router.route(indexed_message)
    {:ok, state}
  end

  defp make_indexed_message(message, state) do
    {next_index, state} = next_index(state)
    [_ | onward_route] = Message.onward_route(message)

    forwarded_message = %{
      onward_route: onward_route,
      return_route: Message.return_route(message),
      payload: Message.payload(message)
    }

    indexed_message = %{
      onward_route: receiver_route(state),
      return_route: local_address(state),
      payload: Wrapper.wrap_message(next_index, forwarded_message)
    }

    {indexed_message, state}
  end

  defp next_index(state) do
    index = Map.get(state, :last_index, 0) + 1
    {index, Map.put(state, :last_index, index)}
  end

  defp receiver_route(state) do
    Map.get(state, :receiver_route)
  end

  defp local_address(state) do
    Map.get(state, :address)
  end
end

defmodule Ockam.Messaging.Ordering.Monotonic.IndexPipe.Receiver do
  @moduledoc """
  Receiver side of monotonic ordered pipe using indexing to enforce ordering
  Maintains a monotonic sent message index

  Receives wrapped messages from the sender, unwraps them and only forwards
  if the message index is higher then the monotonic index.
  After sending a message updates the index to the message index

  """
  use Ockam.Worker

  alias Ockam.Messaging.Ordering.Monotonic.IndexPipe.Wrapper

  require Logger

  @impl true
  def handle_message(indexed_message, state) do
    case Wrapper.unwrap_message(Ockam.Message.payload(indexed_message)) do
      {:ok, index, message} ->
        case index_valid?(index, state) do
          true ->
            Logger.info("routing #{inspect(message)}")
            Ockam.Router.route(message)
            {:ok, Map.put(state, :current_index, index)}

          false ->
            Logger.error(
              "Cannot send message #{inspect(message)} with index #{inspect(index)}. Current index: #{
                inspect(current_index(state))
              }"
            )

            ## TODO: fail?
            {:ok, state}
        end

      other ->
        Logger.error(
          "Unable to decode indexed message: #{inspect(indexed_message)}, reason: #{
            inspect(other)
          }"
        )

        {:error, :unable_to_decode_message}
    end
  end

  defp index_valid?(index, state) do
    index > current_index(state)
  end

  defp current_index(state) do
    Map.get(state, :current_index, 0)
  end
end
