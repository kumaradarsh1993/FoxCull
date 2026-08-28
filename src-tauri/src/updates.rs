//! In-app updates — the shared module every Fox desktop app carries.
//!
//! This file is deliberately **identical** in wispr-fox, FoxCull, Fox MD
//! (md-reader) and Fox Mark. Only the three constants in the "Per-app
//! configuration" block below differ. If you fix something here, fix it in all
//! four — a divergence is a bug, not a customisation.
//!
//! ## What it does
//!
//! One screen answers three separate questions — *what am I running*, *what is
//! the newest stable*, *is there a newer nightly* — and one button moves
//! between them. On Windows that button is genuinely one click: the installer
//! is downloaded, run silently, and the app relaunches itself. No wizard, no
//! uninstall/reinstall, no trip to a browser.
//!
//! ## Why not `tauri-plugin-updater`
//!
//! The plugin resolves ONE update per endpoint from a signed `latest.json`
//! manifest, and wants a keypair whose private half lives in CI secrets.
//! GitHub's `releases/latest` deliberately excludes pre-releases and there is
//! no "latest pre-release" equivalent, so a two-channel picker would need a
//! second hand-published manifest. These apps ship unsigned builds from public
//! repos, so the signing apparatus would buy nothing and cost a key-management
//! story. This reads the public releases API instead: no new secrets, no
//! release-pipeline change.
//!
//! ## Two consequences worth knowing
//!
//! - **Nightlies must be published pre-releases, not drafts.** GitHub does not
//!   return draft releases to an unauthenticated caller, so a draft nightly is
//!   invisible here. Every repo's `release.yml` publishes `*-nightly*` tags as
//!   pre-releases for exactly this reason.
//! - **The network call lives in Rust, not the webview.** These apps run a
//!   strict CSP with no `connect-src` for external hosts, and opening one up to
//!   reach api.github.com would widen the attack surface of every page the
//!   renderer touches. Rust fetches; the frontend only sees the result.
//!
//! ## Trust boundary
//!
//! Downloads come only from `browser_download_url` values the GitHub API
//! returned for **this** repository, over HTTPS, to a host on a small
//! allowlist, and the finished file must match the byte size the API declared.
//! Nothing here builds a URL from renderer input — `download_and_install` takes
//! a *tag*, re-resolves it server-side, and never accepts a URL across the IPC
//! boundary. The installer fetched is the same unsigned artifact the user would
//! otherwise download by hand; this is strictly less error-prone than that.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};

// ─── Per-app configuration — the ONLY lines that differ between apps ────────

/// `owner/name` of the GitHub repository whose releases this app installs.
const REPO: &str = "kumaradarsh1993/FoxCull";
/// Product name as the user knows it. Only ever shown in status strings.
const PRODUCT: &str = "FoxCull";
/// Token used in the User-Agent. GitHub 403s any API request without one.
const UA_NAME: &str = "FoxCull";

// ─── Everything below is shared ─────────────────────────────────────────────

/// One page is plenty: releases come back newest-first, and a build older than
/// the last 30 is not something anyone is updating *to*.
const PER_PAGE: u32 = 30;

