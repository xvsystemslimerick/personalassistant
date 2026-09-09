#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$root/Cargo.toml" | head -1)
app="$root/outputs/Personal Assistant.app"
output="$root/outputs/PersonalAssistant-$version-development.dmg"
digest="$output.sha256"

"$root/scripts/sign-development-app.sh"
[ -d "$app" ] || { echo "Signed development application is missing." >&2; exit 1; }
codesign --verify --deep --strict --verbose=2 "$app"

staging=$(mktemp -d)
trap 'find "$staging" -depth -delete' EXIT HUP INT TERM
ditto "$app" "$staging/Personal Assistant.app"
ln -s /Applications "$staging/Applications"
rm -f "$output" "$digest"
hdiutil create -volname "Personal Assistant Development" -srcfolder "$staging" -format UDZO -ov "$output"
shasum -a 256 "$output" > "$digest"

echo "Private, non-notarized development DMG: $output"
echo "This artifact is not approved for public distribution."
