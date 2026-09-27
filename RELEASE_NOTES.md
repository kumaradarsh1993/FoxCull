<!-- NO VERSION HEADING IN THIS FILE. release.yml pastes it verbatim into the
     release body; the GitHub release title is the version source. -->

## Opening a whole drive no longer drags in the whole computer

Clicking `C:\` or Macintosh HD used to scan everything on it, including the
operating system and every installed app. On a Mac that meant 241,000 files,
more than a minute of waiting, and a few hundred MB of cache built for app icons
nobody wanted to cull.

- **System and app folders are skipped by default** — `Windows`, `Program Files`,
  `AppData`, `/System`, `/Library`, `~/Library`, `.app` bundles, `node_modules`,
  game libraries. The same Mac drive now scans in about 9 seconds.
- **Only in the places they belong.** A folder of photos *of* windows, or a shoot
  called "Library" on your SSD, still shows up. Only `C:\Windows` and the Mac's own
  `/Library` are hidden.
- **FoxCull won't reopen a whole system drive at launch.** If you left one open,
  the welcome screen offers it back instead of silently re-scanning it.

## Excluded folders, your way

**Settings → Excluded folders** lists what's skipped, grouped: Windows system,
macOS system, app data & bundles, developer folders, game libraries. They're all
ticked by default, and each one shows exactly which folders it hides. Untick any of them.

Add your own too:
- **A specific folder**: right-click it in the sidebar → **Exclude from scans**,
  or use **Add folder…**.
- **A name, anywhere**: type `Proxy` or `*_cache`.

Your choices are saved on this computer. **Reset to defaults** puts it back.

## A calmer welcome screen

With no folder open, the main panel now simply says where to start: pick a folder
on the left, or jump straight to a **camera card** FoxCull spotted, Pictures,
Movies, Desktop or Downloads.

## Fixed

- **Mac: external drives keep their own library again.** Opening an SD card or SSD
  stored its catalog and thumbnails on the Mac instead of the drive. Each drive
  now gets its own `_FoxCull` folder, as on Windows.
- **Mac: the startup disk appears once in the sidebar**, as "Macintosh HD", instead
  of twice.
- **Folder counts in the sidebar match what opens.** They used to count files
  inside skipped folders.

## Also since nightly.5

The notes for nightly.6 and nightly.7 were never refreshed, so here's what they added:
- **In-app updates** — Settings → Version shows when a newer build exists and
  installs it (Windows) or downloads it (Mac, Linux).
- **Mac: "FoxCull is damaged and can't be opened" is fixed.** Builds are now
  properly signed.

---

**Pick your installer:**
- **Windows:** installer `.exe` or `foxcull_*_x64_portable.zip`
- **macOS Apple Silicon:** `.dmg`
- **Linux:** `*.AppImage` or `*.deb`

**Windows:** the app is not code-signed yet; use "More info" → "Run anyway".
**macOS:** the app is not notarized yet. On first launch macOS says it "could not
verify" FoxCull: open **System Settings → Privacy & Security** and click **Open
Anyway**. (Right-click → Open no longer works for this on macOS 15.)
