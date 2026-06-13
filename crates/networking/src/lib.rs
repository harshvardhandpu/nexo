use common::{
    ConnectionId, MessageEnvelope, PeerId, SessionId, StreamId, TransportError, TransportEvent,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};

pub mod discovery;
pub mod quic;
pub use discovery::{
    DiscoveryEvent, LocalDiscoveryProvider, PeerAdvertisement, PeerDiscovery, PeerInfo,
};
pub use quic::{QuicConnection, QuicListener, QuicStream, QuicTransportProvider};

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

#[derive(Debug, Clone)]
pub struct LoopbackNetwork {
    inner: Arc<LoopbackNetworkInner>,
}

#[derive(Debug)]
struct LoopbackNetworkInner {
    listeners: Mutex<HashMap<PeerId, mpsc::Sender<LoopbackConnection>>>,
    next_id: AtomicU64,
}

impl LoopbackNetwork {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(LoopbackNetworkInner {
                listeners: Mutex::new(HashMap::new()),
                next_id: AtomicU64::new(1),
            }),
        }
    }

    fn next_connection_id(&self) -> u64 {
        self.inner.next_id.fetch_add(1, Ordering::Relaxed)
    }

    fn listeners(
        &self,
    ) -> Result<
        std::sync::MutexGuard<'_, HashMap<PeerId, mpsc::Sender<LoopbackConnection>>>,
        TransportError,
    > {
        self.inner
            .listeners
            .lock()
            .map_err(|_| TransportError::Protocol {
                reason: "loopback listener registry is unavailable".to_owned(),
            })
    }
}

impl Default for LoopbackNetwork {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct LoopbackTransportProvider {
    local_peer: PeerId,
    network: LoopbackNetwork,
}

impl LoopbackTransportProvider {
    pub fn new(local_peer: PeerId) -> Self {
        Self::with_network(local_peer, LoopbackNetwork::new())
    }

    pub fn with_network(local_peer: PeerId, network: LoopbackNetwork) -> Self {
        Self {
            local_peer,
            network,
        }
    }

    pub fn paired(sender_peer: PeerId, receiver_peer: PeerId) -> (Self, Self) {
        let network = LoopbackNetwork::new();
        (
            Self::with_network(sender_peer, network.clone()),
            Self::with_network(receiver_peer, network),
        )
    }

    pub fn local_peer(&self) -> &PeerId {
        &self.local_peer
    }
}

impl TransportProvider for LoopbackTransportProvider {
    type Listener = LoopbackListener;
    type Connection = LoopbackConnection;

    fn listen(&mut self) -> Result<Self::Listener, TransportError> {
        let (incoming_tx, incoming_rx) = mpsc::channel();

        self.network
            .listeners()?
            .insert(self.local_peer.clone(), incoming_tx);

        Ok(LoopbackListener {
            local_peer: self.local_peer.clone(),
            incoming_rx,
        })
    }

