import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useVirtualizer } from "@tanstack/react-virtual";
import {
  ArrowLeft,
  Check,
  CheckCheck,
  Eye,
  EyeOff,
  FolderCheck,
  Lock,
  Play,
  RefreshCw,
  Send,
  Trash2,
} from "lucide-react";
import {
  DateGroupingMode,
  FolderSummary,
  MacMediaItem,
  MediaDateGroup,
  formatBytes,
  thumbUrl,
} from "../types";

interface Props {
  folder: FolderSummary | null;
  hasPairedPixel: boolean;
  refreshToken: number;
  onBack: () => void;
  onGoToFreeUpSpace: (folderId?: string) => void;
  onGoToPairing: () => void;
  onRefreshLibrary: () => void;
}

type VirtualRow =
  | { kind: "HEADER"; group: MediaDateGroup }
  | { kind: "GRID"; rowKey: string; items: MacMediaItem[] };

export function FolderGallery({
  folder,
  hasPairedPixel,
  refreshToken,
  onBack,
  onGoToFreeUpSpace,
  onGoToPairing,
  onRefreshLibrary,
}: Props) {
  const [groups, setGroups] = useState<MediaDateGroup[]>([]);
  const [loading, setLoading] = useState(true);
  const [filter, setFilter] = useState<"ALL" | "UNSENT" | "ON_PIXEL" | "CLEARED" | "VIDEOS">("ALL");
  const [hideUploaded, setHideUploaded] = useState(false);
  const [grouping, setGrouping] = useState<DateGroupingMode>("DAY");
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [columns, setColumns] = useState(6);

  const scrollParentRef = useRef<HTMLDivElement | null>(null);

  const loadMedia = useCallback(async () => {
    setLoading(true);
    try {
      const res = await invoke<MediaDateGroup[]>("get_folder_media", {
        folderId: folder ? folder.folderId : null,
        grouping,
        filter,
        hideUploaded,
      });
      setGroups(res);
    } finally {
      setLoading(false);
    }
  }, [folder?.folderId, grouping, filter, hideUploaded, refreshToken]);

  useEffect(() => {
    loadMedia();
  }, [loadMedia]);

  useEffect(() => {
    const el = scrollParentRef.current;
    if (!el) return;
    const obs = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 800;
      setColumns(Math.max(3, Math.min(8, Math.floor(w / 148))));
    });
    obs.observe(el);
    return () => obs.disconnect();
  }, []);

  const rows: VirtualRow[] = useMemo(() => {
    const out: VirtualRow[] = [];
    for (const group of groups) {
      out.push({ kind: "HEADER", group });
      for (let i = 0; i < group.items.length; i += columns) {
        out.push({
          kind: "GRID",
          rowKey: `${group.groupKey}_${i}`,
          items: group.items.slice(i, i + columns),
        });
      }
    }
    return out;
  }, [groups, columns]);

  const rowVirtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollParentRef.current,
    estimateSize: (index) => (rows[index]?.kind === "HEADER" ? 54 : 152),
    overscan: 5,
  });

  const allVisibleItems = useMemo(() => groups.flatMap((g) => g.items), [groups]);

  const selectedBytes = useMemo(() => {
    let total = 0;
    for (const item of allVisibleItems) {
      if (selectedPaths.has(item.filePath)) total += item.sizeBytes;
    }
    return total;
  }, [allVisibleItems, selectedPaths]);

  const toggleItem = (filePath: string) => {
    setSelectedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(filePath)) next.delete(filePath);
      else next.add(filePath);
      return next;
    });
  };

  const toggleGroupUnsent = (group: MediaDateGroup) => {
    const unsent = group.items.filter((i) => i.syncStatus === "UNSENT").map((i) => i.filePath);
    if (unsent.length === 0) return;
    setSelectedPaths((prev) => {
      const next = new Set(prev);
      const allSelected = unsent.every((p) => next.has(p));
      if (allSelected) unsent.forEach((p) => next.delete(p));
      else unsent.forEach((p) => next.add(p));
      return next;
    });
  };

  const selectAllUnsent = () => {
    const unsent = allVisibleItems.filter((i) => i.syncStatus === "UNSENT").map((i) => i.filePath);
    setSelectedPaths(new Set(unsent));
  };

  const sendSelected = async () => {
    if (!hasPairedPixel) {
      onGoToPairing();
      return;
    }
    if (selectedPaths.size === 0) return;
    await invoke("send_selected_files", { filePaths: Array.from(selectedPaths) });
    setSelectedPaths(new Set());
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0 }}>
      {/* Top Toolbar */}
      <div style={{ padding: "18px 24px 12px", borderBottom: "1px solid var(--border)", display: "flex", flexDirection: "column", gap: 12 }}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: 10 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 12 }}>
            <button className="btn btn-outline btn-sm" onClick={onBack}>
              <ArrowLeft size={14} /> Folders
            </button>
            <div>
              <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
                <h2 style={{ margin: 0, fontSize: 20, fontWeight: 800 }}>
                  {folder ? folder.displayName : "All Media"}
                </h2>
                {folder?.isApplePhotosLibrary && (
                  <span className="badge badge-primary">
                    <Lock size={11} /> Read-Only Apple Photos Sync
                  </span>
                )}
                {folder?.is100PercentUploaded && (
                  <span className="badge badge-verified">
                    <FolderCheck size={12} /> 100% Uploaded
                  </span>
                )}
              </div>
              {folder && (
                <div className="tabular" style={{ fontSize: 12, color: "var(--text-muted)", marginTop: 2 }}>
                  {folder.totalCount} total · {folder.unsentCount} unsent · {folder.onPixelCount} on Pixel ·{" "}
                  <span style={{ color: "var(--verified)" }}>{folder.clearedCount} cleared by Pixel</span>
                </div>
              )}
            </div>
          </div>

          <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
            <button
              className="btn btn-outline btn-sm"
              onClick={() => invoke("sync_cleared_status_now").then(() => { onRefreshLibrary(); loadMedia(); })}
            >
              <RefreshCw size={13} /> Sync Status
            </button>
            {folder && !folder.isApplePhotosLibrary && folder.reclaimableAllSentBytes > 0 && (
              <button className="btn btn-verified btn-sm" onClick={() => onGoToFreeUpSpace(folder.folderId)}>
                <Trash2 size={13} /> Free Up Space
              </button>
            )}
            <button className="btn btn-tonal btn-sm" onClick={selectAllUnsent}>
              Select Unsent
            </button>
            {selectedPaths.size > 0 && (
              <button className="btn btn-primary btn-sm" onClick={sendSelected}>
                <Send size={13} /> Send {selectedPaths.size} ({formatBytes(selectedBytes)})
              </button>
            )}
          </div>
        </div>

        {/* Filter & Grouping Row */}
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", flexWrap: "wrap", gap: 10 }}>
          <div style={{ display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" }}>
            <div className="pill-bar">
              {(
                [
                  ["ALL", "All"],
                  ["UNSENT", "Unsent"],
                  ["ON_PIXEL", "On Pixel"],
                  ["CLEARED", "Cleared ✓"],
                  ["VIDEOS", "Videos"],
                ] as const
              ).map(([key, label]) => (
                <button
                  key={key}
                  className={`pill-btn ${filter === key ? "active" : ""}`}
                  onClick={() => setFilter(key)}
                >
                  {label}
                </button>
              ))}
            </div>

            <button
              className={`btn btn-sm ${hideUploaded ? "btn-tonal" : "btn-outline"}`}
              onClick={() => setHideUploaded((v) => !v)}
            >
              {hideUploaded ? <EyeOff size={13} /> : <Eye size={13} />}
              {hideUploaded ? "Hiding Uploaded" : "Hide Uploaded"}
            </button>
          </div>

          <div className="pill-bar">
            {(["DAY", "WEEK", "MONTH"] as DateGroupingMode[]).map((mode) => (
              <button
                key={mode}
                className={`pill-btn ${grouping === mode ? "active" : ""}`}
                onClick={() => setGrouping(mode)}
              >
                {mode === "DAY" ? "Day" : mode === "WEEK" ? "Week" : "Month"}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Virtualized Grid */}
      <div
        ref={scrollParentRef}
        style={{ flex: 1, overflowY: "auto", padding: "8px 24px 88px", position: "relative" }}
      >
        {loading ? (
          <div style={{ padding: 40, textAlign: "center", color: "var(--text-muted)" }}>
            Scanning photos &amp; videos...
          </div>
        ) : rows.length === 0 ? (
          <div style={{ padding: 48, textAlign: "center", color: "var(--text-muted)" }}>
            No media items match your current filter.
          </div>
        ) : (
          <div
            style={{
              height: `${rowVirtualizer.getTotalSize()}px`,
              width: "100%",
              position: "relative",
            }}
          >
            {rowVirtualizer.getVirtualItems().map((vRow) => {
              const row = rows[vRow.index];
              if (!row) return null;
              return (
                <div
                  key={vRow.key}
                  data-index={vRow.index}
                  ref={rowVirtualizer.measureElement}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    transform: `translateY(${vRow.start}px)`,
                  }}
                >
                  {row.kind === "HEADER" ? (
                    <div
                      style={{
                        display: "flex",
                        alignItems: "center",
                        justifyContent: "space-between",
                        padding: "14px 2px 8px",
                      }}
                    >
                      <div>
                        <span style={{ fontWeight: 700, fontSize: 15 }}>{row.group.groupTitle}</span>
                        <span className="tabular" style={{ fontSize: 12, color: "var(--text-muted)", marginLeft: 10 }}>
                          {row.group.totalCount} items
                          {row.group.unsentCount > 0 ? ` · ${row.group.unsentCount} unsent` : ""}
                        </span>
                      </div>
                      {row.group.unsentCount > 0 && (
                        <button
                          className="btn btn-outline btn-sm"
                          onClick={() => toggleGroupUnsent(row.group)}
                        >
                          Select {row.group.unsentCount}
                        </button>
                      )}
                    </div>
                  ) : (
                    <div
                      style={{
                        display: "grid",
                        gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
                        gap: 6,
                        paddingBottom: 6,
                      }}
                    >
                      {row.items.map((item) => {
                        const selected = selectedPaths.has(item.filePath);
                        return (
                          <div
                            key={item.id}
                            onClick={() => toggleItem(item.filePath)}
                            title={`${item.displayName} (${formatBytes(item.sizeBytes)})`}
                            style={{
                              position: "relative",
                              aspectRatio: "1 / 1",
                              borderRadius: 10,
                              overflow: "hidden",
                              cursor: "pointer",
                              background: selected ? "var(--primary-container)" : "var(--placeholder)",
                              border: selected ? "2px solid var(--primary)" : "1px solid var(--border)",
                              padding: selected ? 4 : 0,
                            }}
                          >
                            <img
                              loading="lazy"
                              decoding="async"
                              src={thumbUrl(item.filePath)}
                              alt={item.displayName}
                              style={{
                                width: "100%",
                                height: "100%",
                                objectFit: "cover",
                                borderRadius: selected ? 7 : 9,
                                display: "block",
                              }}
                            />

                            {/* Top-right selection checkbox */}
                            <div
                              style={{
                                position: "absolute",
                                top: 7,
                                right: 7,
                                width: 22,
                                height: 22,
                                borderRadius: "50%",
                                background: selected ? "var(--primary)" : "rgba(0,0,0,0.45)",
                                border: `1.5px solid ${selected ? "var(--primary)" : "rgba(255,255,255,0.8)"}`,
                                display: "flex",
                                alignItems: "center",
                                justifyContent: "center",
                              }}
                            >
                              {selected && <Check size={13} color="var(--on-primary)" />}
                            </div>

                            {/* Bottom-left two-tier sync badge */}
                            {item.syncStatus !== "UNSENT" && (
                              <div
                                title={
                                  item.syncStatus === "CLEARED_BY_PIXEL"
                                    ? "Uploaded to Google Photos & cleared on Pixel (Safe to free on Mac)"
                                    : "Sent to Pixel (awaiting Google Photos upload)"
                                }
                                style={{
                                  position: "absolute",
                                  bottom: 6,
                                  left: 6,
                                  borderRadius: 999,
                                  background: "var(--glass-scrim)",
                                  padding: 5,
                                  display: "flex",
                                  alignItems: "center",
                                  justifyContent: "center",
                                }}
                              >
                                {item.syncStatus === "CLEARED_BY_PIXEL" ? (
                                  <CheckCheck size={13} color="var(--verified)" />
                                ) : (
                                  <Check size={13} color="var(--primary)" />
                                )}
                              </div>
                            )}

                            {/* Bottom-right video / RAW pill */}
                            {(item.isVideo || item.isRaw) && (
                              <div
                                style={{
                                  position: "absolute",
                                  bottom: 6,
                                  right: 6,
                                  borderRadius: 999,
                                  background: "var(--glass-scrim)",
                                  color: "#fff",
                                  padding: "3px 7px",
                                  fontSize: 10,
                                  fontWeight: 700,
                                  display: "flex",
                                  alignItems: "center",
                                  gap: 3,
                                }}
                              >
                                {item.isVideo ? <Play size={10} fill="#fff" /> : null}
                                {item.isRaw ? "RAW" : "VID"}
                              </div>
                            )}
                          </div>
                        );
                      })}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
