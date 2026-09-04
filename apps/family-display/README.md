# Family display

The display is a separately deployable, read-only Raspberry Pi client. The native launcher—not Chromium—owns the pairing credential and pinned Mac certificate. It serves the bundled React UI only on `127.0.0.1:4173`; the browser cannot contact the Mac or Internet directly.

Native commands are deliberately narrow:

- `family-display-launcher pair <config> <host-url> <certificate.der> <sha256> <six-digit-code> <display-name>` completes one TLS-pinned pairing and atomically creates an owner-only config.
- `family-display-launcher serve <config> <assets-directory>` starts the loopback kiosk origin and proxies only the bounded snapshot contract.

Pairing codes are created by an explicit action in desktop Settings, expire after five minutes, are single-use, and lock after five failed attempts. The final 256-bit credential never enters either React application.

## Raspberry Pi OS installation

The CI-built `personal-assistant-display_<version>_arm64.deb` targets 64-bit Raspberry Pi OS on Pi 4/5. It contains the native launcher, static UI, a hardened systemd service, and Chromium kiosk autostart; no language runtime or container engine is installed.

Install it with `sudo apt install ./personal-assistant-display_<version>_arm64.deb`. Create a pairing code in desktop Settings and copy the displayed public-certificate Base64 value into `certificate.txt` on the Pi. Decode and pair with:

`base64 --decode certificate.txt > personal-assistant.der`

`sudo personal-assistant-display-pair https://<private-mac-ip>:8765 personal-assistant.der <sha256> <six-digit-code> "Kitchen Display"`

The helper writes the credential as the dedicated unprivileged service account with mode 0600 and starts the service. The kiosk opens at the next graphical login. Removing the package intentionally retains `/var/lib/personal-assistant-display/config.json`; revoke the display in the desktop app before deleting that local file.
