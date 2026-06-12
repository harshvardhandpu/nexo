use common::{
    Chunk, ChunkId, FileManifest, MessageEnvelope, PeerId, SessionId, TransferAcceptance,
    TransferChunkMessage, TransferId, TransferMessage, TransferResponse, TransferSessionMessage,
    TransportEvent,
};
use networking::{
    QuicConnection, QuicListener, QuicTransportProvider, TransportConnection, TransportListener,
    TransportProvider, TransportStream,
};

#[test]
fn quic_transport_connects_over_localhost() {
    let (sender_connection, receiver_connection) = connected_pair();

    assert_eq!(sender_connection.remote_peer(), &peer_b());
    assert_eq!(receiver_connection.remote_peer(), &peer_a());
}

#[test]
fn quic_streams_deliver_bidirectional_messages() {
    let (mut sender_connection, mut receiver_connection) = connected_pair();
    let mut sender_stream = sender_connection.open_stream().expect("sender stream");
    let mut receiver_stream = receiver_connection
        .accept_stream()
        .expect("receiver stream");
    let request = request_envelope();
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
fn quic_transport_supports_multiple_streams() {
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
fn quic_transport_delivers_transfer_messages() {
    let (mut sender_connection, mut receiver_connection) = connected_pair();
    let mut sender_stream = sender_connection.open_stream().expect("sender stream");
    let mut receiver_stream = receiver_connection
        .accept_stream()
        .expect("receiver stream");
    let envelope = chunk_envelope(7);

    sender_stream
        .send_message(envelope.clone())
        .expect("send transfer message");

    assert_eq!(
        receiver_stream
            .receive_message()
            .expect("receive transfer message"),
        envelope
    );
}

#[test]
fn quic_transport_generates_events() {
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
            .expect("receiver connected event"),
        TransportEvent::Connected { .. }
    ));

    let mut sender_stream = sender_connection.open_stream().expect("sender stream");
    let mut receiver_stream = receiver_connection
        .accept_stream()
        .expect("receiver stream");
    let envelope = request_envelope();

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
        sender_connection.next_event().expect("message sent event"),
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
        receiver_connection
            .next_event()
            .expect("message received event"),
        TransportEvent::MessageReceived {
            envelope: received,
            ..
        } if received == envelope
    ));
}

fn connected_pair() -> (QuicConnection, QuicConnection) {
    let (mut sender, mut listener) = quic_pair();
    let sender_thread = std::thread::spawn(move || sender.connect(&peer_b(), session_id()));
    let receiver_connection = listener.accept().expect("receiver connection");
    let sender_connection = sender_thread
        .join()
        .expect("sender thread")
        .expect("sender connection");

    (sender_connection, receiver_connection)
}

fn quic_pair() -> (QuicTransportProvider, QuicListener) {
    let mut receiver = QuicTransportProvider::localhost(peer_b()).expect("receiver QUIC provider");
    let listener = receiver.listen().expect("receiver QUIC listener");
    let mut sender = QuicTransportProvider::localhost(peer_a()).expect("sender QUIC provider");

    sender.register_peer(
        peer_b(),
        listener.local_addr(),
        listener.certificate_der().to_vec(),
    );

    (sender, listener)
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
        message: TransferMessage::Chunk(TransferChunkMessage::Data(Chunk {
            id: ChunkId(chunk_id),
            offset: chunk_id * 4,
            size: 4,
            data: format!("data{chunk_id}").into_bytes(),
        })),
    }
}
