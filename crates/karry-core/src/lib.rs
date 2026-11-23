//! Karry core: back up, restore and sync a KDE Plasma setup.
//!
//! catalog.toml says what to copy and how (scope shared/machine/secret, apply strategy).
//! scan reads the system, store writes/reads/validates backups and snapshots, apply turns a backup
//! into key-level changes and runs them safely (kwriteconfig6 while Plasma runs, staged for logout,
//! plasmashell restarted around its own file), shortcuts checks kglobalshortcutsrc, sync merges
//! machines through a shared folder or git. Rules: secrets are never read, machine files never
//! leave their machine, every write is preceded by a snapshot.

pub mod apply;
pub mod catalog;
pub mod kconfig;
pub mod plugins;
pub mod scan;
pub mod shortcuts;
pub mod store;
pub mod sync;

use catalog::Catalog;
use plugins::Packages;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Msg(String),
    #[error("{0}: {1}")]
    Io(String, std::io::Error),
    #[error("validation failed: {}", .0.join("; "))]
    Validation(Vec<String>),
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn err<T>(m: impl Into<String>) -> Result<T> {
    Err(Error::Msg(m.into()))
}
pub(crate) fn io<T>(r: std::io::Result<T>, p: &Path) -> Result<T> {
    r.map_err(|e| Error::Io(p.display().to_string(), e))
}

// ---- time and hashing
pub fn sha256(b: &[u8]) -> String {
    hex::encode(Sha256::digest(b))
}
/// (compact id like 20261123T101500, ISO 8601 UTC)
pub fn now_stamp() -> (String, String) {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let z = s.div_euclid(86400) + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let t = s.rem_euclid(86400);
    let (h, mi, se) = (t / 3600, t % 3600 / 60, t % 60);
    (format!("{y:04}{m:02}{d:02}T{h:02}{mi:02}{se:02}"), format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{se:02}Z"))
}

// ---- roots and store paths
/// Where everything lives. Tests point every field into a temp dir.
#[derive(Clone, Debug)]
pub struct Roots {
    pub config: PathBuf,
    pub data: PathBuf,
    pub state: PathBuf,
    pub home: PathBuf,
    pub sys: PathBuf, // prefix for /usr and /etc
    pub path_dirs: Vec<PathBuf>,
}

impl Roots {
    pub fn from_env() -> Roots {
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let home = env("HOME").unwrap_or_else(|| PathBuf::from("/"));
        Roots {
            config: env("XDG_CONFIG_HOME").unwrap_or(home.join(".config")),
            data: env("XDG_DATA_HOME").unwrap_or(home.join(".local/share")),
            state: env("XDG_STATE_HOME").unwrap_or(home.join(".local/state")),
            home,
            sys: PathBuf::from("/"),
            path_dirs: std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect(),
        }
    }
    pub fn under(base: &Path) -> Roots {
        let home = base.join("home");
        Roots {
            config: home.join(".config"),
            data: home.join(".local/share"),
            state: home.join(".local/state"),
            sys: base.join("sys"),
            path_dirs: vec![base.join("sys/usr/bin")],
            home,
        }
    }
    pub fn base(&self, kind: &str) -> Option<&Path> {
        Some(match kind {
            "config" => &self.config,
            "data" => &self.data,
            "state" => &self.state,
            "home" => &self.home,
            _ => return None,
        })
    }
    /// "config:foo/bar" -> absolute path (also accepts store paths "config/foo/bar").
    pub fn resolve(&self, spec: &str) -> PathBuf {
        let (k, rel) = spec.split_once(':').or_else(|| spec.split_once('/')).unwrap_or((spec, ""));
        self.base(k).unwrap_or(&self.home).join(rel.trim_end_matches('/'))
    }
    /// Store path ("config/foo/bar") -> absolute path; rejects anything that could escape.
    pub fn abs(&self, sp: &str) -> Result<PathBuf> {
        check_sp(sp)?;
        let (k, rel) = sp.split_once('/').ok_or_else(|| Error::Msg(format!("bad store path {sp}")))?;
        match self.base(k) {
            Some(b) => Ok(b.join(rel)),
            None => err(format!("bad store path root in {sp}")),
        }
    }
    pub fn to_store(&self, abs: &Path) -> Option<String> {
        for k in ["config", "data", "state", "home"] {
            if let Ok(r) = abs.strip_prefix(self.base(k)?) {
                return Some(format!("{k}/{}", r.to_string_lossy()));
            }
        }
        None
    }
    pub fn sys_path(&self, p: &str) -> PathBuf {
        self.sys.join(p.trim_start_matches('/'))
    }
    pub fn data_dirs(&self) -> Vec<PathBuf> {
        vec![self.data.clone(), self.sys_path("/usr/local/share"), self.sys_path("/usr/share")]
    }
    pub fn karry_state(&self) -> PathBuf {
        self.state.join("karry")
    }
}

/// Lexical check of a store-relative path: no "..", no absolute, no empty parts.
pub fn check_sp(rel: &str) -> Result<()> {
    if rel.is_empty() || rel.contains('\0') || rel.contains('\\') || rel.starts_with('/') {
        return err(format!("unsafe store path {rel:?}"));
    }
    if rel.split('/').any(|c| c.is_empty() || c == "." || c == "..") {
        return err(format!("unsafe store path {rel:?}"));
    }
    Ok(())
}

/// Join `rel` under `base`; fails on "..", absolute paths, or any symlink on the way.
pub fn safe_join(base: &Path, rel: &str) -> Result<PathBuf> {
    check_sp(rel)?;
    let mut p = base.to_path_buf();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(n) => p.push(n),
            _ => return err(format!("unsafe store path {rel:?}")),
        }
        if std::fs::symlink_metadata(&p).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
            return err(format!("symlink in store path {rel:?}"));
        }
    }
    Ok(p)
}

