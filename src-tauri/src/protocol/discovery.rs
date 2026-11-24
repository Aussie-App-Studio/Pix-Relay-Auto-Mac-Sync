use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::sync::broadcast;

use crate::db::Database;
use crate::protocol::messages::{DeviceRole, DiscoveredPeer, UdpBeaconPayload};

pub const DEFAULT_TCP_PORT: i32 = 42888;
pub const UDP_DISCOVERY_PORT: u16 = 42889;
const NSD_SERVICE_TYPE: &str = "_pixrelay._tcp.local.";

pub fn mac_model_name() -> String {
    #[cfg(target_os = "macos")]
    {
        if let Ok(out) = std::process::Command::new("/usr/sbin/sysctl")
            .args(["-n", "hw.model"])
            .output()
        {
            if out.status.success() {
                let model = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if !model.is_empty() {
                    return model;
                }
            }
        }
    }
    "Mac".to_string()
}

#[derive(Clone)]
pub struct DiscoveryManager {
    db: Database,
    peers: Arc<Mutex<HashMap<String, (Instant, DiscoveredPeer)>>>,
    peer_tx: broadcast::Sender<Vec<DiscoveredPeer>>,
}

impl DiscoveryManager {
    pub fn new(db: Database) -> Self {
        let (peer_tx, _) = broadcast::channel(32);
        Self {
            db,
            peers: Arc::new(Mutex::new(HashMap::new())),
            peer_tx,
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Vec<DiscoveredPeer>> {
        self.peer_tx.subscribe()
    }

    pub fn current_peers(&self) -> Vec<DiscoveredPeer> {
        self.prune_and_build()
    }

    pub fn start(&self) {
        let this_listen = self.clone();
        std::thread::Builder::new()
            .name("pixrelay-udp-listen".to_string())
            .spawn(move || this_listen.run_udp_listener())
            .ok();

        let this_bcast = self.clone();
        std::thread::Builder::new()
            .name("pixrelay-udp-bcast".to_string())
            .spawn(move || this_bcast.run_udp_broadcaster())
            .ok();

        let this_mdns = self.clone();
        std::thread::Builder::new()
            .name("pixrelay-mdns".to_string())
            .spawn(move || this_mdns.run_mdns_browser())
            .ok();
    }

    pub fn is_udp_listener_healthy(&self) -> bool {
        !self.peers.lock().unwrap().is_empty() || bind_reusable_udp(UDP_DISCOVERY_PORT).is_ok()
    }

    pub async fn rediscover_peer_endpoint(
        &self,
        target_device_id: &str,
        timeout: Duration,
    ) -> Option<(String, i32)> {
        if let Some(ep) = self.find_fresh_peer_endpoint(target_device_id, Duration::from_secs(15)) {
            return Some(ep);
        }

        let mut rx = self.subscribe();
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(ep) = self.find_fresh_peer_endpoint(target_device_id, Duration::from_secs(15)) {
                return Some(ep);
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return None;
            }
            match tokio::time::timeout(remaining, rx.recv()).await {
                Ok(Ok(peers)) => {
                    if let Some(p) = peers.into_iter().find(|p| p.device_id == target_device_id) {
                        if !p.ip_address.is_empty() && p.port > 0 {
                            return Some((p.ip_address, p.port));
                        }
                    }
                }
                _ => return None,
            }
        }
    }

    fn find_fresh_peer_endpoint(&self, device_id: &str, max_age: Duration) -> Option<(String, i32)> {
        let guard = self.peers.lock().unwrap();
        let (seen, peer) = guard.get(device_id)?;
        if seen.elapsed() <= max_age && !peer.ip_address.is_empty() && peer.port > 0 {
            Some((peer.ip_address.clone(), peer.port))
        } else {
            None
        }
    }

    fn run_udp_listener(&self) {
        let Ok(sock) = bind_reusable_udp(UDP_DISCOVERY_PORT) else {
            return;
        };
        let _ = sock.set_read_timeout(Some(Duration::from_millis(2500)));
        let mut buf = [0u8; 4096];
        loop {
            match sock.recv_from(&mut buf) {
                Ok((len, addr)) => {
                    if let Ok(text) = std::str::from_utf8(&buf[..len]) {
                        if let Ok(beacon) = serde_json::from_str::<UdpBeaconPayload>(text) {
                            let my_id = self
                                .db
                                .get_settings()
                                .map(|s| s.device_id)
                                .unwrap_or_default();
                            if beacon.app == "PIX_RELAY_1" && beacon.device_id != my_id {
                                let ip = addr.ip().to_string();
                                self.on_peer_discovered(
                                    beacon.device_id,
                                    beacon.device_name,
                                    beacon.model_name,
                                    beacon.role,
                                    ip,
                                    beacon.tcp_port,
                                );
                            }
                        }
                    }
                }
                Err(_) => {
                    self.prune_and_build();
                }
            }
        }
    }

    fn run_udp_broadcaster(&self) {
        let Ok(sock) = UdpSocket::bind("0.0.0.0:0") else {
            return;
        };
        let _ = sock.set_broadcast(true);
        let model_name = mac_model_name();
        loop {
            if let Ok(settings) = self.db.get_settings() {
                let payload = UdpBeaconPayload {
                    app: "PIX_RELAY_1".to_string(),
                    device_id: settings.device_id,
                    device_name: settings.device_name,
                    model_name: model_name.clone(),
                    role: DeviceRole::Sender,
                    tcp_port: DEFAULT_TCP_PORT,
                };
                if let Ok(bytes) = serde_json::to_vec(&payload) {
                    let mut targets: Vec<Ipv4Addr> = vec![Ipv4Addr::BROADCAST];
                    for local_ip in list_local_lan_ipv4s() {
                        let o = local_ip.octets();
                        let subnet_bcast = Ipv4Addr::new(o[0], o[1], o[2], 255);
                        if !targets.contains(&subnet_bcast) {
                            targets.push(subnet_bcast);
                        }
                    }
                    for target in targets {
                        let _ = sock.send_to(
                            &bytes,
                            SocketAddr::V4(SocketAddrV4::new(target, UDP_DISCOVERY_PORT)),
                        );
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(2500));
        }
    }

    fn run_mdns_browser(&self) {
        let Ok(mdns) = ServiceDaemon::new() else {
            return;
        };
        let Ok(receiver) = mdns.browse(NSD_SERVICE_TYPE) else {
            return;
        };
        while let Ok(event) = receiver.recv() {
            if let ServiceEvent::ServiceResolved(info) = event {
                let my_id = self
                    .db
                    .get_settings()
                    .map(|s| s.device_id)
                    .unwrap_or_default();
                let props = info.get_properties();
                let id = props
                    .get_property_val_str("id")
                    .map(|s| s.to_string())
                    .or_else(|| {
                        info.get_fullname()
                            .strip_prefix("PixRelay_")
                            .and_then(|s| s.split('_').next())
                            .map(|s| s.to_string())
                    });
                let Some(device_id) = id else { continue };
                if device_id == my_id {
                    continue;
                }
                let name = props
                    .get_property_val_str("name")
                    .unwrap_or("Backup Pixel")
                    .to_string();
                let model = props
                    .get_property_val_str("model")
                    .unwrap_or("Android")
                    .to_string();
                let role = match props.get_property_val_str("role") {
                    Some("SENDER") => DeviceRole::Sender,
                    _ => DeviceRole::Receiver,
                };
                let ip_opt = info
                    .get_addresses()
                    .iter()
                    .copied()
                    .find(|ip| matches!(ip, IpAddr::V4(_)))
                    .or_else(|| info.get_addresses().iter().copied().next());
                if let Some(ip) = ip_opt {
                    let port = info.get_port() as i32;
                    if port > 0 {
                        self.on_peer_discovered(device_id, name, model, role, ip.to_string(), port);
                    }
                }
            }
        }
    }

    fn on_peer_discovered(
        &self,
        device_id: String,
        device_name: String,
        model_name: String,
        role: DeviceRole,
        ip_address: String,
        port: i32,
    ) {
        let is_paired = self
            .db
            .get_paired_device(&device_id)
            .ok()
            .flatten()
            .is_some();
        if is_paired {
            let _ = self.db.update_peer_endpoint(&device_id, &ip_address, port);
        }
        {
            let mut guard = self.peers.lock().unwrap();
            guard.insert(
                device_id.clone(),
                (
                    Instant::now(),
                    DiscoveredPeer {
                        device_id,
                        device_name,
                        model_name,
                        role,
                        ip_address,
                        port,
                        is_paired,
                    },
                ),
            );
        }
        let list = self.prune_and_build();
        let _ = self.peer_tx.send(list);
    }

    fn prune_and_build(&self) -> Vec<DiscoveredPeer> {
        let paired_ids: std::collections::HashSet<String> = self
            .db
            .list_paired_devices()
            .unwrap_or_default()
            .into_iter()
            .map(|d| d.device_id)
            .collect();
        let mut guard = self.peers.lock().unwrap();
        guard.retain(|_, (seen, _)| seen.elapsed() <= Duration::from_secs(15));
        let mut list: Vec<DiscoveredPeer> = guard
            .values()
            .map(|(_, p)| {
                let mut item = p.clone();
                item.is_paired = paired_ids.contains(&item.device_id);
                item
            })
            .collect();
        list.sort_by(|a, b| {
            b.is_paired
                .cmp(&a.is_paired)
                .then_with(|| a.device_name.cmp(&b.device_name))
        });
        list
    }
}

fn bind_reusable_udp(port: u16) -> std::io::Result<UdpSocket> {
    let sock = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    sock.set_reuse_address(true)?;
    sock.set_broadcast(true)?;
    let addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, port);
    sock.bind(&addr.into())?;
    Ok(sock.into())
}

pub fn list_local_lan_ipv4s() -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            if let IpAddr::V4(v4) = iface.ip() {
                if v4.is_private() {
                    ips.push(v4);
                }
            }
        }
    }
    ips
}

pub fn primary_local_ipv4() -> String {
    list_local_lan_ipv4s()
        .into_iter()
        .next()
        .map(|ip| ip.to_string())
        .unwrap_or_default()
}
