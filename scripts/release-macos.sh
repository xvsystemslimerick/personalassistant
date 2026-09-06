#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$root/Cargo.toml" | head -1)
identity=${PA_DEVELOPER_ID_APPLICATION:?Set PA_DEVELOPER_ID_APPLICATION to the Developer ID Application identity}
notary_profile=${PA_NOTARY_KEYCHAIN_PROFILE:?Set PA_NOTARY_KEYCHAIN_PROFILE to a notarytool Keychain profile}
app="$root/target/release/bundle/macos/Personal Assistant.app"
dmg_directory="$root/target/release/bundle/dmg"
dmg="$dmg_directory/PersonalAssistant-${version}.dmg"
entitlements="$root/apps/desktop/src-tauri/Entitlements.plist"

node_major=$(node -p 'Number(process.versions.node.split(".")[0])')
if [ "$node_major" -ne 22 ]; then
  echo "Production releases require Node.js 22 LTS." >&2
  exit 1
fi

case "$identity" in "Developer ID Application:"*) ;; *) echo "A Developer ID Application identity is required." >&2; exit 1;; esac
security find-identity -v -p codesigning | grep -Fq "\"$identity\"" || { echo "Signing identity unavailable." >&2; exit 1; }

cd "$root"
npm ci
npm test
cargo test --workspace --locked
cd apps/desktop
node ../../node_modules/@tauri-apps/cli/tauri.js build --bundles app
cd "$root"
[ -d "$app" ] || { echo "Release application is missing." >&2; exit 1; }

find "$app/Contents" -type f -perm -111 -print | while IFS= read -r executable; do
  if file "$executable" | grep -q 'Mach-O'; then
    codesign --force --timestamp --options runtime --entitlements "$entitlements" --sign "$identity" "$executable"
  fi
done
codesign --force --timestamp --options runtime --entitlements "$entitlements" --sign "$identity" "$app"
codesign --verify --deep --strict --verbose=2 "$app"
staging=$(mktemp -d)
trap 'find "$staging" -depth -delete' EXIT HUP INT TERM
ditto "$app" "$staging/Personal Assistant.app"
ln -s /Applications "$staging/Applications"
mkdir -p "$dmg_directory"
hdiutil create -volname "Personal Assistant" -srcfolder "$staging" -format UDZO -ov "$dmg"
xcrun notarytool submit "$dmg" --keychain-profile "$notary_profile" --wait
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
shasum -a 256 "$dmg" > "$dmg.sha256"
echo "Notarized release: $dmg"
