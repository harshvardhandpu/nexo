use common::{
    Chunk, ChunkId, FileManifest, MessageEnvelope, PeerId, SessionId, TransferAcceptance,
    TransferChunkMessage, TransferId, TransferMessage, TransferResponse, TransferSessionMessage,
    TransportEvent,
};
use networking::{
    QuicConnection, QuicListener, QuicTransportProvider, QuicTransportTuning, TransportConnection,
    TransportListener, TransportProvider, TransportStream,
};
use std::time::Duration;

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

#[test]
fn restarted_listener_reuses_persisted_identity_and_address() {
    // Reproduces the resume precondition: a receiver advertises a certificate and
    // address, then "restarts". With a persisted identity bound to the same
    // address, a sender that only ever learned the original certificate and
    // address must still be able to reconnect after the restart.
    let identity = QuicTransportProvider::generate_server_identity().expect("server identity");

    let first_addr = {
        let mut receiver =
            QuicTransportProvider::localhost(peer_b()).expect("receiver QUIC provider");
        let listener = receiver
            .listen_with_identity(&identity)
            .expect("first listener");
        assert_eq!(
            listener.certificate_der(),
            identity.certificate_der.as_slice()
        );
        listener.local_addr()
    };

    // Rebind a brand-new provider to the same address using the same identity,
    // exactly as a restarted `nexo receive` does.
    let mut receiver = QuicTransportProvider::new(peer_b(), first_addr).expect("rebound provider");
    let mut listener = receiver
        .listen_with_identity(&identity)
        .expect("rebound listener");
    assert_eq!(listener.local_addr(), first_addr);
    assert_eq!(
        listener.certificate_der(),
        identity.certificate_der.as_slice()
    );

    // The sender only ever trusted the original certificate and address.
    let mut sender = QuicTransportProvider::localhost(peer_a()).expect("sender QUIC provider");
    sender.register_peer(peer_b(), first_addr, identity.certificate_der.clone());
    let sender_thread = std::thread::spawn(move || sender.connect(&peer_b(), session_id()));
    let receiver_connection = listener
        .accept()
        .expect("receiver connection after restart");
    let sender_connection = sender_thread
        .join()
        .expect("sender thread")
        .expect("sender connection after restart");

    assert_eq!(sender_connection.remote_peer(), &peer_b());
    assert_eq!(receiver_connection.remote_peer(), &peer_a());
}

#[test]
fn transport_events_do_not_accumulate_without_draining() {
    // Regression for the large-transfer OOM: the transfer path never drains
    // connection events, and MessageSent/MessageReceived each retain a full
    // chunk payload. With an unbounded event queue, moving N messages without
    // draining retained N payloads (~N * chunk_size bytes), exhausting memory
    // around chunk ~700 of a 5 GB transfer. The event buffer must stay bounded.
    let (mut sender_connection, mut receiver_connection) = connected_pair();
    let mut sender_stream = sender_connection.open_stream().expect("sender stream");
    let mut receiver_stream = receiver_connection
        .accept_stream()
        .expect("receiver stream");

    // Move far more messages than any reasonable event buffer, never draining
    // events. Each must still send/receive correctly.
    let total = 500u64;
    for chunk_id in 0..total {
        let envelope = chunk_envelope(chunk_id);
        sender_stream
            .send_message(envelope.clone())
            .expect("send message");
        let received = receiver_stream.receive_message().expect("receive message");
        assert_eq!(received, envelope);
    }

    // Drain every event still buffered on each connection. A bounded buffer caps
    // how many remain regardless of how many messages were moved; an unbounded
    // buffer would have retained one event per message.
    let drained = |connection: &mut QuicConnection| -> usize {
        let mut count = 0;
        while let Some(_event) = connection.try_next_event().expect("try_next_event") {
            count += 1;
        }
        count
    };
    let sender_events = drained(&mut sender_connection);
    let receiver_events = drained(&mut receiver_connection);

    assert!(
        sender_events < total as usize,
        "sender retained {sender_events} events for {total} messages; queue is not bounded"
    );
    assert!(
        receiver_events < total as usize,
        "receiver retained {receiver_events} events for {total} messages; queue is not bounded"
    );
}

