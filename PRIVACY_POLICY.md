# Privacy Policy — Pix Relay: Auto Mac Sync

**Effective Date:** November 24, 2026  
**Publisher:** Aussie App Studio

## The Short Version
**Pix Relay: Auto Mac Sync never connects to the internet and never collects, tracks, or shares any of your data.** Everything stays on your Mac and your own Pixel phone over your home Wi-Fi network.

---

## 1. Zero Internet Connections
Pix Relay: Auto Mac Sync does not communicate with any remote servers, analytics services, telemetry endpoints, ad networks, or cloud backends. Your photos, videos, filenames, and settings never leave your local Wi-Fi network through this app.

## 2. Zero Data Collection or Tracking
We do **not** collect, store, transmit, sell, or share:
- Personal information (name, email, Apple ID, or Google account)
- Photos, videos, albums, or photo metadata
- Device identifiers, IP addresses, or usage statistics
- Crash reports or behavioral tracking data

## 3. Local Wi-Fi Transfer Only
To back up your photos, the app discovers your paired Pixel 1–4 receiver directly on your local Wi-Fi network (`UDP 42889` / Bonjour mDNS) and sends media over a direct encrypted local connection (`TCP 42888` using P-256 ECDH + AES-256-GCM) straight to your Pixel device. Once files arrive on your Pixel, your Pixel's own Google Photos app handles cloud backup according to your Google account settings.

## 4. Local-Only Mac Storage
Your preferences, paired Pixel keys, watched folder list, cached thumbnails, and transfer history are stored strictly locally on your Mac inside:
- `~/Library/Application Support/Pix Relay Auto Mac Sync/`
- `~/Library/Caches/Pix Relay Auto Mac Sync/`

You can remove all app data at any time by deleting the app and those local folders.

## 5. Apple Photos Library Read-Only Access
If you choose to connect your Apple Photos Library (`.photoslibrary`), Pix Relay reads original photos and videos in **strictly read-only mode** (`Photos.sqlite` opened immutable/read-only) solely to display them in the app and relay them to your paired Pixel. Pix Relay never modifies or deletes files inside your Apple Photos Library.

## 6. External Links
If you click an external link inside Settings (such as the project homepage), your default web browser opens that webpage directly. Pix Relay itself never makes background network requests.

## 7. Contact
Questions about this Privacy Policy can be directed to **Aussie App Studio** via the [Pix Relay Auto Mac Sync GitHub repository](https://github.com/Aussie-App-Studio/Pix-Relay-Auto-Mac-Sync).
