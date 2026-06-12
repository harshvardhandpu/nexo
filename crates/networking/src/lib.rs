use common::{
    ConnectionId, MessageEnvelope, PeerId, SessionId, StreamId, TransportError, TransportEvent,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub peer_id: PeerId,
    pub display_name: String,
}

pub trait DiscoveryProvider {
    fn discover_peers(&self) -> std::io::Result<Vec<DiscoveredPeer>>;
}

pub trait TransportProvider {
    type Listener: TransportListener;
    type Connection: TransportConnection;

    fn listen(&mut self) -> Result<Self::Listener, TransportError>;
    fn connect(
        &mut self,
        peer: &PeerId,
        session_id: SessionId,
    ) -> Result<Self::Connection, TransportError>;
}

pub trait TransportListener {
    type Connection: TransportConnection;

    fn accept(&mut self) -> Result<Self::Connection, TransportError>;
}

pub trait TransportConnection {
    type Stream: TransportStream;

    fn connection_id(&self) -> &ConnectionId;
    fn remote_peer(&self) -> &PeerId;
    fn open_stream(&mut self) -> Result<Self::Stream, TransportError>;
    fn accept_stream(&mut self) -> Result<Self::Stream, TransportError>;
    fn next_event(&mut self) -> Result<TransportEvent, TransportError>;
    fn close(&mut self) -> Result<(), TransportError>;
}

pub trait TransportStream {
    fn stream_id(&self) -> &StreamId;
    fn send_message(&mut self, envelope: MessageEnvelope) -> Result<(), TransportError>;
    fn receive_message(&mut self) -> Result<MessageEnvelope, TransportError>;
    fn close(&mut self) -> Result<(), TransportError>;
}
