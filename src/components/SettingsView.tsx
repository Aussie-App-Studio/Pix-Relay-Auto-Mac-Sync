import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Activity, ExternalLink, KeyRound, Monitor, Moon, Sparkles, Sun, Terminal } from "lucide-react";
import { ActivityEntry, MacLicenseStatus, MacSettings, ThemeMode } from "../types";

interface Props {
  settings: MacSettings;
  license: MacLicenseStatus | null;
  onUpdateSettings: (next: MacSettings) => Promise<void>;
  onLicenseChanged: () => void;
}

export function SettingsView({
  settings,
  license,
  onUpdateSettings,
  onLicenseChanged,
}: Props) {
  const [nameDraft, setNameDraft] = useState(settings.deviceName);
  const [licenseKeyDraft, setLicenseKeyDraft] = useState(settings.licenseKey ?? "");
  const [licenseError, setLicenseError] = useState<string | null>(null);
  const [activity, setActivity] = useState<ActivityEntry[]>([]);

  useEffect(() => {
    setNameDraft(settings.deviceName);
    setLicenseKeyDraft(settings.licenseKey ?? "");
  }, [settings.deviceName, settings.licenseKey]);

  useEffect(() => {
    invoke<ActivityEntry[]>("list_activity_log").then(setActivity).catch(() => {});
  }, [settings]);

  const setDevOverride = async (mode: string | null) => {
    await invoke("set_dev_license_override", { mode });
    onLicenseChanged();
  };

  const handleActivateKey = async () => {
    setLicenseError(null);
    try {
      await invoke("activate_license_key", { key: licenseKeyDraft });
      onLicenseChanged();
    } catch (e) {
      setLicenseError(String(e));
    }
  };

  const handleClearKey = async () => {
    setLicenseError(null);
    await invoke("clear_license_key");
    setLicenseKeyDraft("");
    onLicenseChanged();
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16, maxWidth: 880 }}>
      <div>
        <div className="eyebrow" style={{ marginBottom: 4 }}>Preferences &amp; System</div>
        <h1 style={{ margin: 0, fontSize: 24, fontWeight: 800 }}>Settings</h1>
      </div>

      {/* Distribution & License Card */}
      {license && (
        <div className="m3-hero-card" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: 12 }}>
            <div>
              <div className="eyebrow" style={{ marginBottom: 4 }}>Mac Edition &amp; License</div>
              <div style={{ fontSize: 17, fontWeight: 800, display: "flex", alignItems: "center", gap: 8 }}>
                <Sparkles size={17} color="var(--primary)" /> {license.statusLabel}
              </div>
              <div style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 4 }}>
                {license.requiresLicense
                  ? "7-day full-featured free trial · Activate with a license key from the Pix Relay website."
                  : "Direct Mac Release — All Mac-to-Pixel sync features are unlocked with no Store restrictions."}
              </div>
            </div>
            <button
              className="btn btn-outline btn-sm"
              onClick={() => invoke("open_website_url")}
            >
              <ExternalLink size={13} /> Project Website
            </button>
          </div>

          {license.requiresLicense && (
            <div style={{ display: "flex", flexDirection: "column", gap: 8, paddingTop: 6, borderTop: "1px solid var(--border)" }}>
              <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
                <KeyRound size={15} color="var(--primary)" />
                <input
                  className="input-field"
                  placeholder="Enter license key (PIXM-XXXX-XXXX-XXXX)"
                  value={licenseKeyDraft}
                  onChange={(e) => setLicenseKeyDraft(e.target.value)}
                  style={{ flex: 1, fontFamily: "monospace", fontSize: 12 }}
                />
                <button className="btn btn-primary btn-sm" onClick={handleActivateKey}>
                  Activate
                </button>
                {license.hasLicenseKey && (
                  <button className="btn btn-outline btn-sm" onClick={handleClearKey}>
                    Remove Key
                  </button>
                )}
              </div>
              {licenseError && (
                <div style={{ fontSize: 12, color: "var(--error)", fontWeight: 600 }}>{licenseError}</div>
              )}
            </div>
          )}
        </div>
      )}

      {/* Mac Name & Theme */}
      <div className="m3-card" style={{ display: "flex", flexDirection: "column", gap: 14 }}>
        <div style={{ fontWeight: 700, fontSize: 15 }}>Mac Identity &amp; Theme</div>

        <div style={{ display: "flex", gap: 10, alignItems: "center" }}>
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 5 }}>Mac Name (shown on your Pixel)</div>
            <input
              className="input-field"
              value={nameDraft}
              onChange={(e) => setNameDraft(e.target.value)}
            />
          </div>
          <button
            className="btn btn-tonal"
            style={{ alignSelf: "flex-end" }}
            onClick={() => onUpdateSettings({ ...settings, deviceName: nameDraft })}
          >
            Save Name
          </button>
        </div>

        <div>
          <div style={{ fontSize: 12, color: "var(--text-muted)", marginBottom: 8 }}>App Theme</div>
          <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 8 }}>
            {(
              [
                ["SYSTEM", "Auto", Monitor],
                ["DARK", "Darkroom", Moon],
                ["LIGHT", "Gallery Paper", Sun],
                ["MATRIX", "Matrix", Terminal],
              ] as [ThemeMode, string, typeof Sun][]
            ).map(([mode, label, Icon]) => (
              <button
                key={mode}
                className={`btn ${settings.themeMode === mode ? "btn-primary" : "btn-outline"}`}
                onClick={() => onUpdateSettings({ ...settings, themeMode: mode })}
              >
                <Icon size={14} /> {label}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Sync & macOS Behavior */}
      <div className="m3-card" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
        <div style={{ fontWeight: 700, fontSize: 15 }}>Sync &amp; Lightweight Background Mode</div>

        <ToggleRow
          title="Auto-Sync Watched Folders"
          subtitle="Automatically relay photos in Auto folders whenever your Pixel is reachable"
          checked={settings.autoSyncEnabled}
          onChange={(v) => onUpdateSettings({ ...settings, autoSyncEnabled: v })}
        />

        <div
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            gap: 12,
            flexWrap: "wrap",
            padding: "8px 0",
            borderBottom: "1px solid var(--border-subtle)",
          }}
        >
          <div style={{ flex: "1 1 240px" }}>
            <div style={{ fontWeight: 600, fontSize: 13 }}>
              Auto-Send Photos:{" "}
              <span style={{ color: "var(--primary)" }}>
                {settings.autoSendCutoffEpochMs <= 0
                  ? "All Photos"
                  : `New from ${new Date(settings.autoSendCutoffEpochMs).toLocaleDateString()}`}
              </span>
            </div>
            <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 2 }}>
              {settings.autoSendCutoffEpochMs <= 0
                ? "Uploads all existing photos in Auto folders, then any new ones."
                : "Only uploads new photos from this point on (skips older or restored photos)."}
            </div>
          </div>

          <div style={{ display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }}>
            <div className="pill-bar">
              <button
                className={`pill-btn ${settings.autoSendCutoffEpochMs <= 0 ? "active" : ""}`}
                onClick={() => onUpdateSettings({ ...settings, autoSendCutoffEpochMs: 0 })}
              >
                All Photos
              </button>
              <button
                className={`pill-btn ${settings.autoSendCutoffEpochMs > 0 ? "active" : ""}`}
                onClick={() => onUpdateSettings({ ...settings, autoSendCutoffEpochMs: Date.now() })}
              >
                New Photos Only
              </button>
            </div>
            <input
              type="date"
              className="m3-input"
              aria-label="Custom auto-send start date"
              style={{ width: "auto", padding: "4px 8px", fontSize: 12 }}
              value={
                settings.autoSendCutoffEpochMs > 0
                  ? new Date(settings.autoSendCutoffEpochMs).toISOString().slice(0, 10)
                  : ""
              }
              onChange={(e) => {
                if (!e.target.value) {
                  onUpdateSettings({ ...settings, autoSendCutoffEpochMs: 0 });
                } else {
                  const [y, m, d] = e.target.value.split("-").map(Number);
                  const localMidnight = new Date(y, m - 1, d, 0, 0, 0, 0).getTime();
                  if (!Number.isNaN(localMidnight)) {
                    onUpdateSettings({ ...settings, autoSendCutoffEpochMs: localMidnight });
                  }
                }
              }}
            />
          </div>
        </div>

        <ToggleRow
          title="Send Photos Before Videos"
          subtitle="Uploads smaller still photos first before large video files"
          checked={settings.photosBeforeVideos}
          onChange={(v) => onUpdateSettings({ ...settings, photosBeforeVideos: v })}
        />

        <ToggleRow
          title="Include Camera RAW Files (.DNG, .CR3, .ARW, .NEF)"
          subtitle="Opt-in: RAW files are very large (25–80 MB each). Leave off if your Pixel storage is limited"
          checked={settings.includeRawFiles}
          onChange={(v) => onUpdateSettings({ ...settings, includeRawFiles: v })}
        />

        <ToggleRow
          title="Keep Running in Menu Bar on Close (Ultra-Low RAM)"
          subtitle="Closing the window frees WebView memory and keeps only the ~15 MB Rust background engine alive in the Menu Bar"
          checked={settings.minimizeToTray}
          onChange={(v) => onUpdateSettings({ ...settings, minimizeToTray: v })}
        />

        <ToggleRow
          title="Open Quietly at Login"
          subtitle="Launches minimized to the macOS Menu Bar when you log in to your Mac"
          checked={settings.startAtLogin}
          onChange={(v) => onUpdateSettings({ ...settings, startAtLogin: v })}
        />
      </div>

      {/* Dev Trial Preview (shown only in debug builds) */}
      {import.meta.env.DEV && (
        <div className="m3-card" style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: 10 }}>
          <div>
            <div style={{ fontWeight: 700, fontSize: 13 }}>Developer License State Preview</div>
            <div style={{ fontSize: 11, color: "var(--text-muted)" }}>Simulate trial and license states in local dev builds</div>
          </div>
          <div className="pill-bar">
            <button className={`pill-btn ${!license?.devOverride ? "active" : ""}`} onClick={() => setDevOverride(null)}>Real</button>
            <button className={`pill-btn ${license?.devOverride === "trial_5d" ? "active" : ""}`} onClick={() => setDevOverride("trial_5d")}>5d Trial</button>
            <button className={`pill-btn ${license?.devOverride === "trial_expired" ? "active" : ""}`} onClick={() => setDevOverride("trial_expired")}>Expired</button>
            <button className={`pill-btn ${license?.devOverride === "unlocked" ? "active" : ""}`} onClick={() => setDevOverride("unlocked")}>Unlocked</button>
          </div>
        </div>
      )}

      {/* Activity Log */}
      <div className="m3-card" style={{ display: "flex", flexDirection: "column", gap: 8 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, fontWeight: 700, fontSize: 14 }}>
          <Activity size={15} color="var(--primary)" /> Recent Activity
        </div>
        {activity.length === 0 ? (
          <div style={{ fontSize: 12, color: "var(--text-muted)" }}>No sync activity recorded yet.</div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 6, maxHeight: 200, overflowY: "auto" }}>
            {activity.map((item) => (
              <div
                key={item.id}
                style={{
                  display: "flex",
                  justifyContent: "space-between",
                  fontSize: 12,
                  padding: "6px 8px",
                  borderRadius: 8,
                  background: "var(--surface-low)",
                }}
              >
                <span>
                  <span className="badge badge-muted" style={{ marginRight: 8 }}>{item.kind}</span>
                  {item.message}
                </span>
                <span className="tabular" style={{ color: "var(--text-muted)", fontSize: 11 }}>
                  {new Date(item.timestampMs).toLocaleTimeString()}
                </span>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

function ToggleRow({
  title,
  subtitle,
  checked,
  onChange,
}: {
  title: string;
  subtitle: string;
  checked: boolean;
  onChange: (val: boolean) => void;
}) {
  return (
    <label
      style={{
        display: "flex",
        justifyContent: "space-between",
        alignItems: "center",
        gap: 16,
        padding: "10px 12px",
        borderRadius: 12,
        background: "var(--surface-low)",
        cursor: "pointer",
      }}
    >
      <div>
        <div style={{ fontWeight: 600, fontSize: 13 }}>{title}</div>
        <div style={{ fontSize: 12, color: "var(--text-muted)" }}>{subtitle}</div>
      </div>
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        style={{ width: 18, height: 18, accentColor: "var(--primary)" }}
      />
    </label>
  );
}
