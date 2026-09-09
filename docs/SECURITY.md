# Security

## Security posture

Personal Assistant treats email, calendar data, derived classifications, settings, tokens, and household display data as sensitive. The default is local processing, least privilege, no telemetry, and no content egress.

## Trust boundaries

The React webview is untrusted presentation code. It receives only the Tauri commands explicitly registered by the Rust shell. Rust services validate all command input. SQLite and future credentials are never directly accessible to JavaScript. Remote navigation and a localhost HTTP server are not required.

Future external boundaries—Microsoft Graph, model downloads, Raspberry Pi clients, and DAKboard—must each receive a dedicated threat model before implementation.

## Family Display threat model (Milestone 6)

The Family Display is an untrusted shared screen on a potentially hostile home network. A paired Raspberry Pi is not trusted with the desktop database, provider identifiers, email metadata or content, account credentials, local-model access, Keychain access, audit internals, or mutation capabilities. Its only permitted input is a bounded display projection produced after privacy filtering.

Primary threats are unauthorised LAN discovery and scraping, pairing-code guessing, stolen display credentials, replay after revocation, cross-display privilege confusion, malicious browser input, accidental public binding/router exposure, private detail leakage through titles or identifiers, and a compromised Pi attempting writes or high-rate resource exhaustion.

The implementation gates are:

- The server remains absent and disabled until explicit desktop opt-in, bind-address validation, authentication, rate limiting, request/body limits, and shutdown/restart recovery tests exist. It must never bind a public/WAN address automatically or configure router port forwarding.
- A six-digit pairing code is only a short-lived rendezvous challenge, never a bearer credential. It expires after five minutes, is single-use, is rate limited globally and per source, and requires confirmation in the unlocked desktop app.
- Pairing issues an independent random 256-bit display credential. Only a one-way digest is retained by the host; the Pi stores the credential with owner-only permissions. Credentials are scoped read-only, rotated on re-pair, individually revocable, compared in constant time, and never placed in URL query strings or logs.
- Every request is bound to one display identity and its current policy. Revocation and policy changes take effect before the next projection response. Authentication failures return generic errors and no pairing/display existence oracle.
- `display-api` output types structurally omit provider/account/email/source fields. `SENSITIVE` never projects in the initial implementation. `PRIVATE` and `WORK_PRIVATE` can be hidden or generic but cannot emit full titles. Unknown categories and missing policy entries fail hidden.
- The first network protocol is read-only. Touch actions, DAKboard, weather, remote access, discovery, and Home Assistant Node operation require separate threat-model extensions and credentials; they are not inherited from display-read scope.
- Responses use no-store caching policy, a strict content security policy for the client, bounded item counts/text, stable schema versions, and content-free audit events. Raw credentials, display payloads, and rejected private values are never logged.

Local transport security must be explicit. Plain HTTP is not accepted merely because the endpoint is on a home network. The networking slice must choose and test an authenticated encrypted channel suitable for unattended Chromium kiosk operation, including certificate bootstrap and rotation, before projected data leaves the Mac process.

The native display launcher now accepts only a pairing-delivered self-signed host certificate whose DER bytes match the accompanying SHA-256 fingerprint. It disables public trust roots and redirects, connects only to the fixed read-only snapshot path, sends the display credential in an Authorization header, requires JSON, and caps the streamed response at 128 KiB before strict schema validation. Certificate material and credentials never enter Chromium. This client is tested against a real TLS socket, but remains inactive until the Mac listener, explicit opt-in, pairing confirmation, rate limits, and shutdown behavior pass their own gate.

The disconnected Mac listener now refuses disabled configuration, wildcard/public addresses, and every port except 8765. It generates a native self-signed identity for the selected private address and `personal-assistant.local`, caps concurrent connections at 16, limits each source to 60 requests and the service to 600 requests per minute, bounds headers to 8 KiB, rejects request bodies and transfer encoding, and terminates stalled handshakes/requests. Shutdown stops acceptance and aborts and joins in-flight work. All rejected HTTP requests receive the same content-free response. Desktop persistence, Keychain storage, startup activation, and pairing UI remain separate gates.