    fn connect(
        &mut self,
        peer: &PeerId,
        _session_id: SessionId,
    ) -> Result<Self::Connection, TransportError> {
        let listener = self
            .network
            .listeners()?
            .get(peer)
            .cloned()
            .ok_or_else(|| TransportError::ConnectionFailed {
                connection_id: None,
                reason: format!("loopback peer is not listening: {}", peer.0),
            })?;

        let connection_number = self.network.next_connection_id();
        let client_connection_id =
            ConnectionId(format!("loopback-connection-{connection_number}-client"));
        let server_connection_id =
            ConnectionId(format!("loopback-connection-{connection_number}-server"));

        let (client_event_tx, client_event_rx) = mpsc::channel();
        let (server_event_tx, server_event_rx) = mpsc::channel();
        let (client_stream_tx, client_stream_rx) = mpsc::channel();
        let (server_stream_tx, server_stream_rx) = mpsc::channel();
        let next_stream_id = Arc::new(AtomicU64::new(1));

        let client = LoopbackConnection {
            connection_id: client_connection_id.clone(),
            peer_connection_id: server_connection_id.clone(),
            local_peer: self.local_peer.clone(),
            remote_peer: peer.clone(),
            event_tx: client_event_tx.clone(),
            event_rx: client_event_rx,
            incoming_stream_rx: client_stream_rx,
            peer_event_tx: server_event_tx.clone(),
            peer_incoming_stream_tx: server_stream_tx,
            next_stream_id: next_stream_id.clone(),
            closed: false,
        };

        let server = LoopbackConnection {
            connection_id: server_connection_id.clone(),
            peer_connection_id: client_connection_id.clone(),
            local_peer: peer.clone(),
            remote_peer: self.local_peer.clone(),
            event_tx: server_event_tx.clone(),
            event_rx: server_event_rx,
            incoming_stream_rx: server_stream_rx,
            peer_event_tx: client_event_tx.clone(),
            peer_incoming_stream_tx: client_stream_tx,
            next_stream_id,
            closed: false,
        };

        client_event_tx
            .send(TransportEvent::Connecting {
                connection_id: client_connection_id.clone(),
                peer_id: peer.clone(),
            })
            .map_err(|_| TransportError::ConnectionFailed {
                connection_id: Some(client_connection_id.clone()),
                reason: "loopback connection event queue is closed".to_owned(),
            })?;
        client_event_tx
            .send(TransportEvent::Connected {
                connection_id: client_connection_id.clone(),
                peer_id: peer.clone(),
            })
            .map_err(|_| TransportError::ConnectionFailed {
                connection_id: Some(client_connection_id.clone()),
                reason: "loopback connection event queue is closed".to_owned(),
            })?;
        server_event_tx
            .send(TransportEvent::Connected {
                connection_id: server_connection_id,
                peer_id: self.local_peer.clone(),
            })
            .map_err(|_| TransportError::ConnectionFailed {
                connection_id: Some(client_connection_id.clone()),
                reason: "loopback listener event queue is closed".to_owned(),
            })?;

        listener
            .send(server)
            .map_err(|_| TransportError::ConnectionFailed {
                connection_id: Some(client_connection_id),
                reason: format!("loopback peer stopped listening: {}", peer.0),
            })?;

        Ok(client)
    }
}

#[derive(Debug)]
pub struct LoopbackListener {
    local_peer: PeerId,
    incoming_rx: mpsc::Receiver<LoopbackConnection>,
}

impl LoopbackListener {
    pub fn local_peer(&self) -> &PeerId {
        &self.local_peer
    }
}

impl TransportListener for LoopbackListener {
    type Connection = LoopbackConnection;

    fn accept(&mut self) -> Result<Self::Connection, TransportError> {
        self.incoming_rx
            .recv()
            .map_err(|_| TransportError::ConnectionFailed {
                connection_id: None,
                reason: format!("loopback listener is closed: {}", self.local_peer.0),
            })
    }
}

#[derive(Debug)]
pub struct LoopbackConnection {
    connection_id: ConnectionId,
    peer_connection_id: ConnectionId,
    local_peer: PeerId,
    remote_peer: PeerId,
    event_tx: mpsc::Sender<TransportEvent>,
    event_rx: mpsc::Receiver<TransportEvent>,
    incoming_stream_rx: mpsc::Receiver<LoopbackStream>,
    peer_event_tx: mpsc::Sender<TransportEvent>,
    peer_incoming_stream_tx: mpsc::Sender<LoopbackStream>,
    next_stream_id: Arc<AtomicU64>,
    closed: bool,
}

impl LoopbackConnection {
    pub fn local_peer(&self) -> &PeerId {
        &self.local_peer
    }

    pub fn try_next_event(&mut self) -> Result<Option<TransportEvent>, TransportError> {
        match self.event_rx.try_recv() {
            Ok(event) => Ok(Some(event)),
            Err(mpsc::TryRecvError::Empty) => Ok(None),
            Err(mpsc::TryRecvError::Disconnected) => Err(TransportError::ConnectionClosed {
                connection_id: self.connection_id.clone(),
            }),
        }
    }

    fn ensure_open(&self) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::ConnectionClosed {
                connection_id: self.connection_id.clone(),
            });
        }

        Ok(())
    }
}

impl TransportConnection for LoopbackConnection {
    type Stream = LoopbackStream;

