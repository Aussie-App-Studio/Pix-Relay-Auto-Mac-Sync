use serde::Serialize;

use crate::protocol::discovery::{list_local_lan_ipv4s, DiscoveryManager, UDP_DISCOVERY_PORT};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterfaceInfo {
    pub interface_alias: String,
    pub ip_address: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FirewallHealthStatus {
    pub local_ipv4s: Vec<String>,
    pub interfaces: Vec<NetworkInterfaceInfo>,
    pub has_firewall_warning: bool,
    pub udp_listener_healthy: bool,
    pub udp_port: u16,
    pub firewall_inbound_status: String,
    pub firewall_summary: String,
    pub recommended_command: String,
    pub local_network_privacy_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MacFirewallInspection {
    pub query_succeeded: bool,
    pub firewall_enabled: bool,
    pub block_all_incoming: bool,
    pub app_allowed: Option<bool>,
}

pub fn check_firewall_health(discovery: &DiscoveryManager) -> FirewallHealthStatus {
    let interfaces = collect_lan_interfaces();
    let local_ipv4s: Vec<String> = interfaces
        .iter()
        .map(|i| i.ip_address.clone())
        .collect::<Vec<_>>();
    let fallback_ips: Vec<String> = if local_ipv4s.is_empty() {
        list_local_lan_ipv4s()
            .into_iter()
            .map(|ip| ip.to_string())
            .collect()
    } else {
        local_ipv4s
    };

    let exe_path = std::env::current_exe()
        .ok()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let inspection = query_macos_firewall(&exe_path);
    let listener_bound = discovery.is_udp_listener_healthy();

    let (firewall_inbound_status, firewall_summary, udp_listener_healthy, has_firewall_warning) =
        evaluate_macos_firewall_verdict(listener_bound, &inspection, UDP_DISCOVERY_PORT);

    let recommended_command = "sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add \"/Applications/Pix Relay Auto Mac Sync.app\" --unblockapp \"/Applications/Pix Relay Auto Mac Sync.app\"".to_string();

    FirewallHealthStatus {
        local_ipv4s: fallback_ips,
        interfaces,
        has_firewall_warning,
        udp_listener_healthy,
        udp_port: UDP_DISCOVERY_PORT,
        firewall_inbound_status,
        firewall_summary,
        recommended_command,
        local_network_privacy_note: "On macOS Sequoia (15+), make sure Pix Relay is toggled ON under System Settings → Privacy & Security → Local Network.".to_string(),
    }
}

fn collect_lan_interfaces() -> Vec<NetworkInterfaceInfo> {
    let mut out = Vec::new();
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for iface in ifaces {
            if iface.is_loopback() {
                continue;
            }
            if let std::net::IpAddr::V4(v4) = iface.ip() {
                if v4.is_private() {
                    out.push(NetworkInterfaceInfo {
                        interface_alias: iface.name,
                        ip_address: v4.to_string(),
                    });
                }
            }
        }
    }
    out
}

pub fn evaluate_macos_firewall_verdict(
    listener_bound: bool,
    inspection: &MacFirewallInspection,
    udp_port: u16,
) -> (String, String, bool, bool) {
    if !listener_bound {
        return (
            "UNCONFIGURED".to_string(),
            format!(
                "Could not bind UDP discovery port {udp_port} locally (another app may be using it). Use Connect by IP or a PIX1 pairing code."
            ),
            false,
            true,
        );
    }

    if !inspection.query_succeeded {
        return (
            "UNKNOWN".to_string(),
            "Local discovery socket is listening. If your Pixel does not appear, allow Pix Relay in macOS Local Network privacy settings or use Connect by IP.".to_string(),
            true,
            false,
        );
    }

    if !inspection.firewall_enabled {
        return (
            "FIREWALL_OFF".to_string(),
            format!(
                "macOS Application Firewall is off — LAN discovery on UDP {udp_port} and mDNS are open."
            ),
            true,
            false,
        );
    }

    if inspection.block_all_incoming {
        return (
            "BLOCK_ALL".to_string(),
            "macOS Firewall is set to 'Block all incoming connections', which blocks Wi-Fi Pixel auto-discovery. Disable 'Block all incoming' in Network Firewall settings or use Connect by IP.".to_string(),
            false,
            true,
        );
    }

    match inspection.app_allowed {
        Some(false) => (
            "BLOCKED".to_string(),
            "macOS Application Firewall currently blocks incoming connections for Pix Relay. Allow it in Firewall Options or run the unblock command below.".to_string(),
            false,
            true,
        ),
        Some(true) => (
            "ALLOWED".to_string(),
            format!(
                "macOS Application Firewall allows incoming connections for Pix Relay (UDP {udp_port} & mDNS)."
            ),
            true,
            false,
        ),
        None => (
            "UNCONFIGURED".to_string(),
            "macOS Firewall is enabled and Pix Relay has no explicit entry yet. When macOS prompts to allow incoming or Local Network connections, click Allow.".to_string(),
            true,
            false,
        ),
    }
}

pub fn parse_socketfilterfw_state(
    global_out: &str,
    blockall_out: &str,
    listapps_out: &str,
    exe_path: &str,
) -> MacFirewallInspection {
    let global_low = global_out.to_lowercase();
    let firewall_enabled = global_low.contains("enabled") || global_low.contains("state = 1") || global_low.contains("state = 2");

    let block_low = blockall_out.to_lowercase();
    let block_all_incoming = block_low.contains("enabled") && !block_low.contains("disabled");

    let app_allowed = parse_app_allowance(listapps_out, exe_path);

    MacFirewallInspection {
        query_succeeded: !global_out.trim().is_empty(),
        firewall_enabled,
        block_all_incoming,
        app_allowed,
    }
}

fn parse_app_allowance(listapps_out: &str, exe_path: &str) -> Option<bool> {
    let bundle_hint = "pix relay auto mac sync";
    let exe_low = exe_path.to_lowercase();
    let lines: Vec<&str> = listapps_out.lines().collect();

    for (idx, line) in lines.iter().enumerate() {
        let low = line.to_lowercase();
        let matches_app = low.contains(bundle_hint)
            || low.contains("pix_relay_auto_mac_sync")
            || (!exe_low.is_empty() && low.contains(&exe_low));
        if matches_app {
            let next_line = lines.get(idx + 1).map(|l| l.to_lowercase()).unwrap_or_default();
            if next_line.contains("block incoming") {
                return Some(false);
            }
            if next_line.contains("allow incoming") {
                return Some(true);
            }
            if low.contains("block incoming") {
                return Some(false);
            }
            if low.contains("allow incoming") {
                return Some(true);
            }
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn query_macos_firewall(exe_path: &str) -> MacFirewallInspection {
    let fw_bin = "/usr/libexec/ApplicationFirewall/socketfilterfw";
    let global = std::process::Command::new(fw_bin)
        .arg("--getglobalstate")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    if global.trim().is_empty() {
        return MacFirewallInspection {
            query_succeeded: false,
            firewall_enabled: false,
            block_all_incoming: false,
            app_allowed: None,
        };
    }

    let blockall = std::process::Command::new(fw_bin)
        .arg("--getblockall")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let listapps = std::process::Command::new(fw_bin)
        .arg("--listapps")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    parse_socketfilterfw_state(&global, &blockall, &listapps, exe_path)
}

#[cfg(not(target_os = "macos"))]
fn query_macos_firewall(_exe_path: &str) -> MacFirewallInspection {
    MacFirewallInspection {
        query_succeeded: false,
        firewall_enabled: false,
        block_all_incoming: false,
        app_allowed: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_socketfilterfw_enabled_blocked_and_allowed_states() {
        let global_on = "Firewall is enabled. (State = 1)";
        let block_off = "Block all non-essential incoming connections is disabled";
        let apps_allowed = "ALF: total number of apps = 2\n1 : /Applications/Pix Relay Auto Mac Sync.app\n\t(Allow incoming connections)\n";

        let insp = parse_socketfilterfw_state(global_on, block_off, apps_allowed, "");
        assert!(insp.query_succeeded);
        assert!(insp.firewall_enabled);
        assert!(!insp.block_all_incoming);
        assert_eq!(insp.app_allowed, Some(true));

        let (status, _, healthy, warn) = evaluate_macos_firewall_verdict(true, &insp, 42889);
        assert_eq!(status, "ALLOWED");
        assert!(healthy);
        assert!(!warn);

        let apps_blocked = "1 : /Applications/Pix Relay Auto Mac Sync.app\n\t(Block incoming connections)\n";
        let insp_blocked = parse_socketfilterfw_state(global_on, block_off, apps_blocked, "");
        let (status, _, healthy, warn) = evaluate_macos_firewall_verdict(true, &insp_blocked, 42889);
        assert_eq!(status, "BLOCKED");
        assert!(!healthy);
        assert!(warn);

        let block_on = "Block all non-essential incoming connections is ENABLED";
        let insp_block_all = parse_socketfilterfw_state(global_on, block_on, apps_allowed, "");
        let (status, _, healthy, warn) = evaluate_macos_firewall_verdict(true, &insp_block_all, 42889);
        assert_eq!(status, "BLOCK_ALL");
        assert!(!healthy);
        assert!(warn);

        let global_off = "Firewall is disabled. (State = 0)";
        let insp_off = parse_socketfilterfw_state(global_off, block_off, "", "");
        let (status, _, healthy, warn) = evaluate_macos_firewall_verdict(true, &insp_off, 42889);
        assert_eq!(status, "FIREWALL_OFF");
        assert!(healthy);
        assert!(!warn);
    }
}
