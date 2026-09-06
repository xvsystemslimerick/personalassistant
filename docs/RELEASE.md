# Production release operations

Development-signed builds are never public releases. A production build requires an Apple **Developer ID Application** certificate, hardened runtime, timestamping, Apple notarisation, stapling, Gatekeeper assessment, a release SBOM, and published SHA-256 digests.

Store notarisation credentials with `xcrun notarytool store-credentials` in a dedicated CI or release-machine Keychain profile. Never place certificates, private keys, Apple credentials, updater signing keys, or tokens in the repository or shell history.

Set `PA_DEVELOPER_ID_APPLICATION` and `PA_NOTARY_KEYCHAIN_PROFILE`, then run `scripts/release-macos.sh` on an Apple Silicon release machine. The script fails closed unless the identity exists, signs nested Mach-O code explicitly with the least-privilege entitlements, verifies the app, submits the DMG, staples it, validates Gatekeeper acceptance, and writes a SHA-256 sidecar.

Automatic updates remain disabled until an offline updater signing key, embedded public key, HTTPS release endpoint, rollback policy, and preservation tests for Application Support and Keychain state are configured. A normal app replacement must never overwrite the database or user data.

Encrypted backup/restore is a separate gate. It must use an independently derived recovery key and authenticated encryption, default to excluding raw email bodies, validate archive bounds and schema before mutation, create a rollback copy, and restore transactionally.