pub fn atomic_write(path: &Path, data: &[u8], mode: Option<u32>) -> Result<()> {
    use std::io::Write;
    let dir = path.parent().ok_or_else(|| Error::Msg("no parent".into()))?;
    io(std::fs::create_dir_all(dir), dir)?;
    let tmp = dir.join(format!(".{}.karry-tmp", path.file_name().unwrap_or_default().to_string_lossy()));
    let mut f = io(std::fs::File::create(&tmp), &tmp)?;
    io(f.write_all(data), &tmp)?;
    io(f.sync_all(), &tmp)?;
    drop(f);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let m = mode.or_else(|| std::fs::metadata(path).ok().map(|m| m.permissions().mode()));
        if let Some(m) = m {
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(m));
        }
    }
    io(std::fs::rename(&tmp, path), path)
}

pub fn file_mode(p: &Path) -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).ok().map(|m| m.permissions().mode())
    }
    #[cfg(not(unix))]
    {
        let _ = p;
        None
    }
}

// ---- running external commands
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cmd {
    pub program: String,
    pub args: Vec<String>,
}
impl Cmd {
    pub fn new(program: &str, args: &[&str]) -> Cmd {
        Cmd { program: program.into(), args: args.iter().map(|s| s.to_string()).collect() }
    }
    pub fn parse(line: &str) -> Cmd {
        let mut it = line.split_whitespace().map(String::from);
        Cmd { program: it.next().unwrap_or_default(), args: it.collect() }
    }
    pub fn line(&self) -> String {
        std::iter::once(self.program.as_str()).chain(self.args.iter().map(|s| s.as_str())).collect::<Vec<_>>().join(" ")
    }
}
#[derive(Clone, Debug, Default)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}
pub trait Runner: Send + Sync {
    fn run(&self, cmd: &Cmd) -> std::io::Result<Output>;
}
pub struct RealRunner;
impl Runner for RealRunner {
    fn run(&self, cmd: &Cmd) -> std::io::Result<Output> {
        let o = std::process::Command::new(&cmd.program).args(&cmd.args).output()?;
        Ok(Output {
            code: o.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&o.stdout).into(),
            stderr: String::from_utf8_lossy(&o.stderr).into(),
        })
    }
}
/// Records every call; answers with the first canned response whose prefix matches the command line.
#[derive(Default)]
pub struct RecordingRunner {
    calls: Mutex<Vec<Cmd>>,
    canned: Mutex<Vec<(String, Output)>>,
}
impl RecordingRunner {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn respond(&self, prefix: &str, code: i32, stdout: &str) {
        self.canned.lock().unwrap().push((prefix.into(), Output { code, stdout: stdout.into(), stderr: String::new() }));
    }
    pub fn calls(&self) -> Vec<Cmd> {
        self.calls.lock().unwrap().clone()
    }
}
impl Runner for RecordingRunner {
    fn run(&self, cmd: &Cmd) -> std::io::Result<Output> {
        self.calls.lock().unwrap().push(cmd.clone());
        let l = cmd.line();
        Ok(self.canned.lock().unwrap().iter().find(|(p, _)| l.starts_with(p)).map(|(_, o)| o.clone()).unwrap_or_default())
    }
}
impl<T: Runner + ?Sized> Runner for Arc<T> {
    fn run(&self, cmd: &Cmd) -> std::io::Result<Output> {
        (**self).run(cmd)
    }
}

/// Everything the engine needs; the UI builds one with `Ctx::new(Roots::from_env())`.
#[derive(Clone)]
pub struct Ctx {
    pub roots: Roots,
    pub catalog: Arc<Catalog>,
    pub packages: Arc<Packages>,
    pub runner: Arc<dyn Runner>,
    /// None = ask the system (`pgrep -x plasmashell`); Some(b) = fixed answer (tests).
    pub plasma: Option<bool>,
}
impl Ctx {
    pub fn new(roots: Roots) -> Result<Ctx> {
        let catalog = Catalog::load(&roots)?;
        Ok(Ctx { roots, catalog: Arc::new(catalog), packages: Arc::new(Packages::embedded()?), runner: Arc::new(RealRunner), plasma: None })
    }
    pub fn with_runner(mut self, r: Arc<dyn Runner>, plasma: Option<bool>) -> Ctx {
        self.runner = r;
        self.plasma = plasma;
        self
    }
    pub fn plasma_running(&self) -> bool {
        self.plasma.unwrap_or_else(|| self.runner.run(&Cmd::new("pgrep", &["-x", "plasmashell"])).map(|o| o.code == 0).unwrap_or(false))
    }
    pub fn run(&self, line: &str) -> Option<Output> {
        self.runner.run(&Cmd::parse(line)).ok().filter(|o| o.code == 0)
    }
}