Pairing is a separately bounded TLS route. The desktop exposes only a six-digit, five-minute, single-use challenge after an explicit Settings action. The server accepts strict JSON containing only that code and a bounded display name, shares the listener's rate/concurrency/time limits, and locks the active challenge after five failures. Successful completion stores only the credential digest on the Mac and returns the random 256-bit credential once over the pinned TLS channel. The response buffer is zeroized and the credential never enters a webview. The Pi launcher atomically stores it in an owner-only file and subsequently exposes only a loopback browser origin.

The pairing screen may expose the configured private-LAN host URL, SHA-256 certificate fingerprint, and Base64-encoded public DER certificate for out-of-band transfer to the Pi. It reads that certificate only from the owner-only, non-symlink identity file after digest verification. The Keychain private key is neither read nor serialized for this operation. These public bootstrap values grant no display access without the separate short-lived code and the credential returned directly to the native Pi client.

## Data classification

- Secrets: OAuth tokens, API keys, display credentials, database/backup keys. Store only in Keychain or an equivalent platform vault.
- Sensitive content: email bodies, attachments, medical/work details, AI inputs, private event details. Encrypt when persisted; never log or display-share by default.
- Derived private data: summaries, extracted actions, waiting items, classifications. Keep local and apply retention/display policies.
- Display-safe projections: explicitly transformed titles, times, categories, reminders, tasks, notices, and optional weather. Never treat source records as display-safe.

## Milestone 1 controls

- Tauri CSP permits packaged application assets only and disables object embedding.
- No remote URLs, analytics, crash upload, updater, or external LLM are configured.
- SQLite uses parameterized statements, foreign keys, WAL, and explicit migrations.
- Settings input has length and enum validation at the Rust domain boundary.
- Release builds use Rust overflow checks and thin LTO.
- Dependency lock files are committed; CI should run `cargo audit`, npm audit review, tests, and artifact verification.
- Logs must never contain message bodies, credentials, tokens, or settings values that may identify household members.

## Untrusted email and local AI

Email HTML, text, headers, links, attachments, calendar payloads, AI output, and provider error text are untrusted. Rendering requires sanitization and external-resource blocking. Inference prompts separate instructions from quoted content. Message text cannot select tools, change automation policy, disclose other records, or execute actions. Structured output is schema-validated, bounded, linked to source evidence, and passed to deterministic rules. Mutating actions use an allowlist, confidence/confirmation policy, idempotency key, and audit record.

Automation policy is user-owned configuration, never model output. Migration 11 restricts it to three known values and creates an append-only audit ledger whose rows cannot be updated or deleted. This foundation does not authorize execution: calendar, correspondence, provider, and notification mutations remain unavailable until separate action-specific safeguards and tests are implemented.

Policy evaluation fails closed on non-finite/out-of-range confidence and unverified sources. Values below the 0.95 automation threshold require review. Calendar creation/update requires a confirmed fact; irreversible and important actions require confirmation; correspondence and sensitive actions require confirmation under every policy. Explicit user confirmation cannot override an unverified source. The policy result itself cannot perform I/O.

The calendar provider adapter uses delegated `Calendars.ReadWrite`, never application permission or a client secret. Create requests accept no caller-controlled URL, are pinned to Microsoft Graph v1 `/me/events`, reject blank/control/oversized titles, invalid or excessive time ranges, and malformed transaction identifiers, and normalize times to UTC. The stable transaction ID supplies provider-side duplicate suppression. The adapter remains unreachable from IPC until local execution audit and recovery gates pass.

Migration 17's calendar execution tables are immutable and content-free. Unique proposal, execution, and transaction identifiers prevent duplicate preparation; terminal outcomes are append-only and one-to-one. Database triggers reject update and deletion. Event titles and source correspondence are deliberately absent from the execution ledger.

Calendar updates require a freshly synchronized Microsoft ETag and send it as `If-Match`; a concurrently changed event must fail instead of being overwritten. The disconnected PATCH adapter is pinned to one encoded Graph event path and serializes only start/end fields. It cannot change title, body, attendees, recurrence, or arbitrary provider resources.

