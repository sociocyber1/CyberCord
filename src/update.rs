//! Updates from the project's GitHub releases, without losing data.
//!
//! * Data lives in its own folder (`~/.local/share/cybercord`, `%APPDATA%`),
//!   never next to the program, so replacing the program does not touch it.
//!   New settings fields always have defaults, so old files load as they are.
//! * Before installing, the data folder is copied to a sibling
//!   `cybercord-backups/<old>-to-<new>-<date>` (without the file cache and the
//!   developer test nodes); the last 5 backups are kept.
//! * The archive is checked against the release's SHA-256 file; only then is
//!   the program file replaced (rename — atomic on Linux; on Windows the
//!   running .exe is moved aside to `CyberCord.old.exe`).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};

pub const REPO: &str = "sociocyber1/CyberCord";
pub const RELEASES_PAGE: &str = "https://github.com/sociocyber1/CyberCord/releases";
const KEEP_BACKUPS: usize = 5;
/// Folders that are not copied to a backup (re-downloadable cache, test nodes).
const SKIP: [&str; 3] = ["blobs", "dev", "update"];

static EXE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// Remember where this program lives, before an update replaces the file
/// (afterwards Linux reports the running one as "… (deleted)").
pub fn remember_exe() {
    if let Ok(p) = std::env::current_exe() {
        let _ = EXE.set(p.canonicalize().unwrap_or(p));
    }
}

fn exe_path() -> Result<PathBuf, String> {
    if let Some(p) = EXE.get() {
        return Ok(p.clone());
    }
    let p = std::env::current_exe().map_err(|e| e.to_string())?;
    let s = p.to_string_lossy();
    Ok(PathBuf::from(s.strip_suffix(" (deleted)").unwrap_or(&s)))
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    /// Archive for this platform, and its .sha256 file.
    pub asset: Option<(String, String, u64)>,
    pub sha: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub enum State {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    /// downloaded bytes / total
    Downloading(Release),
    Installing(Release),
    /// installed: version, backup folder
    Ready(String, PathBuf),
    Failed(String),
}

#[derive(Clone, Default)]
pub struct Updater {
    pub state: Arc<Mutex<State>>,
    pub done: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .try_proxy_from_env(true)
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .user_agent(&format!("CyberCord/{}", env!("CARGO_PKG_VERSION")))
        .build()
}

/// "0.18.0" / "v0.18.0" → (0, 18, 0).
pub fn parse_version(v: &str) -> (u32, u32, u32) {
    let mut it = v.trim().trim_start_matches('v').split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).map(|s| s.parse::<u32>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

/// This program's version (test switch: CYBERCORD_UPDATE_FROM=0.1.0 pretends to be older).
pub fn local_version() -> String {
    std::env::var("CYBERCORD_UPDATE_FROM").unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string())
}

pub fn newer(remote: &str, local: &str) -> bool {
    parse_version(remote) > parse_version(local)
}

/// The archive name suffix for this platform.
fn platform_suffix() -> Option<&'static str> {
    if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("-linux-x86_64.tar.gz")
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        Some("-windows-x86_64.zip")
    } else {
        None
    }
}

/// Ask GitHub for the latest release.
pub fn latest() -> Result<Release, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let v: serde_json::Value = agent().get(&url).set("Accept", "application/vnd.github+json").call().map_err(|e| e.to_string())?.into_json().map_err(|e| e.to_string())?;
    let tag = v["tag_name"].as_str().ok_or("no tag_name")?.to_string();
    let mut rel = Release {
        version: tag.trim_start_matches('v').to_string(),
        notes: v["body"].as_str().unwrap_or("").to_string(),
        page: v["html_url"].as_str().unwrap_or(RELEASES_PAGE).to_string(),
        asset: None,
        sha: None,
    };
    if let (Some(suffix), Some(assets)) = (platform_suffix(), v["assets"].as_array()) {
        for a in assets {
            let name = a["name"].as_str().unwrap_or("");
            let url = a["browser_download_url"].as_str().unwrap_or("").to_string();
            if name.ends_with(suffix) {
                rel.asset = Some((name.to_string(), url, a["size"].as_u64().unwrap_or(0)));
            } else if name.ends_with(&format!("{suffix}.sha256")) {
                rel.sha = Some(url);
            }
        }
    }
    Ok(rel)
}

impl Updater {
    pub fn set(&self, s: State) {
        if let Ok(mut st) = self.state.lock() {
            *st = s;
        }
    }