    fn connection_id(&self) -> &ConnectionId {
        &self.connection_id
    }

    fn remote_peer(&self) -> &PeerId {
        &self.remote_peer
    }

    fn open_stream(&mut self) -> Result<Self::Stream, TransportError> {
        self.ensure_open()?;

        let stream_number = self.next_stream_id.fetch_add(1, Ordering::Relaxed);
        let stream_id = StreamId(format!("loopback-stream-{stream_number}"));
        let (local_to_remote_tx, local_to_remote_rx) = mpsc::channel();
        let (remote_to_local_tx, remote_to_local_rx) = mpsc::channel();

        let local_stream = LoopbackStream {
            stream_id: stream_id.clone(),
            connection_id: self.connection_id.clone(),
            event_tx: self.event_tx.clone(),
            inbound_rx: remote_to_local_rx,
            outbound_tx: local_to_remote_tx,
            closed: false,
        };

        let remote_stream = LoopbackStream {
            stream_id: stream_id.clone(),
            connection_id: self.peer_connection_id.clone(),
            event_tx: self.peer_event_tx.clone(),
            inbound_rx: local_to_remote_rx,
            outbound_tx: remote_to_local_tx,
            closed: false,
        };

        self.peer_incoming_stream_tx
            .send(remote_stream)
            .map_err(|_| TransportError::StreamFailed {
                connection_id: self.connection_id.clone(),
                stream_id: stream_id.clone(),
                reason: "loopback peer is not accepting streams".to_owned(),
            })?;

        self.event_tx
            .send(TransportEvent::StreamOpened {
                connection_id: self.connection_id.clone(),
                stream_id: stream_id.clone(),
            })
            .map_err(|_| TransportError::ConnectionClosed {
                connection_id: self.connection_id.clone(),
            })?;
        self.peer_event_tx
            .send(TransportEvent::StreamOpened {
                connection_id: self.peer_connection_id.clone(),
                stream_id,
            })
            .map_err(|_| TransportError::ConnectionClosed {
                connection_id: self.connection_id.clone(),
            })?;

        Ok(local_stream)
    }

    fn accept_stream(&mut self) -> Result<Self::Stream, TransportError> {
        self.ensure_open()?;

        self.incoming_stream_rx
            .recv()
            .map_err(|_| TransportError::ConnectionClosed {
                connection_id: self.connection_id.clone(),
            })
    }

    fn next_event(&mut self) -> Result<TransportEvent, TransportError> {
        self.event_rx
            .recv()
            .map_err(|_| TransportError::ConnectionClosed {
                connection_id: self.connection_id.clone(),
            })
    }

    fn close(&mut self) -> Result<(), TransportError> {
        if self.closed {
            return Ok(());
        }

        self.closed = true;
        self.event_tx
            .send(TransportEvent::Closed {
                connection_id: self.connection_id.clone(),
            })
            .map_err(|_| TransportError::ConnectionClosed {
                connection_id: self.connection_id.clone(),
            })?;
        self.peer_event_tx
            .send(TransportEvent::Closed {
                connection_id: self.peer_connection_id.clone(),
            })
            .map_err(|_| TransportError::ConnectionClosed {
                connection_id: self.connection_id.clone(),
            })?;

        Ok(())
    }
}

#[derive(Debug)]
pub struct LoopbackStream {
    stream_id: StreamId,
    connection_id: ConnectionId,
    event_tx: mpsc::Sender<TransportEvent>,
    inbound_rx: mpsc::Receiver<MessageEnvelope>,
    outbound_tx: mpsc::Sender<MessageEnvelope>,
    closed: bool,
}

impl LoopbackStream {
    fn ensure_open(&self) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::StreamFailed {
                connection_id: self.connection_id.clone(),
                stream_id: self.stream_id.clone(),
                reason: "loopback stream is closed".to_owned(),
            });
        }

        Ok(())
    }
}

impl TransportStream for LoopbackStream {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn send_message(&mut self, envelope: MessageEnvelope) -> Result<(), TransportError> {
        self.ensure_open()?;

