# Windows implementation plan

The React UI, domain crates, SQLite migrations, Graph adapter, rules, AI contracts, display projection, and protocol types remain shared. Windows replaces only platform boundaries: WebView2/MSI or MSIX packaging, Credential Manager secret storage, Windows notification delivery, DPAPI-backed local keys, Windows code signing, and service/startup integration.

No Windows port may substitute plaintext files for Keychain secrets, weaken certificate pinning, send email content to a remote model, or change action-confirmation semantics. Windows support requires dedicated migration, installer-upgrade, credential persistence, notification, local-inference packaging, and signing tests before release.
