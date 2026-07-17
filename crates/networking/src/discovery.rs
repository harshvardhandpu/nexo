use common::PeerId;
use mdns_sd::{Receiver, ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::{HashMap, VecDeque};
use std::io::{Error, ErrorKind, Result};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::time::{Duration, Instant};

pub const NEXO_SERVICE_TYPE: &str = "_nexo._udp.local.";
pub const DEFAULT_PEER_TIMEOUT: Duration = Duration::from_secs(180);

const DISCOVERY_VERSION: &str = "1";
const PEER_ID_PROPERTY: &str = "peer_id";
const DISPLAY_NAME_PROPERTY: &str = "display_name";
const VERSION_PROPERTY: &str = "version";
/// TXT property carrying the SHA-256 fingerprint of the peer's QUIC certificate.
/// Used by the desktop pairing flow to show the user a verifiable fingerprint;
/// it is advertised by the peer itself, so trust still requires explicit
/// user confirmation (never granted from discovery alone).
const FINGERPRINT_PROPERTY: &str = "fingerprint";
/// TXT property holding the number of certificate chunks (`cert0`..`certN-1`).
const CERTIFICATE_CHUNKS_PROPERTY: &str = "certn";
/// Prefix for certificate chunk properties. The peer's DER certificate is
/// hex-encoded and split across `cert0`, `cert1`, … because a single mDNS TXT
/// value is capped at 255 bytes (RFC 6763) while a cert is ~700 hex chars.
/// Publishing the certificate lets a pairing peer store it and later connect
/// (the QUIC client pins it) without any out-of-band cert exchange.
const CERTIFICATE_CHUNK_PREFIX: &str = "cert";
/// Hex chars per certificate chunk. Stays well under the 255-byte TXT limit.
const CERTIFICATE_CHUNK_LEN: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAdvertisement {
    pub peer_id: PeerId,
    pub display_name: String,
    pub port: u16,
    /// Optional certificate fingerprint to publish for pairing. `None` omits the
    /// TXT field (older peers / no identity available).
    pub fingerprint: Option<String>,
    /// Optional DER certificate to publish for pairing. When present it is
    /// hex-encoded and chunked across TXT records so a pairing peer can store it
    /// and later establish a QUIC connection (which pins this exact cert).
    pub certificate_der: Option<Vec<u8>>,
}

impl PeerAdvertisement {
    pub fn new(peer_id: PeerId, display_name: impl Into<String>, port: u16) -> Self {
        Self {
            peer_id,
            display_name: display_name.into(),
            port,
            fingerprint: None,
            certificate_der: None,
        }
    }

    /// Sets the certificate fingerprint advertised for pairing.
    pub fn with_fingerprint(mut self, fingerprint: impl Into<String>) -> Self {
        self.fingerprint = Some(fingerprint.into());
        self
    }

    /// Sets the DER certificate advertised for pairing.
    pub fn with_certificate(mut self, certificate_der: impl Into<Vec<u8>>) -> Self {
        self.certificate_der = Some(certificate_der.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub display_name: String,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
    /// The peer's advertised certificate fingerprint, if it published one.
    pub fingerprint: Option<String>,
    /// The peer's advertised DER certificate, reassembled from TXT chunks. Used
    /// by the desktop pairing flow to store a trusted peer's certificate so it
    /// can later be sent to. `None` when the peer published no certificate.
    pub certificate_der: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscoveryEvent {
    PeerDiscovered(PeerInfo),
    PeerUpdated(PeerInfo),
    PeerExpired(PeerInfo),
}

pub trait PeerDiscovery {
    fn next_event(&mut self, timeout: Duration) -> Result<Option<DiscoveryEvent>>;
    fn peers(&self) -> Vec<PeerInfo>;
    fn shutdown(&mut self) -> Result<()>;
}

#[derive(Debug)]
struct TrackedPeer {
    info: PeerInfo,
    fullname: String,
    last_seen: Instant,
}

pub struct LocalDiscoveryProvider {
    daemon: ServiceDaemon,
    events: Receiver<ServiceEvent>,
    service_fullname: String,
    cache: PeerCache,
    stopped: bool,
}

#[derive(Debug, Clone)]
struct ResolvedPeer {
    info: PeerInfo,
    fullname: String,
}

#[derive(Debug)]
struct PeerCache {
    local_peer: PeerId,
    peer_timeout: Duration,
    peers: HashMap<PeerId, TrackedPeer>,
    peers_by_fullname: HashMap<String, PeerId>,
    pending: VecDeque<DiscoveryEvent>,
}

impl PeerCache {
    fn new(local_peer: PeerId, peer_timeout: Duration) -> Self {
        Self {
            local_peer,
            peer_timeout,
            peers: HashMap::new(),
            peers_by_fullname: HashMap::new(),
            pending: VecDeque::new(),
        }
    }

    fn handle_resolved(&mut self, resolved: ResolvedPeer, now: Instant) {
        let peer_id = resolved.info.peer_id.clone();
        if peer_id == self.local_peer {
            return;
        }

        match self.peers.get_mut(&peer_id) {
            Some(tracked) => {
                let changed = tracked.info != resolved.info;
                if tracked.fullname != resolved.fullname {
                    self.peers_by_fullname.remove(&tracked.fullname);
                }
                tracked.info = resolved.info.clone();
                tracked.fullname = resolved.fullname.clone();
                tracked.last_seen = now;
                self.peers_by_fullname.insert(resolved.fullname, peer_id);
                if changed {
                    self.pending
                        .push_back(DiscoveryEvent::PeerUpdated(resolved.info));
                }
            }
            None => {
                self.peers_by_fullname
                    .insert(resolved.fullname.clone(), peer_id.clone());
                self.peers.insert(
                    peer_id,
                    TrackedPeer {
                        info: resolved.info.clone(),
                        fullname: resolved.fullname,
                        last_seen: now,
                    },
                );
                self.pending
                    .push_back(DiscoveryEvent::PeerDiscovered(resolved.info));
            }
        }
    }

    fn handle_removed(&mut self, fullname: &str) {
        if let Some(peer_id) = self.peers_by_fullname.remove(fullname)
            && let Some(peer) = self.peers.remove(&peer_id)
        {
            self.pending
                .push_back(DiscoveryEvent::PeerExpired(peer.info));
        }
    }

    fn expire_stale_peers(&mut self) {
        let expired = self
            .peers
            .iter()
            .filter(|(_, peer)| peer.last_seen.elapsed() >= self.peer_timeout)
            .map(|(peer_id, _)| peer_id.clone())
            .collect::<Vec<_>>();

        for peer_id in expired {
            if let Some(peer) = self.peers.remove(&peer_id) {
                self.peers_by_fullname.remove(&peer.fullname);
                self.pending
                    .push_back(DiscoveryEvent::PeerExpired(peer.info));
            }
        }
    }

    fn next_expiration(&self) -> Option<Duration> {
        self.peers
            .values()
            .map(|peer| self.peer_timeout.saturating_sub(peer.last_seen.elapsed()))
            .min()
    }

    fn pop_event(&mut self) -> Option<DiscoveryEvent> {
        self.pending.pop_front()
    }

    fn peers(&self) -> Vec<PeerInfo> {
        let mut peers = self
            .peers
            .values()
            .map(|peer| peer.info.clone())
            .collect::<Vec<_>>();
        peers.sort_by(|left, right| {
            left.display_name
                .cmp(&right.display_name)
                .then_with(|| left.peer_id.0.cmp(&right.peer_id.0))
        });
        peers
    }
}

impl LocalDiscoveryProvider {
    pub fn new(advertisement: PeerAdvertisement) -> Result<Self> {
        Self::with_peer_timeout(advertisement, DEFAULT_PEER_TIMEOUT)
    }

    pub fn with_peer_timeout(
        advertisement: PeerAdvertisement,
        peer_timeout: Duration,
    ) -> Result<Self> {
        validate_advertisement(&advertisement)?;
        if peer_timeout.is_zero() {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "peer timeout must be greater than zero",
            ));
        }

        let daemon = ServiceDaemon::new().map_err(discovery_error)?;
        let events = daemon.browse(NEXO_SERVICE_TYPE).map_err(discovery_error)?;
        let service = build_service_info(&advertisement)?;
        let service_fullname = service.get_fullname().to_owned();
        daemon.register(service).map_err(discovery_error)?;

        Ok(Self {
            daemon,
            events,
            service_fullname,
            cache: PeerCache::new(advertisement.peer_id, peer_timeout),
            stopped: false,
        })
    }

    fn handle_service_event(&mut self, event: ServiceEvent) {
        match event {
            ServiceEvent::ServiceResolved(service) => {
                if let Some(resolved) = resolved_peer_from_service(&service) {
                    self.cache.handle_resolved(resolved, Instant::now());
                }
            }
            ServiceEvent::ServiceRemoved(_, fullname) => {
                self.cache.handle_removed(&fullname);
            }
            _ => {}
        }
    }

    fn stop(&mut self, wait_for_goodbye: bool) -> Result<()> {
        if self.stopped {
            return Ok(());
        }

        let unregister = self
            .daemon
            .unregister(&self.service_fullname)
            .map_err(discovery_error)?;
        if wait_for_goodbye {
            let _ = unregister.recv_timeout(Duration::from_millis(500));
        }
        self.daemon
            .stop_browse(NEXO_SERVICE_TYPE)
            .map_err(discovery_error)?;
        self.daemon.shutdown().map_err(discovery_error)?;
        self.stopped = true;
        Ok(())
    }
}

impl PeerDiscovery for LocalDiscoveryProvider {
    fn next_event(&mut self, timeout: Duration) -> Result<Option<DiscoveryEvent>> {
        if self.stopped {
            return Err(Error::new(
                ErrorKind::NotConnected,
                "local discovery provider is stopped",
            ));
        }

        let deadline = Instant::now()
            .checked_add(timeout)
            .unwrap_or_else(Instant::now);

        loop {
            self.cache.expire_stale_peers();
            if let Some(event) = self.cache.pop_event() {
                return Ok(Some(event));
            }

            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }

            let remaining = deadline.saturating_duration_since(now);
            let wait = self
                .cache
                .next_expiration()
                .map(|expiration| expiration.min(remaining))
                .unwrap_or(remaining);

            match self.events.recv_timeout(wait) {
                Ok(event) => self.handle_service_event(event),
                Err(flume::RecvTimeoutError::Timeout) => {}
                Err(error) => return Err(Error::other(error.to_string())),
            }
        }
    }

    fn peers(&self) -> Vec<PeerInfo> {
        self.cache.peers()
    }

    fn shutdown(&mut self) -> Result<()> {
        self.stop(true)
    }
}

impl Drop for LocalDiscoveryProvider {
    fn drop(&mut self) {
        let _ = self.stop(false);
    }
}

/// An advertise-only presence on the local network.
///
/// Registers this peer as a discoverable `_nexo._udp` service and unregisters it
/// on drop. Unlike [`LocalDiscoveryProvider`] it does **not** browse, so a
/// long-lived process (such as a receiver waiting for a transfer) can stay
/// discoverable without accumulating inbound discovery events. This is the
/// missing piece that lets a receiver appear in another peer's `discover`.
pub struct ServiceAdvertisement {
    daemon: ServiceDaemon,
    service_fullname: String,
    stopped: bool,
}

impl ServiceAdvertisement {
    /// Publishes `advertisement` on the local network until dropped.
    pub fn register(advertisement: PeerAdvertisement) -> Result<Self> {
        validate_advertisement(&advertisement)?;
        let daemon = ServiceDaemon::new().map_err(discovery_error)?;
        let service = build_service_info(&advertisement)?;
        let service_fullname = service.get_fullname().to_owned();
        daemon.register(service).map_err(discovery_error)?;

        Ok(Self {
            daemon,
            service_fullname,
            stopped: false,
        })
    }

    pub fn service_fullname(&self) -> &str {
        &self.service_fullname
    }

    pub fn shutdown(&mut self) -> Result<()> {
        self.stop(true)
    }

    fn stop(&mut self, wait_for_goodbye: bool) -> Result<()> {
        if self.stopped {
            return Ok(());
        }

        let unregister = self
            .daemon
            .unregister(&self.service_fullname)
            .map_err(discovery_error)?;
        if wait_for_goodbye {
            let _ = unregister.recv_timeout(Duration::from_millis(500));
        }
        self.daemon.shutdown().map_err(discovery_error)?;
        self.stopped = true;
        Ok(())
    }
}

impl Drop for ServiceAdvertisement {
    fn drop(&mut self) {
        let _ = self.stop(false);
    }
}

fn build_service_info(advertisement: &PeerAdvertisement) -> Result<ServiceInfo> {
    build_service_info_with(advertisement, &local_advertisable_addresses())
}

/// Builds the mDNS `ServiceInfo` from an advertisement and an explicit set of
/// addresses to publish.
///
/// Previously this used `enable_addr_auto()`, which makes mdns-sd attach *every*
/// interface address — including `127.0.0.1` / `::1`. Peers then discovered a
/// loopback endpoint they could never reach. Instead we publish the caller-
/// supplied addresses directly (already filtered to reachable, non-loopback
/// ones) so a discovered peer always exposes a routable LAN address.
///
/// Any loopback/unspecified entries are filtered out defensively here too. If no
/// advertisable address is available (e.g. an isolated CI host), we fall back to
/// `enable_addr_auto` so the service still registers — loopback is only ever
/// advertised as this last resort, which keeps localhost-only integration tests
/// working.
fn build_service_info_with(
    advertisement: &PeerAdvertisement,
    addresses: &[IpAddr],
) -> Result<ServiceInfo> {
    let properties = HashMap::from([
        (PEER_ID_PROPERTY.to_owned(), advertisement.peer_id.0.clone()),
        (
            DISPLAY_NAME_PROPERTY.to_owned(),
            advertisement.display_name.clone(),
        ),
        (VERSION_PROPERTY.to_owned(), DISCOVERY_VERSION.to_owned()),
    ]);
    let mut properties = properties;
    if let Some(fingerprint) = &advertisement.fingerprint {
        properties.insert(FINGERPRINT_PROPERTY.to_owned(), fingerprint.clone());
    }
    if let Some(certificate_der) = &advertisement.certificate_der {
        let hex = hex_encode(certificate_der);
        let chunks: Vec<&str> = split_chunks(&hex, CERTIFICATE_CHUNK_LEN);
        properties.insert(
            CERTIFICATE_CHUNKS_PROPERTY.to_owned(),
            chunks.len().to_string(),
        );
        for (index, chunk) in chunks.iter().enumerate() {
            properties.insert(
                format!("{CERTIFICATE_CHUNK_PREFIX}{index}"),
                (*chunk).to_owned(),
            );
        }
    }

    let advertisable: Vec<IpAddr> = addresses
        .iter()
        .copied()
        .filter(|address| is_advertisable_address(*address))
        .collect();

    let service = ServiceInfo::new(
        NEXO_SERVICE_TYPE,
        &advertisement.peer_id.0,
        &service_hostname(&advertisement.peer_id),
        advertisable.as_slice(),
        advertisement.port,
        properties,
    )
    .map_err(discovery_error)?;

    // Only enable automatic (all-interface) address selection when we found no
    // routable address to publish — otherwise loopback would leak back in.
    if advertisable.is_empty() {
        Ok(service.enable_addr_auto())
    } else {
        Ok(service)
    }
}

/// Reassembles a peer's DER certificate from its `certn` + `cert0..certN-1` TXT
/// chunks. Returns `None` if the peer published no certificate or the chunks are
/// incomplete/malformed (in which case pairing simply falls back to being
/// fingerprint-only for that peer).
fn reassemble_certificate(service: &ResolvedService) -> Option<Vec<u8>> {
    let count: usize = service
        .get_property_val_str(CERTIFICATE_CHUNKS_PROPERTY)?
        .parse()
        .ok()?;
    let mut hex = String::new();
    for index in 0..count {
        let chunk = service.get_property_val_str(&format!("{CERTIFICATE_CHUNK_PREFIX}{index}"))?;
        hex.push_str(chunk);
    }
    hex_decode(&hex)
}

/// Splits `text` into `chunk_len`-char slices (the last may be shorter).
fn split_chunks(text: &str, chunk_len: usize) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    let bytes = text.as_bytes();
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        let end = (start + chunk_len).min(bytes.len());
        chunks.push(&text[start..end]);
        start = end;
    }
    chunks
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap());
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap());
    }
    out
}

