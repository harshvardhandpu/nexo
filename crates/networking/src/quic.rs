use crate::{TransportConnection, TransportListener, TransportProvider, TransportStream};
use common::{
    ConnectionId, MessageEnvelope, PeerId, SessionId, StreamId, TransportError, TransportEvent,
};
use quinn::rustls::{
    RootCertStore,
    pki_types::{CertificateDer, PrivatePkcs8KeyDer},
};
use quinn::{ClientConfig, Endpoint, EndpointConfig, RecvStream, SendStream, ServerConfig};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::{Arc, mpsc};
use tokio::io::AsyncWriteExt;

const LOCALHOST_SERVER_NAME: &str = "localhost";
const STREAM_PREFACE: &[u8; 8] = b"NEXOQST1";
const MAX_FRAME_SIZE: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct QuicPeerConfig {
    pub address: SocketAddr,
    pub certificate_der: Vec<u8>,
}

/// A reusable QUIC server identity (self-signed localhost certificate and its
/// private key).
///
/// The transport generates a fresh identity on every `listen()` by default.
/// Callers that need a *stable* listener identity across process restarts (for
/// example, so a previously advertised address and certificate remain valid for
/// a resuming peer) can generate an identity once, persist it, and rebind with
/// it through [`QuicTransportProvider::listen_with_identity`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuicServerIdentity {
    pub certificate_der: Vec<u8>,
    pub private_key_der: Vec<u8>,
}

#[derive(Debug)]
pub struct QuicTransportProvider {
    local_peer: PeerId,
    bind_addr: SocketAddr,
    runtime: Arc<tokio::runtime::Runtime>,
    peers: HashMap<PeerId, QuicPeerConfig>,
}

impl QuicTransportProvider {
    pub fn new(local_peer: PeerId, bind_addr: SocketAddr) -> Result<Self, TransportError> {
        Ok(Self {
            local_peer,
            bind_addr,
            runtime: new_runtime()?,
            peers: HashMap::new(),
        })
    }

    pub fn localhost(local_peer: PeerId) -> Result<Self, TransportError> {
        Self::new(
            local_peer,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0),
        )
    }

    pub fn local_peer(&self) -> &PeerId {
        &self.local_peer
    }

    pub fn register_peer(&mut self, peer: PeerId, address: SocketAddr, certificate_der: Vec<u8>) {
        self.peers.insert(
            peer,
            QuicPeerConfig {
                address,
                certificate_der,
            },
        );
    }

    /// Generates a fresh self-signed localhost server identity.
    ///
    /// The returned identity can be persisted and later reused with
    /// [`Self::listen_with_identity`] so that a restarted listener keeps the
    /// same certificate (and, when bound to the same address, the same
    /// endpoint) that peers were previously told to trust.
    pub fn generate_server_identity() -> Result<QuicServerIdentity, TransportError> {
        let cert = rcgen::generate_simple_self_signed(vec![LOCALHOST_SERVER_NAME.to_owned()])
            .map_err(|error| TransportError::Protocol {
                reason: format!("failed to generate QUIC localhost certificate: {error}"),
            })?;
        let certificate_der = CertificateDer::from(cert.cert).as_ref().to_vec();
        let private_key_der = cert.signing_key.serialize_der();

        Ok(QuicServerIdentity {
            certificate_der,
            private_key_der,
        })
    }

    /// Binds a listener using a caller-supplied [`QuicServerIdentity`] instead of
    /// generating a fresh certificate.
    ///
    /// Combined with binding to a fixed address, this lets a restarted receiver
    /// present the same address and certificate it advertised before, which is
    /// what allows an interrupted sender to reconnect and resume.
    pub fn listen_with_identity(
        &mut self,
        identity: &QuicServerIdentity,
    ) -> Result<QuicListener, TransportError> {
        let server_config = server_config_from_identity(identity)?;
        self.bind_listener(server_config, identity.certificate_der.clone())
    }

    fn bind_listener(
        &self,
        server_config: ServerConfig,
        certificate_der: Vec<u8>,
    ) -> Result<QuicListener, TransportError> {
        let socket =
            UdpSocket::bind(self.bind_addr).map_err(|error| TransportError::ConnectionFailed {
                connection_id: None,
                reason: format!("failed to bind QUIC listener socket: {error}"),
            })?;
        let _runtime_guard = self.runtime.enter();
        let endpoint = Endpoint::new(
            EndpointConfig::default(),
            Some(server_config),
            socket,
            Arc::new(quinn::TokioRuntime),
        )
        .map_err(|error| TransportError::ConnectionFailed {
            connection_id: None,
            reason: format!("failed to create QUIC server endpoint: {error}"),
        })?;
        let local_addr =
            endpoint
                .local_addr()
                .map_err(|error| TransportError::ConnectionFailed {
                    connection_id: None,
                    reason: format!("failed to read QUIC listener address: {error}"),
                })?;

        Ok(QuicListener {
            local_peer: self.local_peer.clone(),
            local_addr,
            certificate_der,
            endpoint,
            runtime: self.runtime.clone(),
        })
    }

    fn client_endpoint(&self, peer: &QuicPeerConfig) -> Result<Endpoint, TransportError> {
        let mut roots = RootCertStore::empty();
        roots
            .add(CertificateDer::from(peer.certificate_der.clone()))
            .map_err(|error| TransportError::Protocol {
                reason: format!("failed to trust QUIC peer certificate: {error}"),
            })?;
        let client_config =
            ClientConfig::with_root_certificates(Arc::new(roots)).map_err(|error| {
                TransportError::Protocol {
                    reason: format!("failed to configure QUIC client: {error}"),
                }
            })?;
        let socket =
            UdpSocket::bind(self.bind_addr).map_err(|error| TransportError::ConnectionFailed {
                connection_id: None,
                reason: format!("failed to bind QUIC client socket: {error}"),
            })?;
        let _runtime_guard = self.runtime.enter();
        let mut endpoint = Endpoint::new(
            EndpointConfig::default(),
            None,
            socket,
            Arc::new(quinn::TokioRuntime),
        )
        .map_err(|error| TransportError::ConnectionFailed {
            connection_id: None,
            reason: format!("failed to create QUIC client endpoint: {error}"),
        })?;
        endpoint.set_default_client_config(client_config);

        Ok(endpoint)
    }
}