#[test]
#[ignore = "slow (35s): validates the real production tuning against Quinn's 30s default idle timeout; run with `--ignored`"]
fn quic_transport_keeps_connection_alive_across_long_application_idle_gap() {
    // Regression for large transfers: after the last chunk, the receiver may
    // spend more than Quinn's default 30 second idle timeout verifying a large
    // destination file before sending the final verification frame. The QUIC
    // transport must keep the connection alive during that application-level
    // quiet period so the next length-prefixed frame can still be exchanged.
    //
    // This exercises the *real* production tuning end to end, so it must idle
    // past the 30s default. The deterministic, sub-second version of this proof
    // (with a negative control) is
    // `quic_connection_survives_idle_gap_only_with_keep_alive`.
    let (mut sender_connection, mut receiver_connection) = connected_pair();
    let mut sender_stream = sender_connection.open_stream().expect("sender stream");
    let mut receiver_stream = receiver_connection
        .accept_stream()
        .expect("receiver stream");
    let before_idle = chunk_envelope(1);
    let after_idle = accepted_envelope();

    sender_stream
        .send_message(before_idle.clone())
        .expect("send first message");
    assert_eq!(
        receiver_stream
            .receive_message()
            .expect("receive first message"),
        before_idle
    );

    std::thread::sleep(Duration::from_secs(35));

    receiver_stream
        .send_message(after_idle.clone())
        .expect("send after idle gap");
    assert_eq!(
        sender_stream
            .receive_message()
            .expect("receive after idle gap"),
        after_idle
    );
}

#[test]
fn quic_connection_survives_idle_gap_only_with_keep_alive() {
    // Deterministic, sub-second proof of the large-transfer failure and its fix.
    //
    // Both peers use a short idle timeout and idle (no packets in either
    // direction) for longer than it — the small-scale analog of the receiver's
    // multi-minute whole-file verification during which neither side transmits.
    //
    // Negative control: with keep-alive DISABLED (Quinn's own default), the
    // connection is torn down and the next frame fails with exactly the observed
    // symptom, "connection lost".
    //
    // Fix: with keep-alive ENABLED and comfortably below the idle timeout, the
    // driver PINGs across the gap on the runtime worker threads and the frame is
    // exchanged normally.
    let idle_timeout = Duration::from_millis(600);
    let idle_gap = Duration::from_millis(1_800);

    // --- Negative control: no keep-alive -> connection is lost across the gap.
    let without_keep_alive = QuicTransportTuning {
        keep_alive_interval: None,
        max_idle_timeout: idle_timeout,
    };
    let (mut sender_connection, mut receiver_connection) = tuned_connected_pair(without_keep_alive);
    let mut sender_stream = sender_connection.open_stream().expect("sender stream");
    let mut receiver_stream = receiver_connection
        .accept_stream()
        .expect("receiver stream");
    sender_stream
        .send_message(chunk_envelope(1))
        .expect("send before idle gap");
    receiver_stream
        .receive_message()
        .expect("receive before idle gap");

    std::thread::sleep(idle_gap);

    let error = sender_stream
        .send_message(chunk_envelope(2))
        .expect_err("connection must be lost after idling past the timeout without keep-alive");
    assert!(
        error.to_string().contains("connection lost"),
        "expected an idle-timeout 'connection lost', got: {error}"
    );

    // --- Fix: keep-alive keeps the same-length gap alive.
    let with_keep_alive = QuicTransportTuning {
        keep_alive_interval: Some(Duration::from_millis(120)),
        max_idle_timeout: idle_timeout,
    };
    let (mut sender_connection, mut receiver_connection) = tuned_connected_pair(with_keep_alive);
    let mut sender_stream = sender_connection.open_stream().expect("sender stream");
    let mut receiver_stream = receiver_connection
        .accept_stream()
        .expect("receiver stream");
    sender_stream
        .send_message(chunk_envelope(1))
        .expect("send before idle gap");
    receiver_stream
        .receive_message()
        .expect("receive before idle gap");

    std::thread::sleep(idle_gap);

    let after_gap = accepted_envelope();
    receiver_stream
        .send_message(after_gap.clone())
        .expect("keep-alive must hold the connection open across the idle gap");
    assert_eq!(
        sender_stream
            .receive_message()
            .expect("receive after idle gap"),
        after_gap
    );
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

fn tuned_connected_pair(tuning: QuicTransportTuning) -> (QuicConnection, QuicConnection) {
    let mut receiver = QuicTransportProvider::localhost(peer_b()).expect("receiver QUIC provider");
    receiver.set_transport_tuning(tuning);
    let mut listener = receiver.listen().expect("receiver QUIC listener");
    let mut sender = QuicTransportProvider::localhost(peer_a()).expect("sender QUIC provider");
    sender.set_transport_tuning(tuning);
    sender.register_peer(
        peer_b(),
        listener.local_addr(),
        listener.certificate_der().to_vec(),
    );

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
