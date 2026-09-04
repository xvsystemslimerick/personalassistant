# Roadmap

Milestones are strict gates. Every milestone must retain migrations, tests, privacy invariants, and a working macOS package before the next begins.

## Milestone 1 — Desktop shell and packaging

- Tauri 2, React, TypeScript, and Rust workspace.
- First-run shell and settings interface.
- Versioned local SQLite database and persistent validated settings.
- Apple Silicon `PersonalAssistant.app` and drag-to-Applications DMG.
- Development signature verification and production signing/notarisation configuration.

Exit criteria: clean checkout builds without global runtime dependencies; tests and audits pass; settings survive restart; the app bundle launches; DMG installs normally.

## Milestone 2 — Microsoft integration

- Microsoft identity OAuth using system browser and authorization-code flow with PKCE.
- Keychain-backed tokens; no passwords or refresh tokens in SQLite.
- User consent and selectable access scopes.
- Microsoft Graph adapter for inbox, sent items, and calendar reads.
- Delta synchronization, pagination, retry/backoff, offline recovery, and provider fixtures.
- Metadata-only retention by default; full content storage remains an explicit opt-in.

Exit criteria: connect/disconnect succeeds, tokens remain outside SQLite, incremental sync is repeatable and crash-safe, and no Graph payload reaches an external AI service.

## Milestone 3 — Local AI and extraction

- Hardware/disk detection and explicit private-model download with hash verification.
- Embedded or bundled local inference runtime with Apple Silicon acceleration.
- Structured classification, action extraction, urgency, dates, appointments, and waiting-for detection.
- Prompt-injection defenses, schema validation, deterministic rule engine, confidence thresholds, and evaluation corpus.
- Principle enforced in code: AI understands; rules decide; AI never executes.

Exit criteria: pinned local runtime and verified model run without external dependencies; isolated persistent worker passes the packaged Apple Metal corpus; prompt injection and evidence gates fail closed; no provider content reaches an external model or an unqualified local inference route.

Current gate: passed. The bounded process adapter and packaged Apple Metal corpus pass, and a live explicitly selected Microsoft message completed transient retrieval, qualified local inference, deterministic rules, and evidence-free persistence. Exact-build qualification and default-off consent continue to guard provider analysis. Automatic mailbox inference remains disabled while Milestone 4 Review UX is built.

## Milestone 4 — Assistant interface

- Home, Today, To Do, Waiting For, Calendar, Review, Work, Kids, and Personal views.
- Task lifecycle: complete, snooze, reschedule, open source email, draft reply, ignore.
- Combined category-aware calendar without merging provider calendars.
- Accessible review and explanation flows for uncertain extractions.

Exit gate: passed. The required Home, Today, To Do, Waiting For, Calendar, and Review experiences are present, with additional Work, Kids, Personal, provenance, lifecycle, dashboard, and accessible navigation slices. Draft correspondence remains intentionally deferred to the Milestone 5 correspondence-confirmation gate; no provider mutation was introduced.

## Milestone 5 — Automation and notifications

- Conservative, Balanced (default), and Assistant automation policies.
- Calendar creation/update safeguards and correspondence confirmation rules.
- Native macOS reminders, urgent alerts, morning summary, and evening summary.
- Offline queueing, idempotency, audit history, and undo/recovery behavior.

Notification gate: passed live. Packaged fixed-content one-minute scheduling worked, and revoking delivery consent immediately reduced the pending automatic-request count to zero. Default-off consent, validated plans, real macOS authorization, append-only content-free ledger, restart recovery, quiet hours, and past-delivery replay suppression remain connected. The next bounded gate is calendar execution readiness; provider writes stay disabled until delegated scope, confirmation, idempotency, audit, and recovery requirements are all satisfied.

Calendar-create gate: passed live. A verified appointment required local proposal confirmation plus a second immediate execution confirmation, created exactly one Microsoft event, and preserved append-only preparation/result history. Recovery reused the original provider transaction ID after an interrupted local attempt. Calendar update remains disabled while existing-event matching, explicit before/after review, PATCH preconditions, and immutable recovery safeguards are built.

Calendar-update gate: passed live. Synchronized ETags, immutable before/after targets, explicit local confirmation, a second immediate execution confirmation, `If-Match`, stale-write rejection, append-only terminal outcomes, and no retry after ambiguous transport are connected. The packaged app changed the selected event exactly once; follow-up sync captured the expected UTC time, a refreshed ETag, and one successful terminal result.

Private reply-draft gate: passed live. Plaintext is stored only in a dedicated macOS Keychain service, immediately read back and digest/size verified, and zeroized after use. SQLite retains only immutable source targeting and append-only content-free revision metadata. Replacement recovery and a packaged content-free verification control passed; no `Mail.Send` permission or send route exists.

Correspondence provider activation: live-qualified. Delegated `Mail.Send` is included in OAuth; one reply-only route is available solely for a confirmed source-bound proposal and immediate second confirmation. It revalidates the exact latest Keychain revision, cannot select recipients or URLs, records one immutable terminal result, and never retries an ambiguous transport outcome.

Correspondence live gate: passed. A newly saved 19-byte private revision was independently read and verified, confirmed locally, immediately confirmed again for execution, and accepted exactly once by Microsoft Graph. The content-free ledger contains one successful terminal result and no duplicate; follow-up delta synchronization retrieved the new sent record with the expected reply subject and provider timestamp. Two earlier Keychain-missing attempts remain immutable closed failures and never reached Graph.

## Milestone 6 — Family Display

- Separate responsive read-optimised Raspberry Pi/Chromium application.
- Mac-hosted read-only display API, pairing codes, scoped tokens, revocation, and multiple displays.
- Today/week/month/morning/evening modes and optional touchscreen-safe actions.
- Privacy projection enforcing PUBLIC_FAMILY, PRIVATE, WORK_PRIVATE, and SENSITIVE.
- Raspberry Pi 4/5 kiosk deployment and automatic startup.

Qualification: complete. The signed macOS control centre, encrypted private-LAN listener, single-use pairing, privacy projection, immediate revocation, loopback-only kiosk client, and native Raspberry Pi OS ARM64 Debian package passed their live or native CI gates. The desktop exposes only verified public trust material during setup; private keys and display credentials remain outside both webviews.

## Milestone 7 — DAKboard

- Private, revocable ICS feeds by category and combined family feed.
- Navigation-free DAKboard display page.
- Optional `DakboardProvider` API adapter isolated from the core.
- Conservative display privacy identical to the Family Display projection.

## Milestone 8 — Production release

- Developer ID signing, hardened runtime, notarisation, stapled DMG, and automatic updater.
- Encrypted backup/restore excluding raw email unless explicitly enabled.
- Migration/recovery testing, accessibility/performance audits, SBOM, dependency review, privacy documentation, and release operations.
- Windows implementation plan preserving service and UI boundaries.
