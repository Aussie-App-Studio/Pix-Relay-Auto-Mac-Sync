import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  AlertTriangle,
  CheckCircle2,
  Copy,
  ExternalLink,
  KeyRound,
  RefreshCw,
  ShieldCheck,
  Sliders,
  Smartphone,
  Trash2,
  Wifi,
} from "lucide-react";
import {
  DiscoveredPeer,
  FirewallHealthStatus,
  PairedDevice,
  PairingSessionState,
  formatBytes,
} from "../types";

interface Props {
  discoveredPeers: DiscoveredPeer[];
  pairedDevices: PairedDevice[];
  selectedReceiverId: string | null;
  onRefresh: () => void;
  onOpenRemoteSettings: (dev: PairedDevice) => void;
}

export function PairingView({
  discoveredPeers,
  pairedDevices,
  selectedReceiverId,
  onRefresh,
  onOpenRemoteSettings,
}: Props) {
  const [manualMode, setManualMode] = useState<"IP" | "CODE">("IP");
  const [directIp, setDirectIp] = useState("");
  const [directPort, setDirectPort] = useState("42888");
  const [pairingCode, setPairingCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [session, setSession] = useState<PairingSessionState | null>(null);
  const [firewall, setFirewall] = useState<FirewallHealthStatus | null>(null);
  const [copiedRule, setCopiedRule] = useState(false);

  const loadFirewall = () => {
    invoke<FirewallHealthStatus>("get_firewall_health")
      .then(setFirewall)
      .catch(() => {});
  };

  useEffect(() => {
    loadFirewall();
  }, []);

  const receiversOnWifi = discoveredPeers.filter((p) => p.role === "RECEIVER");

  const startSasPairing = async (peer: DiscoveredPeer) => {
    setBusy(true);
    setMessage(null);
    try {
      const s = await invoke<PairingSessionState>("start_interactive_pairing", { peer });
      setSession(s);
    } catch (e) {
      setMessage({ kind: "err", text: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const startDirectIpPairing = async () => {
    if (!directIp.trim()) return;
    setBusy(true);
    setMessage(null);
    try {
      const parsedPort = Number.parseInt(directPort.trim(), 10);
      const s = await invoke<PairingSessionState>("start_direct_ip_pairing", {
        ipAddress: directIp.trim(),
        port: Number.isFinite(parsedPort) ? parsedPort : null,
      });
      setSession(s);
    } catch (e) {
      setMessage({ kind: "err", text: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const confirmSasPairing = async () => {
    if (!session) return;
    setBusy(true);
    setMessage(null);
    try {
      const paired = await invoke<PairedDevice>("complete_interactive_pairing", { session });
      setSession(null);
      setMessage({ kind: "ok", text: `Paired with ${paired.deviceName}!` });
      onRefresh();
    } catch (e) {
      setMessage({ kind: "err", text: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const pairViaCode = async () => {
    if (!pairingCode.trim()) return;
    setBusy(true);
    setMessage(null);
    try {
      const paired = await invoke<PairedDevice>("pair_pixel_with_code", { code: pairingCode.trim() });
      setPairingCode("");
      setMessage({ kind: "ok", text: `Instantly paired with ${paired.deviceName}!` });
      onRefresh();
    } catch (e) {
      setMessage({ kind: "err", text: String(e) });
    } finally {
      setBusy(false);
    }
  };

  const selectPrimary = async (deviceId: string) => {
    await invoke("set_selected_receiver", { deviceId });
    onRefresh();
  };

  const removeDevice = async (deviceId: string) => {
    await invoke("remove_paired_device", { deviceId });
    onRefresh();
  };

  const copyCommand = () => {
    if (!firewall) return;
    navigator.clipboard.writeText(firewall.recommendedCommand);
    setCopiedRule(true);
    setTimeout(() => setCopiedRule(false), 2000);
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16, maxWidth: 940 }}>
      <div>
        <div className="eyebrow" style={{ marginBottom: 4 }}>Wi-Fi Direct Encrypted Link</div>
        <h1 style={{ margin: 0, fontSize: 24, fontWeight: 800 }}>Pair Backup Pixel</h1>
        <div style={{ fontSize: 13, color: "var(--text-muted)", marginTop: 4 }}>
          Open Pix Relay on your Backup Pixel (Pixel 1–4) in Receiver mode on the same Wi-Fi network.
        </div>
      </div>

      {message && (
        <div
          className="m3-card"
          style={{
            background: message.kind === "ok" ? "var(--verified-container)" : "var(--error-container)",
            color: message.kind === "ok" ? "var(--on-verified-container)" : "var(--on-error-container)",
            fontWeight: 600,
            fontSize: 13,
          }}
        >
          {message.text}
        </div>
      )}

      <div style={{ display: "grid", gridTemplateColumns: "1fr 1fr", gap: 14 }}>
        {/* Discovered Pixels */}
        <div className="m3-card" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700 }}>
              <Wifi size={16} color="var(--primary)" /> Nearby Pixels on Wi-Fi
            </div>
            <span className="badge badge-primary tabular">{receiversOnWifi.length} found</span>
          </div>

          {receiversOnWifi.length === 0 ? (
            <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "14px 4px", lineHeight: 1.5 }}>
              Listening on UDP 42889 &amp; Bonjour mDNS... Make sure Pix Relay is open on your Backup Pixel in{" "}
              <strong>Receiver</strong> mode, or connect manually on the right.
            </div>
          ) : (
            <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
              {receiversOnWifi.map((peer) => (
                <div
                  key={peer.deviceId}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    padding: "10px 12px",
                    borderRadius: 14,
                    background: "var(--surface-low)",
                    border: "1px solid var(--border)",
                  }}
                >
                  <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                    <Smartphone size={18} color="var(--primary)" />
                    <div>
                      <div style={{ fontWeight: 700, fontSize: 13 }}>{peer.deviceName}</div>
                      <div className="tabular" style={{ fontSize: 11, color: "var(--text-muted)" }}>
                        {peer.modelName} · {peer.ipAddress}:{peer.port}
                      </div>
                    </div>
                  </div>
                  {peer.isPaired ? (
                    <span className="badge badge-verified">Paired</span>
                  ) : (
                    <button
                      className="btn btn-primary btn-sm"
                      disabled={busy}
                      onClick={() => startSasPairing(peer)}
                    >
                      Pair
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>

        {/* Manual / Direct Fallback Card (Connect by IP or Paste PIX1 Code) */}
        <div className="m3-card" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8, flexWrap: "wrap" }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700 }}>
              <KeyRound size={16} color="var(--secondary)" /> Manual Connect
            </div>
            <div className="pill-bar">
              <button
                className={`pill-btn ${manualMode === "IP" ? "active" : ""}`}
                style={{ padding: "3px 10px", fontSize: 11 }}
                onClick={() => setManualMode("IP")}
              >
                Connect by IP
              </button>
              <button
                className={`pill-btn ${manualMode === "CODE" ? "active" : ""}`}
                style={{ padding: "3px 10px", fontSize: 11 }}
                onClick={() => setManualMode("CODE")}
              >
                PIX1 Code
              </button>
            </div>
          </div>

          {manualMode === "IP" ? (
            <>
              <div style={{ fontSize: 12, color: "var(--text-muted)", lineHeight: 1.5 }}>
                If auto-discovery is blocked by Local Network privacy or router isolation, type the IP address shown on your Pixel’s Receiver screen.
              </div>
              <div style={{ display: "flex", gap: 8 }}>
                <input
                  className="input-field"
                  placeholder="Pixel IP (e.g. 192.168.1.42)"
                  value={directIp}
                  onChange={(e) => setDirectIp(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" && directIp.trim() && !busy) {
                      startDirectIpPairing();
                    }
                  }}
                  style={{ flex: 1, fontFamily: "monospace", fontSize: 12 }}
                />
                <input
                  className="input-field tabular"
                  placeholder="Port"
                  value={directPort}
                  onChange={(e) => setDirectPort(e.target.value)}
                  style={{ width: 82, fontFamily: "monospace", fontSize: 12 }}
                />
              </div>
              <div style={{ display: "flex", justifyContent: "flex-end", marginTop: "auto" }}>
                <button
                  className="btn btn-primary"
                  disabled={busy || !directIp.trim()}
                  onClick={startDirectIpPairing}
                >
                  {busy ? "Connecting..." : "Connect & Pair"}
                </button>
              </div>
            </>
          ) : (
            <>
              <div style={{ fontSize: 12, color: "var(--text-muted)", lineHeight: 1.5 }}>
                Tap <strong>Copy Pairing Code</strong> on your Pixel (starts with{" "}
                <code style={{ color: "var(--primary)" }}>PIX1:</code>) and paste it below for instant pairing.
              </div>
              <textarea
                className="input-field"
                rows={3}
                placeholder="Paste PIX1:... code here"
                value={pairingCode}
                onChange={(e) => setPairingCode(e.target.value)}
                style={{ resize: "none", fontFamily: "monospace", fontSize: 12 }}
              />
              <div style={{ display: "flex", justifyContent: "flex-end" }}>
                <button
                  className="btn btn-primary"
                  disabled={busy || !pairingCode.trim()}
                  onClick={pairViaCode}
                >
                  Pair Instantly
                </button>
              </div>
            </>
          )}
        </div>
      </div>

      {/* Paired Devices */}
      <div className="m3-card" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ fontWeight: 700, fontSize: 15 }}>Paired Backup Pixels</div>
        {pairedDevices.length === 0 ? (
          <div style={{ fontSize: 13, color: "var(--text-muted)" }}>No Pixels paired yet.</div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            {pairedDevices.map((dev) => {
              const isPrimary = dev.deviceId === selectedReceiverId || (!selectedReceiverId && pairedDevices[0]?.deviceId === dev.deviceId);
              return (
                <div
                  key={dev.deviceId}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    flexWrap: "wrap",
                    gap: 10,
                    padding: "12px 14px",
                    borderRadius: 14,
                    background: "var(--surface-low)",
                    border: `1px solid ${isPrimary ? "var(--hero-border)" : "var(--border)"}`,
                  }}
                >
                  <div>
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <strong style={{ fontSize: 14 }}>{dev.deviceName}</strong>
                      <span className="badge badge-muted">{dev.modelName}</span>
                      {isPrimary && <span className="badge badge-verified">Primary Target</span>}
                    </div>
                    <div className="tabular" style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 4 }}>
                      {dev.lastIpAddress}:{dev.lastPort} · {dev.totalFilesTransferred} files sent ({formatBytes(dev.totalBytesTransferred)})
                    </div>
                  </div>
                  <div style={{ display: "flex", gap: 8 }}>
                    {!isPrimary && (
                      <button className="btn btn-tonal btn-sm" onClick={() => selectPrimary(dev.deviceId)}>
                        Set Primary
                      </button>
                    )}
                    <button className="btn btn-outline btn-sm" onClick={() => onOpenRemoteSettings(dev)}>
                      <Sliders size={13} /> Remote Controls
                    </button>
                    <button className="btn btn-danger btn-sm" onClick={() => removeDevice(dev.deviceId)}>
                      <Trash2 size={13} />
                    </button>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {/* macOS Wi-Fi & Firewall Health Card */}
      <div className="m3-card" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 14 }}>
            <ShieldCheck size={16} color={firewall?.hasFirewallWarning ? "var(--secondary)" : "var(--verified)"} />
            macOS Wi-Fi &amp; Firewall Health
          </div>
          <button className="btn btn-outline btn-sm" onClick={loadFirewall}>
            <RefreshCw size={12} /> Recheck
          </button>
        </div>

        {firewall && (
          <>
            <div style={{ display: "flex", gap: 8, flexWrap: "wrap", fontSize: 12 }}>
              <span className="badge badge-muted tabular">
                Mac LAN IP: {firewall.localIpv4s.join(", ") || "None detected"}
              </span>
              <span
                className={`badge ${
                  firewall.firewallInboundStatus === "BLOCKED" ||
                  firewall.firewallInboundStatus === "BLOCK_ALL"
                    ? "badge-error"
                    : firewall.firewallInboundStatus === "ALLOWED" ||
                        firewall.firewallInboundStatus === "FIREWALL_OFF"
                      ? "badge-verified"
                      : "badge-warm"
                }`}
              >
                {firewall.firewallInboundStatus === "ALLOWED"
                  ? "Firewall: Pix Relay Allowed"
                  : firewall.firewallInboundStatus === "FIREWALL_OFF"
                    ? `Firewall: Off (UDP ${firewall.udpPort} Open)`
                    : firewall.firewallInboundStatus === "BLOCK_ALL"
                      ? "Firewall: Blocking All Incoming"
                      : firewall.firewallInboundStatus === "BLOCKED"
                        ? "Firewall: Pix Relay Blocked"
                        : firewall.firewallInboundStatus === "UNCONFIGURED"
                          ? "Firewall: No Explicit Rule Yet"
                          : `Discovery Socket Listening (UDP ${firewall.udpPort})`}
              </span>
              {firewall.interfaces.map((iface, i) => (
                <span key={i} className="badge badge-muted tabular">
                  {iface.interfaceAlias}: {iface.ipAddress}
                </span>
              ))}
            </div>

            <div style={{ fontSize: 12, color: "var(--text-muted)", lineHeight: 1.45 }}>
              {firewall.firewallSummary}
            </div>

            <div
              style={{
                background: firewall.hasFirewallWarning ? "var(--secondary-container)" : "var(--surface-low)",
                color: firewall.hasFirewallWarning ? "var(--on-secondary-container)" : "var(--text-muted)",
                padding: "10px 12px",
                borderRadius: 12,
                fontSize: 12,
                display: "flex",
                justifyContent: "space-between",
                alignItems: "center",
                gap: 10,
                flexWrap: "wrap",
              }}
            >
              <div style={{ display: "flex", alignItems: "center", gap: 8, flex: "1 1 280px" }}>
                {firewall.hasFirewallWarning && <AlertTriangle size={16} />}
                <span>{firewall.localNetworkPrivacyNote}</span>
              </div>
              <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                <button
                  className="btn btn-outline btn-sm"
                  onClick={() => invoke("open_macos_settings", { pane: "local-network" })}
                >
                  <ExternalLink size={12} /> Local Network
                </button>
                <button
                  className="btn btn-outline btn-sm"
                  onClick={() => invoke("open_macos_settings", { pane: "firewall" })}
                >
                  <ExternalLink size={12} /> Firewall
                </button>
                <button
                  className="btn btn-outline btn-sm"
                  onClick={() => invoke("open_macos_settings", { pane: "wifi" })}
                >
                  <ExternalLink size={12} /> Wi-Fi
                </button>
              </div>
            </div>

            <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11, color: "var(--text-muted)" }}>
              <code style={{ flex: 1, background: "var(--surface-lowest)", padding: "6px 10px", borderRadius: 8, overflowX: "auto" }}>
                {firewall.recommendedCommand}
              </code>
              <button className="btn btn-outline btn-sm" onClick={copyCommand}>
                {copiedRule ? <CheckCircle2 size={12} color="var(--verified)" /> : <Copy size={12} />}
                {copiedRule ? "Copied" : "Copy Command"}
              </button>
            </div>
          </>
        )}
      </div>

      {/* 6-Digit SAS Confirmation Modal */}
      {session && (
        <div className="modal-backdrop" onClick={() => setSession(null)}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()} style={{ textAlign: "center" }}>
            <div className="eyebrow" style={{ marginBottom: 6 }}>Verify 6-Digit Security Code</div>
            <h2 style={{ margin: "0 0 8px", fontSize: 19 }}>Pair with {session.peerName}</h2>
            <p style={{ fontSize: 13, color: "var(--text-muted)", lineHeight: 1.5 }}>
              Check that this exact code appears on <strong>{session.peerName}</strong>, tap <strong>Approve</strong> on the Pixel, then click Confirm below.
            </p>
            <div
              className="tabular"
              style={{
                fontSize: 34,
                fontWeight: 900,
                letterSpacing: "0.14em",
                color: "var(--primary)",
                background: "var(--surface-container)",
                border: "1px solid var(--hero-border)",
                borderRadius: 16,
                padding: "16px 20px",
                margin: "16px 0",
              }}
            >
              {session.verificationCode6Digit}
            </div>
            <div style={{ display: "flex", justifyContent: "center", gap: 10 }}>
              <button className="btn btn-outline" onClick={() => setSession(null)}>Cancel</button>
              <button className="btn btn-primary" disabled={busy} onClick={confirmSasPairing}>
                {busy ? "Waiting for Pixel..." : "Codes Match — Pair"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
