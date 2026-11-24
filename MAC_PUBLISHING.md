# Building & Distributing Pix Relay: Auto Mac Sync

This app builds a **Universal macOS binary** (`aarch64-apple-darwin` + `x86_64-apple-darwin`) packaged as a standard `.app` inside a drag-to-Applications `.dmg` for **GitHub Releases** and **direct website downloads** (no Mac App Store sandbox restrictions).

---

## 1. Automated Builds via GitHub Actions (Recommended)

Because macOS `.app` and `.dmg` bundles must be compiled and linked on macOS, [`.github/workflows/build-mac.yml`](.github/workflows/build-mac.yml) builds everything automatically in the cloud on `macos-latest`:

1. Push this project to a GitHub repository.
2. Run **Actions -> Build macOS Universal App & DMG -> Run workflow** to produce a downloadable `.dmg` artifact anytime, **or** push a version tag (`git tag v1.0.0 && git push origin v1.0.0`) to publish a GitHub Release with the `.dmg` attached automatically.

---

## 2. Building Locally on a Mac

Prerequisites on macOS 12+:
- Xcode Command Line Tools (`xcode-select --install`)
- Node.js 20+
- Rust (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`)

Run:
```bash
chmod +x scripts/build-mac.sh
./scripts/build-mac.sh
```

The Universal `.dmg` and `.app` are written to:
`src-tauri/target/universal-apple-darwin/release/bundle/`

---

## 3. Code Signing, Gatekeeper, & Dedicated Website Downloads

### Early GitHub Releases (Unsigned)
When distributing an unsigned `.dmg` via GitHub Releases before setting up an Apple Developer ID certificate, macOS Gatekeeper shows *"cannot be opened because Apple cannot check it for malicious software"* on first launch. Users can either:
- Right-click (Control-click) `Pix Relay Auto Mac Sync.app` -> **Open**, or
- Run in Terminal after dragging to Applications:
  ```bash
  xattr -cr "/Applications/Pix Relay Auto Mac Sync.app"
  ```

### Direct Website Downloads (Signed & Notarized)
Before distributing from your own website, sign and notarize with an **Apple Developer ID Application** certificate (`$99/yr` Apple Developer Program):
1. Add these GitHub Actions secrets (or export them before running `scripts/build-mac.sh`):
   - `APPLE_CERTIFICATE` (base64-encoded `.p12` Developer ID Application cert)
   - `APPLE_CERTIFICATE_PASSWORD`
   - `APPLE_SIGNING_IDENTITY` (e.g. `Developer ID Application: Your Name (TEAMID)`)
   - `APPLE_ID` (your Apple ID email)
   - `APPLE_PASSWORD` (App-Specific Password from appleid.apple.com)
   - `APPLE_TEAM_ID`
2. Tagged builds will then open cleanly on any Mac with zero Gatekeeper warnings.

---

## 4. Apple Photos Library & Sequoia Permissions

- **Apple Photos Library (`.photoslibrary`)**: Pix Relay detects `~/Pictures/Photos Library.photoslibrary` and offers a one-click **Sync Apple Photos Library** card. It opens `database/Photos.sqlite` strictly with `mode=ro&immutable=1` and only reads `originals/` (or `Masters/`), never modifying or deleting inside the bundle. If macOS denies read access, grant **Full Disk Access** under *System Settings -> Privacy & Security -> Full Disk Access*.
- **Local Network Privacy (macOS 15 Sequoia+)**: `src-tauri/Info.plist` declares `NSLocalNetworkUsageDescription` and `_pixrelay._tcp` so macOS prompts once to allow LAN Wi-Fi discovery (`UDP 42889` + Bonjour mDNS) and encrypted transfers (`TCP 42888`).

---

## 5. Enabling Paid Website License Keys Later

`src-tauri/src/license.rs` currently ships with:
```rust
pub const REQUIRE_PAID_LICENSE: bool = false;
pub const WEBSITE_URL: &str = "https://github.com";
```
When you are ready to sell licenses on your dedicated website:
1. Update `WEBSITE_URL` to your website URL.
2. Set `REQUIRE_PAID_LICENSE = true` (and customize `is_valid_license_key_format` / online validation as needed).
3. Settings and the sidebar automatically switch to a 7-day trial + `PIXM-XXXX-XXXX-XXXX` license key activation flow.
