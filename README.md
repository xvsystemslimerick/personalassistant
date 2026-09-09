# Personal Assistant

A local-first desktop control centre for a private personal assistant. Milestone 1 provides a production-shaped Tauri 2 application, a versioned local SQLite database, and persistent settings.

## Prerequisites

- macOS 12+ on Apple Silicon
- Xcode Command Line Tools
- Node.js 20+
- Rust 1.88 (selected by `rust-toolchain.toml`)

## Development

```sh
npm ci
npm test
npm run tauri -- build
```

Use Node.js 20 or 22 LTS. To run the complete test and macOS packaging pipeline:

```sh
npm run build:macos
```

The final command creates the `.app` and drag-to-Applications DMG. DMG creation uses macOS `hdiutil` and therefore must run in a macOS session/CI runner that permits disk-image devices.

Private testing can use `scripts/package-development-dmg.sh` with the local `Personal Assistant Development` certificate. The resulting `-development.dmg` is intentionally non-notarized and must not be distributed publicly. Public distribution requires paid Apple Developer Program membership, Developer ID Application signing, and Apple notarization through the protected production workflow.

Application data is stored under the operating system application-data directory. No network service or telemetry is used in Milestone 1.

Microsoft sign-in uses the product's embedded public client ID, authorization code with PKCE, and the registered `http://localhost/oauth/callback` desktop loopback URI. A client secret is neither required nor permitted in the desktop application. White-label builds can override the public ID by setting `PA_MICROSOFT_CLIENT_ID` at compile time.

Milestone 3 includes native local capability detection. The desktop reports RAM, processor/acceleration, and free storage and recommends a private-model tier. Model downloading is available only through the pinned and verified lifecycle. The macOS application now bundles a pinned llama.cpp runtime, so users do not need Python, Node, Docker, or Ollama; its local health gate re-verifies both runtime version and model integrity before loading. Email inference remains disabled pending the structured-output safety pipeline.

The pinned model catalog uses official Apache-2.0 Qwen GGUF artifacts: Qwen3 1.7B Q8 for the compact tier and Qwen3 4B Q4_K_M for the standard tier. Artifact URLs include immutable repository revisions and exact expected sizes and SHA-256 digests. Installation is explicit, resumable, size-bounded, digest-verified, and confined to the application model directory. Inference remains disabled until its separate runtime and safety gate is complete.

See [Architecture](docs/ARCHITECTURE.md), [Security](docs/SECURITY.md), [Privacy](docs/PRIVACY.md), [Roadmap](ROADMAP.md), and [Status](STATUS.md).
