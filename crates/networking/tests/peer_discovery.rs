use common::PeerId;
use networking::{
    DiscoveryEvent, LocalDiscoveryProvider, PeerAdvertisement, PeerDiscovery, PeerInfo,
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[test]
#[ignore = "requires reliable local mDNS multicast loopback"]
fn local_discovery_advertises_discovers_expires_and_lists_multiple_peers() {
    let run_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_nanos();
    let observer_id = PeerId(format!("observer-{run_id}"));
    let laptop_id = PeerId(format!("laptop-{run_id}"));
    let desktop_id = PeerId(format!("desktop-{run_id}"));

    let mut laptop = provider_with_fingerprint(
        laptop_id.clone(),
        "Harsh-Laptop",
        41001,
        "AAAA:BBBB:CCCC:DDDD",
    );
    let mut desktop = provider(desktop_id.clone(), "Desktop-PC", 41002);
    let mut observer = provider(observer_id, "Observer", 41000);

    let laptop_info = wait_for_peer(&mut observer, &laptop_id);
    let desktop_info = wait_for_peer(&mut observer, &desktop_id);

    assert_eq!(laptop_info.display_name, "Harsh-Laptop");
    assert_eq!(laptop_info.port, 41001);
    assert!(!laptop_info.addresses.is_empty());
    // The certificate fingerprint advertised for pairing must round-trip over
    // real mDNS so the desktop pairing flow can read it.
    assert_eq!(
        laptop_info.fingerprint.as_deref(),
        Some("AAAA:BBBB:CCCC:DDDD")
    );
    // The full certificate (chunked across TXT records) must also round-trip so
    // the desktop can store it and later connect to this peer.
    assert_eq!(laptop_info.certificate_der, Some(sample_certificate()));
    assert_eq!(desktop_info.display_name, "Desktop-PC");
    assert_eq!(desktop_info.port, 41002);
    // A peer that advertises no fingerprint resolves with `None`.
    assert_eq!(desktop_info.fingerprint, None);
    assert_eq!(desktop_info.certificate_der, None);

    let visible = observer.peers();
    assert!(visible.iter().any(|peer| peer.peer_id == laptop_id));
    assert!(visible.iter().any(|peer| peer.peer_id == desktop_id));

    laptop.shutdown().expect("stop laptop advertisement");
    let expired = wait_for_expiration("laptop removal", &mut observer, &laptop_id);
    assert_eq!(expired.display_name, "Harsh-Laptop");
    assert!(
        !observer
            .peers()
            .iter()
            .any(|peer| peer.peer_id == laptop_id)
    );
    assert!(
        observer
            .peers()
            .iter()
            .any(|peer| peer.peer_id == desktop_id)
    );

    desktop.shutdown().expect("stop desktop advertisement");
    observer.shutdown().expect("stop observer discovery");

    let timeout_observer_id = PeerId(format!("timeout-observer-{run_id}"));
    let timed_peer_id = PeerId(format!("timed-peer-{run_id}"));
    let mut timed_peer = provider(timed_peer_id.clone(), "SteamDeck", 42001);
    let mut timeout_observer = provider_with_timeout(
        timeout_observer_id,
        "Timeout Observer",
        42000,
        Duration::from_millis(400),
    );

    wait_for_peer(&mut timeout_observer, &timed_peer_id);
    let timed_out = wait_for_expiration("cache timeout", &mut timeout_observer, &timed_peer_id);
    assert_eq!(timed_out.display_name, "SteamDeck");

    timed_peer.shutdown().expect("stop timed peer");
    timeout_observer.shutdown().expect("stop timeout observer");
}

fn provider(peer_id: PeerId, display_name: &str, port: u16) -> LocalDiscoveryProvider {
    provider_with_timeout(peer_id, display_name, port, Duration::from_secs(10))
}

/// A ~355-byte stand-in for a DER certificate, large enough to force multi-chunk
/// TXT encoding (exercising the reassembly path over real mDNS).
fn sample_certificate() -> Vec<u8> {
    (0..355u16)
        .map(|i| (i.wrapping_mul(13) % 256) as u8)
        .collect()
}

fn provider_with_fingerprint(
    peer_id: PeerId,
    display_name: &str,
    port: u16,
    fingerprint: &str,
) -> LocalDiscoveryProvider {
    LocalDiscoveryProvider::with_peer_timeout(
        PeerAdvertisement::new(peer_id, display_name, port)
            .with_fingerprint(fingerprint)
            .with_certificate(sample_certificate()),
        Duration::from_secs(10),
    )
    .expect("local discovery provider")
}

fn provider_with_timeout(
    peer_id: PeerId,
    display_name: &str,
    port: u16,
    timeout: Duration,
) -> LocalDiscoveryProvider {
    LocalDiscoveryProvider::with_peer_timeout(
        PeerAdvertisement::new(peer_id, display_name, port),
        timeout,
    )
    .expect("local discovery provider")
}

fn wait_for_peer(provider: &mut LocalDiscoveryProvider, peer_id: &PeerId) -> PeerInfo {
    if let Some(peer) = provider
        .peers()
        .into_iter()
        .find(|peer| peer.peer_id == *peer_id)
    {
        return peer;
    }

    wait_for_event("peer discovery", provider, |event| match event {
        DiscoveryEvent::PeerDiscovered(peer) | DiscoveryEvent::PeerUpdated(peer)
            if &peer.peer_id == peer_id =>
        {
            Some(peer)
        }
        _ => None,
    })
}

fn wait_for_expiration(
    label: &'static str,
    provider: &mut LocalDiscoveryProvider,
    peer_id: &PeerId,
) -> PeerInfo {
    wait_for_event(label, provider, |event| match event {
        DiscoveryEvent::PeerExpired(peer) if &peer.peer_id == peer_id => Some(peer),
        _ => None,
    })
}

fn wait_for_event<F>(
    label: &'static str,
    provider: &mut LocalDiscoveryProvider,
    mut match_event: F,
) -> PeerInfo
where
    F: FnMut(DiscoveryEvent) -> Option<PeerInfo>,
{
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if let Some(event) = provider
            .next_event(Duration::from_millis(1))
            .expect("discovery event")
        {
            if let Some(peer) = match_event(event.clone()) {
                return peer;
            }
        } else {
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    panic!("timed out waiting for discovery event: {label}");
}