        self.outbound_tx
            .send(envelope.clone())
            .map_err(|_| TransportError::StreamFailed {
                connection_id: self.connection_id.clone(),
                stream_id: self.stream_id.clone(),
                reason: "loopback stream receiver is closed".to_owned(),
            })?;
        self.event_tx
            .send(TransportEvent::MessageSent {
                connection_id: self.connection_id.clone(),
                stream_id: self.stream_id.clone(),
                envelope,
            })
            .map_err(|_| TransportError::ConnectionClosed {
                connection_id: self.connection_id.clone(),
            })?;

        Ok(())
    }

    fn receive_message(&mut self) -> Result<MessageEnvelope, TransportError> {
        self.ensure_open()?;

        let envelope = self
            .inbound_rx
            .recv()
            .map_err(|_| TransportError::StreamFailed {
                connection_id: self.connection_id.clone(),
                stream_id: self.stream_id.clone(),
                reason: "loopback stream sender is closed".to_owned(),
            })?;

        self.event_tx
            .send(TransportEvent::MessageReceived {
                connection_id: self.connection_id.clone(),
                stream_id: self.stream_id.clone(),
                envelope: envelope.clone(),
            })
            .map_err(|_| TransportError::ConnectionClosed {
                connection_id: self.connection_id.clone(),
            })?;

        Ok(envelope)
    }