Reply drafting and execution are source-bound: the adapter derives a fixed encoded `/me/messages/{id}/reply` path from one synchronized provider identifier and exposes no recipient or arbitrary URL. Reply text is capped at 16 KiB, excluded from `Debug`, and zeroized after use. Migration 20 stores only opaque source targeting, SHA-256, and byte count; plaintext is stored in a dedicated macOS Keychain service, never SQLite. Local-data deletion removes Keychain entries before metadata. The application requests delegated `Mail.Send` only, never application permission or a client secret. Execution requires a separately confirmed local proposal followed by an immediate second confirmation.

Migration 22 adds disconnected correspondence execution preparation and terminal-result ledgers. Both are immutable and content-free: they retain opaque execution/proposal/account/revision identifiers, digest and byte count, outcome, and bounded reason codes, but no reply text, message body, subject, recipient, sender, or provider payload. Preparation requires a confirmed unexpired correspondence proposal, a still-present synchronized source message, and the exact latest draft revision. An unfinished attempt recovers its original execution key; any terminal result prohibits retry. Before any future provider call, the native layer must independently read the selected revision from Keychain and revalidate its digest and byte count. Ambiguous transport outcomes must be recorded as `unknown` and never retried automatically.

Offline action proposals are immutable inert records, not executable jobs. Idempotency and audit keys accept narrow identifier characters and are one-to-one; exact replay returns the original record and any conflicting reuse fails closed. Expiry must be valid RFC 3339, in the future, and no more than 30 days away. Display labels are bounded and reject control characters. The schema intentionally has no raw body, content, payload, evidence, credential, or provider URL field. Blocked decisions create only an audit event.

Proposal confirmation and cancellation append a single terminal event and matching audit row; proposal records are never rewritten. Exact confirmation replay is safe, while a conflicting event key or decision is rejected. Expired proposals cannot be decided. The confirmation UI states that consent is recorded locally and cannot itself create tasks, change calendars, send correspondence, or deliver notifications.

Proposal construction requires an accepted Review decision and an undeleted synchronized source. It never reads provider content. Undated tasks/waiting items do not create proposals. Stable keys contain only local numeric IDs, target references are opaque local identifiers, and display labels are sanitized and bounded. Policy eligibility remains inert even after explicit review acceptance.

Notification planning rejects control characters, oversized text, invalid identifiers, invalid delivery horizons, and invalid quiet hours. Work-private and sensitive notifications must use generic content. Routine delivery inside quiet hours shifts to the end boundary; urgent alerts may bypass quiet hours but still undergo all validation.

The webview has no notification-plugin capability. Native commands expose only the real macOS UserNotifications authorization state/request and a fixed-content test. Test delivery accepts no caller-controlled content and uses “Notifications are working. No email content is included.” Each request is recorded in the append-only audit ledger with an idempotency key before delivery is attempted, and a native submission error fails visibly. Preview scheduling remains disconnected, so granting OS permission does not enable automated notifications.

Migration 15 adds a separate automatic-local-delivery consent. It defaults off independently of macOS permission and independently of each notification-type preference. Granting OS permission, sending a test, or enabling a preview type cannot silently enable delivery. The scheduler requires this consent, reconciles immediately after Settings changes, and treats revocation as an empty desired set so pending automatic requests are removed. The UI receives only a numeric pending count, never request identifiers or content.

Migration 16 adds an append-only notification scheduling ledger. Rows contain only constrained event and delivery identifiers, notification kind, intended delivery time, outcome/reason code, and creation time. Database triggers reject updates and deletions; the schema intentionally has no title, body, content, payload, evidence, provider URL, or email reference columns. Exact event replay is idempotent and conflicting reuse fails closed.

Automatic scheduling requires real macOS authorization and explicit delivery consent on every reconciliation pass. Revocation supplies an empty desired set and removes app-owned pending requests. Already scheduled past identifiers are never recreated, preventing repeated urgent delivery. A fixed one-minute diagnostic uses generic native-owned content, is separately audited, and is excluded from automatic cancellation so it can complete its live gate.

