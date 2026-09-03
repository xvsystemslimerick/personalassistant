#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
service=$repo_root/apps/family-display/deploy/personal-assistant-display.service
kiosk=$repo_root/apps/family-display/deploy/personal-assistant-display-kiosk
pair=$repo_root/apps/family-display/deploy/personal-assistant-display-pair

grep -q '^User=personal-assistant-display$' "$service"
grep -q '^NoNewPrivileges=true$' "$service"
grep -q '^ProtectSystem=strict$' "$service"
grep -q '^RestrictAddressFamilies=AF_INET AF_INET6$' "$service"
grep -q '127.0.0.1:4173' "$kiosk"
if grep -Eq -- '--no-sandbox|--disable-web-security|https?://[^1]' "$kiosk"; then
    echo "Unsafe Chromium configuration detected." >&2
    exit 1
fi
grep -q 'runuser -u personal-assistant-display' "$pair"
grep -q 'install -m 0600 -o personal-assistant-display' "$pair"
grep -q 'systemctl enable --now personal-assistant-display.service' "$pair"
grep -q 'ELF\*64-bit\*ARM\*aarch64' "$repo_root/scripts/package-family-display-deb.sh"

echo "Family Display package policy checks passed."
