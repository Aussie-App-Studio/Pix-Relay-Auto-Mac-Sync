import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import {
  AlertTriangle,
  Archive,
  CheckCheck,
  FolderOpen,
  Lock,
  RefreshCw,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import {
  CleanupActionMode,
  CleanupExecuteResult,
  CleanupPreview,
  LibrarySnapshot,
  ReceiverTelemetry,
  formatBytes,
} from "../types";

interface Props {
  snapshot: LibrarySnapshot;
  initialFolderId: string | null;
  telemetry: ReceiverTelemetry | null;
  onCompleted: () => void;
}

export function FreeUpSpaceView({ snapshot, initialFolderId, telemetry, onCompleted }: Props) {
  const cleanableFolders = snapshot.folders.filter((f) => !f.isApplePhotosLibrary);
  const protectedLibraries = snapshot.folders.filter((f) => f.isApplePhotosLibrary);

  const [selectedFolderIds, setSelectedFolderIds] = useState<Set<string>>(() => {
    if (initialFolderId && cleanableFolders.some((f) => f.folderId === initialFolderId)) {
      return new Set([initialFolderId]);
    }
    return new Set(cleanableFolders.map((f) => f.folderId));
  });
  const [includeOnPixelUnconfirmed, setIncludeOnPixelUnconfirmed] = useState(false);
  const [minAgeDays, setMinAgeDays] = useState<number>(0);
  const [mode, setMode] = useState<CleanupActionMode>("TRASH");
  const [archiveRoot, setArchiveRoot] = useState<string>("");
  const [removeEmptyFolders, setRemoveEmptyFolders] = useState(false);
  const [preview, setPreview] = useState<CleanupPreview | null>(null);
  const [loadingPreview, setLoadingPreview] = useState(false);
  const [executing, setExecuting] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [result, setResult] = useState<CleanupExecuteResult | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (initialFolderId && snapshot.folders.some((f) => f.folderId === initialFolderId && !f.isApplePhotosLibrary)) {
      setSelectedFolderIds(new Set([initialFolderId]));
    }
  }, [initialFolderId, snapshot.folders]);

  const loadPreview = useCallback(async () => {
    setLoadingPreview(true);
    setError(null);
    try {
      const res = await invoke<CleanupPreview>("preview_cleanup", {
        request: {
          folderIds: Array.from(selectedFolderIds),
          includeOnPixelUnconfirmed,
          minAgeDays,
        },
      });
      setPreview(res);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoadingPreview(false);
    }
  }, [selectedFolderIds, includeOnPixelUnconfirmed, minAgeDays]);

  useEffect(() => {
    loadPreview();
  }, [loadPreview]);

  const toggleFolder = (id: string) => {
    setSelectedFolderIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };

  const pickArchiveFolder = async () => {
    const picked = await open({ directory: true, multiple: false, title: "Choose Archive Folder" });
    if (typeof picked === "string") {
      setArchiveRoot(picked);
    }
  };

  const runCleanup = async () => {
    setExecuting(true);
    setError(null);
    try {
      const res = await invoke<CleanupExecuteResult>("execute_cleanup", {
        request: {
          folderIds: Array.from(selectedFolderIds),
          includeOnPixelUnconfirmed,
          minAgeDays,
          mode,
          archiveRoot: mode === "ARCHIVE_FOLDER" ? archiveRoot : null,
          removeEmptyFolders,
        },
      });
      setResult(res);
      setConfirmOpen(false);
      onCompleted();
      loadPreview();
    } catch (e) {
      setError(String(e));
    } finally {
      setExecuting(false);
    }
  };

  const stagedOnPixel = telemetry?.stagedFilesOnDiskCount ?? 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16, maxWidth: 940 }}>
      <div style={{ display: "flex", justifyContent: "space-between", alignItems: "flex-end", flexWrap: "wrap", gap: 12 }}>
        <div>
          <div className="eyebrow" style={{ marginBottom: 4 }}>Safe Mac Storage Cleanup</div>
          <h1 style={{ margin: 0, fontSize: 24, fontWeight: 800 }}>Free Up Space on Mac</h1>
          <div style={{ fontSize: 13, color: "var(--text-muted)", marginTop: 4 }}>
            Clean up Mac copies of media that your Backup Pixel has already uploaded — unsent photos are never touched.
          </div>
        </div>
        <button
          className="btn btn-tonal btn-sm"
          onClick={() => invoke("sync_cleared_status_now").then(() => { onCompleted(); loadPreview(); })}
        >
          <RefreshCw size={13} /> Refresh Pixel Upload Status
        </button>
      </div>

      {/* Cloud Safety Status Banner */}
      {stagedOnPixel > 0 && (
        <div
          className="m3-card"
          style={{
            background: "var(--secondary-container)",
            borderColor: "rgba(245, 185, 113, 0.4)",
            display: "flex",
            alignItems: "center",
            gap: 12,
          }}
        >
          <AlertTriangle size={20} color="var(--secondary)" style={{ flexShrink: 0 }} />
          <div style={{ fontSize: 13 }}>
            <strong style={{ color: "var(--on-secondary-container)" }}>
              Your Pixel still has {stagedOnPixel} {stagedOnPixel === 1 ? "file" : "files"} waiting to upload to Google Photos.
            </strong>{" "}
            <span style={{ color: "var(--on-secondary-container)", opacity: 0.9 }}>
              Keeping "Only files cleared by Pixel" selected ensures you only remove photos that Google Photos has already finished backing up.
            </span>
          </div>
        </div>
      )}

      {result && (
        <div className="m3-card" style={{ background: "var(--verified-container)", borderColor: "rgba(92,224,168,0.4)" }}>
          <div style={{ fontWeight: 700, fontSize: 14, color: "var(--on-verified-container)" }}>
            Done: {result.message} ({formatBytes(result.freedBytes)} freed)
            {result.removedEmptyDirs > 0 ? ` · Removed ${result.removedEmptyDirs} empty folders` : ""}
          </div>
        </div>
      )}

      {error && <div className="badge badge-error" style={{ padding: 10, borderRadius: 12 }}>{error}</div>}

      {/* Main Split Grid */}
      <div style={{ display: "grid", gridTemplateColumns: "1.1fr 0.9fr", gap: 14 }}>
        {/* Left: Folders & Filters */}
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div className="m3-card">
            <div style={{ fontWeight: 700, fontSize: 14, marginBottom: 10 }}>1. Choose Folders to Clean</div>
            {snapshot.folders.length === 0 ? (
              <div style={{ fontSize: 13, color: "var(--text-muted)" }}>No folders added yet.</div>
            ) : (
              <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
                {cleanableFolders.map((f) => (
                  <label
                    key={f.folderId}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      padding: "8px 10px",
                      borderRadius: 12,
                      background: "var(--surface-low)",
                      cursor: "pointer",
                    }}
                  >
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <input
                        type="checkbox"
                        checked={selectedFolderIds.has(f.folderId)}
                        onChange={() => toggleFolder(f.folderId)}
                      />
                      <FolderOpen size={14} color="var(--primary)" />
                      <span style={{ fontWeight: 600, fontSize: 13 }}>{f.displayName}</span>
                      {f.is100PercentUploaded && <span className="badge badge-verified">100%</span>}
                    </div>
                    <span className="tabular" style={{ fontSize: 12, color: "var(--text-muted)" }}>
                      {formatBytes(
                        includeOnPixelUnconfirmed ? f.reclaimableAllSentBytes : f.reclaimableClearedBytes
                      )}
                    </span>
                  </label>
                ))}

                {protectedLibraries.map((lib) => (
                  <div
                    key={lib.folderId}
                    style={{
                      display: "flex",
                      alignItems: "center",
                      justifyContent: "space-between",
                      padding: "8px 10px",
                      borderRadius: 12,
                      background: "var(--surface-low)",
                      opacity: 0.72,
                    }}
                  >
                    <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      <Lock size={14} color="var(--secondary)" />
                      <div>
                        <div style={{ fontWeight: 600, fontSize: 13 }}>{lib.displayName}</div>
                        <div style={{ fontSize: 11, color: "var(--text-muted)" }}>
                          Protected: delete inside the Apple Photos app to avoid library corruption
                        </div>
                      </div>
                    </div>
                    <span className="badge badge-muted">Protected</span>
                  </div>
                ))}
              </div>
            )}
          </div>

          <div className="m3-card" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div style={{ fontWeight: 700, fontSize: 14 }}>2. Safety Scope</div>

            <label style={{ display: "flex", alignItems: "flex-start", gap: 10, cursor: "pointer" }}>
              <input
                type="radio"
                checked={!includeOnPixelUnconfirmed}
                onChange={() => setIncludeOnPixelUnconfirmed(false)}
                style={{ marginTop: 3 }}
              />
              <div>
                <div style={{ fontWeight: 700, fontSize: 13, display: "flex", alignItems: "center", gap: 6 }}>
                  <ShieldCheck size={14} color="var(--verified)" /> Only files confirmed cleared by Pixel (Safest)
                </div>
                <div style={{ fontSize: 12, color: "var(--text-muted)" }}>
                  Google Photos finished uploading and cleared them from your Pixel’s local storage.
                </div>
              </div>
            </label>

            <label style={{ display: "flex", alignItems: "flex-start", gap: 10, cursor: "pointer" }}>
              <input
                type="radio"
                checked={includeOnPixelUnconfirmed}
                onChange={() => setIncludeOnPixelUnconfirmed(true)}
                style={{ marginTop: 3 }}
              />
              <div>
                <div style={{ fontWeight: 700, fontSize: 13 }}>
                  Include all files already sent to Pixel (even if still queued for cloud)
                </div>
                <div style={{ fontSize: 12, color: "var(--text-muted)" }}>
                  Use if you already know your Pixel holds the copy you want.
                </div>
              </div>
            </label>

            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", paddingTop: 4 }}>
              <span style={{ fontSize: 13, fontWeight: 600 }}>Only clean files sent at least:</span>
              <div className="pill-bar">
                {[
                  [0, "Anytime"],
                  [7, "7+ days"],
                  [30, "30+ days"],
                ].map(([days, label]) => (
                  <button
                    key={days}
                    className={`pill-btn ${minAgeDays === days ? "active" : ""}`}
                    onClick={() => setMinAgeDays(Number(days))}
                  >
                    {label}
                  </button>
                ))}
              </div>
            </div>
          </div>
        </div>

        {/* Right: Action Mode & Live Preview */}
        <div style={{ display: "flex", flexDirection: "column", gap: 12 }}>
          <div className="m3-card" style={{ display: "flex", flexDirection: "column", gap: 10 }}>
            <div style={{ fontWeight: 700, fontSize: 14 }}>3. How should files be removed?</div>

            <label style={{ display: "flex", alignItems: "center", gap: 10, padding: 10, borderRadius: 12, background: mode === "TRASH" ? "var(--primary-container)" : "var(--surface-low)", cursor: "pointer" }}>
              <input type="radio" checked={mode === "TRASH"} onChange={() => setMode("TRASH")} />
              <Trash2 size={16} />
              <div>
                <div style={{ fontWeight: 700, fontSize: 13 }}>Move to macOS Trash (Recommended)</div>
                <div style={{ fontSize: 11, color: "var(--text-muted)" }}>Instant undo anytime from Finder Trash</div>
              </div>
            </label>

            <label style={{ display: "flex", alignItems: "center", gap: 10, padding: 10, borderRadius: 12, background: mode === "ARCHIVE_FOLDER" ? "var(--primary-container)" : "var(--surface-low)", cursor: "pointer" }}>
              <input type="radio" checked={mode === "ARCHIVE_FOLDER"} onChange={() => setMode("ARCHIVE_FOLDER")} />
              <Archive size={16} />
              <div>
                <div style={{ fontWeight: 700, fontSize: 13 }}>Move to Archive Drive / Folder</div>
                <div style={{ fontSize: 11, color: "var(--text-muted)" }}>Moves files off your Mac SSD to an external drive</div>
              </div>
            </label>

            {mode === "ARCHIVE_FOLDER" && (
              <div style={{ display: "flex", gap: 8 }}>
                <input
                  className="input-field"
                  readOnly
                  placeholder="Pick archive folder..."
                  value={archiveRoot}
                />
                <button className="btn btn-outline btn-sm" onClick={pickArchiveFolder}>Browse</button>
              </div>
            )}

            <label style={{ display: "flex", alignItems: "center", gap: 10, padding: 10, borderRadius: 12, background: mode === "PERMANENT" ? "var(--error-container)" : "var(--surface-low)", cursor: "pointer" }}>
              <input type="radio" checked={mode === "PERMANENT"} onChange={() => setMode("PERMANENT")} />
              <AlertTriangle size={16} color="var(--error)" />
              <div>
                <div style={{ fontWeight: 700, fontSize: 13 }}>Delete Permanently</div>
                <div style={{ fontSize: 11, color: "var(--text-muted)" }}>Bypasses Finder Trash — cannot be undone</div>
              </div>
            </label>

            <label style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 12, marginTop: 2, cursor: "pointer" }}>
              <input
                type="checkbox"
                checked={removeEmptyFolders}
                onChange={(e) => setRemoveEmptyFolders(e.target.checked)}
              />
              Remove empty subfolders left behind after cleaning
            </label>
          </div>

          {/* Preview & Execute Card */}
          <div className="m3-hero-card" style={{ display: "flex", flexDirection: "column", gap: 12 }}>
            <div className="eyebrow">Ready to Reclaim</div>
            {loadingPreview || !preview ? (
              <div style={{ color: "var(--text-muted)", fontSize: 13 }}>Calculating eligible files...</div>
            ) : (
              <>
                <div style={{ display: "flex", alignItems: "baseline", justifyContent: "space-between" }}>
                  <div className="tabular" style={{ fontSize: 28, fontWeight: 800, color: "var(--verified)" }}>
                    {formatBytes(preview.eligibleBytes)}
                  </div>
                  <div className="tabular" style={{ fontSize: 13, fontWeight: 700 }}>
                    {preview.eligibleCount} {preview.eligibleCount === 1 ? "file" : "files"}
                  </div>
                </div>
                <div className="tabular" style={{ fontSize: 12, color: "var(--text-muted)" }}>
                  <CheckCheck size={12} style={{ verticalAlign: "middle", color: "var(--verified)" }} /> {preview.clearedCount} confirmed cleared ({formatBytes(preview.clearedBytes)})
                  {includeOnPixelUnconfirmed && ` · ${preview.onPixelUnconfirmedCount} on Pixel (${formatBytes(preview.onPixelUnconfirmedBytes)})`}
                </div>

                <button
                  className={mode === "PERMANENT" ? "btn btn-danger" : "btn btn-verified"}
                  style={{ width: "100%", padding: "11px 16px" }}
                  disabled={preview.eligibleCount === 0 || executing || (mode === "ARCHIVE_FOLDER" && !archiveRoot)}
                  onClick={() => setConfirmOpen(true)}
                >
                  <Trash2 size={15} /> Clean {preview.eligibleCount} Files ({formatBytes(preview.eligibleBytes)})
                </button>
              </>
            )}
          </div>
        </div>
      </div>

      {/* Confirmation Modal */}
      {confirmOpen && preview && (
        <div className="modal-backdrop" onClick={() => setConfirmOpen(false)}>
          <div className="modal-card" onClick={(e) => e.stopPropagation()}>
            <h3 style={{ margin: "0 0 8px", fontSize: 18 }}>Confirm Free Up Space</h3>
            <p style={{ fontSize: 13, color: "var(--text-muted)", lineHeight: 1.5 }}>
              You are about to{" "}
              <strong>
                {mode === "TRASH"
                  ? "move to Trash"
                  : mode === "ARCHIVE_FOLDER"
                    ? `move to ${archiveRoot}`
                    : "permanently delete"}
              </strong>{" "}
              <strong className="tabular">{preview.eligibleCount}</strong> uploaded files totaling{" "}
              <strong className="tabular">{formatBytes(preview.eligibleBytes)}</strong>. Unsent files will remain untouched.
            </p>
            <div style={{ maxHeight: 160, overflowY: "auto", background: "var(--surface-lowest)", borderRadius: 12, padding: 10, fontSize: 11, color: "var(--text-muted)", marginBottom: 14 }}>
              {preview.samplePaths.map((p) => (
                <div key={p} style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{p}</div>
              ))}
              {preview.eligibleCount > preview.samplePaths.length && (
                <div style={{ marginTop: 4, fontWeight: 700 }}>...and {preview.eligibleCount - preview.samplePaths.length} more</div>
              )}
            </div>
            <div style={{ display: "flex", justifyContent: "flex-end", gap: 10 }}>
              <button className="btn btn-outline" onClick={() => setConfirmOpen(false)}>Cancel</button>
              <button
                className={mode === "PERMANENT" ? "btn btn-danger" : "btn btn-verified"}
                disabled={executing}
                onClick={runCleanup}
              >
                {executing ? "Cleaning..." : "Confirm Cleanup"}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