Preview generation reads only active local derived items and sanitized analysis attributes. It excludes undone/completed items, never retrieves email content, and exposes only validated title/body/time/privacy metadata through IPC. Preview generation is read-only and cannot be confused with permission or delivery state.

Extraction schema version 1 rejects unknown fields, non-finite/out-of-range confidence, oversized source/output/fields/collections, unsupported versions, and evidence that does not exactly match valid UTF-8 source byte boundaries. Prompt delimiter characters inside source data are escaped. The deterministic rules boundary emits only index-based inert proposals; low-confidence candidates/classification, critical urgency, and classification/action contradictions require Review. No rule-engine type can directly invoke a provider or persistence mutation.

Fixture generation is constrained by a compiled-in GBNF grammar and deterministic sampler settings. Runtime banners and footers are excluded by bounded JSON framing before strict decoding. The model never controls evidence offsets: it returns a quote, and Rust accepts it only when it occurs exactly once in the source, then derives the offsets. Synthetic evaluation failures return sanitized categories without model output or source content. Live provider records are not connected to this runner.

Known prompt-injection phrases are conservatively detected before inference and routed to Review; a safety rejection is the required pass condition for the adversarial corpus case. This is defense in depth rather than the sole injection control. Evaluation diagnostics expose only classification and candidate counts for synthetic fixtures.

Relative dates are resolved only against an explicit trusted message timestamp and UTC offset. Unsupported or vague expressions, invalid clock values, missing appointment starts, and reversed appointment ranges enter Review; they are never silently normalized. Synthetic evaluation reports expose only stable case IDs and aggregate pass/fail counts.

## Private model supply chain

Hardware detection is read-only and exposes only coarse device capability to the local webview. Model downloads remain disabled until a pinned, versioned catalog is bundled with the signed application. Each catalog entry must constrain source host, exact byte size, SHA-256 digest, runtime compatibility, minimum resources, and license metadata. Downloads use TLS, bounded resumable staging files, size limits, atomic promotion after digest verification, and restrictive local permissions. The redirect allowlist contains only exact Hugging Face delivery hosts observed for the pinned artifacts, including `us.aws.cdn.hf.co`; suffix or substring matches are prohibited. Model files are treated as non-executable data and are never loaded before verification. Removal targets only the application-owned model directory. Neither model download requests nor diagnostics contain email, calendar, account, or derived private data.

The embedded inference runtime is pinned to the official llama.cpp macOS ARM64 b10434 release archive and its published SHA-256 digest. Its executable, dependent libraries, licence, and provenance manifest are sealed inside the signed application bundle. Before execution, the application requires the expected runtime build string; before every model load it recomputes and checks the complete model byte length and SHA-256 digest. The health gate uses a fixed harmless prompt, suppresses child-process input/output, applies a deadline, and kills an over-time process. The runtime opens no listener and receives no email or calendar content at this stage. Metal is preferred, with a no-offload CPU fallback when Metal cannot initialize.

The persistent-worker boundary does not use TCP, Unix-domain sockets, shared HTTP services, command-line payloads, or filesystem logs. It accepts only size-bounded framed JSON over inherited anonymous pipes, validates the protocol version and request identifier before dispatch, and reports stable error codes without echoing content. Malformed/truncated/oversized frames terminate or fail closed. The production adapter enforces one active/eight pending, absolute deadlines, strict response-ID correlation, process termination on active cancellation, and at most three restarts per 60-second window. It never retries the failed active payload; only still-valid queued work can be promoted into a fresh process.

The production private-work queue couples payload and metadata in one bounded allocation, rejects duplicate IDs, removes only the matching queued payload on cancellation, and discards expired items before promotion. Debug output is explicitly redacted and regression-tested not to contain payload content or trusted environment values. The queue has no persistence or logging path. Subprocess tests prove that cancellation and deadlines kill a blocked worker without accepting late output, while repeated crashes exhaust the bounded restart allowance. Provider activation remains disabled pending an adapter-routed packaged corpus pass.

