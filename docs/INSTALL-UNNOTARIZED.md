# Installing this unnotarized macOS build

This copy of Personal Assistant is distributed directly by its developer. It is **not notarized by Apple** and is not an App Store application.

1. Compare the DMG's SHA-256 value with the value supplied separately by the developer.
2. Open the DMG and drag **Personal Assistant** to **Applications**.
3. In Applications, Control-click Personal Assistant and choose **Open**.
4. If macOS still blocks it, open **System Settings → Privacy & Security**, review the warning for Personal Assistant, and select **Open Anyway**.
5. Never bypass a warning if the application name, source, or checksum is unexpected.

The first use of Microsoft synchronization, private drafts, notifications, or Family Display keys may cause normal macOS permission or Keychain prompts. Do not enter a password into any prompt that does not identify macOS Keychain or Personal Assistant.

Updates are manual for this distribution channel. Download each replacement DMG from the same trusted source, verify its separately supplied SHA-256 value, quit Personal Assistant, and replace the application in Applications. Application Support data and Keychain credentials remain outside the application bundle.

Removing the `com.apple.quarantine` attribute by Terminal command is neither required nor recommended by these instructions.
