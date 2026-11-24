import { useState } from "react";
import {
  BatteryCharging,
  Check,
  FolderCheck,
  FolderOpen,
  FolderPlus,
  HardDrive,
  Image,
  Lock,
  Monitor,
  Moon,
  Send,
  Settings,
  Sliders,
  Smartphone,
  Sparkles,
  Sun,
  Terminal,
  Thermometer,
  Trash2,
  Wifi,
} from "lucide-react";
import {
  FolderSummary,
  LibrarySnapshot,
  MacLicenseStatus,
  PairedDevice,
  ReceiverTelemetry,
  ThemeMode,
  formatBytes,
} from "../types";

export type NavPage =
  | { kind: "FOLDERS_OVERVIEW" }
  | { kind: "FOLDER_GALLERY"; folderId: string | null }
  | { kind: "FREE_UP_SPACE" }
  | { kind: "PAIRING" }
  | { kind: "SETTINGS" };

interface Props {
  page: NavPage;
  onNavigate: (page: NavPage) => void;
  snapshot: LibrarySnapshot;
  primaryPixel: PairedDevice | null;
  telemetry: ReceiverTelemetry | null;
  license: MacLicenseStatus | null;
  themeMode: ThemeMode;
  onCycleTheme: () => void;
  onAddFolder: () => void;
  onOpenRemoteSettings: (dev: PairedDevice) => void;
  onOpenLicenseSettings: () => void;
}