Before persistent-backend model access, a compiled C shim loads the application-bundled llama.cpp library locally and verifies the complete required symbol surface. Its ARM64 structure-size contract was generated from the official b10434 header at pinned commit `7e4c0a968`; source digests and sizes are recorded beside the worker. Compilation treats shim warnings as errors. Missing libraries or symbols produce only sanitized local error categories and extraction remains unavailable.

The complete transitive llama.cpp C header set used by the shim is vendored at the pinned commit and each file's SHA-256 is verified during every build. Native by-value types come directly from those headers rather than locally mirrored declarations. The feature-enabled packaged worker applies the model's embedded chat template through the pinned native API and has passed its Apple Metal four-case corpus. Default non-packaged builds still return `backend_unavailable` for extraction.

The qualified worker executable is present in the signed application's private resources and can currently be launched only by a dedicated synthetic-corpus Tauri command. Backend activation requires trusted inherited environment paths supplied by Rust; webview code cannot set paths or submit source text. The command re-verifies the complete model digest, suppresses child stderr, sanitizes results, and now routes health and extraction through the production adapter. Diagnostics contain only case identifiers, stable failure categories, classification, and candidate counts—never source or generated text. Explicit waiting-for rules derive exact offsets from narrowly matched source phrases and never synthesize provider facts. Provider records remain unreachable until the adapter-routed packaged corpus preserves the existing 4/4 gate.

The adapter-routed packaged corpus has preserved the 4/4 gate. A successful run now records a local qualification tuple containing only model SHA-256, pinned runtime build, corpus version, and timestamp. The separate local-email-analysis consent remains OFF by default and cannot be saved unless the current tuple matches. Model removal clears the qualification and consent. This setting is authority for a later local-only retrieval path; it does not itself fetch content, retain bodies, or permit network AI calls.

The future Microsoft-to-local-worker path begins with a bounded transient retrieval primitive. It disables HTTP redirects to prevent bearer/content boundary drift, encodes and limits provider identifiers, requests only required fields and plaintext, caps the complete streamed JSON response at 96 KiB and the body at 64 KiB, and requires the returned ID to match. HTML, malformed, mismatched, or oversized responses fail closed. Subject/body values are excluded from debug output and zeroed on drop. No Tauri command or background synchronization path can invoke this primitive yet.

Derived persistence is separately constrained. The database API accepts a validated extraction and deterministic rule plan, then creates its own evidence-free representation; callers cannot pass serialized extraction JSON directly. It stores bounded summaries and inert candidate fields but never evidence quotes or offsets. A SQLite-level regression test verifies a private source excerpt is absent. The provider handoff remains disabled until source ownership is zeroizing across every queue and framed-write copy.

Source ownership is now zeroizing across the application-controlled IPC path. Worker requests carry a debug-redacted sensitive-string type, the bounded queue owns that type, and every cloned request remains zeroizing. Complete serialized outbound frames and received frame bodies are also zeroed on drop. Regression coverage confirms request diagnostics cannot reveal source text. This clears the memory-ownership prerequisite, but no provider command is enabled in this slice.

The first provider command is explicit rather than automatic. It accepts only an account/message pair already present in local metadata, is globally single-flight, and rechecks consent, exact qualification, full model integrity, and Keychain-backed account authority before retrieval. The body stays inside Rust and inherited worker pipes; it is never returned to the webview or written to SQLite. Worker output is independently revalidated against the source, then rules produce inert suggestions and the evidence-free database API persists them. Prompt-injection or invalid-output rejection produces a sanitized manual-review failure and no derived record.

## Microsoft and network security

OAuth uses system-browser authorization code with PKCE, exact redirect validation, random state/nonce, minimal scopes, and Keychain token storage. Graph clients enforce TLS, bounded response sizes, timeouts, pagination limits, throttling backoff, and sanitized errors. Disconnect supports credential removal and local-data deletion.

The current Microsoft permission set is delegated `User.Read`, `Mail.Read`, and `Calendars.Read`, plus OIDC `openid`, `profile`, and `offline_access`. It intentionally excludes mail/calendar write permissions. The product build contains a public client ID but no client secret.

