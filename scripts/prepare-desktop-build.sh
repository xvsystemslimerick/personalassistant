#!/bin/sh
set -eu

project_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
worker_resource="$project_root/apps/desktop/src-tauri/resources/inference-worker/macos-arm64"

cd "$project_root"
npm run build
cargo build --release --locked -p inference-worker \
  --features persistent-backend-experimental \
  --bin personal-assistant-inference-worker

mkdir -p "$worker_resource"
install -m 755 \
  "$project_root/target/release/personal-assistant-inference-worker" \
  "$worker_resource/personal-assistant-inference-worker"