export function Sidebar({
  page,
  onNavigate,
  snapshot,
  primaryPixel,
  telemetry,
  license,
  themeMode,
  onCycleTheme,
  onAddFolder,
  onOpenRemoteSettings,
  onOpenLicenseSettings,
}: Props) {
  const [folderFilter, setFolderFilter] = useState<"ALL" | "COMPLETE" | "PENDING">("ALL");

  const filteredFolders = snapshot.folders.filter((f) => {
    if (folderFilter === "COMPLETE") return f.is100PercentUploaded;
    if (folderFilter === "PENDING") return f.unsentCount > 0;
    return true;
  });

  const ThemeIcon =
    themeMode === "DARK"
      ? Moon
      : themeMode === "LIGHT"
        ? Sun
        : themeMode === "MATRIX"
          ? Terminal
          : Monitor;

  const isOnline =
    telemetry !== null &&
    primaryPixel !== null &&
    telemetry.deviceId === primaryPixel.deviceId;
  const usedPct =
    telemetry && telemetry.totalBytes > 0
      ? Math.min(
          100,
          Math.round(((telemetry.totalBytes - telemetry.freeBytes) / telemetry.totalBytes) * 100)
        )
      : 0;

  return (
    <aside className="sidebar">
      {/* Brand header */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "2px 4px" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <div
            style={{
              width: 34,
              height: 34,
              borderRadius: 11,
              background: "linear-gradient(135deg, #003B54, #0A749A)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
              border: "1px solid rgba(135, 207, 249, 0.35)",
            }}
          >
            <Send size={16} color="#FFFFFF" />
          </div>
          <div>
            <div style={{ fontWeight: 800, fontSize: 15, letterSpacing: "-0.02em" }}>Pix Relay</div>
            <div style={{ fontSize: 11, color: "var(--text-muted)", fontWeight: 600 }}>Auto Mac Sync</div>
          </div>
        </div>
        <button
          className="btn btn-outline btn-sm"
          title={`Theme: ${themeMode} (click to cycle)`}
          onClick={onCycleTheme}
          style={{ padding: "6px 8px" }}
        >
          <ThemeIcon size={14} />
        </button>
      </div>

      {/* Live Backup Pixel HUD Card */}
      <div className="m3-hero-card">
        {primaryPixel ? (
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
                <span className={`pulse-dot ${isOnline ? "online" : "offline"}`} />
                <div style={{ minWidth: 0 }}>
                  <div style={{ fontWeight: 700, fontSize: 13, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {primaryPixel.deviceName}
                  </div>
                  <div style={{ fontSize: 11, color: "var(--text-muted)" }}>
                    {primaryPixel.modelName} · {isOnline ? "Online" : "Checking Wi-Fi..."}
                  </div>
                </div>
              </div>
              <button
                className="btn btn-tonal btn-sm"
                title="Remote Pixel Controls"
                onClick={() => onOpenRemoteSettings(primaryPixel)}
                style={{ padding: "5px 8px" }}
              >
                <Sliders size={13} />
              </button>
            </div>

            {telemetry && (
              <>
                <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
                  <span className="badge badge-muted tabular">
                    <BatteryCharging size={11} /> {telemetry.batteryPercent}%{telemetry.isCharging ? " ⚡" : ""}
                  </span>
                  <span
                    className={`badge tabular ${telemetry.isThermalPaused ? "badge-warm" : "badge-muted"}`}
                  >
                    <Thermometer size={11} /> {telemetry.temperatureCelsius.toFixed(1)}°C
                  </span>
                  <span
                    className={`badge tabular ${telemetry.isStoragePaused ? "badge-error" : "badge-muted"}`}
                  >
                    <HardDrive size={11} /> {formatBytes(telemetry.freeBytes)} free
                  </span>
                </div>

                <div>
                  <div className="progress-track" style={{ height: 5 }}>
                    <div
                      className={`progress-fill ${telemetry.isStoragePaused ? "" : "verified"}`}
                      style={{
                        width: `${usedPct}%`,
                        background: telemetry.isStoragePaused ? "var(--error)" : undefined,
                      }}
                    />
                  </div>
                  <div style={{ fontSize: 11, color: "var(--text-muted)", marginTop: 5 }}>
                    {telemetry.friendlyCloudStatus || "Ready on Wi-Fi"}
                    {telemetry.stagedFilesOnDiskCount > 0 && ` · ${telemetry.stagedFilesOnDiskCount} uploading`}
                  </div>
                </div>
              </>
            )}
          </div>
        ) : (
          <div style={{ display: "flex", flexDirection: "column", gap: 8 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
              <Smartphone size={16} color="var(--primary)" />
              <div style={{ fontWeight: 700, fontSize: 13 }}>No Backup Pixel Paired</div>
            </div>
            <div style={{ fontSize: 12, color: "var(--text-muted)", lineHeight: 1.4 }}>
              Pair your Pixel 1–4 receiver over Wi-Fi to start automatic encrypted backups.
            </div>
            <button
              className="btn btn-primary btn-sm"
              onClick={() => onNavigate({ kind: "PAIRING" })}
            >
              <Wifi size={13} /> Pair Pixel
            </button>
          </div>
        )}
      </div>

      {/* Primary Nav */}
      <div style={{ display: "flex", flexDirection: "column", gap: 3 }}>
        <button
          className={`nav-item ${page.kind === "FOLDERS_OVERVIEW" ? "active" : ""}`}
          onClick={() => onNavigate({ kind: "FOLDERS_OVERVIEW" })}
        >
          <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <FolderOpen size={15} /> All Folders
          </span>
          <span className="badge badge-muted tabular">{snapshot.folders.length}</span>
        </button>

        <button
          className={`nav-item ${page.kind === "FREE_UP_SPACE" ? "active" : ""}`}
          onClick={() => onNavigate({ kind: "FREE_UP_SPACE" })}
        >
          <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Trash2 size={15} /> Free Up Space
          </span>
          {snapshot.totalReclaimableAllSentBytes > 0 && (
            <span
              className={`badge tabular ${
                snapshot.totalReclaimableClearedBytes > 0 ? "badge-verified" : "badge-primary"
              }`}
            >
              {formatBytes(
                snapshot.totalReclaimableClearedBytes > 0
                  ? snapshot.totalReclaimableClearedBytes
                  : snapshot.totalReclaimableAllSentBytes
              )}
            </span>
          )}
        </button>
      </div>

      {/* Included Folders Section */}
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", padding: "2px 4px" }}>
        <span className="eyebrow">Included Folders</span>
        <button className="btn btn-tonal btn-sm" onClick={onAddFolder}>
          <FolderPlus size={13} /> Add
        </button>
      </div>

      <div className="pill-bar" style={{ width: "100%", justifyContent: "space-between" }}>
        {(["ALL", "COMPLETE", "PENDING"] as const).map((tab) => (
          <button
            key={tab}
            className={`pill-btn ${folderFilter === tab ? "active" : ""}`}
            style={{ flex: 1, justifyContent: "center", padding: "4px 6px", fontSize: 11 }}
            onClick={() => setFolderFilter(tab)}
          >
            {tab === "ALL" ? "All" : tab === "COMPLETE" ? "100% ✓" : "Pending"}
          </button>
        ))}
      </div>

      <div style={{ flex: 1, overflowY: "auto", display: "flex", flexDirection: "column", gap: 4, minHeight: 0 }}>
        {filteredFolders.length === 0 ? (
          <div style={{ fontSize: 12, color: "var(--text-muted)", padding: "14px 8px", textAlign: "center" }}>
            {snapshot.folders.length === 0
              ? "Click Add or drag a photo folder here."
              : "No folders match this filter."}
          </div>
        ) : (
          filteredFolders.map((folder) => (
            <SidebarFolderRow
              key={folder.folderId}
              folder={folder}
              active={page.kind === "FOLDER_GALLERY" && page.folderId === folder.folderId}
              onClick={() => onNavigate({ kind: "FOLDER_GALLERY", folderId: folder.folderId })}
            />
          ))
        )}
      </div>

      {/* Bottom Settings / Pairing + Edition Badge */}
      <div style={{ borderTop: "1px solid var(--border)", paddingTop: 8, display: "flex", flexDirection: "column", gap: 4 }}>
        <button
          className={`nav-item ${page.kind === "PAIRING" ? "active" : ""}`}
          onClick={() => onNavigate({ kind: "PAIRING" })}
        >
          <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Wifi size={15} /> Pair Pixel
          </span>
          {primaryPixel && <span className="badge badge-verified">Paired</span>}
        </button>

        <button
          className={`nav-item ${page.kind === "SETTINGS" ? "active" : ""}`}
          onClick={() => onNavigate({ kind: "SETTINGS" })}
        >
          <span style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <Settings size={15} /> Settings
          </span>
        </button>

        {license && (
          <div
            style={{
              marginTop: 4,
              padding: "9px 11px",
              borderRadius: 14,
              background:
                license.mode === "TRIAL_EXPIRED"
                  ? "var(--error-container)"
                  : license.mode === "UNLOCKED"
                    ? "var(--verified-container)"
                    : "var(--surface-container)",
              border: "1px solid var(--border)",
              display: "flex",
              alignItems: "center",
              justifyContent: "space-between",
              gap: 8,
            }}
          >
            <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
              {license.mode === "UNLOCKED" ? (
                <Sparkles size={14} color="var(--verified)" />
              ) : (
                <Lock size={14} color={license.mode === "TRIAL_EXPIRED" ? "var(--error)" : "var(--secondary)"} />
              )}
              <span style={{ fontSize: 11, fontWeight: 700, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {license.statusLabel}
              </span>
            </div>
            {license.requiresLicense && license.mode !== "UNLOCKED" && (
              <button className="btn btn-primary btn-sm" style={{ padding: "4px 9px", fontSize: 11 }} onClick={onOpenLicenseSettings}>
                Unlock
              </button>
            )}
          </div>
        )}
      </div>
    </aside>
  );
}

function SidebarFolderRow({
  folder,
  active,
  onClick,
}: {
  folder: FolderSummary;
  active: boolean;
  onClick: () => void;
}) {
  const pct = Math.round(folder.percentUploaded);
  return (
    <button
      className={`folder-sidebar-row ${active ? "active" : ""}`}
      onClick={onClick}
    >
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 6 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
          {folder.isApplePhotosLibrary ? (
            <Image size={15} color={folder.is100PercentUploaded ? "var(--verified)" : "var(--primary)"} style={{ flexShrink: 0 }} />
          ) : folder.is100PercentUploaded ? (
            <FolderCheck size={15} color="var(--verified)" style={{ flexShrink: 0 }} />
          ) : (
            <FolderOpen size={15} color="var(--primary)" style={{ flexShrink: 0 }} />
          )}
          <span
            style={{
              fontWeight: 700,
              fontSize: 13,
              overflow: "hidden",
              textOverflow: "ellipsis",
              whiteSpace: "nowrap",
            }}
          >
            {folder.displayName}
          </span>
        </div>

        {folder.is100PercentUploaded ? (
          <span className="badge badge-verified tabular" style={{ flexShrink: 0 }}>
            <Check size={10} /> 100%
          </span>
        ) : folder.unsentCount > 0 ? (
          <span className="badge badge-primary tabular" style={{ flexShrink: 0 }}>
            {folder.unsentCount} left
          </span>
        ) : (
          <span className="badge badge-muted" style={{ flexShrink: 0 }}>Empty</span>
        )}
      </div>

      <div className="progress-track" style={{ height: 4 }}>
        <div
          className={`progress-fill ${folder.is100PercentUploaded ? "verified" : ""}`}
          style={{ width: `${pct}%` }}
        />
      </div>

      <div style={{ display: "flex", justifyContent: "space-between", fontSize: 11, color: "var(--text-muted)" }}>
        <span className="tabular">
          {folder.onPixelCount + folder.clearedCount} / {folder.totalCount} files
        </span>
        <span>
          {folder.isApplePhotosLibrary ? "Apple Photos" : folder.policy === "AUTO" ? "Auto" : folder.policy === "CHOOSE" ? "Manual" : "Skip"}
        </span>
      </div>
    </button>
  );
}