impl TransportProvider for QuicTransportProvider {
    type Listener = QuicListener;
    type Connection = QuicConnection;

    fn listen(&mut self) -> Result<Self::Listener, TransportError> {
        let identity = Self::generate_server_identity()?;
        self.listen_with_identity(&identity)
    }

    fn connect(
        &mut self,
        peer: &PeerId,
        session_id: SessionId,
    ) -> Result<Self::Connection, TransportError> {
        let peer_config =
            self.peers
                .get(peer)
                .cloned()
                .ok_or_else(|| TransportError::ConnectionFailed {
                    connection_id: None,
                    reason: format!("QUIC peer is not registered: {}", peer.0),
                })?;
        let endpoint = self.client_endpoint(&peer_config)?;
        let connecting_id = ConnectionId(format!(
            "quic-connecting-{}-{}-{}",
            self.local_peer.0, peer.0, session_id.0
        ));
        let (event_tx, event_rx) = mpsc::channel();
        send_event(
            &event_tx,
            TransportEvent::Connecting {
                connection_id: connecting_id.clone(),
                peer_id: peer.clone(),
            },
            &connecting_id,
        )?;

        let connection = self.runtime.block_on(async {
            let connection = endpoint
                .connect(peer_config.address, LOCALHOST_SERVER_NAME)
                .map_err(|error| TransportError::ConnectionFailed {
                    connection_id: Some(connecting_id.clone()),
                    reason: format!("failed to start QUIC connection: {error}"),
                })?
                .await
                .map_err(|error| TransportError::ConnectionFailed {
                    connection_id: Some(connecting_id.clone()),
                    reason: format!("failed to establish QUIC connection: {error}"),
                })?;
            send_peer_handshake(&connection, &self.local_peer, &session_id, &connecting_id).await?;
            Ok::<_, TransportError>(connection)
        })?;

        let connection_id = quic_connection_id(&connection);
        send_event(
            &event_tx,
            TransportEvent::Connected {
                connection_id: connection_id.clone(),
                peer_id: peer.clone(),
            },
            &connection_id,
        )?;

        Ok(QuicConnection {
            connection_id,
            remote_peer: peer.clone(),
            endpoint,
            connection,
            runtime: self.runtime.clone(),
            event_tx,
            event_rx,
            closed: false,
        })
    }
}

#[derive(Debug)]
pub struct QuicListener {
    local_peer: PeerId,
    local_addr: SocketAddr,
    certificate_der: Vec<u8>,
    endpoint: Endpoint,
    runtime: Arc<tokio::runtime::Runtime>,
}

impl QuicListener {
    pub fn local_peer(&self) -> &PeerId {
        &self.local_peer
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn certificate_der(&self) -> &[u8] {
        &self.certificate_der
    }
}

impl TransportListener for QuicListener {
    type Connection = QuicConnection;