/// Hosts a release asset may be served from. GitHub redirects
/// `browser_download_url` to its CDN, so all of these must be allowed — but
/// nothing else is, which is what stops a tampered API response pointing a
/// download somewhere arbitrary.
const ALLOWED_HOSTS: [&str; 3] = [
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// The event the download emits progress on. Same name in every app.
pub const PROGRESS_EVENT: &str = "update://progress";

// ─── Wire types ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct GhAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

#[derive(Debug, Deserialize)]
struct GhRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    published_at: Option<String>,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<GhAsset>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseAsset {
    pub name: String,
    pub size: u64,
}

/// One release, reduced to what the update panel renders.
#[derive(Debug, Clone, Serialize)]
pub struct ReleaseInfo {
    /// Tag as published, e.g. `v3.4.0-nightly.1`.
    pub tag: String,
    /// Tag minus the leading `v` — what gets version-compared and displayed.
    pub version: String,
    pub html_url: String,
    /// RFC-3339, straight from GitHub. The frontend formats it; a relative time
    /// computed here would be stale the moment it crossed the IPC boundary.
    pub published_at: Option<String>,
    pub prerelease: bool,
    /// Newer than the build that is running right now.
    pub newer: bool,
    /// The installer for THIS platform, when the release has one. `None` means
    /// the release exists but shipped no artifact installable here — the UI
    /// must offer the release page rather than a button that cannot work.
    pub asset: Option<ReleaseAsset>,
    /// First meaningful line of the release notes, for a one-line "what's in it".
    pub summary: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub product: String,
    pub current: String,
    /// True when the running build is itself a pre-release.
    pub current_is_nightly: bool,
    pub stable: Option<ReleaseInfo>,
    /// Only populated when a pre-release is NEWER than the newest stable. A
    /// nightly already superseded by a stable is not an "available nightly" —
    /// offering it would be offering a downgrade.
    pub nightly: Option<ReleaseInfo>,
    /// Whether this platform can complete the install unattended (Windows).
    /// Elsewhere the download still happens; the last step stays manual.
    pub can_self_install: bool,
    /// True when either channel has something newer — the one flag a caller
    /// needs to decide whether to badge a menu item.
    pub update_available: bool,
    pub releases_url: String,
}

/// Progress for the download, emitted on [`PROGRESS_EVENT`].
#[derive(Debug, Clone, Serialize)]
pub struct UpdateProgress {
    /// `starting` | `downloading` | `verifying` | `launching`
    pub phase: &'static str,
    pub downloaded: u64,
    pub total: u64,
    pub tag: String,
}

// ─── Version comparison ─────────────────────────────────────────────────────

/// Semver-ish "is `candidate` newer than `current`".
///
/// Numeric `major.minor.patch` first. On a tie, semver's rule applies: a build
/// with no pre-release suffix beats one that has a suffix (`3.4.0` > `3.4.0-
/// nightly.9`), and two suffixes compare dot-segment by dot-segment, numerically
/// where both segments are numbers. That last part is what makes `nightly.10`
/// sort above `nightly.9` instead of below it, which a plain string compare
/// gets wrong.
pub fn version_is_newer(current: &str, candidate: &str) -> bool {
    use std::cmp::Ordering;

    fn parts(v: &str) -> (Vec<u32>, &str) {
        let v = v.trim().trim_start_matches('v');
        let (head, tail) = v.split_once('-').unwrap_or((v, ""));
        let nums: Vec<u32> = head.split('.').filter_map(|s| s.parse().ok()).collect();
        (nums, tail)
    }

    fn compare_prerelease(a: &str, b: &str) -> Ordering {
        // Empty means "not a pre-release", which outranks any pre-release.
        match (a.is_empty(), b.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            (false, false) => {}
        }
        let mut aa = a.split('.');
        let mut bb = b.split('.');
        loop {
            match (aa.next(), bb.next()) {
                (None, None) => return Ordering::Equal,
                (None, Some(_)) => return Ordering::Less,
                (Some(_), None) => return Ordering::Greater,
                (Some(x), Some(y)) => {
                    let ord = match (x.parse::<u32>(), y.parse::<u32>()) {
                        (Ok(xn), Ok(yn)) => xn.cmp(&yn),
                        _ => x.cmp(y),
                    };
                    if ord != Ordering::Equal {
                        return ord;
                    }
                }
            }
        }
    }

    let (cur_nums, cur_pre) = parts(current);
    let (can_nums, can_pre) = parts(candidate);
    for i in 0..cur_nums.len().max(can_nums.len()) {
        let a = can_nums.get(i).copied().unwrap_or(0);
        let b = cur_nums.get(i).copied().unwrap_or(0);
        match a.cmp(&b) {
            Ordering::Greater => return true,
            Ordering::Less => return false,
            Ordering::Equal => {}
        }
    }
    compare_prerelease(can_pre, cur_pre) == Ordering::Greater
}

// ─── Asset selection ────────────────────────────────────────────────────────

/// Filename suffixes this platform can install, best first.
///
/// Windows prefers the NSIS `-setup.exe` over the `.msi` because only NSIS
/// takes a silent switch that also relaunches the app — the whole point of the
/// one-click path. macOS takes whichever `.dmg` the repo publishes (`universal`
/// in Fox MD, `aarch64` elsewhere). Linux prefers the AppImage, which needs no
/// package manager and no root.
///
/// Matching is by suffix rather than an exact marker on purpose: the four repos
/// name their artifacts differently (`wispr-fox_3.4.0_x64-setup.exe`,
/// `Fox.Mark_0.5.0-nightly.1_x64-setup.exe`, `Fox.MD_0.9.0_x64-setup.exe`) and
/// an exact-name rule silently degrades to "no installer for this platform".
fn wanted_suffixes() -> &'static [&'static str] {
    #[cfg(target_os = "windows")]
    {
        &["x64-setup.exe", "-setup.exe", ".msi"]
    }
    #[cfg(target_os = "macos")]
    {
        &["universal.dmg", "aarch64.dmg", ".dmg"]
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        &[".appimage", ".deb"]
    }
}