    pub fn get(&self) -> State {
        self.state.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Check in the background; `quiet` keeps "up to date" / errors as Idle
    /// (for the check at start).
    pub fn check(&self, ctx: egui::Context, quiet: bool) {
        if matches!(self.get(), State::Checking | State::Downloading(_) | State::Installing(_) | State::Ready(..)) {
            return;
        }
        self.set(State::Checking);
        let me = self.clone();
        std::thread::spawn(move || {
            let s = match latest() {
                Ok(r) if newer(&r.version, &local_version()) => State::Available(r),
                Ok(_) => if quiet { State::Idle } else { State::UpToDate },
                Err(e) => if quiet { State::Idle } else { State::Failed(e) },
            };
            me.set(s);
            ctx.request_repaint();
        });
    }

    /// Download, verify, back up the data, replace the program.
    pub fn install(&self, ctx: egui::Context, rel: Release) {
        self.set(State::Downloading(rel.clone()));
        let me = self.clone();
        std::thread::spawn(move || {
            let r = me.do_install(&rel, &ctx);
            me.set(match r {
                Ok(backup) => State::Ready(rel.version.clone(), backup),
                Err(e) => State::Failed(e),
            });
            ctx.request_repaint();
        });
    }

    fn do_install(&self, rel: &Release, ctx: &egui::Context) -> Result<PathBuf, String> {
        let (name, url, size) = rel.asset.clone().ok_or("no archive for this system")?;
        let exe = exe_path()?;
        let dir = exe.parent().ok_or("no program folder")?.to_path_buf();
        // can we write next to the program at all?
        let probe = dir.join(".cybercord-write-test");
        std::fs::write(&probe, b"").map_err(|e| format!("{}: {e}", dir.display()))?;
        let _ = std::fs::remove_file(&probe);

        // 1. download
        let work = cybercord_node::store::data_dir().join("update");
        std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let archive = work.join(&name);
        self.total.store(size, Ordering::Relaxed);
        self.done.store(0, Ordering::Relaxed);
        let resp = agent().get(&url).call().map_err(|e| e.to_string())?;
        if let Some(n) = resp.header("Content-Length").and_then(|v| v.parse::<u64>().ok()) {
            self.total.store(n, Ordering::Relaxed);
        }
        let mut rd = resp.into_reader();
        let mut f = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        loop {
            let n = rd.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            hasher.update(&buf[..n]);
            self.done.fetch_add(n as u64, Ordering::Relaxed);
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        drop(f);
        let digest = hex(&hasher.finalize());

        // 2. verify
        let sha_url = rel.sha.clone().ok_or("the release has no SHA-256 file")?;
        let sha_text = agent().get(&sha_url).call().map_err(|e| e.to_string())?.into_string().map_err(|e| e.to_string())?;
        let expected = sha_text.split_whitespace().next().unwrap_or("").to_lowercase();
        if expected != digest {
            let _ = std::fs::remove_file(&archive);
            return Err(format!("SHA-256 mismatch ({} ≠ {})", &digest[..12], &expected[..expected.len().min(12)]));
        }
        self.set(State::Installing(rel.clone()));
        ctx.request_repaint();

        // 3. back up the data
        let backup = backup_data(&local_version(), &rel.version)?;

        // 4. take the new program out of the archive
        let want = wanted_file();
        let new_bin = dir.join(".cybercord-new");
        extract(&archive, want, &new_bin)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&new_bin, std::fs::Permissions::from_mode(0o755));
        }

        // 5. swap
        #[cfg(windows)]
        {
            let old = exe.with_extension("old.exe");
            let _ = std::fs::remove_file(&old);
            std::fs::rename(&exe, &old).map_err(|e| e.to_string())?;
            if let Err(e) = std::fs::rename(&new_bin, &exe) {
                let _ = std::fs::rename(&old, &exe);
                return Err(e.to_string());
            }
        }
        #[cfg(not(windows))]
        std::fs::rename(&new_bin, &exe).map_err(|e| e.to_string())?;

        let _ = std::fs::remove_file(&archive);
        let note = serde_json::json!({ "from": local_version(), "to": rel.version, "backup": backup });
        let _ = std::fs::write(work.join("last.json"), note.to_string());
        Ok(backup)
    }
}

