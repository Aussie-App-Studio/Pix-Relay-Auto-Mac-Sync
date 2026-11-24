<div align="center">

<img src="src-tauri/icons/128x128@2x.png" alt="Pix Relay Icon" width="96" height="96" />

# Pix Relay — Auto Mac Sync

**Automatically send photos & videos from your Mac straight to a Pixel 1–4 over home Wi-Fi — so you can use its free, unlimited Google Photos backup.**

<p>
  <img src="https://img.shields.io/badge/macOS-12%2B%20(Monterey%20–%20Sequoia)-111827?style=flat-square&logo=apple&logoColor=white" alt="macOS 12+" />
  <img src="https://img.shields.io/badge/Chip-Apple%20Silicon%20(M1–M4)%20%26%20Intel-0A749A?style=flat-square" alt="Apple Silicon and Intel" />
  <img src="https://img.shields.io/badge/Price-100%25%20Free%20%26%20Open%20Source-16A34A?style=flat-square" alt="Free and Open Source" />
  <img src="https://img.shields.io/badge/Privacy-100%25%20Local%20Wi--Fi-56C0FF?style=flat-square" alt="Local Wi-Fi Only" />
</p>

<br />

<a href="https://github.com/Aussie-App-Studio/Pix-Relay-Auto-Mac-Sync/releases/latest/download/Pix-Relay-Auto-Mac-Sync-Universal.dmg">
  <img src="https://img.shields.io/badge/%E2%AC%87%EF%B8%8F%20Download%20for%20Mac%20(.dmg)-Click%20Here%20(Apple%20Silicon%20%26%20Intel)-0071E3?style=for-the-badge" alt="Download for Mac" height="44" />
</a>

<br />
<sub>Works on every Mac (M1, M2, M3, M4 &amp; Intel) · No account required · Or browse <a href="https://github.com/Aussie-App-Studio/Pix-Relay-Auto-Mac-Sync/releases/latest">All Releases</a></sub>

</div>

---

## How to Install & Open on Mac (Takes 30 Seconds)

> [!IMPORTANT]
> **Why does macOS show an "Apple could not verify" warning?**
>
> Apple charges every developer **$99 USD per year** for a paid Developer ID certificate just to hide that warning screen — even for completely free, open-source apps.
>
> **Nothing is wrong with the app.** Because we refuse to pay Apple $99/year to give away a free tool, you just need to tell your Mac **once** to allow it.

### Step 1: Install the App
1. Click the blue **Download for Mac (.dmg)** button above.
2. Double-click the downloaded **`Pix-Relay-Auto-Mac-Sync-Universal.dmg`** file.
3. Drag **Pix Relay Auto Mac Sync** into the **Applications** folder icon.

### Step 2: Bypass Apple's "Unverified Developer" Block (Pick Either Way)

#### Option A — Using Mac Settings (No typing needed · Works on macOS Sequoia, Sonoma & Ventura)
1. Open your **Applications** folder and double-click **Pix Relay Auto Mac Sync**.
2. A popup will say *"Apple could not verify..."* or *"Not Opened"*. Click **Done** (or **OK**) — **do NOT click Move to Trash**.
3. Open **System Settings** (click the **** Apple logo in the top-left corner → **System Settings**).
4. Click **Privacy & Security** in the left sidebar.
5. Scroll down to the **Security** section — you'll see a message saying *"Pix Relay Auto Mac Sync was blocked..."*.
6. Click **Open Anyway** → enter your Mac password or Touch ID → click **Open**.

That's it! You will **never** see that popup again.

---

#### Option B — 1-Line Copy & Paste (Fastest · Also fixes "App is damaged" popups)
1. Press **⌘ Command + Spacebar**, type **`Terminal`**, and press **Enter**.
2. Paste this single line and press **Enter**:
   ```bash
   xattr -cr "/Applications/Pix Relay Auto Mac Sync.app"
   ```
3. Now open **Pix Relay Auto Mac Sync** from Applications normally!

---

## How to Start Syncing Photos to Your Pixel

1. **Open Pix Relay on your Backup Pixel (Pixel 1–4)** in **Receiver** mode (make sure your Mac and Pixel are on the same home Wi-Fi).
2. **Pair Once:** Open **Pix Relay Auto Mac Sync** on your Mac, go to **Pair Pixel**, and click **Pair** (or type the Pixel's Wi-Fi IP / paste its `PIX1:` code).
   - *Tip for macOS Sequoia (15+):* If macOS asks *"Allow Pix Relay to find devices on local networks?"*, click **Allow**.
3. **Pick What to Sync:**
   - **Apple Photos Users:** Click the **Sync Apple Photos Library** button right on the home screen (if macOS asks for permission, grant **Full Disk Access** in *System Settings → Privacy & Security* and click **Recheck**).
   - **Folder Users:** Click **Add Folder** or drag any photo/video folder (or SD card folder) directly into the window.

---

## What It Does (In Plain English)

- **Syncs Your Apple Photos Library Safely:** Reads full-resolution originals straight out of your Mac's Photos Library **read-only** — it never edits, corrupts, or deletes anything inside Apple Photos.
- **Watches Regular Mac Folders Automatically:** Drop in your Camera Roll, Lightroom exports, Downloads, or external hard drive folders and new photos upload to your Pixel automatically in the background.
- **Safely Free Up Mac Hard Drive Space:** Once your Pixel confirms Google Photos has finished backing up a photo, Pix Relay marks it green (`Cleared by Pixel ✓`) so you can safely send the Mac copy to **macOS Trash** or an **Archive Drive** in one click.
- **Private & Encrypted on Home Wi-Fi:** Your photos go straight from your Mac to your Pixel over encrypted local Wi-Fi — zero cloud servers, zero accounts, zero tracking.
- **Ultra-Lightweight Menu Bar Mode:** Closing the window tucks the app neatly into the top-right Mac Menu Bar using ~15 MB of memory.

---

<details>
<summary><strong>For Developers: Building from Source</strong></summary>

Prerequisites: macOS 12+, Xcode Command Line Tools (`xcode-select --install`), Node.js 20+, Rust (`rustup`).

```bash
git clone https://github.com/Aussie-App-Studio/Pix-Relay-Auto-Mac-Sync.git
cd Pix-Relay-Auto-Mac-Sync
chmod +x scripts/build-mac.sh
./scripts/build-mac.sh
```

Output `.app` and `.dmg` bundles are written to `src-tauri/target/universal-apple-darwin/release/bundle/`. See [`MAC_PUBLISHING.md`](MAC_PUBLISHING.md) for optional Developer ID signing, notarization, and license key gating.

</details>
