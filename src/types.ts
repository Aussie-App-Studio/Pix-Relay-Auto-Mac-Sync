export type ThemeMode = "SYSTEM" | "LIGHT" | "DARK" | "MATRIX";
export type FolderPolicy = "AUTO" | "CHOOSE" | "SKIP";
export type CleanupActionMode = "TRASH" | "ARCHIVE_FOLDER" | "PERMANENT";
export type MediaSyncStatus = "UNSENT" | "ON_PIXEL" | "CLEARED_BY_PIXEL";
export type DateGroupingMode = "DAY" | "WEEK" | "MONTH";
export type SaveLocationMode = "CAMERA_FOLDER" | "SEPARATE_BY_PHONE" | "MIRROR_ORIGINAL_FOLDERS";
export type StorageFullCleanupMode = "NOTIFY_FOR_FREE_UP_SPACE" | "AUTO_CLEAN_AFTER_UPLOAD" | "PAUSE_ONLY";
export type LicenseMode = "TRIAL_ACTIVE" | "TRIAL_EXPIRED" | "UNLOCKED";

export interface MacSettings {
  deviceId: string;
  deviceName: string;
  themeMode: ThemeMode;
  autoSyncEnabled: boolean;
  autoSendCutoffEpochMs: number;
  photosBeforeVideos: boolean;
  includeRawFiles: boolean;
  minimizeToTray: boolean;
  startAtLogin: boolean;
  selectedReceiverDeviceId: string | null;
  defaultCleanupMode: CleanupActionMode;
  archiveFolderPath: string | null;
  licenseKey: string | null;
}

export interface ApplePhotosLibraryInfo {
  path: string;
  displayName: string;
  alreadyAdded: boolean;
  readable: boolean;
  needsPermission: boolean;
}

export interface FolderSummary {
  folderId: string;
  path: string;
  displayName: string;
  policy: FolderPolicy;
  recursive: boolean;
  existsOnDisk: boolean;
  isApplePhotosLibrary: boolean;
  totalCount: number;
  unsentCount: number;
  onPixelCount: number;
  clearedCount: number;
  totalBytes: number;
  unsentBytes: number;
  reclaimableClearedBytes: number;
  reclaimableAllSentBytes: number;
  percentUploaded: number;
  is100PercentUploaded: boolean;
  latestDateTakenMs: number;
  coverPath: string | null;
}

export interface LibrarySnapshot {
  folders: FolderSummary[];
  totalMediaCount: number;
  totalUnsentCount: number;
  totalClearedCount: number;
  totalReclaimableClearedBytes: number;
  totalReclaimableAllSentBytes: number;
}

export interface MacMediaItem {
  id: string;
  folderId: string;
  filePath: string;
  displayName: string;
  bucketName: string;
  relativePath: string;
  sizeBytes: number;
  dateTakenEpochMs: number;
  dateModifiedEpochSec: number;
  mimeType: string;
  isVideo: boolean;
  isRaw: boolean;
  fingerprintKey: string;
  syncStatus: MediaSyncStatus;
}

export interface MediaDateGroup {
  groupKey: string;
  groupTitle: string;
  totalCount: number;
  unsentCount: number;
  items: MacMediaItem[];
}

export interface DiscoveredPeer {
  deviceId: string;
  deviceName: string;
  modelName: string;
  role: "SENDER" | "RECEIVER";
  ipAddress: string;
  port: number;
  isPaired: boolean;
}

export interface PairedDevice {
  deviceId: string;
  deviceName: string;
  modelName: string;
  role: "SENDER" | "RECEIVER";
  lastIpAddress: string;
  lastPort: number;
  sharedSecret: string;
  pairedAtEpochMs: number;
  lastSeenEpochMs: number;
  saveLocationOverride: SaveLocationMode | null;
  totalFilesTransferred: number;
  totalBytesTransferred: number;
}

export interface PairingSessionState {
  peerId: string;
  peerName: string;
  peerModel: string;
  peerIp: string;
  peerPort: number;
  verificationCode6Digit: string;
  sharedSecretCandidate: string;
  sessionNonce: string;
}