/// Windows is the only platform where the installer can be handed to the OS and
/// complete on its own. macOS needs a drag out of a `.dmg` (and these builds are
/// not notarised); a Linux AppImage has nothing to install in the first place.
pub fn can_self_install() -> bool {
    cfg!(target_os = "windows")
}

fn pick_asset(assets: &[GhAsset]) -> Option<&GhAsset> {
    for suffix in wanted_suffixes() {
        if let Some(a) = assets
            .iter()
            .find(|a| a.name.to_ascii_lowercase().ends_with(suffix))
        {
            return Some(a);
        }
    }
    None
}

/// First meaningful line of the release notes — skips the markdown heading,
/// blockquotes and blank lines so the UI gets a sentence, not a `#`.
fn summarize(body: Option<&str>) -> Option<String> {
    let body = body?;
    for raw in body.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with("> ") {
            continue;
        }
        let cleaned = line.trim_start_matches(['*', '-', ' ']).replace("**", "");
        if cleaned.chars().count() < 3 {
            continue;
        }
        return Some(cleaned.chars().take(180).collect());
    }
    None
}

fn to_info(rel: &GhRelease, current: &str) -> ReleaseInfo {
    let version = rel.tag_name.trim_start_matches('v').to_string();
    ReleaseInfo {
        newer: version_is_newer(current, &version),
        asset: pick_asset(&rel.assets).map(|a| ReleaseAsset {
            name: a.name.clone(),
            size: a.size,
        }),
        summary: summarize(rel.body.as_deref()),
        version,
        tag: rel.tag_name.clone(),
        html_url: rel.html_url.clone(),
        published_at: rel.published_at.clone(),
        prerelease: rel.prerelease,
    }
}

// ─── GitHub ─────────────────────────────────────────────────────────────────

fn http() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(600))
        .connect_timeout(std::time::Duration::from_secs(10))
        .user_agent(format!(
            "{UA_NAME}/{} (+https://github.com/{REPO})",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
        .map_err(|e| format!("http client: {e}"))
}

async fn fetch_releases() -> Result<Vec<GhRelease>, String> {
    let client = http()?;
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page={PER_PAGE}");
    let resp = client
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| format!("could not reach GitHub: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub returned HTTP {}", resp.status().as_u16()));
    }
    let releases: Vec<GhRelease> = resp
        .json()
        .await
        .map_err(|e| format!("unexpected response from GitHub: {e}"))?;
    // Drafts are filtered defensively. An authenticated token in the
    // environment would make them visible, and a draft has no public download
    // URL, so offering one would produce a 404 at install time.
    Ok(releases.into_iter().filter(|r| !r.draft).collect())
}