fn hex_decode(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let bytes = hex.as_bytes();
    let mut out = Vec::with_capacity(hex.len() / 2);
    let mut index = 0;
    while index < bytes.len() {
        let hi = (bytes[index] as char).to_digit(16)?;
        let lo = (bytes[index + 1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
        index += 2;
    }
    Some(out)
}

/// Non-loopback local addresses suitable for advertising to LAN peers, in
/// preference order (LAN/Wi-Fi IPv4 first). Loopback, link-local, and other
/// unreachable ranges are excluded.
fn local_advertisable_addresses() -> Vec<IpAddr> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };

    let mut addresses: Vec<IpAddr> = interfaces
        .into_iter()
        .filter(|interface| interface.is_oper_up() && !interface.is_loopback())
        .map(|interface| interface.ip())
        .filter(|address| is_advertisable_address(*address))
        .collect();

    // Prefer IPv4 (typical LAN/Wi-Fi) before IPv6, and de-duplicate.
    addresses.sort_by_key(|address| (address.is_ipv6(), address.to_string()));
    addresses.dedup();
    addresses
}

/// Whether an address may be advertised to peers: it must be routable on a
/// local network. Loopback (`127.0.0.1`, `::1`), unspecified, link-local,
/// broadcast, multicast, and documentation ranges are never advertised.
fn is_advertisable_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => is_advertisable_ipv4(v4),
        IpAddr::V6(v6) => is_advertisable_ipv6(v6),
    }
}