    fn accept(&mut self) -> Result<Self::Connection, TransportError> {
        let endpoint = self.endpoint.clone();
        let connection = self.runtime.block_on(async {
            let incoming =
                endpoint
                    .accept()
                    .await
                    .ok_or_else(|| TransportError::ConnectionFailed {
                        connection_id: None,
                        reason: "QUIC listener is closed".to_owned(),
                    })?;
            incoming
                .await
                .map_err(|error| TransportError::ConnectionFailed {
                    connection_id: None,
                    reason: format!("failed to accept QUIC connection: {error}"),
                })
        })?;
        let connection_id = quic_connection_id(&connection);
        let (remote_peer, _session_id) = self
            .runtime
            .block_on(receive_peer_handshake(&connection, &connection_id))?;
        let (event_tx, event_rx) = mpsc::channel();

        send_event(
            &event_tx,
            TransportEvent::Connected {
                connection_id: connection_id.clone(),
                peer_id: remote_peer.clone(),
            },
            &connection_id,
        )?;

        Ok(QuicConnection {
            connection_id,
            remote_peer,
            endpoint: self.endpoint.clone(),
            connection,
            runtime: self.runtime.clone(),
            event_tx,
            event_rx,
            closed: false,
        })
    }
}

#[derive(Debug)]
pub struct QuicConnection {
    connection_id: ConnectionId,
    remote_peer: PeerId,
    endpoint: Endpoint,
    connection: quinn::Connection,
    runtime: Arc<tokio::runtime::Runtime>,
    event_tx: mpsc::Sender<TransportEvent>,
    event_rx: mpsc::Receiver<TransportEvent>,
    closed: bool,
}

impl QuicConnection {
    pub fn local_addr(&self) -> Result<SocketAddr, TransportError> {
        self.endpoint
            .local_addr()
            .map_err(|error| TransportError::Protocol {
                reason: format!("failed to read QUIC endpoint address: {error}"),
            })
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

impl TransportConnection for QuicConnection {
    type Stream = QuicStream;

    fn connection_id(&self) -> &ConnectionId {
        &self.connection_id
    }

    fn remote_peer(&self) -> &PeerId {
        &self.remote_peer
    }

    fn open_stream(&mut self) -> Result<Self::Stream, TransportError> {
        self.ensure_open()?;
        let connection_id = self.connection_id.clone();
        let (send, recv) =
            self.runtime.block_on(async {
                let (mut send, recv) = self.connection.open_bi().await.map_err(|error| {
                    TransportError::StreamFailed {
                        connection_id: connection_id.clone(),
                        stream_id: StreamId("quic-stream-pending".to_owned()),
                        reason: format!("failed to open QUIC stream: {error}"),
                    }
                })?;
                send.write_all(STREAM_PREFACE).await.map_err(|error| {
                    TransportError::StreamFailed {
                        connection_id: connection_id.clone(),
                        stream_id: quic_stream_id(send.id()),
                        reason: format!("failed to write QUIC stream preface: {error}"),
                    }
                })?;
                Ok::<_, TransportError>((send, recv))
            })?;
        let stream_id = quic_stream_id(send.id());

        send_event(
            &self.event_tx,
            TransportEvent::StreamOpened {
                connection_id: self.connection_id.clone(),
                stream_id: stream_id.clone(),
            },
            &self.connection_id,
        )?;

        Ok(QuicStream {
            stream_id,
            connection_id: self.connection_id.clone(),
            send,
            recv,
            runtime: self.runtime.clone(),
            event_tx: self.event_tx.clone(),
            closed: false,
        })
    }

    fn accept_stream(&mut self) -> Result<Self::Stream, TransportError> {
        self.ensure_open()?;
        let connection_id = self.connection_id.clone();
        let (send, recv) = self.runtime.block_on(async {
            let (send, mut recv) = self.connection.accept_bi().await.map_err(|error| {
                TransportError::StreamFailed {
                    connection_id: connection_id.clone(),
                    stream_id: StreamId("quic-stream-pending".to_owned()),
                    reason: format!("failed to accept QUIC stream: {error}"),
                }
            })?;
            let stream_id = quic_stream_id(send.id());
            read_stream_preface(&mut recv, &connection_id, &stream_id).await?;
            Ok::<_, TransportError>((send, recv))
        })?;
        let stream_id = quic_stream_id(send.id());

        send_event(
            &self.event_tx,
            TransportEvent::StreamOpened {
                connection_id: self.connection_id.clone(),
                stream_id: stream_id.clone(),
            },
            &self.connection_id,
        )?;

        Ok(QuicStream {
            stream_id,
            connection_id: self.connection_id.clone(),
            send,
            recv,
            runtime: self.runtime.clone(),
            event_tx: self.event_tx.clone(),
            closed: false,
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
        self.connection.close(0u32.into(), b"nexo quic close");
        send_event(
            &self.event_tx,
            TransportEvent::Closed {
                connection_id: self.connection_id.clone(),
            },
            &self.connection_id,
        )
    }
}

#[derive(Debug)]
pub struct QuicStream {
    stream_id: StreamId,
    connection_id: ConnectionId,
    send: SendStream,
    recv: RecvStream,
    runtime: Arc<tokio::runtime::Runtime>,
    event_tx: mpsc::Sender<TransportEvent>,
    closed: bool,
}

impl QuicStream {
    fn ensure_open(&self) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::StreamFailed {
                connection_id: self.connection_id.clone(),
                stream_id: self.stream_id.clone(),
                reason: "QUIC stream is closed".to_owned(),
            });
        }

        Ok(())
    }
}

impl TransportStream for QuicStream {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn send_message(&mut self, envelope: MessageEnvelope) -> Result<(), TransportError> {
        self.ensure_open()?;
        let frame =
            bincode::serialize(&envelope).map_err(|error| TransportError::MessageRejected {
                reason: format!("failed to encode transfer message: {error}"),
            })?;
        self.runtime.block_on(write_frame(
            &mut self.send,
            &frame,
            &self.connection_id,
            &self.stream_id,
        ))?;
        send_event(
            &self.event_tx,
            TransportEvent::MessageSent {
                connection_id: self.connection_id.clone(),
                stream_id: self.stream_id.clone(),
                envelope,
            },
            &self.connection_id,
        )
    }