/// Both channels at once, each compared against the running build.
#[tauri::command]
pub async fn update_status() -> Result<UpdateStatus, String> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    let releases = fetch_releases().await?;

    // The API returns newest-first, so the first match in each class wins.
    let stable = releases
        .iter()
        .find(|r| !r.prerelease)
        .map(|r| to_info(r, &current));
    let nightly = releases
        .iter()
        .find(|r| r.prerelease)
        .map(|r| to_info(r, &current));

    // A nightly older than the newest stable is not an upgrade path. Hiding it
    // is the difference between "here are your options" and a button that
    // quietly moves the user backwards.
    let nightly = match (&stable, nightly) {
        (Some(s), Some(n)) if !version_is_newer(&s.version, &n.version) => None,
        (_, n) => n,
    };

    Ok(UpdateStatus {
        product: PRODUCT.to_string(),
        current_is_nightly: current.contains('-'),
        update_available: stable.as_ref().is_some_and(|r| r.newer)
            || nightly.as_ref().is_some_and(|r| r.newer),
        current,
        stable,
        nightly,
        can_self_install: can_self_install(),
        releases_url: format!("https://github.com/{REPO}/releases"),
    })
}

// ─── Download + install ─────────────────────────────────────────────────────

fn host_allowed(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    parsed
        .host_str()
        .map(|h| {
            let h = h.to_ascii_lowercase();
            ALLOWED_HOSTS
                .iter()
                .any(|a| h == *a || h.ends_with(&format!(".{a}")))
        })
        .unwrap_or(false)
}

