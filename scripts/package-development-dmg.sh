#!/bin/sh
set -eu

root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
export PATH="$HOME/.cargo/bin:$PATH"
version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' "$root/Cargo.toml" | head -1)
app="$root/outputs/Personal Assistant.app"
output="$root/outputs/PersonalAssistant-$version-unnotarized.dmg"
digest="$output.sha256"
instructions="$root/docs/INSTALL-UNNOTARIZED.md"

cd "$root"
node_major=$(node -p 'Number(process.versions.node.split(".")[0])')
if [ "$node_major" -ne 20 ] && [ "$node_major" -ne 22 ]; then
  echo "Unnotarized packaging requires Node.js 20 or 22 LTS (found $(node --version))." >&2
  exit 1
fi
command -v cargo >/dev/null || { echo "The repository-pinned Rust toolchain is unavailable." >&2; exit 1; }
node "$root/scripts/verify-release-metadata.mjs"
npm ci
npm audit --audit-level=high
npm test
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
npm --prefix apps/desktop run tauri -- build --bundles app
"$root/scripts/sign-development-app.sh"
[ -d "$app" ] || { echo "Signed development application is missing." >&2; exit 1; }
[ -f "$instructions" ] || { echo "Unnotarized installation instructions are missing." >&2; exit 1; }
codesign --verify --deep --strict --verbose=2 "$app"

staging=$(mktemp -d)
trap 'find "$staging" -depth -delete' EXIT HUP INT TERM
ditto "$app" "$staging/Personal Assistant.app"
ditto "$instructions" "$staging/READ ME FIRST.md"
ln -s /Applications "$staging/Applications"
rm -f "$output" "$digest"
hdiutil create -volname "Personal Assistant Development" -srcfolder "$staging" -format UDZO -ov "$output"
(cd "$(dirname "$output")" && shasum -a 256 "$(basename "$output")") > "$digest"

echo "Unnotarized direct-distribution DMG: $output"
echo "Recipients must follow READ ME FIRST.md and verify the separately supplied SHA-256 digest."
echo "This artifact is not an Apple-notarized production release and cannot use automatic updates."
