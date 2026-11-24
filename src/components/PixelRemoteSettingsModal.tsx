import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Sliders, X } from "lucide-react";
import {
  PairedDevice,
  RemoteReceiverSettingsPayload,
  SaveLocationMode,
  StorageFullCleanupMode,
} from "../types";

interface Props {
  device: PairedDevice;
  onClose: () => void;
  onSaved: () => void;
}

export function PixelRemoteSettingsModal({ device, onClose, onSaved }: Props) {
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [draft, setDraft] = useState<RemoteReceiverSettingsPayload | null>(null);

  useEffect(() => {
    let alive = true;
    setLoading(true);
    setError(null);
    invoke<RemoteReceiverSettingsPayload>("fetch_remote_pixel_settings", {
      deviceId: device.deviceId,
    })
      .then((res) => {
        if (alive) setDraft(res);
      })
      .catch((e) => {
        if (alive) setError(String(e));
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, [device.deviceId]);

  const handleSave = async () => {
    if (!draft) return;
    setSaving(true);
    setError(null);
    try {
      await invoke("push_remote_pixel_settings", {
        deviceId: device.deviceId,
        payload: draft,
      });
      onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop" onClick={onClose}>
      <div className="modal-card" onClick={(e) => e.stopPropagation()}>
        <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", marginBottom: 14 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
            <Sliders size={18} color="var(--primary)" />
            <div>
              <div style={{ fontWeight: 700, fontSize: 16 }}>Remote Pixel Controls</div>
              <div style={{ fontSize: 12, color: "var(--text-muted)" }}>{device.deviceName} ({device.modelName})</div>
            </div>
          </div>
          <button className="btn btn-outline btn-sm" onClick={onClose}>
            <X size={15} />
          </button>
        </div>

        {loading && <div style={{ padding: 24, color: "var(--text-muted)" }}>Connecting to {device.deviceName}...</div>}
        {error && (
          <div className="badge badge-error" style={{ width: "100%", padding: 10, borderRadius: 12, marginBottom: 12 }}>
            {error}
          </div>
        )}

        {draft && (
          <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
            <label style={{ display: "flex", justifyContent: "space-between", alignItems: "center", padding: "10px 12px", background: "var(--surface-container)", borderRadius: 12 }}>
              <div>
                <div style={{ fontWeight: 600, fontSize: 13 }}>Pause Receiving on Pixel</div>
                <div style={{ fontSize: 12, color: "var(--text-muted)" }}>Temporarily stops incoming transfers to this Pixel</div>
              </div>
              <input
                type="checkbox"
                checked={draft.receivingPaused}
                onChange={(e) => setDraft({ ...draft, receivingPaused: e.target.checked })}
              />
            </label>

            <div className="m3-card" style={{ padding: 12 }}>
              <div style={{ fontWeight: 600, fontSize: 13, marginBottom: 8 }}>Save Folder on Pixel</div>
              <select
                className="input-field"
                value={draft.defaultSaveLocationMode}
                onChange={(e) =>
                  setDraft({ ...draft, defaultSaveLocationMode: e.target.value as SaveLocationMode })
                }
              >
                <option value="CAMERA_FOLDER">DCIM/Camera (Auto Google Photos Backup)</option>
                <option value="SEPARATE_BY_PHONE">Pictures/PixRelay/&lt;Mac Name&gt;</option>
                <option value="MIRROR_ORIGINAL_FOLDERS">Pictures/PixRelay/&lt;Folder Name&gt;</option>
              </select>
            </div>

            <div className="m3-card" style={{ padding: 12, display: "flex", flexDirection: "column", gap: 10 }}>
              <div style={{ display: "flex", justifyContent: "space-between", fontSize: 13 }}>
                <span style={{ fontWeight: 600 }}>Storage Reserve Guard</span>
                <span className="tabular" style={{ color: "var(--primary)", fontWeight: 700 }}>{draft.storageReserveGb} GB</span>
              </div>
              <input
                type="range"
                min={1}
                max={24}
                value={draft.storageReserveGb}
                onChange={(e) => setDraft({ ...draft, storageReserveGb: Number(e.target.value) })}
              />

              <div style={{ fontWeight: 600, fontSize: 13, marginTop: 4 }}>Pixel Storage Cleanup Mode</div>
              <select
                className="input-field"
                value={draft.storageCleanupMode}
                onChange={(e) =>
                  setDraft({ ...draft, storageCleanupMode: e.target.value as StorageFullCleanupMode })
                }
              >
                <option value="NOTIFY_FOR_FREE_UP_SPACE">Pause &amp; Remind Me (Safest)</option>
                <option value="AUTO_CLEAN_AFTER_UPLOAD">Auto-Clean Old Copies After Upload</option>
                <option value="PAUSE_ONLY">Just Pause When Full</option>
              </select>

              {draft.storageCleanupMode === "AUTO_CLEAN_AFTER_UPLOAD" && (
                <div style={{ display: "flex", justifyContent: "space-between", alignItems: "center", fontSize: 13 }}>
                  <span>Auto-clean after (days)</span>
                  <input
                    type="number"
                    min={1}
                    max={60}
                    className="input-field"
                    style={{ width: 90 }}
                    value={draft.autoCleanAfterDays}
                    onChange={(e) => setDraft({ ...draft, autoCleanAfterDays: Number(e.target.value) || 7 })}
                  />
                </div>
              )}
            </div>

            <div className="m3-card" style={{ padding: 12, display: "flex", flexDirection: "column", gap: 8 }}>
              <label style={{ display: "flex", justifyContent: "space-between", alignItems: "center" }}>
                <span style={{ fontWeight: 600, fontSize: 13 }}>Thermal Battery Guard</span>
                <input
                  type="checkbox"
                  checked={draft.thermalGuardEnabled}
                  onChange={(e) => setDraft({ ...draft, thermalGuardEnabled: e.target.checked })}
                />
              </label>
              {draft.thermalGuardEnabled && (
                <div style={{ display: "flex", justifyContent: "space-between", fontSize: 12, color: "var(--text-muted)" }}>
                  <span>Pause above temperature</span>
                  <span className="tabular" style={{ color: "var(--secondary)", fontWeight: 700 }}>
                    {draft.maxSafeTempCelsius.toFixed(1)} °C
                  </span>
                </div>
              )}
              {draft.thermalGuardEnabled && (
                <input
                  type="range"
                  min={35}
                  max={45}
                  step={0.5}
                  value={draft.maxSafeTempCelsius}
                  onChange={(e) => setDraft({ ...draft, maxSafeTempCelsius: Number(e.target.value) })}
                />
              )}
            </div>

            <div style={{ display: "flex", justifyContent: "flex-end", gap: 10 }}>
              <button className="btn btn-outline" onClick={onClose}>Cancel</button>
              <button className="btn btn-primary" onClick={handleSave} disabled={saving}>
                {saving ? "Saving..." : "Apply to Pixel"}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