/// Where downloaded installers are staged. Per-release filenames, so two
/// downloads of different versions never collide and a half-written file from a
/// failed attempt is replaced rather than appended to.
fn staging_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("no cache directory: {e}"))?
        .join("updates");
    std::fs::create_dir_all(&dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

/// Download the installer for `tag` and hand it to the OS.
///
/// Takes a tag rather than a URL on purpose: the URL is re-resolved here from
/// the GitHub API, so a compromised renderer cannot ask this to run an
/// arbitrary download. Returns the path the installer was written to.
#[tauri::command]
pub async fn download_and_install(app: AppHandle, tag: String) -> Result<String, String> {
    let releases = fetch_releases().await?;
    let rel = releases
        .iter()
        .find(|r| r.tag_name == tag)
        .ok_or_else(|| format!("release {tag} is no longer listed on GitHub"))?;
    let asset = pick_asset(&rel.assets).ok_or_else(|| {
        format!("{tag} has no installer for this platform — open the release page instead")
    })?;

    // The URL came from the API, but check it anyway: a download is the one
    // place where trusting a response field blindly would be expensive.
    if !host_allowed(&asset.browser_download_url) {
        return Err("refusing to download: that asset is not hosted by GitHub".into());
    }

    let emit = |phase: &'static str, downloaded: u64, total: u64| {
        let _ = app.emit(
            PROGRESS_EVENT,
            UpdateProgress {
                phase,
                downloaded,
                total,
                tag: tag.clone(),
            },
        );
    };
    emit("starting", 0, asset.size);

    let client = http()?;
    let resp = client
        .get(&asset.browser_download_url)
        .send()
        .await
        .map_err(|e| format!("download failed: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("download failed: HTTP {}", resp.status().as_u16()));
    }

    let total = resp.content_length().unwrap_or(asset.size);
    let dest = staging_dir(&app)?.join(&asset.name);

    // Stream to disk so a 60 MB installer never sits in memory twice, and so
    // the progress bar reports something real rather than a fake spinner.
    let mut file =
        std::fs::File::create(&dest).map_err(|e| format!("could not write {}: {e}", dest.display()))?;
    let mut downloaded: u64 = 0;
    let mut stream = resp;
    loop {
        let chunk = stream
            .chunk()
            .await
            .map_err(|e| format!("download interrupted: {e}"))?;
        let Some(bytes) = chunk else { break };
        use std::io::Write;
        file.write_all(&bytes)
            .map_err(|e| format!("could not write the installer: {e}"))?;
        downloaded += bytes.len() as u64;
        emit("downloading", downloaded, total);
    }
    file.sync_all().ok();
    drop(file);

    // Size check. Not a signature — these artifacts are unsigned, exactly as a
    // manual download would be — but it catches the realistic failure, which is
    // a truncated or interrupted transfer being handed to the OS as an .exe.
    emit("verifying", downloaded, total);
    let written = std::fs::metadata(&dest).map(|m| m.len()).unwrap_or(0);
    if asset.size > 0 && written != asset.size {
        let _ = std::fs::remove_file(&dest);
        return Err(format!(
            "download incomplete ({written} of {} bytes) — not installing",
            asset.size
        ));
    }

    emit("launching", written, total);
    launch_installer(&app, &dest)
}

#[cfg(target_os = "windows")]
fn launch_installer(app: &AppHandle, path: &Path) -> Result<String, String> {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    let is_msi = path
        .extension()
        .map(|e| e.eq_ignore_ascii_case("msi"))
        .unwrap_or(false);

    // Detached, because the installer must outlive us: the very next thing this
    // process does is exit so its own files can be replaced.
    let mut cmd = if is_msi {
        // An .msi is data, not a program — it needs msiexec. `/passive` shows a
        // progress bar with no prompts (there is no equivalent of NSIS's /R, so
        // an MSI update does not relaunch the app).
        let mut c = std::process::Command::new("msiexec");
        c.arg("/i").arg(path).arg("/passive").arg("/norestart");
        c
    } else {
        // Tauri's NSIS template: /S = silent, /R = relaunch the app when the
        // install finishes. This is the whole one-click story. An unrecognised
        // switch is ignored by NSIS, so this stays safe if the template changes.
        let mut c = std::process::Command::new(path);
        c.arg("/S").arg("/R");
        c
    };

    cmd.creation_flags(DETACHED_PROCESS)
        .spawn()
        .map_err(|e| format!("could not start the installer: {e}"))?;

    // Give the installer a beat to take its own lock before this process
    // vanishes. Quitting instantly races it and looks like a crash; not
    // quitting at all deadlocks it on a file it cannot replace.
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        app.exit(0);
    });

    Ok(if is_msi {
        format!("Installing {PRODUCT} — it will close while Windows updates it.")
    } else {
        format!("Installing {PRODUCT} — it will close and reopen on the new version.")
    })
}

#[cfg(not(target_os = "windows"))]
fn launch_installer(app: &AppHandle, path: &Path) -> Result<String, String> {
    // macOS opens the .dmg so the app can be dragged across; Linux reveals the
    // AppImage. Neither can be a true in-place upgrade without either
    // notarisation (mac) or knowing how the user installed it (Linux), so the
    // app deliberately stays running and says what to do next.
    //
    // Plain OS launchers rather than the opener plugin's Rust API: this branch
    // never compiles on the Windows dev machine, so an API guess here would
    // surface only as a CI failure on mac/linux twenty minutes later. `open`
    // and `xdg-open` are stable interfaces that predate this app by decades.
    let _ = app;
    #[cfg(target_os = "macos")]
    let (launcher, target) = ("open", path.to_path_buf());
    #[cfg(all(unix, not(target_os = "macos")))]
    let (launcher, target) = (
        "xdg-open",
        path.parent().unwrap_or(path).to_path_buf(),
    );

    std::process::Command::new(launcher)
        .arg(&target)
        .spawn()
        .map_err(|e| format!("downloaded, but could not open {}: {e}", target.display()))?;

    Ok(format!(
        "Downloaded to {} — finish the install from the window that just opened.",
        path.display()
    ))
}