/// Which file of the release archive replaces this program.
fn wanted_file() -> &'static str {
    if cfg!(windows) {
        "CyberCord.exe"
    } else if cybercord_node::voice::AUDIO_ENABLED {
        "cybercord"
    } else {
        "cybercord-nosound"
    }
}

fn extract(archive: &Path, want: &str, out: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    if archive.to_string_lossy().ends_with(".zip") {
        let mut z = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        for i in 0..z.len() {
            let mut e = z.by_index(i).map_err(|e| e.to_string())?;
            let name = e.name().replace('\\', "/");
            if name.rsplit('/').next() == Some(want) {
                let mut o = std::fs::File::create(out).map_err(|e| e.to_string())?;
                std::io::copy(&mut e, &mut o).map_err(|e| e.to_string())?;
                return Ok(());
            }
        }
    } else {
        let mut t = tar::Archive::new(flate2::read::GzDecoder::new(file));
        for e in t.entries().map_err(|e| e.to_string())? {
            let mut e = e.map_err(|e| e.to_string())?;
            let path = e.path().map_err(|e| e.to_string())?.to_path_buf();
            if path.file_name().and_then(|n| n.to_str()) == Some(want) {
                let mut o = std::fs::File::create(out).map_err(|e| e.to_string())?;
                std::io::copy(&mut e, &mut o).map_err(|e| e.to_string())?;
                return Ok(());
            }
        }
    }
    Err(format!("{want} is not in the archive"))
}

/// Where backups go: a sibling of the data folder.
pub fn backups_dir() -> PathBuf {
    let d = cybercord_node::store::data_dir();
    let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "cybercord".into());
    d.with_file_name(format!("{name}-backups"))
}

/// Copy the data folder (without caches) into a new backup; keep the last 5.
pub fn backup_data(from: &str, to: &str) -> Result<PathBuf, String> {
    let src = cybercord_node::store::data_dir();
    let root = backups_dir();
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let dst = root.join(format!("{from}-to-{to}-{stamp}"));
    copy_dir(&src, &dst, true).map_err(|e| format!("backup: {e}"))?;
    // keep the newest few: only folders this function made (name "<v>-to-<v>-<date>")
    if let Ok(rd) = std::fs::read_dir(&root) {
        let mut ours: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir() && p.file_name().and_then(|n| n.to_str()).map(|n| n.contains("-to-")).unwrap_or(false)).collect();
        ours.sort();
        while ours.len() > KEEP_BACKUPS {
            let old = ours.remove(0);
            let _ = std::fs::remove_dir_all(&old);
        }
    }
    Ok(dst)
}

fn copy_dir(src: &Path, dst: &Path, top: bool) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for e in std::fs::read_dir(src)? {
        let e = e?;
        let p = e.path();
        let name = e.file_name();
        if top && SKIP.iter().any(|s| name == *s) {
            continue;
        }
        if e.file_type()?.is_dir() {
            copy_dir(&p, &dst.join(&name), false)?;
        } else {
            std::fs::copy(&p, dst.join(&name))?;
        }
    }
    Ok(())
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// After an update: "updated from X to Y, backup at …", once.
pub fn take_last_note() -> Option<(String, String, String)> {
    let p = cybercord_node::store::data_dir().join("update").join("last.json");
    let t = std::fs::read_to_string(&p).ok()?;
    let _ = std::fs::remove_file(&p);
    let v: serde_json::Value = serde_json::from_str(&t).ok()?;
    Some((v["from"].as_str()?.to_string(), v["to"].as_str()?.to_string(), v["backup"].as_str().unwrap_or("").to_string()))
}

/// Remove the Windows leftover from the previous update.
pub fn cleanup_old() {
    if let Ok(exe) = exe_path() {
        let _ = std::fs::remove_file(exe.with_extension("old.exe"));
    }
}

/// Start the new program with the same arguments and leave.
pub fn restart() {
    if let Ok(exe) = exe_path() {
        let args: Vec<String> = std::env::args().skip(1).collect();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1500));
            let _ = std::process::Command::new(exe).args(args).spawn();
            std::process::exit(0);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("v0.19.0", "0.18.0"));
        assert!(newer("0.18.1", "0.18.0"));
        assert!(newer("1.0.0", "0.99.9"));
        assert!(!newer("0.18.0", "0.18.0"));
        assert!(!newer("v0.17.5", "0.18.0"));
        assert_eq!(parse_version("v0.18.0"), (0, 18, 0));
    }
}
