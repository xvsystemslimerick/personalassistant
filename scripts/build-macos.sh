#!/bin/sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$project_root"

node_major=$(node -p 'Number(process.versions.node.split(".")[0])')
if [ "$node_major" -lt 20 ] || [ "$node_major" -ge 23 ]; then
  echo "Personal Assistant requires Node.js 20 or 22 LTS to build (found $(node --version))." >&2
  exit 1
fi

npm ci
npm test
cargo test --workspace --locked
cd apps/desktop
node ../../node_modules/@tauri-apps/cli/tauri.js build --bundles app,dmg
