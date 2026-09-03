#!/bin/sh
set -eu

repo_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
architecture=${PA_DISPLAY_ARCHITECTURE:-arm64}
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$repo_root/Cargo.toml" | head -n 1)
binary=${PA_DISPLAY_BINARY:-$repo_root/target/release/family-display-launcher}
output_directory=${PA_DISPLAY_OUTPUT_DIR:-$repo_root/outputs}

if [ "$architecture" != arm64 ]; then
    echo "Only the Raspberry Pi OS arm64 package is supported." >&2
    exit 1
fi
if [ ! -x "$binary" ]; then
    echo "Missing executable launcher: $binary" >&2
    exit 1
fi
binary_description=$(file -b "$binary")
case "$binary_description" in
    *ELF*64-bit*ARM*aarch64*) ;;
    *)
        echo "Launcher is not a Linux ARM64 ELF executable: $binary_description" >&2
        exit 1
        ;;
esac
if [ ! -f "$repo_root/apps/family-display/dist/index.html" ]; then
    echo "Missing Family Display web build." >&2
    exit 1
fi

staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT HUP INT TERM
package_root=$staging/personal-assistant-display
install -d "$package_root/DEBIAN" \
    "$package_root/usr/lib/personal-assistant-display/web" \
    "$package_root/usr/lib/systemd/system" \
    "$package_root/usr/bin" \
    "$package_root/etc/xdg/autostart"
install -m 0755 "$binary" "$package_root/usr/lib/personal-assistant-display/family-display-launcher"
cp -R "$repo_root/apps/family-display/dist/." "$package_root/usr/lib/personal-assistant-display/web/"
find "$package_root/usr/lib/personal-assistant-display/web" -type f -exec chmod 0644 {} \;
install -m 0644 "$repo_root/apps/family-display/deploy/personal-assistant-display.service" "$package_root/usr/lib/systemd/system/"
install -m 0644 "$repo_root/apps/family-display/deploy/personal-assistant-display.desktop" "$package_root/etc/xdg/autostart/"
install -m 0755 "$repo_root/apps/family-display/deploy/personal-assistant-display-kiosk" "$package_root/usr/bin/"
install -m 0755 "$repo_root/apps/family-display/deploy/personal-assistant-display-pair" "$package_root/usr/bin/"
install -m 0755 "$repo_root/apps/family-display/deploy/postinst" "$package_root/DEBIAN/postinst"
install -m 0755 "$repo_root/apps/family-display/deploy/prerm" "$package_root/DEBIAN/prerm"

installed_size=$(du -sk "$package_root/usr" | awk '{print $1}')
cat > "$package_root/DEBIAN/control" <<EOF
Package: personal-assistant-display
Version: $version
Section: utils
Priority: optional
Architecture: $architecture
Installed-Size: $installed_size
Depends: adduser, systemd, chromium | chromium-browser
Maintainer: Personal Assistant
Description: Privacy-first read-only Personal Assistant family display
 Native TLS-pinned proxy and loopback-only kiosk UI for 64-bit Raspberry Pi OS.
EOF

install -d "$output_directory"
dpkg-deb --root-owner-group --build "$package_root" "$output_directory/personal-assistant-display_${version}_${architecture}.deb"