    fn close(&mut self) -> Result<(), TransportError> {
        if self.closed {
            return Ok(());
        }

        self.closed = true;
        self.event_tx
            .send(TransportEvent::StreamClosed {
                connection_id: self.connection_id.clone(),
                stream_id: self.stream_id.clone(),
            })
            .map_err(|_| TransportError::ConnectionClosed {
                connection_id: self.connection_id.clone(),
            })?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use common::{
        Checkpoint, ChunkId, FileManifest, SessionInfo, SessionState, TransferAcceptance,
        TransferChunkMessage, TransferControlMessage, TransferId, TransferMessage,
        TransferResponse, TransferSessionMessage,
    };
    use storage::{
        CheckpointStore, SessionStore, SqliteStorageBackend, Storage as PersistentStorage,
    };

    #[test]
    fn sender_can_connect_to_receiver() {
        let (mut sender, mut listener, _receiver) = loopback_pair();

        let sender_connection = sender
            .connect(&peer_b(), SessionId("session-1".to_owned()))
            .expect("sender connection");
        let receiver_connection = listener.accept().expect("receiver connection");

        assert_eq!(sender_connection.remote_peer(), &peer_b());
        assert_eq!(receiver_connection.remote_peer(), &peer_a());
    }

    #[test]
    fn messages_are_delivered_between_streams() {
        let (mut sender_connection, mut receiver_connection) = connected_pair();
        let mut sender_stream = sender_connection.open_stream().expect("sender stream");
        let mut receiver_stream = receiver_connection
            .accept_stream()
            .expect("receiver stream");
        let envelope = chunk_envelope(1);

        sender_stream
            .send_message(envelope.clone())
            .expect("send message");
        let received = receiver_stream.receive_message().expect("receive message");

        assert_eq!(received, envelope);
    }

    #[test]
    fn streams_function_bidirectionally() {
        let (mut sender_connection, mut receiver_connection) = connected_pair();
        let mut sender_stream = sender_connection.open_stream().expect("sender stream");
        let mut receiver_stream = receiver_connection
            .accept_stream()
            .expect("receiver stream");
        let request = control_envelope("pause");
        let response = accepted_envelope();

        sender_stream
            .send_message(request.clone())
            .expect("send request");
        assert_eq!(
            receiver_stream.receive_message().expect("receive request"),
            request
        );

        receiver_stream
            .send_message(response.clone())
            .expect("send response");
        assert_eq!(
            sender_stream.receive_message().expect("receive response"),
            response
        );
    }

    #[test]
    fn transport_events_are_generated() {
        let (mut sender_connection, mut receiver_connection) = connected_pair();

        assert!(matches!(
            sender_connection.next_event().expect("connecting event"),
            TransportEvent::Connecting { .. }
        ));
        assert!(matches!(
            sender_connection.next_event().expect("connected event"),
            TransportEvent::Connected { .. }
        ));
        assert!(matches!(
            receiver_connection
                .next_event()
                .expect("receiver connected"),
            TransportEvent::Connected { .. }
        ));

        let mut sender_stream = sender_connection.open_stream().expect("sender stream");
        let mut receiver_stream = receiver_connection
            .accept_stream()
            .expect("receiver stream");
        let envelope = chunk_envelope(7);

        assert!(matches!(
            sender_connection
                .next_event()
                .expect("sender stream opened"),
            TransportEvent::StreamOpened { .. }
        ));
        assert!(matches!(
            receiver_connection
                .next_event()
                .expect("receiver stream opened"),
            TransportEvent::StreamOpened { .. }
        ));

        sender_stream
            .send_message(envelope.clone())
            .expect("send message");
        assert!(matches!(
            sender_connection.next_event().expect("message sent"),
            TransportEvent::MessageSent {
                envelope: sent,
                ..
            } if sent == envelope
        ));

        assert_eq!(
            receiver_stream.receive_message().expect("receive message"),
            envelope
        );
        assert!(matches!(
            receiver_connection.next_event().expect("message received"),
            TransportEvent::MessageReceived {
                envelope: received,
                ..
            } if received == envelope
        ));
    }

    #[test]
    fn multiple_simultaneous_streams_deliver_independent_messages() {
        let (mut sender_connection, mut receiver_connection) = connected_pair();
        let mut sender_stream_a = sender_connection.open_stream().expect("sender stream a");
        let mut sender_stream_b = sender_connection.open_stream().expect("sender stream b");
        let mut receiver_stream_a = receiver_connection
            .accept_stream()
            .expect("receiver stream a");
        let mut receiver_stream_b = receiver_connection
            .accept_stream()
            .expect("receiver stream b");
        let first = chunk_envelope(1);
        let second = chunk_envelope(2);

        sender_stream_a
            .send_message(first.clone())
            .expect("send first");
        sender_stream_b
            .send_message(second.clone())
            .expect("send second");

        assert_eq!(
            receiver_stream_a.receive_message().expect("receive first"),
            first
        );
        assert_eq!(
            receiver_stream_b.receive_message().expect("receive second"),
            second
        );
        assert_ne!(sender_stream_a.stream_id(), sender_stream_b.stream_id());
    }

    #[test]
    fn session_transport_and_storage_work_together_without_networking() {
        let (mut sender_connection, mut receiver_connection) = connected_pair();
        let mut storage =
            PersistentStorage::new(SqliteStorageBackend::in_memory().expect("sqlite backend"))
                .expect("storage");
        let mut session = SessionInfo::new(session_id(), transfer_id(), peer_a(), peer_b());

        for _ in 0..2 {
            let event = sender_connection.next_event().expect("connect event");
            if let Some(next_state) = event.session_state_hint() {
                session
                    .transition_to(next_state)
                    .expect("session transition");
                storage.save_session(&session).expect("persist session");
            }
        }
        assert!(matches!(
            receiver_connection
                .next_event()
                .expect("receiver connected"),
            TransportEvent::Connected { .. }
        ));

        let mut sender_stream = sender_connection.open_stream().expect("sender stream");
        let mut receiver_stream = receiver_connection
            .accept_stream()
            .expect("receiver stream");
        assert!(matches!(
            sender_connection
                .next_event()
                .expect("sender stream opened"),
            TransportEvent::StreamOpened { .. }
        ));
        assert!(matches!(
            receiver_connection
                .next_event()
                .expect("receiver stream opened"),
            TransportEvent::StreamOpened { .. }
        ));

        sender_stream
            .send_message(request_envelope())
            .expect("send transfer request");
        assert!(matches!(
            sender_connection.next_event().expect("request sent"),
            TransportEvent::MessageSent { .. }
        ));
        assert_eq!(
            receiver_stream
                .receive_message()
                .expect("receive transfer request"),
            request_envelope()
        );
        assert!(matches!(
            receiver_connection.next_event().expect("request received"),
            TransportEvent::MessageReceived { .. }
        ));

        storage
            .save_checkpoint(&Checkpoint {
                transfer_id: transfer_id(),
                completed_chunks: vec![ChunkId(0)],
            })
            .expect("persist checkpoint");

        receiver_stream
            .send_message(accepted_envelope())
            .expect("send acceptance");
        assert!(matches!(
            receiver_connection.next_event().expect("acceptance sent"),
            TransportEvent::MessageSent { .. }
        ));
        assert_eq!(
            sender_stream
                .receive_message()
                .expect("receive transfer acceptance"),
            accepted_envelope()
        );

        let event = sender_connection
            .next_event()
            .expect("message received event");
        let next_state = event.session_state_hint().expect("accepted state hint");
        session
            .transition_to(next_state)
            .expect("accepted transition");
        storage.save_session(&session).expect("persist accepted");

        let stored_session = storage
            .load_session(&session_id())
            .expect("load session")
            .expect("session exists");
        let stored_checkpoint = storage
            .load_checkpoint(&transfer_id())
            .expect("load checkpoint")
            .expect("checkpoint exists");

        assert_eq!(stored_session.state, SessionState::Accepted);
        assert_eq!(stored_checkpoint.completed_chunks, vec![ChunkId(0)]);
    }

    fn loopback_pair() -> (
        LoopbackTransportProvider,
        LoopbackListener,
        LoopbackTransportProvider,
    ) {
        let (sender, mut receiver) = LoopbackTransportProvider::paired(peer_a(), peer_b());
        let listener = receiver.listen().expect("receiver listener");

        (sender, listener, receiver)
    }

    fn connected_pair() -> (LoopbackConnection, LoopbackConnection) {
        let (mut sender, mut listener, _receiver) = loopback_pair();
        let sender_connection = sender
            .connect(&peer_b(), SessionId("session-1".to_owned()))
            .expect("sender connection");
        let receiver_connection = listener.accept().expect("receiver connection");

        (sender_connection, receiver_connection)
    }

    fn peer_a() -> PeerId {
        PeerId("peer-a".to_owned())
    }

    fn peer_b() -> PeerId {
        PeerId("peer-b".to_owned())
    }

    fn session_id() -> SessionId {
        SessionId("session-1".to_owned())
    }

    fn transfer_id() -> TransferId {
        TransferId("transfer-1".to_owned())
    }

    fn manifest() -> FileManifest {
        FileManifest {
            name: "file.bin".to_owned(),
            size: 4,
            chunk_size: 4,
            total_chunks: 1,
            sha256: "sha256".to_owned(),
        }
    }

    fn request_envelope() -> MessageEnvelope {
        MessageEnvelope {
            session_id: session_id(),
            transfer_id: transfer_id(),
            message: TransferMessage::Session(TransferSessionMessage::Request(
                common::TransferRequest {
                    session_id: session_id(),
                    transfer_id: transfer_id(),
                    from_peer: peer_a(),
                    to_peer: peer_b(),
                    manifest: manifest(),
                },
            )),
        }
    }

    fn accepted_envelope() -> MessageEnvelope {
        MessageEnvelope {
            session_id: session_id(),
            transfer_id: transfer_id(),
            message: TransferMessage::Session(TransferSessionMessage::Response(
                TransferResponse::Accepted(TransferAcceptance {
                    session_id: session_id(),
                    transfer_id: transfer_id(),
                }),
            )),
        }
    }

    fn chunk_envelope(chunk_id: u64) -> MessageEnvelope {
        MessageEnvelope {
            session_id: session_id(),
            transfer_id: transfer_id(),
            message: TransferMessage::Chunk(TransferChunkMessage::Metadata(
                common::ChunkMetadata {
                    id: ChunkId(chunk_id),
                    offset: chunk_id * 4,
                    size: 4,
                    sha256: format!("chunk-{chunk_id}"),
                },
            )),
        }
    }

    fn control_envelope(reason: &str) -> MessageEnvelope {
        MessageEnvelope {
            session_id: session_id(),
            transfer_id: transfer_id(),
            message: TransferMessage::Control(TransferControlMessage::Cancel {
                transfer_id: transfer_id(),
                reason: reason.to_owned(),
            }),
        }
    }
}
