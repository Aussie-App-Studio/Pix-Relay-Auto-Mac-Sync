import { invoke } from "@tauri-apps/api/core";
import { AlertTriangle, CheckCircle2, Loader2, Pause, X } from "lucide-react";
import { TransferProgressEvent, formatBytes } from "../types";

interface Props {
  status: TransferProgressEvent;
  onDismiss: () => void;
}

export function TransferBar({ status, onDismiss }: Props) {
  if (status.state === "IDLE") return null;

  const pct =
    status.state === "ACTIVE" && status.totalBytes > 0
      ? Math.min(100, Math.round((status.bytesTransferred / status.totalBytes) * 100))
      : 0;

  return (
    <div
      style={{
        position: "absolute",
        left: 20,
        right: 20,
        bottom: 14,
        background: "var(--surface-low)",
        border: "1px solid var(--hero-border)",
        borderRadius: 18,
        padding: "12px 16px",
        boxShadow: "0 12px 36px rgba(0, 0, 0, 0.45)",
        display: "flex",
        alignItems: "center",
        gap: 14,
        zIndex: 20,
      }}
    >
      {status.state === "SCANNING" && (
        <>
          <Loader2 size={18} color="var(--primary)" className="spin" />
          <div style={{ flex: 1, fontSize: 13, fontWeight: 600 }}>{status.message}</div>
        </>
      )}

      {status.state === "ACTIVE" && (
        <>
          <div style={{ flex: 1, minWidth: 0 }}>
            <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 6 }}>
              <div style={{ fontSize: 13, fontWeight: 700, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                Sending {status.currentFileName} → {status.targetDeviceName}
              </div>
              <div className="tabular" style={{ fontSize: 12, color: "var(--text-muted)", whiteSpace: "nowrap", marginLeft: 12 }}>
                {status.currentFileIndex} / {status.totalFiles} · {formatBytes(status.bytesTransferred)} / {formatBytes(status.totalBytes)} · {status.speedMbPerSec.toFixed(1)} MB/s
              </div>
            </div>
            <div className="progress-track">
              <div className="progress-fill" style={{ width: `${pct}%` }} />
            </div>
          </div>
          <button
            className="btn btn-outline btn-sm"
            onClick={() => invoke("cancel_transfer")}
          >
            <Pause size={14} /> Pause
          </button>
        </>
      )}

      {status.state === "PAUSED" && (
        <>
          <AlertTriangle size={18} color="var(--secondary)" />
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 13, fontWeight: 700 }}>Paused — {status.targetDeviceName}</div>
            <div style={{ fontSize: 12, color: "var(--text-muted)" }}>
              {status.friendlyReason} ({status.completedFiles} sent, {status.remainingFiles} remaining)
            </div>
          </div>
          <button className="btn btn-outline btn-sm" onClick={onDismiss}>
            <X size={14} />
          </button>
        </>
      )}

      {status.state === "COMPLETED" && (
        <>
          <CheckCircle2 size={18} color="var(--verified)" />
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 13, fontWeight: 700 }}>
              Synced {status.transferredCount} {status.transferredCount === 1 ? "file" : "files"} to {status.targetDeviceName}
            </div>
            <div style={{ fontSize: 12, color: "var(--text-muted)" }}>
              {status.skippedDuplicateCount > 0 ? `${status.skippedDuplicateCount} duplicates skipped · ` : ""}
              {formatBytes(status.totalBytesSent)} in {status.elapsedSeconds.toFixed(1)}s
            </div>
          </div>
          <button className="btn btn-outline btn-sm" onClick={onDismiss}>
            <X size={14} />
          </button>
        </>
      )}

      {status.state === "FAILED" && (
        <>
          <AlertTriangle size={18} color="var(--error)" />
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 13, fontWeight: 700, color: "var(--error)" }}>Transfer Stopped</div>
            <div style={{ fontSize: 12, color: "var(--text-muted)" }}>{status.friendlyMessage}</div>
          </div>
          <button className="btn btn-outline btn-sm" onClick={onDismiss}>
            <X size={14} />
          </button>
        </>
      )}
    </div>
  );
}
