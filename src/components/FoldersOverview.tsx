import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Check,
  CheckCheck,
  ExternalLink,
  FolderCheck,
  FolderOpen,
  FolderPlus,
  Image,
  Lock,
  RefreshCw,
  Send,
  Trash2,
  Upload,
} from "lucide-react";
import {
  ApplePhotosLibraryInfo,
  FolderPolicy,
  LibrarySnapshot,
  formatBytes,
  thumbUrl,
} from "../types";

interface Props {
  snapshot: LibrarySnapshot;
  hasPairedPixel: boolean;
  onAddFolder: () => void;
  onOpenFolder: (folderId: string) => void;
  onGoToFreeUpSpace: (folderId?: string) => void;
  onGoToPairing: () => void;
  onRefresh: () => void;
}

export function FoldersOverview({
  snapshot,
  hasPairedPixel,
  onAddFolder,
  onOpenFolder,
  onGoToFreeUpSpace,
  onGoToPairing,
  onRefresh,
}: Props) {
  const [filter, setFilter] = useState<"ALL" | "COMPLETE" | "PENDING">("ALL");
  const [appleLibraries, setAppleLibraries] = useState<ApplePhotosLibraryInfo[]>([]);
  const [connectingLib, setConnectingLib] = useState(false);

  const loadAppleLibraries = () => {
    invoke<ApplePhotosLibraryInfo[]>("detect_apple_photos_libraries")
      .then(setAppleLibraries)
      .catch(() => {});
  };

  useEffect(() => {
    loadAppleLibraries();
  }, [snapshot.folders.length]);

  const unaddedAppleLibraries = appleLibraries.filter((lib) => !lib.alreadyAdded);

  const handleConnectApplePhotos = async (libPath: string) => {
    setConnectingLib(true);
    try {
      await invoke("add_apple_photos_library", { path: libPath });
      loadAppleLibraries();
      onRefresh();
    } finally {
      setConnectingLib(false);
    }
  };

  const visibleFolders = snapshot.folders.filter((f) => {
    if (filter === "COMPLETE") return f.is100PercentUploaded;
    if (filter === "PENDING") return f.unsentCount > 0;
    return true;
  });

  const completeCount = snapshot.folders.filter((f) => f.is100PercentUploaded).length;

  const setPolicy = async (folderId: string, policy: FolderPolicy) => {
    await invoke("update_folder_policy", { folderId, policy });
    onRefresh();
  };

  const handleRemove = async (folderId: string) => {
    await invoke("remove_watched_folder", { folderId });
    onRefresh();
  };

  const handleSyncAllAuto = async () => {
    if (!hasPairedPixel) {
      onGoToPairing();
      return;
    }
    await invoke("send_all_auto_unsent");
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
      {/* Top Header & Summary Banner */}
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-end", flexWrap: "wrap", gap: 12 }}>
        <div>
          <div className="eyebrow" style={{ marginBottom: 4 }}>Mac Relay Library</div>
          <h1 style={{ margin: 0, fontSize: 24, fontWeight: 800, letterSpacing: "-0.02em" }}>
            Included Folders
          </h1>
          <div style={{ fontSize: 13, color: "var(--text-muted)", marginTop: 4 }} className="tabular">
            {snapshot.folders.length} sources · {snapshot.totalMediaCount} items ·{" "}
            <strong style={{ color: "var(--verified)" }}>{completeCount} completely uploaded</strong> ·{" "}
            {snapshot.totalUnsentCount} pending
          </div>
        </div>

        <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
          <button className="btn btn-outline" onClick={() => invoke("sync_cleared_status_now").then(onRefresh)}>
            <RefreshCw size={14} /> Check Pixel Uploads
          </button>
          <button className="btn btn-tonal" onClick={onAddFolder}>
            <FolderPlus size={14} /> Add Folder
          </button>
          <button
            className="btn btn-primary"
            onClick={handleSyncAllAuto}
            disabled={snapshot.totalUnsentCount === 0}
          >
            <Send size={14} /> Sync Auto Folders
          </button>
        </div>
      </div>

      {/* Apple Photos Library One-Click Sync Card */}
      {unaddedAppleLibraries.map((lib) => (
        <div
          key={lib.path}
          className="m3-hero-card"
          style={{
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
            flexWrap: "wrap",
            gap: 14,
          }}
        >
          <div style={{ display: "flex", alignItems: "center", gap: 12, flex: "1 1 320px" }}>
            <div
              style={{
                width: 44,
                height: 44,
                borderRadius: 14,
                background: "var(--primary-container)",
                display: "flex",
                alignItems: "center",
                justifyContent: "center",
                flexShrink: 0,
              }}
            >
              <Image size={22} color="var(--primary)" />
            </div>
            <div>
              <div style={{ fontWeight: 800, fontSize: 14, display: "flex", alignItems: "center", gap: 8 }}>
                Apple Photos Library Detected: {lib.displayName}
                <span className="badge badge-primary">Read-Only Safe Sync</span>
              </div>
              <div style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 3, lineHeight: 1.45 }}>
                {lib.needsPermission
                  ? "macOS requires permission to read originals inside your Photos Library. Grant Full Disk Access in Privacy & Security, then re-check."
                  : "Sync original full-resolution photos & videos straight from your Apple Photos Library to your Backup Pixel automatically — without altering your Mac library."}
              </div>
            </div>
          </div>

          <div style={{ display: "flex", gap: 8, alignItems: "center" }}>
            {lib.needsPermission ? (
              <>
                <button
                  className="btn btn-outline btn-sm"
                  onClick={() => invoke("open_macos_settings", { pane: "full-disk-access" })}
                >
                  <Lock size={13} /> Open Privacy Settings
                </button>
                <button className="btn btn-tonal btn-sm" onClick={loadAppleLibraries}>
                  <RefreshCw size={13} /> Recheck
                </button>
              </>
            ) : (
              <button
                className="btn btn-primary"
                disabled={connectingLib}
                onClick={() => handleConnectApplePhotos(lib.path)}
              >
                <ExternalLink size={14} /> {connectingLib ? "Connecting..." : "Sync Apple Photos Library"}
              </button>
            )}
          </div>
        </div>
      ))}

      {/* Filter Pills */}
      {snapshot.folders.length > 0 && (
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", flexWrap: "wrap", gap: 10 }}>
          <div className="pill-bar">
            <button className={`pill-btn ${filter === "ALL" ? "active" : ""}`} onClick={() => setFilter("ALL")}>
              All Folders ({snapshot.folders.length})
            </button>
            <button className={`pill-btn ${filter === "COMPLETE" ? "active" : ""}`} onClick={() => setFilter("COMPLETE")}>
              <CheckCheck size={13} /> 100% Uploaded ({completeCount})
            </button>
            <button className={`pill-btn ${filter === "PENDING" ? "active" : ""}`} onClick={() => setFilter("PENDING")}>
              <Upload size={13} /> Pending ({snapshot.folders.filter((f) => f.unsentCount > 0).length})
            </button>
          </div>

          {snapshot.totalReclaimableAllSentBytes > 0 && (
            <button className="btn btn-verified btn-sm" onClick={() => onGoToFreeUpSpace()}>
              <Trash2 size={13} /> Free Up Space ({formatBytes(
                snapshot.totalReclaimableClearedBytes > 0
                  ? snapshot.totalReclaimableClearedBytes
                  : snapshot.totalReclaimableAllSentBytes
              )})
            </button>
          )}
        </div>
      )}

      {/* Empty Welcome Card */}
      {snapshot.folders.length === 0 ? (
        <div
          className="m3-hero-card"
          style={{
            padding: 36,
            textAlign: "center",
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: 14,
            marginTop: 12,
          }}
        >
          <div
            style={{
              width: 60,
              height: 60,
              borderRadius: 18,
              background: "var(--primary-container)",
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <FolderPlus size={28} color="var(--primary)" />
          </div>
          <div style={{ fontSize: 18, fontWeight: 800 }}>Add your first Mac photo folder</div>
          <div style={{ fontSize: 13, color: "var(--text-muted)", maxWidth: 460, lineHeight: 1.5 }}>
            Pick any Mac folder (or drag and drop folders or a{" "}
            <code>.photoslibrary</code> bundle directly into this window). Pix Relay tracks which photos
            are 100% uploaded to your Pixel 1–4 so you can safely free up Mac storage anytime.
          </div>
          <div style={{ display: "flex", gap: 10 }}>
            <button className="btn btn-primary" onClick={onAddFolder}>
              <FolderPlus size={15} /> Choose Mac Folder
            </button>
            {!hasPairedPixel && (
              <button className="btn btn-outline" onClick={onGoToPairing}>
                Pair Backup Pixel
              </button>
            )}
          </div>
        </div>
      ) : (
        <div
          style={{
            display: "grid",
            gridTemplateColumns: "repeat(auto-fill, minmax(330px, 1fr))",
            gap: 14,
          }}
        >
          {visibleFolders.map((folder) => {
            const uploadedCount = folder.onPixelCount + folder.clearedCount;
            const pct = Math.round(folder.percentUploaded);

            return (
              <div
                key={folder.folderId}
                className="m3-card"
                style={{
                  display: "flex",
                  flexDirection: "column",
                  gap: 12,
                  borderColor: folder.is100PercentUploaded
                    ? "rgba(92, 224, 168, 0.35)"
                    : "var(--border)",
                }}
              >
                {/* Header row with cover thumb + status pill */}
                <div style={{ display: "flex", gap: 12, alignItems: "center" }}>
                  <div
                    onClick={() => onOpenFolder(folder.folderId)}
                    style={{
                      width: 64,
                      height: 64,
                      borderRadius: 14,
                      background: "var(--placeholder)",
                      overflow: "hidden",
                      flexShrink: 0,
                      cursor: "pointer",
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "center",
                      border: "1px solid var(--border)",
                    }}
                  >
                    {folder.coverPath ? (
                      <img
                        src={thumbUrl(folder.coverPath)}
                        alt={folder.displayName}
                        style={{ width: "100%", height: "100%", objectFit: "cover" }}
                      />
                    ) : (
                      <FolderOpen size={24} color="var(--text-muted)" />
                    )}
                  </div>

                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 6 }}>
                      <div
                        onClick={() => onOpenFolder(folder.folderId)}
                        style={{
                          fontWeight: 800,
                          fontSize: 15,
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                          cursor: "pointer",
                        }}
                      >
                        {folder.displayName}
                      </div>
                      {folder.is100PercentUploaded ? (
                        <span className="badge badge-verified tabular">
                          <FolderCheck size={12} /> 100% Uploaded
                        </span>
                      ) : folder.unsentCount > 0 ? (
                        <span className="badge badge-primary tabular">
                          {folder.unsentCount} unsent
                        </span>
                      ) : (
                        <span className="badge badge-muted">0 files</span>
                      )}
                    </div>

                    <div
                      title={folder.path}
                      style={{
                        fontSize: 11,
                        color: "var(--text-muted)",
                        overflow: "hidden",
                        textOverflow: "ellipsis",
                        whiteSpace: "nowrap",
                        marginTop: 3,
                      }}
                    >
                      {folder.isApplePhotosLibrary ? (
                        <span className="badge badge-primary" style={{ padding: "1px 7px", fontSize: 10, marginRight: 6 }}>
                          Apple Photos
                        </span>
                      ) : null}
                      {folder.path}
                    </div>

                    <div className="tabular" style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 4 }}>
                      {uploadedCount} / {folder.totalCount} uploaded · {formatBytes(folder.totalBytes)}
                    </div>
                  </div>
                </div>

                {/* Progress bar */}
                <div>
                  <div className="progress-track">
                    <div
                      className={`progress-fill ${folder.is100PercentUploaded ? "verified" : ""}`}
                      style={{ width: `${pct}%` }}
                    />
                  </div>
                  <div style={{ display: "flex", justifyContent: "space-between", fontSize: 11, color: "var(--text-muted)", marginTop: 5 }} className="tabular">
                    <span>
                      <Check size={10} style={{ verticalAlign: "middle" }} /> {folder.onPixelCount} on Pixel ·{" "}
                      <CheckCheck size={10} style={{ verticalAlign: "middle", color: "var(--verified)" }} /> {folder.clearedCount} cleared by Pixel
                    </span>
                    <span>{pct}%</span>
                  </div>
                </div>

                {/* Policy Selector */}
                <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
                  <div className="pill-bar">
                    {(["AUTO", "CHOOSE", "SKIP"] as FolderPolicy[]).map((pol) => (
                      <button
                        key={pol}
                        className={`pill-btn ${folder.policy === pol ? "active" : ""}`}
                        style={{ padding: "3px 9px", fontSize: 11 }}
                        onClick={() => setPolicy(folder.folderId, pol)}
                      >
                        {pol === "AUTO" ? "Auto" : pol === "CHOOSE" ? "Manual" : "Skip"}
                      </button>
                    ))}
                  </div>
                  <span style={{ fontSize: 11, color: "var(--text-muted)" }}>
                    {folder.isApplePhotosLibrary
                      ? "Originals (Read-Only)"
                      : folder.recursive
                        ? "Subfolders On"
                        : "Top Folder"}
                  </span>
                </div>

                {/* Actions */}
                <div style={{ display: "flex", gap: 8, flexWrap: "wrap", borderTop: "1px solid var(--border)", paddingTop: 10 }}>
                  <button className="btn btn-outline btn-sm" onClick={() => onOpenFolder(folder.folderId)}>
                    View Photos
                  </button>

                  {folder.unsentCount > 0 && (
                    <button
                      className="btn btn-primary btn-sm"
                      onClick={() =>
                        hasPairedPixel
                          ? invoke("send_folder_unsent", { folderId: folder.folderId })
                          : onGoToPairing()
                      }
                    >
                      <Send size={12} /> Send {folder.unsentCount}
                    </button>
                  )}

                  {!folder.isApplePhotosLibrary && folder.reclaimableAllSentBytes > 0 && (
                    <button
                      className="btn btn-verified btn-sm"
                      onClick={() => onGoToFreeUpSpace(folder.folderId)}
                    >
                      <Trash2 size={12} /> Free {formatBytes(
                        folder.reclaimableClearedBytes > 0
                          ? folder.reclaimableClearedBytes
                          : folder.reclaimableAllSentBytes
                      )}
                    </button>
                  )}

                  <button
                    className="btn btn-outline btn-sm"
                    style={{ marginLeft: "auto", color: "var(--text-muted)" }}
                    title="Remove folder from Pix Relay (does not delete files)"
                    onClick={() => handleRemove(folder.folderId)}
                  >
                    Remove
                  </button>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