export interface ReceiverTelemetry {
  deviceId: string;
  deviceName: string;
  modelName: string;
  pixelBenefitSummary: string;
  freeBytes: number;
  totalBytes: number;
  reserveStorageGb: number;
  batteryPercent: number;
  isCharging: boolean;
  temperatureCelsius: number;
  maxSafeTempCelsius: number;
  isThermalPaused: boolean;
  isStoragePaused: boolean;
  isScreenOffReady: boolean;
  outboundUploadBytesPerSec: number;
  friendlyCloudStatus: string;
  historicalReceivedCount: number;
  historicalReceivedBytes: number;
  stagedFilesOnDiskCount: number;
  timestampEpochMs: number;
  isManualPaused: boolean;
}

export interface RemoteReceiverSettingsPayload {
  receivingPaused: boolean;
  storageReserveGb: number;
  storageCleanupMode: StorageFullCleanupMode;
  autoCleanAfterDays: number;
  thermalGuardEnabled: boolean;
  maxSafeTempCelsius: number;
  defaultSaveLocationMode: SaveLocationMode;
}

export type TransferProgressEvent =
  | { state: "IDLE" }
  | { state: "SCANNING"; message: string }
  | {
      state: "ACTIVE";
      targetDeviceName: string;
      currentFileName: string;
      currentFileIndex: number;
      totalFiles: number;
      bytesTransferred: number;
      totalBytes: number;
      speedMbPerSec: number;
      verifiedCount: number;
      skippedDuplicateCount: number;
    }
  | {
      state: "PAUSED";
      targetDeviceName: string;
      friendlyReason: string;
      completedFiles: number;
      remainingFiles: number;
    }
  | {
      state: "COMPLETED";
      targetDeviceName: string;
      transferredCount: number;
      skippedDuplicateCount: number;
      totalBytesSent: number;
      elapsedSeconds: number;
    }
  | { state: "FAILED"; friendlyMessage: string };

export interface FolderCleanupBreakdown {
  folderId: string;
  displayName: string;
  path: string;
  clearedCount: number;
  clearedBytes: number;
  onPixelCount: number;
  onPixelBytes: number;
}

export interface CleanupPreview {
  eligibleCount: number;
  eligibleBytes: number;
  clearedCount: number;
  clearedBytes: number;
  onPixelUnconfirmedCount: number;
  onPixelUnconfirmedBytes: number;
  perFolder: FolderCleanupBreakdown[];
  samplePaths: string[];
}

export interface CleanupExecuteResult {
  cleanedCount: number;
  freedBytes: number;
  failedCount: number;
  removedEmptyDirs: number;
  message: string;
}

export interface FirewallHealthStatus {
  localIpv4s: string[];
  interfaces: { interfaceAlias: string; ipAddress: string }[];
  hasFirewallWarning: boolean;
  udpListenerHealthy: boolean;
  udpPort: number;
  firewallInboundStatus: "ALLOWED" | "FIREWALL_OFF" | "BLOCK_ALL" | "BLOCKED" | "UNCONFIGURED" | "UNKNOWN";
  firewallSummary: string;
  recommendedCommand: string;
  localNetworkPrivacyNote: string;
}

export interface MacLicenseStatus {
  mode: LicenseMode;
  requiresLicense: boolean;
  canSync: boolean;
  daysRemaining: number;
  expiresAtMs: number | null;
  devOverride: string | null;
  statusLabel: string;
  websiteUrl: string;
  hasLicenseKey: boolean;
}

export interface ActivityEntry {
  id: number;
  timestampMs: number;
  kind: string;
  message: string;
}

export function thumbUrl(filePath: string): string {
  return `pixthumb://localhost/${encodeURIComponent(filePath)}`;
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const idx = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const val = bytes / Math.pow(1024, idx);
  return `${val >= 10 || idx === 0 ? val.toFixed(0) : val.toFixed(1)} ${units[idx]}`;
}