// ─── Tests ──────────────────────────────────────────────────────────────────
//
// These cannot run in-place: a Tauri app crate's test harness links the whole
// WebView2/tao stack and dies with STATUS_ENTRYPOINT_NOT_FOUND on this machine,
// and none of these repos run `cargo test` in CI. They are verified instead by
// `tools/updates-selftest`, which EXTRACTS this section from this file rather
// than restating it, so a pass is evidence about the shipped code.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_compares_numerically_not_lexically() {
        assert!(version_is_newer("3.4.0", "3.10.0"));
        assert!(!version_is_newer("3.10.0", "3.4.0"));
        assert!(version_is_newer("1.4.0", "1.5.0"));
        assert!(!version_is_newer("1.5.0", "1.5.0"));
    }

    /// The one every naive implementation gets wrong: `nightly.10` sorts BELOW
    /// `nightly.9` as a string, and these repos routinely pass nightly.9.
    #[test]
    fn nightly_ordinals_compare_as_numbers() {
        assert!(version_is_newer("3.4.0-nightly.9", "3.4.0-nightly.10"));
        assert!(!version_is_newer("3.4.0-nightly.10", "3.4.0-nightly.9"));
    }

    /// Semver's rule, and the reason the stable card never offers a downgrade:
    /// a released 3.4.0 is newer than every 3.4.0 nightly that preceded it.
    #[test]
    fn a_release_outranks_its_own_prereleases() {
        assert!(version_is_newer("3.4.0-nightly.2", "3.4.0"));
        assert!(!version_is_newer("3.4.0", "3.4.0-nightly.2"));
        assert!(version_is_newer("3.3.0", "3.4.0-nightly.1"));
    }

    #[test]
    fn leading_v_is_tolerated_on_either_side() {
        assert!(version_is_newer("v0.9.0", "v0.10.0"));
        assert!(!version_is_newer("0.10.0", "v0.9.0"));
    }

    /// Only GitHub's own hosts, and only over HTTPS. This is the check that
    /// stops a tampered or spoofed API response pointing a download somewhere
    /// arbitrary, so it is worth pinning precisely.
    #[test]
    fn host_allowlist_accepts_github_and_rejects_lookalikes() {
        assert!(host_allowed(
            "https://github.com/kumaradarsh1993/wispr-fox/releases/download/v1/a.exe"
        ));
        assert!(host_allowed("https://objects.githubusercontent.com/x"));
        assert!(host_allowed("https://release-assets.githubusercontent.com/y"));
        assert!(host_allowed("https://cdn.objects.githubusercontent.com/z"));

        assert!(!host_allowed("http://github.com/a.exe"));
        assert!(!host_allowed("https://github.com.evil.test/a.exe"));
        assert!(!host_allowed("https://notgithub.com/a.exe"));
        assert!(!host_allowed("https://evilobjects.githubusercontent.com.bad/z"));
        assert!(!host_allowed("a.exe"));
    }

    fn asset(name: &str) -> GhAsset {
        GhAsset {
            name: name.into(),
            browser_download_url: format!("https://github.com/x/y/releases/download/v1/{name}"),
            size: 1,
        }
    }

    /// The real artifact names all four repos publish today. If a picker change
    /// stops matching one of these, that app's Install button silently degrades
    /// to "no installer for this platform" — which is why they are pinned here
    /// verbatim rather than described.
    #[test]
    fn every_repos_real_artifact_set_yields_an_installer() {
        // Refreshed 2026-08-28 from the newest release of each repo. These are
        // real names, copied from `gh release view`, not invented shapes. Two
        // things changed since they were first pinned and neither broke the
        // picker: macOS went from a universal .dmg to an aarch64 one on three
        // of the four apps, and every repo now also publishes an
        // `.app.tar.gz`, which must never be mistaken for an installer.
        let sets: [&[&str]; 4] = [
            &[
                "wispr-fox-3.4.0-nightly.6-1.x86_64.rpm",
                "wispr-fox_3.4.0-nightly.6_aarch64.dmg",
                "wispr-fox_3.4.0-nightly.6_amd64.AppImage",
                "wispr-fox_3.4.0-nightly.6_amd64.deb",
                "wispr-fox_3.4.0-nightly.6_x64-setup.exe",
                "wispr-fox_aarch64.app.tar.gz",
            ],
            &[
                "FoxCull_1.5.0-nightly.7_aarch64.dmg",
                "FoxCull_1.5.0-nightly.7_amd64.AppImage",
                "FoxCull_1.5.0-nightly.7_amd64.deb",
                "FoxCull_1.5.0-nightly.7_x64-setup.exe",
                "foxcull_1.5.0-nightly.7_x64_portable.zip",
                "FoxCull_aarch64.app.tar.gz",
            ],
            &[
                "fox-mark_0.5.0-nightly.3_x64_portable.zip",
                "Fox.Mark_0.5.0-nightly.3_aarch64.dmg",
                "Fox.Mark_0.5.0-nightly.3_amd64.AppImage",
                "Fox.Mark_0.5.0-nightly.3_amd64.deb",
                "Fox.Mark_0.5.0-nightly.3_x64-setup.exe",
                "Fox.Mark_aarch64.app.tar.gz",
            ],
            &[
                "Fox.MD-0.11.0-1-1.x86_64.rpm",
                "Fox.MD_0.11.0-1_amd64.AppImage",
                "Fox.MD_0.11.0-1_amd64.deb",
                "Fox.MD_0.11.0-1_universal.dmg",
                "Fox.MD_0.11.0-1_x64-setup.exe",
                "Fox.MD_0.11.0-1_x64_en-US.msi",
                "Fox.MD_universal.app.tar.gz",
            ],
        ];
        for names in sets {
            let assets: Vec<GhAsset> = names.iter().map(|n| asset(n)).collect();
            let picked = pick_asset(&assets)
                .unwrap_or_else(|| panic!("no installer picked from {names:?}"));
            // Never the portable zip or the .app.tar.gz — those are not installers.
            assert!(!picked.name.ends_with(".zip"), "picked a zip: {}", picked.name);
        }
    }

    /// Windows must prefer NSIS over MSI: only the NSIS switch pair (/S /R)
    /// installs silently AND relaunches, which is the entire one-click promise.
    #[cfg(target_os = "windows")]
    #[test]
    fn windows_prefers_the_nsis_setup_over_the_msi() {
        let assets = vec![
            asset("Fox.MD_0.9.0_x64_en-US.msi"),
            asset("Fox.MD_0.9.0_x64-setup.exe"),
        ];
        assert_eq!(pick_asset(&assets).unwrap().name, "Fox.MD_0.9.0_x64-setup.exe");
    }

    #[test]
    fn no_installer_for_this_platform_is_none_not_a_wrong_guess() {
        let assets = vec![asset("something_x64_portable.zip"), asset("source.tar.gz")];
        assert!(pick_asset(&assets).is_none());
    }

    #[test]
    fn summarize_skips_headings_bullets_and_blank_lines() {
        let body = "# v3.4.0\n\n> Stable release\n\n**Your devices, as one account.** Insights merges.\n";
        assert_eq!(
            summarize(Some(body)).as_deref(),
            Some("Your devices, as one account. Insights merges.")
        );
        assert_eq!(
            summarize(Some("## What\n- **Fixed** the thing\n")).as_deref(),
            Some("Fixed the thing")
        );
        assert_eq!(summarize(Some("#only a heading")), None);
        assert_eq!(summarize(None), None);
    }
}
