#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$root/Cargo.toml" | head -1)
identity=${PA_DEVELOPER_ID_APPLICATION:?Set PA_DEVELOPER_ID_APPLICATION to the Developer ID Application identity}
notary_profile=${PA_NOTARY_KEYCHAIN_PROFILE:?Set PA_NOTARY_KEYCHAIN_PROFILE to a notarytool Keychain profile}
updater_public_key=${PA_UPDATER_PUBLIC_KEY:?Set PA_UPDATER_PUBLIC_KEY to the minisign public key}
release_tag=${PA_RELEASE_TAG:?Set PA_RELEASE_TAG to v followed by the application version}
: "${TAURI_SIGNING_PRIVATE_KEY:?Set TAURI_SIGNING_PRIVATE_KEY to the updater private key path or content}"
app="$root/target/release/bundle/macos/Personal Assistant.app"
dmg_directory="$root/target/release/bundle/dmg"
dmg="$dmg_directory/PersonalAssistant-${version}.dmg"
entitlements="$root/apps/desktop/src-tauri/Entitlements.plist"
release_archive="$dmg_directory/PersonalAssistant-${version}-aarch64.app.tar.gz"
release_signature="$release_archive.sig"
update_manifest="$dmg_directory/latest.json"

node_major=$(node -p 'Number(process.versions.node.split(".")[0])')
if [ "$node_major" -ne 22 ]; then
  echo "Production releases require Node.js 22 LTS." >&2
  exit 1
fi

case "$identity" in "Developer ID Application:"*) ;; *) echo "A Developer ID Application identity is required." >&2; exit 1;; esac
[ -n "$updater_public_key" ] || { echo "The updater public key cannot be empty." >&2; exit 1; }
[ "$updater_public_key" = "$(tr -d '\r\n' < "$root/apps/desktop/src-tauri/updater.pub")" ] || { echo "PA_UPDATER_PUBLIC_KEY does not match the embedded trust anchor." >&2; exit 1; }
[ "$release_tag" = "v$version" ] || { echo "PA_RELEASE_TAG must exactly match v$version." >&2; exit 1; }
security find-identity -v -p codesigning | grep -Fq "\"$identity\"" || { echo "Signing identity unavailable." >&2; exit 1; }

cd "$root"
node scripts/verify-release-metadata.mjs
npm ci
npm test
cargo test --workspace --locked
cd apps/desktop
APPLE_SIGNING_IDENTITY="$identity" node ../../node_modules/@tauri-apps/cli/tauri.js build --bundles app
cd "$root"
[ -d "$app" ] || { echo "Release application is missing." >&2; exit 1; }
[ -f "$entitlements" ] || { echo "Release entitlements are missing." >&2; exit 1; }
[ "$(du -sk "$app" | awk '{print $1}')" -le 98304 ] || { echo "Model-free application bundle exceeds the 96 MiB release budget." >&2; exit 1; }
codesign --verify --deep --strict --verbose=2 "$app"
staging=$(mktemp -d)
trap 'find "$staging" -depth -delete' EXIT HUP INT TERM
codesign -d --entitlements "$staging/signed-entitlements.plist" "$app"
for entitlement in com.apple.security.cs.allow-jit com.apple.security.cs.allow-unsigned-executable-memory com.apple.security.cs.disable-library-validation; do
  [ "$(plutil -extract "$entitlement" raw -o - "$staging/signed-entitlements.plist")" = "false" ] || { echo "Unsafe hardened-runtime entitlement: $entitlement" >&2; exit 1; }
done
ditto -c -k --keepParent "$app" "$staging/notarization.zip"
xcrun notarytool submit "$staging/notarization.zip" --keychain-profile "$notary_profile" --wait
xcrun stapler staple "$app"
xcrun stapler validate "$app"
ditto "$app" "$staging/Personal Assistant.app"
ln -s /Applications "$staging/Applications"
mkdir -p "$dmg_directory"
rm -f "$release_archive" "$release_signature"
COPYFILE_DISABLE=1 tar -czf "$release_archive" -C "$(dirname "$app")" "$(basename "$app")"
node "$root/node_modules/@tauri-apps/cli/tauri.js" signer sign "$release_archive"
[ -s "$release_signature" ] || { echo "Signed updater artifact is missing." >&2; exit 1; }
rm -f "$update_manifest"
node "$root/scripts/generate-update-manifest.mjs" "$version" "$release_tag" "$release_archive" "$release_signature" "$update_manifest"
node "$root/scripts/qualify-update-preservation.mjs" "$release_archive" "$release_signature" "$update_manifest" "$version"
hdiutil create -volname "Personal Assistant" -srcfolder "$staging" -format UDZO -ov "$dmg"
xcrun notarytool submit "$dmg" --keychain-profile "$notary_profile" --wait
xcrun stapler staple "$dmg"
xcrun stapler validate "$dmg"
spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg"
(cd "$dmg_directory" && shasum -a 256 "$(basename "$dmg")") > "$dmg.sha256"
(cd "$dmg_directory" && shasum -a 256 "$(basename "$release_archive")" "$(basename "$release_signature")" "$(basename "$update_manifest")") > "$dmg_directory/update-assets.sha256"
echo "Notarized release and signed updater artifact: $dmg"
