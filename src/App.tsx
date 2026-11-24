import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open } from "@tauri-apps/plugin-dialog";
import { FolderPlus } from "lucide-react";
import { FolderGallery } from "./components/FolderGallery";
import { FoldersOverview } from "./components/FoldersOverview";
import { FreeUpSpaceView } from "./components/FreeUpSpaceView";
import { MatrixBackdrop } from "./components/MatrixBackdrop";
import { PairingView } from "./components/PairingView";
import { PixelRemoteSettingsModal } from "./components/PixelRemoteSettingsModal";
import { SettingsView } from "./components/SettingsView";
import { NavPage, Sidebar } from "./components/Sidebar";
import { TransferBar } from "./components/TransferBar";
import {
  DiscoveredPeer,
  LibrarySnapshot,
  MacLicenseStatus,
  MacSettings,
  PairedDevice,
  ReceiverTelemetry,
  ThemeMode,
  TransferProgressEvent,
} from "./types";

const EMPTY_SNAPSHOT: LibrarySnapshot = {
  folders: [],
  totalMediaCount: 0,
  totalUnsentCount: 0,
  totalClearedCount: 0,
  totalReclaimableClearedBytes: 0,
  totalReclaimableAllSentBytes: 0,
};

export function App() {
  const [page, setPage] = useState<NavPage>({ kind: "FOLDERS_OVERVIEW" });
  const [settings, setSettings] = useState<MacSettings | null>(null);
  const [snapshot, setSnapshot] = useState<LibrarySnapshot>(EMPTY_SNAPSHOT);
  const [peers, setPeers] = useState<DiscoveredPeer[]>([]);
  const [pairedDevices, setPairedDevices] = useState<PairedDevice[]>([]);
  const [telemetry, setTelemetry] = useState<ReceiverTelemetry | null>(null);
  const [transferStatus, setTransferStatus] = useState<TransferProgressEvent>({ state: "IDLE" });
  const [license, setLicense] = useState<MacLicenseStatus | null>(null);
  const [remoteSettingsTarget, setRemoteSettingsTarget] = useState<PairedDevice | null>(null);
  const [cleanupFolderHint, setCleanupFolderHint] = useState<string | null>(null);
  const [isDraggingOver, setIsDraggingOver] = useState(false);
  const [refreshToken, setRefreshToken] = useState(0);
  const [toast, setToast] = useState<string | null>(null);

  const showToast = (msg: string) => {
    setToast(msg);
    setTimeout(() => setToast(null), 4000);
  };

  const refreshAll = useCallback(async () => {
    const [s, snap, disc, paired, lic, status] = await Promise.all([
      invoke<MacSettings>("get_settings"),
      invoke<LibrarySnapshot>("get_library_snapshot"),
      invoke<DiscoveredPeer[]>("get_discovered_peers"),
      invoke<PairedDevice[]>("get_paired_devices"),
      invoke<MacLicenseStatus>("get_license_status"),
      invoke<TransferProgressEvent>("get_transfer_status"),
    ]);
    setSettings(s);
    setSnapshot(snap);
    setPeers(disc);
    setPairedDevices(paired);
    setLicense(lic);
    setTransferStatus(status);
    setRefreshToken((t) => t + 1);
  }, []);

  useEffect(() => {
    refreshAll();
    invoke<ReceiverTelemetry | null>("poll_pixel_telemetry")
      .then((t) => { if (t) setTelemetry(t); })
      .catch(() => {});

    const unlisteners: Array<() => void> = [];
    listen<DiscoveredPeer[]>("peers-updated", (e) => setPeers(e.payload)).then((u) => unlisteners.push(u));
    listen<ReceiverTelemetry>("pixel-telemetry", (e) => setTelemetry(e.payload)).then((u) => unlisteners.push(u));
    listen<TransferProgressEvent>("transfer-progress", (e) => setTransferStatus(e.payload)).then((u) => unlisteners.push(u));
    listen("library-updated", () => {
      invoke<LibrarySnapshot>("get_library_snapshot").then(setSnapshot);
      invoke<PairedDevice[]>("get_paired_devices").then(setPairedDevices);
      invoke<MacSettings>("get_settings").then(setSettings);
      setRefreshToken((t) => t + 1);
    }).then((u) => unlisteners.push(u));

    const win = getCurrentWebviewWindow();
    win.onDragDropEvent(async (event) => {
      if (event.payload.type === "enter" || event.payload.type === "over") {
        setIsDraggingOver(true);
      } else if (event.payload.type === "leave") {
        setIsDraggingOver(false);
      } else if (event.payload.type === "drop") {
        setIsDraggingOver(false);
        const paths = event.payload.paths ?? [];
        if (paths.length > 0) {
          const res = await invoke<{ addedFolders: number; queuedFiles: number }>(
            "handle_dropped_paths",
            { paths }
          );
          if (res.addedFolders > 0) {
            showToast(`Added ${res.addedFolders} folder(s) to Pix Relay`);
          } else if (res.queuedFiles > 0) {
            showToast(`Sending ${res.queuedFiles} dropped item(s) to Pixel...`);
          }
          refreshAll();
        }
      }
    }).then((u) => unlisteners.push(u));

    return () => {
      unlisteners.forEach((fn) => fn());
    };
  }, [refreshAll]);

  const resolvedTheme: "DARK" | "LIGHT" | "MATRIX" = (() => {
    const mode = settings?.themeMode ?? "DARK";
    if (mode === "SYSTEM") {
      return window.matchMedia("(prefers-color-scheme: light)").matches ? "LIGHT" : "DARK";
    }
    return mode;
  })();

  useEffect(() => {
    document.documentElement.setAttribute("data-theme", resolvedTheme);
  }, [resolvedTheme]);

  const primaryPixel =
    pairedDevices.find((d) => d.deviceId === settings?.selectedReceiverDeviceId) ??
    pairedDevices[0] ??
    null;

  const handleAddFolder = async () => {
    const selected = await open({
      directory: true,
      multiple: true,
      title: "Choose Mac Folders or Photos Library to Sync to Pixel",
    });
    if (!selected) return;
    const dirs = Array.isArray(selected) ? selected : [selected];
    for (const path of dirs) {
      await invoke("add_watched_folder", { path, policy: "AUTO", recursive: true });
    }
    refreshAll();
  };

  const handleUpdateSettings = async (next: MacSettings) => {
    const saved = await invoke<MacSettings>("update_settings", { settings: next });
    setSettings(saved);
  };

  const cycleTheme = () => {
    if (!settings) return;
    const order: ThemeMode[] = ["DARK", "LIGHT", "MATRIX", "SYSTEM"];
    const next = order[(order.indexOf(settings.themeMode) + 1) % order.length];
    handleUpdateSettings({ ...settings, themeMode: next });
  };

  const activeFolder =
    page.kind === "FOLDER_GALLERY" && page.folderId
      ? snapshot.folders.find((f) => f.folderId === page.folderId) ?? null
      : null;

  return (
    <div className="app-shell">
      <MatrixBackdrop active={resolvedTheme === "MATRIX"} />

      <Sidebar
        page={page}
        onNavigate={(next) => {
          if (next.kind === "FREE_UP_SPACE") setCleanupFolderHint(null);
          setPage(next);
        }}
        snapshot={snapshot}
        primaryPixel={primaryPixel}
        telemetry={telemetry}
        license={license}
        themeMode={settings?.themeMode ?? "DARK"}
        onCycleTheme={cycleTheme}
        onAddFolder={handleAddFolder}
        onOpenRemoteSettings={(dev) => setRemoteSettingsTarget(dev)}
        onOpenLicenseSettings={() => setPage({ kind: "SETTINGS" })}
      />

      <main className="main-pane">
        {page.kind === "FOLDER_GALLERY" ? (
          <FolderGallery
            folder={activeFolder}
            hasPairedPixel={pairedDevices.length > 0}
            refreshToken={refreshToken}
            onBack={() => setPage({ kind: "FOLDERS_OVERVIEW" })}
            onGoToFreeUpSpace={(fid) => {
              setCleanupFolderHint(fid ?? null);
              setPage({ kind: "FREE_UP_SPACE" });
            }}
            onGoToPairing={() => setPage({ kind: "PAIRING" })}
            onRefreshLibrary={refreshAll}
          />
        ) : (
          <div className="main-scroll">
            {page.kind === "FOLDERS_OVERVIEW" && (
              <FoldersOverview
                snapshot={snapshot}
                hasPairedPixel={pairedDevices.length > 0}
                onAddFolder={handleAddFolder}
                onOpenFolder={(folderId) => setPage({ kind: "FOLDER_GALLERY", folderId })}
                onGoToFreeUpSpace={(fid) => {
                  setCleanupFolderHint(fid ?? null);
                  setPage({ kind: "FREE_UP_SPACE" });
                }}
                onGoToPairing={() => setPage({ kind: "PAIRING" })}
                onRefresh={refreshAll}
              />
            )}

            {page.kind === "FREE_UP_SPACE" && (
              <FreeUpSpaceView
                snapshot={snapshot}
                initialFolderId={cleanupFolderHint}
                telemetry={telemetry}
                onCompleted={refreshAll}
              />
            )}

            {page.kind === "PAIRING" && (
              <PairingView
                discoveredPeers={peers}
                pairedDevices={pairedDevices}
                selectedReceiverId={settings?.selectedReceiverDeviceId ?? null}
                onRefresh={refreshAll}
                onOpenRemoteSettings={(dev) => setRemoteSettingsTarget(dev)}
              />
            )}

            {page.kind === "SETTINGS" && settings && (
              <SettingsView
                settings={settings}
                license={license}
                onUpdateSettings={handleUpdateSettings}
                onLicenseChanged={() =>
                  invoke<MacLicenseStatus>("get_license_status").then(setLicense)
                }
              />
            )}
          </div>
        )}

        <TransferBar
          status={transferStatus}
          onDismiss={() => setTransferStatus({ state: "IDLE" })}
        />
      </main>

      {remoteSettingsTarget && (
        <PixelRemoteSettingsModal
          device={remoteSettingsTarget}
          onClose={() => setRemoteSettingsTarget(null)}
          onSaved={() => {
            invoke<ReceiverTelemetry | null>("poll_pixel_telemetry").then((t) => {
              if (t) setTelemetry(t);
            });
          }}
        />
      )}

      {isDraggingOver && (
        <div className="drop-overlay">
          <FolderPlus size={44} color="var(--primary)" />
          <div style={{ fontSize: 20, fontWeight: 800 }}>Drop Folders, Photos Library, or Photos</div>
          <div style={{ fontSize: 13, color: "var(--text-muted)" }}>
            Folders &amp; Apple Photos Libraries will be watched automatically; loose photos will upload right away.
          </div>
        </div>
      )}

      {toast && (
        <div
          style={{
            position: "fixed",
            top: 16,
            right: 20,
            background: "var(--surface-highest)",
            border: "1px solid var(--hero-border)",
            borderRadius: 12,
            padding: "10px 14px",
            fontSize: 13,
            fontWeight: 600,
            boxShadow: "0 10px 28px rgba(0,0,0,0.4)",
            zIndex: 70,
          }}
        >
          {toast}
        </div>
      )}
    </div>
  );
}
export default App;