fn is_advertisable_ipv4(address: Ipv4Addr) -> bool {
    !(address.is_unspecified()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_broadcast()
        || address.is_multicast()
        || address.is_documentation())
}

fn is_advertisable_ipv6(address: Ipv6Addr) -> bool {
    // `is_unicast_link_local` is unstable on stable Rust; match the fe80::/10
    // link-local prefix directly.
    let is_link_local = (address.segments()[0] & 0xffc0) == 0xfe80;
    !(address.is_unspecified() || address.is_loopback() || address.is_multicast() || is_link_local)
}

fn validate_advertisement(advertisement: &PeerAdvertisement) -> Result<()> {
    if advertisement.peer_id.0.trim().is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "peer ID must not be empty",
        ));
    }
    if advertisement.display_name.trim().is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "peer display name must not be empty",
        ));
    }
    if advertisement.peer_id.0.len() > 200 || advertisement.display_name.len() > 200 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "peer ID and display name must be at most 200 bytes",
        ));
    }
    Ok(())
}

fn service_hostname(peer_id: &PeerId) -> String {
    let mut label = peer_id
        .0
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '-' {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>();
    label.truncate(50);
    let label = label.trim_matches('-');
    let label = if label.is_empty() { "peer" } else { label };
    format!("nexo-{label}.local.")
}

fn discovery_error(error: mdns_sd::Error) -> Error {
    Error::other(error.to_string())
}

fn resolved_peer_from_service(service: &ResolvedService) -> Option<ResolvedPeer> {
    let peer_id = service.get_property_val_str(PEER_ID_PROPERTY)?;
    if service.get_property_val_str(VERSION_PROPERTY) != Some(DISCOVERY_VERSION) {
        return None;
    }

    let peer_id = PeerId(peer_id.to_owned());
    let display_name = service
        .get_property_val_str(DISPLAY_NAME_PROPERTY)
        .filter(|name| !name.is_empty())
        .unwrap_or(&peer_id.0)
        .to_owned();
    let mut addresses = service
        .get_addresses()
        .iter()
        .map(|address| address.to_ip_addr())
        .collect::<Vec<_>>();
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty() {
        return None;
    }

    let fingerprint = service
        .get_property_val_str(FINGERPRINT_PROPERTY)
        .filter(|value| !value.is_empty())
        .map(str::to_owned);

    let certificate_der = reassemble_certificate(service);

    Some(ResolvedPeer {
        info: PeerInfo {
            peer_id,
            display_name,
            addresses,
            port: service.get_port(),
            fingerprint,
            certificate_der,
        },
        fullname: service.get_fullname().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn service_addresses(service: &ServiceInfo) -> Vec<IpAddr> {
        service.get_addresses().iter().copied().collect()
    }

    #[test]
    fn advertisement_uses_non_loopback_address() {
        // A receiver bound to a real LAN address must advertise that address so
        // discovered peers can connect back to it.
        let advertisement = PeerAdvertisement::new(PeerId("peer-1".to_owned()), "archlinux", 50038);
        let lan = IpAddr::V4(Ipv4Addr::new(172, 21, 209, 204));

        let service = build_service_info_with(&advertisement, &[lan]).expect("service info");
        let addresses = service_addresses(&service);

        assert_eq!(addresses, vec![lan]);
        assert!(
            addresses.iter().all(|address| !address.is_loopback()),
            "advertised addresses must not be loopback: {addresses:?}"
        );
        assert_eq!(service.get_port(), 50038);
    }

    #[test]
    fn localhost_is_filtered_from_service_info() {
        // Given both a loopback and a LAN address, only the LAN address is
        // advertised — 127.0.0.1 / ::1 must never reach discovery.
        let advertisement = PeerAdvertisement::new(PeerId("peer-2".to_owned()), "archlinux", 60897);
        let lan = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5));
        let inputs = [
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
            lan,
        ];

        let service = build_service_info_with(&advertisement, &inputs).expect("service info");
        let addresses = service_addresses(&service);

        assert!(addresses.contains(&lan), "LAN address must be advertised");
        assert!(
            !addresses.iter().any(|address| address.is_loopback()),
            "loopback must be filtered out: {addresses:?}"
        );
    }

    #[test]
    fn is_advertisable_rejects_loopback_and_unreachable_ranges() {
        // Reachable LAN addresses are advertisable.
        assert!(is_advertisable_address(IpAddr::V4(Ipv4Addr::new(
            172, 21, 209, 204
        ))));
        assert!(is_advertisable_address(IpAddr::V4(Ipv4Addr::new(
            192, 168, 1, 5
        ))));
        assert!(is_advertisable_address(IpAddr::V4(Ipv4Addr::new(
            10, 0, 0, 8
        ))));

        // Loopback and other unreachable ranges are not.
        assert!(!is_advertisable_address(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(!is_advertisable_address(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(!is_advertisable_address(IpAddr::V4(Ipv4Addr::UNSPECIFIED)));
        assert!(!is_advertisable_address(IpAddr::V4(Ipv4Addr::new(
            169, 254, 1, 1
        ))));
        assert!(!is_advertisable_address(IpAddr::V4(Ipv4Addr::new(
            224, 0, 0, 1
        ))));
        assert!(!is_advertisable_address(IpAddr::V4(Ipv4Addr::BROADCAST)));
        // fe80::/10 link-local IPv6.
        assert!(!is_advertisable_address(IpAddr::V6(
            "fe80::1".parse().unwrap()
        )));
    }

    #[test]
    fn advertised_fingerprint_is_published_in_txt() {
        // The certificate fingerprint a peer advertises for pairing must be
        // carried in the service's TXT record so a discovering peer can read it.
        let advertisement =
            PeerAdvertisement::new(PeerId("peer-fp".to_owned()), "archlinux", 50038)
                .with_fingerprint("AAAA:BBBB:CCCC:DDDD");
        let lan = IpAddr::V4(Ipv4Addr::new(172, 21, 209, 204));

        let service = build_service_info_with(&advertisement, &[lan]).expect("service info");

        assert_eq!(
            service.get_property_val_str(FINGERPRINT_PROPERTY),
            Some("AAAA:BBBB:CCCC:DDDD")
        );
    }

    #[test]
    fn no_fingerprint_omits_the_txt_field() {
        let advertisement = PeerAdvertisement::new(PeerId("peer-nofp".to_owned()), "host", 1);
        let lan = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2));
        let service = build_service_info_with(&advertisement, &[lan]).expect("service info");
        assert_eq!(service.get_property_val_str(FINGERPRINT_PROPERTY), None);
    }

    #[test]
    fn hex_encode_decode_roundtrips_certificate_bytes() {
        // A realistic cert size (~355 bytes) round-trips through the hex codec
        // used for TXT chunking.
        let cert: Vec<u8> = (0..355u16).map(|i| (i % 256) as u8).collect();
        let hex = hex_encode(&cert);
        assert_eq!(hex.len(), cert.len() * 2);
        assert_eq!(hex_decode(&hex), Some(cert));
        // Malformed hex is rejected, not silently truncated.
        assert_eq!(hex_decode("abc"), None); // odd length
        assert_eq!(hex_decode("zz"), None); // non-hex
    }

    #[test]
    fn certificate_is_chunked_into_txt_records_within_size_limit() {
        // ~355-byte cert -> ~710 hex chars -> multiple <=255-byte TXT values.
        let cert: Vec<u8> = (0..355u16)
            .map(|i| (i.wrapping_mul(7) % 256) as u8)
            .collect();
        let advertisement =
            PeerAdvertisement::new(PeerId("peer-cert".to_owned()), "archlinux", 50038)
                .with_certificate(cert.clone());
        let lan = IpAddr::V4(Ipv4Addr::new(172, 21, 209, 204));

        let service = build_service_info_with(&advertisement, &[lan]).expect("service info");

        let count: usize = service
            .get_property_val_str(CERTIFICATE_CHUNKS_PROPERTY)
            .expect("chunk count present")
            .parse()
            .expect("numeric count");
        assert!(count >= 2, "a ~700-char cert should need multiple chunks");

        // Reassemble the way a discovering peer would, and confirm the bytes and
        // that every chunk fits the TXT value limit.
        let mut hex = String::new();
        for index in 0..count {
            let chunk = service
                .get_property_val_str(&format!("{CERTIFICATE_CHUNK_PREFIX}{index}"))
                .expect("chunk present");
            assert!(chunk.len() <= 255, "TXT value must stay within 255 bytes");
            hex.push_str(chunk);
        }
        assert_eq!(hex_decode(&hex), Some(cert));
    }

    #[test]
    fn no_certificate_omits_the_chunk_records() {
        let advertisement = PeerAdvertisement::new(PeerId("peer-nocert".to_owned()), "host", 2);
        let lan = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3));
        let service = build_service_info_with(&advertisement, &[lan]).expect("service info");
        assert_eq!(
            service.get_property_val_str(CERTIFICATE_CHUNKS_PROPERTY),
            None
        );
        assert_eq!(service.get_property_val_str("cert0"), None);
    }

    #[test]
    fn empty_addresses_still_build_a_service() {
        // On a host with no routable address, the service still registers (via
        // addr-auto) rather than failing — localhost only as a last resort.
        let advertisement = PeerAdvertisement::new(PeerId("peer-3".to_owned()), "host", 0);
        let service = build_service_info_with(&advertisement, &[]).expect("service info");
        assert!(service.is_addr_auto());
    }

    #[test]
    fn peer_cache_discovers_updates_lists_and_removes_peers() {
        let mut cache = PeerCache::new(PeerId("local".to_owned()), Duration::from_secs(60));
        let now = Instant::now();

        cache.handle_resolved(resolved("desktop", "Desktop", 41002, "desktop.local."), now);
        cache.handle_resolved(resolved("laptop", "Laptop", 41001, "laptop.local."), now);

        assert_eq!(
            cache.pop_event(),
            Some(DiscoveryEvent::PeerDiscovered(peer(
                "desktop", "Desktop", 41002
            )))
        );
        assert_eq!(
            cache.pop_event(),
            Some(DiscoveryEvent::PeerDiscovered(peer(
                "laptop", "Laptop", 41001
            )))
        );
        assert_eq!(
            cache
                .peers()
                .into_iter()
                .map(|peer| peer.display_name)
                .collect::<Vec<_>>(),
            vec!["Desktop", "Laptop"]
        );

        cache.handle_resolved(
            resolved("laptop", "Laptop Pro", 41003, "laptop.local."),
            now,
        );
        assert_eq!(
            cache.pop_event(),
            Some(DiscoveryEvent::PeerUpdated(peer(
                "laptop",
                "Laptop Pro",
                41003
            )))
        );

        cache.handle_removed("laptop.local.");
        assert_eq!(
            cache.pop_event(),
            Some(DiscoveryEvent::PeerExpired(peer(
                "laptop",
                "Laptop Pro",
                41003
            )))
        );
        assert!(cache.peers().iter().all(|peer| peer.peer_id.0 != "laptop"));
    }

    #[test]
    fn restarted_receiver_updates_endpoint_in_place_not_duplicated() {
        // The reported bug: a receiver restarts with the SAME identity (peer id,
        // hostname/fullname, certificate) but a NEW QUIC port. Discovery must
        // merge it into the existing peer — one cached entry, endpoint updated —
        // rather than creating a second device.
        let mut cache = PeerCache::new(PeerId("local".to_owned()), Duration::from_secs(60));
        let now = Instant::now();

        let mut first = resolved("archlinux", "archlinux", 60897, "nexo-archlinux.local.");
        first.info.fingerprint = Some("AAAA:BBBB:CCCC:DDDD".to_owned());
        cache.handle_resolved(first, now);
        assert!(matches!(
            cache.pop_event(),
            Some(DiscoveryEvent::PeerDiscovered(_))
        ));

        // Same peer id + fullname + fingerprint, new port.
        let mut restarted = resolved("archlinux", "archlinux", 63455, "nexo-archlinux.local.");
        restarted.info.fingerprint = Some("AAAA:BBBB:CCCC:DDDD".to_owned());
        cache.handle_resolved(restarted, now);

        // Exactly one device, at the new endpoint, surfaced as an update.
        let peers = cache.peers();
        assert_eq!(
            peers.len(),
            1,
            "restart must not duplicate the peer: {peers:?}"
        );
        assert_eq!(peers[0].port, 63455, "endpoint updated to the new port");
        assert_eq!(peers[0].peer_id.0, "archlinux");
        assert_eq!(
            cache.pop_event(),
            Some(DiscoveryEvent::PeerUpdated(peers[0].clone()))
        );
    }

    #[test]
    fn peer_cache_ignores_local_peer_and_expires_stale_peers() {
        let mut cache = PeerCache::new(PeerId("local".to_owned()), Duration::from_millis(1));
        let now = Instant::now();

        cache.handle_resolved(resolved("local", "Local", 41000, "local.local."), now);
        assert_eq!(cache.pop_event(), None);

        cache.handle_resolved(resolved("remote", "Remote", 41001, "remote.local."), now);
        assert_eq!(
            cache.pop_event(),
            Some(DiscoveryEvent::PeerDiscovered(peer(
                "remote", "Remote", 41001
            )))
        );

        let tracked = cache
            .peers
            .get_mut(&PeerId("remote".to_owned()))
            .expect("remote peer");
        tracked.last_seen = Instant::now() - Duration::from_secs(1);
        cache.expire_stale_peers();

        assert_eq!(
            cache.pop_event(),
            Some(DiscoveryEvent::PeerExpired(peer("remote", "Remote", 41001)))
        );
        assert!(cache.peers().is_empty());
    }

    fn resolved(id: &str, name: &str, port: u16, fullname: &str) -> ResolvedPeer {
        ResolvedPeer {
            info: peer(id, name, port),
            fullname: fullname.to_owned(),
        }
    }

    fn peer(id: &str, name: &str, port: u16) -> PeerInfo {
        PeerInfo {
            peer_id: PeerId(id.to_owned()),
            display_name: name.to_owned(),
            addresses: vec![IpAddr::from([127, 0, 0, 1])],
            port,
            fingerprint: None,
            certificate_der: None,
        }
    }
}
