use common::PeerId;
use mdns_sd::{Receiver, ResolvedService, ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::{HashMap, VecDeque};
use std::io::{Error, ErrorKind, Result};
use std::net::IpAddr;
use std::time::{Duration, Instant};

pub const NEXO_SERVICE_TYPE: &str = "_nexo._udp.local.";
pub const DEFAULT_PEER_TIMEOUT: Duration = Duration::from_secs(180);

const DISCOVERY_VERSION: &str = "1";
const PEER_ID_PROPERTY: &str = "peer_id";
const DISPLAY_NAME_PROPERTY: &str = "display_name";
const VERSION_PROPERTY: &str = "version";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerAdvertisement {
    pub peer_id: PeerId,
    pub display_name: String,
    pub port: u16,
}

impl PeerAdvertisement {
    pub fn new(peer_id: PeerId, display_name: impl Into<String>, port: u16) -> Self {
        Self {
            peer_id,
            display_name: display_name.into(),
            port,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    pub peer_id: PeerId,
    pub display_name: String,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
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
    let properties = HashMap::from([
        (PEER_ID_PROPERTY.to_owned(), advertisement.peer_id.0.clone()),
        (
            DISPLAY_NAME_PROPERTY.to_owned(),
            advertisement.display_name.clone(),
        ),
        (VERSION_PROPERTY.to_owned(), DISCOVERY_VERSION.to_owned()),
    ]);
    let service = ServiceInfo::new(
        NEXO_SERVICE_TYPE,
        &advertisement.peer_id.0,
        &service_hostname(&advertisement.peer_id),
        "",
        advertisement.port,
        properties,
    )
    .map_err(discovery_error)?
    .enable_addr_auto();

    Ok(service)
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

    Some(ResolvedPeer {
        info: PeerInfo {
            peer_id,
            display_name,
            addresses,
            port: service.get_port(),
        },
        fullname: service.get_fullname().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
        }
    }
}
