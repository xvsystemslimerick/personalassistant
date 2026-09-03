#!/bin/sh
set -eu

repository=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
identity="Personal Assistant Development"
app="$repository/target/release/bundle/macos/Personal Assistant.app"
worker="$app/Contents/Resources/resources/inference-worker/macos-arm64/personal-assistant-inference-worker"
output="$repository/outputs/Personal Assistant.app"

if ! security find-identity -v -p codesigning | grep -Fq "\"$identity\""; then
  echo "The '$identity' code-signing identity is unavailable." >&2
  exit 1
fi
if [ ! -d "$app" ] || [ ! -f "$worker" ]; then
  echo "The built application or nested worker is missing." >&2
  exit 1
fi

codesign --force --sign "$identity" "$worker"
codesign --force --deep --sign "$identity" "$app"
codesign --verify --deep --strict --verbose=2 "$app"

if ! file "$worker" | grep -q 'arm64' ||
  ! file "$app/Contents/MacOS/personal-assistant-desktop" | grep -q 'arm64'; then
  echo "ARM64 architecture verification failed." >&2
  exit 1
fi

mkdir -p "$repository/outputs"
if [ -d "$output" ]; then
  backup="$(mktemp -d)/Personal Assistant.app"
  mv "$output" "$backup"
fi
ditto "$app" "$output"
codesign --verify --deep --strict --verbose=2 "$output"
codesign -d -r- "$output" 2>&1
echo "Signed development app: $output"
