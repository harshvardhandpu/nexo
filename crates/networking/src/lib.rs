use common::{PeerId, SessionId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredPeer {
    pub peer_id: PeerId,
    pub display_name: String,
}

pub trait DiscoveryProvider {
    fn discover_peers(&self) -> std::io::Result<Vec<DiscoveredPeer>>;
}

pub trait Connection {
    fn session_id(&self) -> &SessionId;
    fn remote_peer(&self) -> &PeerId;
    fn send(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    fn receive(&mut self) -> std::io::Result<Vec<u8>>;
    fn close(&mut self) -> std::io::Result<()>;
}

pub trait Transport {
    type Connection: Connection;

    fn connect(
        &mut self,
        peer: &PeerId,
        session_id: SessionId,
    ) -> std::io::Result<Self::Connection>;
    fn accept(&mut self) -> std::io::Result<Self::Connection>;
}