    fn receive_message(&mut self) -> Result<MessageEnvelope, TransportError> {
        self.ensure_open()?;
        let frame = self.runtime.block_on(read_frame(
            &mut self.recv,
            &self.connection_id,
            &self.stream_id,
        ))?;
        let envelope: MessageEnvelope =
            bincode::deserialize(&frame).map_err(|error| TransportError::MessageRejected {
                reason: format!("failed to decode transfer message: {error}"),
            })?;
        send_event(
            &self.event_tx,
            TransportEvent::MessageReceived {
                connection_id: self.connection_id.clone(),
                stream_id: self.stream_id.clone(),
                envelope: envelope.clone(),
            },
            &self.connection_id,
        )?;

        Ok(envelope)
    }

    fn close(&mut self) -> Result<(), TransportError> {
        if self.closed {
            return Ok(());
        }

        self.closed = true;
        self.send
            .finish()
            .map_err(|error| TransportError::StreamFailed {
                connection_id: self.connection_id.clone(),
                stream_id: self.stream_id.clone(),
                reason: format!("failed to finish QUIC stream: {error}"),
            })?;
        send_event(
            &self.event_tx,
            TransportEvent::StreamClosed {
                connection_id: self.connection_id.clone(),
                stream_id: self.stream_id.clone(),
            },
            &self.connection_id,
        )
    }
}

fn new_runtime() -> Result<Arc<tokio::runtime::Runtime>, TransportError> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("nexo-quic")
        .build()
        .map(Arc::new)
        .map_err(|error| TransportError::Protocol {
            reason: format!("failed to create QUIC runtime: {error}"),
        })
}

fn server_config_from_identity(
    identity: &QuicServerIdentity,
) -> Result<ServerConfig, TransportError> {
    let cert_der = CertificateDer::from(identity.certificate_der.clone());
    let private_key = PrivatePkcs8KeyDer::from(identity.private_key_der.clone());
    ServerConfig::with_single_cert(vec![cert_der], private_key.into()).map_err(|error| {
        TransportError::Protocol {
            reason: format!("failed to configure QUIC server certificate: {error}"),
        }
    })
}

async fn send_peer_handshake(
    connection: &quinn::Connection,
    peer_id: &PeerId,
    session_id: &SessionId,
    connection_id: &ConnectionId,
) -> Result<(), TransportError> {
    let mut send =
        connection
            .open_uni()
            .await
            .map_err(|error| TransportError::ConnectionFailed {
                connection_id: Some(connection_id.clone()),
                reason: format!("failed to open QUIC peer handshake stream: {error}"),
            })?;
    let payload = bincode::serialize(&(peer_id.clone(), session_id.clone())).map_err(|error| {
        TransportError::MessageRejected {
            reason: format!("failed to encode QUIC peer handshake: {error}"),
        }
    })?;
    let stream_id = quic_stream_id(send.id());
    write_frame(&mut send, &payload, connection_id, &stream_id).await?;
    send.finish().map_err(|error| TransportError::StreamFailed {
        connection_id: connection_id.clone(),
        stream_id,
        reason: format!("failed to finish QUIC peer handshake stream: {error}"),
    })
}

async fn receive_peer_handshake(
    connection: &quinn::Connection,
    connection_id: &ConnectionId,
) -> Result<(PeerId, SessionId), TransportError> {
    let mut recv =
        connection
            .accept_uni()
            .await
            .map_err(|error| TransportError::ConnectionFailed {
                connection_id: Some(connection_id.clone()),
                reason: format!("failed to accept QUIC peer handshake stream: {error}"),
            })?;
    let stream_id = quic_stream_id(recv.id());
    let payload = read_frame(&mut recv, connection_id, &stream_id).await?;
    bincode::deserialize(&payload).map_err(|error| TransportError::MessageRejected {
        reason: format!("failed to decode QUIC peer handshake: {error}"),
    })
}

async fn read_stream_preface(
    recv: &mut RecvStream,
    connection_id: &ConnectionId,
    stream_id: &StreamId,
) -> Result<(), TransportError> {
    let mut preface = [0u8; STREAM_PREFACE.len()];
    recv.read_exact(&mut preface)
        .await
        .map_err(|error| TransportError::StreamFailed {
            connection_id: connection_id.clone(),
            stream_id: stream_id.clone(),
            reason: format!("failed to read QUIC stream preface: {error}"),
        })?;

    if &preface != STREAM_PREFACE {
        return Err(TransportError::Protocol {
            reason: "invalid QUIC stream preface".to_owned(),
        });
    }

    Ok(())
}

async fn write_frame(
    send: &mut SendStream,
    payload: &[u8],
    connection_id: &ConnectionId,
    stream_id: &StreamId,
) -> Result<(), TransportError> {
    if payload.len() > MAX_FRAME_SIZE {
        return Err(TransportError::MessageRejected {
            reason: format!(
                "QUIC message frame exceeds maximum size: {} > {}",
                payload.len(),
                MAX_FRAME_SIZE
            ),
        });
    }

    send.write_all(&(payload.len() as u32).to_be_bytes())
        .await
        .map_err(|error| TransportError::StreamFailed {
            connection_id: connection_id.clone(),
            stream_id: stream_id.clone(),
            reason: format!("failed to write QUIC frame length: {error}"),
        })?;
    send.write_all(payload)
        .await
        .map_err(|error| TransportError::StreamFailed {
            connection_id: connection_id.clone(),
            stream_id: stream_id.clone(),
            reason: format!("failed to write QUIC frame payload: {error}"),
        })?;
    send.flush()
        .await
        .map_err(|error| TransportError::StreamFailed {
            connection_id: connection_id.clone(),
            stream_id: stream_id.clone(),
            reason: format!("failed to flush QUIC frame: {error}"),
        })
}

async fn read_frame(
    recv: &mut RecvStream,
    connection_id: &ConnectionId,
    stream_id: &StreamId,
) -> Result<Vec<u8>, TransportError> {
    let mut length = [0u8; 4];
    recv.read_exact(&mut length)
        .await
        .map_err(|error| TransportError::StreamFailed {
            connection_id: connection_id.clone(),
            stream_id: stream_id.clone(),
            reason: format!("failed to read QUIC frame length: {error}"),
        })?;
    let length = u32::from_be_bytes(length) as usize;
    if length > MAX_FRAME_SIZE {
        return Err(TransportError::MessageRejected {
            reason: format!("QUIC message frame exceeds maximum size: {length} > {MAX_FRAME_SIZE}"),
        });
    }

    let mut payload = vec![0u8; length];
    recv.read_exact(&mut payload)
        .await
        .map_err(|error| TransportError::StreamFailed {
            connection_id: connection_id.clone(),
            stream_id: stream_id.clone(),
            reason: format!("failed to read QUIC frame payload: {error}"),
        })?;

    Ok(payload)
}

fn send_event(
    event_tx: &mpsc::Sender<TransportEvent>,
    event: TransportEvent,
    connection_id: &ConnectionId,
) -> Result<(), TransportError> {
    event_tx
        .send(event)
        .map_err(|_| TransportError::ConnectionClosed {
            connection_id: connection_id.clone(),
        })
}

fn quic_connection_id(connection: &quinn::Connection) -> ConnectionId {
    ConnectionId(format!("quic-connection-{}", connection.stable_id()))
}

fn quic_stream_id(stream_id: quinn::StreamId) -> StreamId {
    StreamId(format!("quic-stream-{}", u64::from(stream_id)))
}