The display service is disabled until configured, intended for the private LAN, and never deliberately exposed through router/NAT configuration. Pairing codes are short-lived and rate-limited. Per-display random tokens are hashed at rest, scoped, revocable, and compared safely. Responses carry no-store/restrictive browser headers and only privacy-filtered DTOs. Public ICS/DAKboard URLs are unguessable, revocable capabilities and remain an explicit privacy tradeoff.

## Encryption and key lifecycle

Database-level or record-level authenticated encryption protects persisted sensitive records; keys are random and stored in Keychain. Ciphertext records carry algorithm/key-version metadata to support rotation. Backups use an independent user-recoverable key derivation design and authenticated encryption. Deletion covers primary records, search indexes, caches, model working files, and future backup behavior disclosed to the user.

## Secrets and OS storage

No secrets are needed in Milestone 1. OAuth refresh tokens and pairing secrets must later use macOS Keychain (and Windows Credential Manager when supported), never SQLite or frontend storage. Sensitive database encryption and key lifecycle will be decided before ingestion of email content.

## Release requirements

Public releases must be built in a controlled CI environment, signed with Developer ID Application, use hardened runtime and least-privilege entitlements, be notarized by Apple, stapled, and verified with `codesign` and `spctl`. Developer builds are not equivalent to a distributable release.

The owner has separately accepted an unnotarized direct-sharing fallback. That channel is explicitly labeled, uses the stable local development identity, supplies a checksum and opening instructions, and never publishes automatic-update metadata. It does not provide Apple's malware notarization or generally trusted Developer ID chain; recipients must independently trust the source and verify the digest. No instruction removes macOS quarantine metadata. This fallback is not recorded as satisfying the production signing gate.

Release automation pins third-party GitHub Actions to immutable commit SHAs. It generates a deterministic CycloneDX SBOM from both locked Rust metadata and the npm lockfile, records the pinned local-inference runtime manifest digest, and fails dependency review on high-severity advisories. The macOS release script requires Node.js 22 LTS and the repository-pinned Rust toolchain, builds the app before the disk image, explicitly signs nested Mach-O code, signs the app with hardened runtime, then creates, notarizes, staples, Gatekeeper-assesses, and hashes the DMG. It cannot fall back to development or ad-hoc signing.

The updater embeds only its independent offline signing public key. It uses one fixed HTTPS GitHub manifest endpoint, requires an explicit check and install action, rechecks the selected version, and relies on Tauri's mandatory minisign verification before installation. The private updater key is owner-only and remains outside the repository; the release script requires the supplied public key to equal the embedded trust anchor. The private updater key, Developer ID identity, and notarisation credentials are mandatory release inputs and never repository content. The release process archives the already Developer-ID-signed and notarised app before applying the independent updater signature. Backup export, verification, and restore use authenticated encryption, independent recovery-key derivation, bounded input validation, and default raw-email exclusion. Restore requires a second destructive confirmation, stages only after cryptographic and SQLite validation, creates a validated rollback first, and replaces the live database only at process startup. Digest-bound transaction markers and recovery tests cover interruption before and after each rename. A malformed or tampered restore cannot overwrite a valid live database; a missing live database with a recoverable moved-aside original is repaired before startup continues.

Database startup accepts only a contiguous migration ledger beginning at version 1 and no newer than the application-supported schema. A future version or missing historical entry fails before application tables are created or migrated. After migration, every version through the current schema must be present; backup validation separately requires the exact current version and SQLite integrity. This prevents older application code or a corrupted ledger from silently interpreting an incompatible database.

Keychain-dependent local development builds must use the persistent `Personal Assistant Development` code-signing identity through `scripts/sign-development-app.sh`. Plain ad-hoc signatures change their CDHash on every build and lose Keychain continuity. An identifier-only ad-hoc designated requirement is prohibited because an unrelated locally signed application could copy the identifier. The development certificate is local-only and is not a substitute for Developer ID signing, hardened runtime, notarization, or stapling.

## Reporting

Do not include private user data in a vulnerability report. Until a private reporting channel is defined, report only reproducible technical details and sanitized fixtures.
