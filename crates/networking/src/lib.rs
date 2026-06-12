#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PeerId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAddress(pub String);

pub trait PeerDiscovery {
    fn discover_peers(&self) -> Vec<PeerId>;
}

pub trait SessionManager {
    fn open_session(&mut self, peer: &PeerId) -> std::io::Result<SessionId>;
    fn close_session(&mut self, session: &SessionId) -> std::io::Result<()>;
}

pub trait Transport {
    fn send(&mut self, session: &SessionId, bytes: &[u8]) -> std::io::Result<()>;
    fn receive(&mut self, session: &SessionId) -> std::io::Result<Vec<u8>>;
}
