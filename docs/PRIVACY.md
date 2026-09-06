# Privacy

Milestone 1 stores settings only on the user's device. It makes no network requests and contains no telemetry, advertising, analytics, remote crash reporting, or external AI integration.

Future email and calendar access will be opt-in, scoped to the minimum provider permissions, and revocable. Email content and AI prompts derived from it will remain on-device. The UI must explain retention, deletion, model storage, display sharing, and every external data flow before those capabilities are enabled.

Encrypted backups are created only after an explicit native save action. The recovery password is transient, is never stored in SQLite, Keychain, logs, or settings, and is cleared from the interface after each attempt. The current database schema stores no raw email bodies, so backup exports contain metadata and derived local state only. Explicit verification decrypts to an owner-only transient file, returns only byte count and validity, deletes the file, and never displays or restores its contents. Restore is unavailable until rollback qualification passes.
