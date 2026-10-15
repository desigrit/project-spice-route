# App updates

Settings checks the stable GitHub release feed only when the user chooses **Check for updates**. Windows x64, Windows ARM64, and Apple Silicon each have a dedicated package. Installing requires saved settings and an idle app.

Windows streams the installer into local app storage, checks its length and SHA-256 digest, and asks the Rust engine to verify its Minisign signature against the embedded release key. A read handle prevents replacement between signature verification and launch. The installer waits for graceful app shutdown, extracts into a separate folder, replaces the app, and restarts it. A failed extraction leaves the existing installation in place. Codex and sync data are outside the installation directory.

macOS uses Tauri's updater with the same release key. It verifies the signed app archive, replaces the application, and relaunches it. Install the initial app in a writable Applications folder, not inside the mounted DMG. Packages remain ad-hoc signed on macOS, without Apple notarization.

## Publishing

1. Update the release versions and add `docs/releases/<version>.md`.
2. Run **Desktop builds** with that version and **Publish a GitHub release** enabled, or push a `v<version>` tag.
3. Tests and package builds must pass for all three platforms. CI signs the Windows installers and Mac updater archive using `TAURI_SIGNING_PRIVATE_KEY`, a GitHub Actions secret. The public key is `src-tauri/updater.pub` and is also embedded in Tauri configuration.
4. CI creates a draft release, uploads packages, signatures and `latest.json`, then publishes it. Checks never observe a partially uploaded release. Published version assets must not be replaced.

Keep the private signing key backed up securely outside this repository. Losing it prevents existing installations from accepting future updates. Key rotation requires a planned bridge release. This signing key verifies app updates; it is separate from Windows Authenticode and Apple notarization.

The Windows network tests use fixture downloads and do not launch installers. CI separately validates a clean install and replacement of an existing installation. Mac installation uses the maintained Tauri updater; real-device restart testing is still needed on a Mac and a Windows ARM64 device.
