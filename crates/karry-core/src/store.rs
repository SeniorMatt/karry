//! Backup store on disk, backup writer, validation of untrusted stores, and snapshots/undo.
use crate::catalog::{Apply, Kind, Scope};
use crate::kconfig::KConfig;
use crate::plugins::{self, MachineInfo, PluginRecord};
use crate::scan::{self, Item, Skipped};
use crate::{atomic_write, check_sp, err, file_mode, io, now_stamp, safe_join, sha256, Ctx, Error, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub const FORMAT: u32 = 1;
const KEEP_SNAPSHOTS: usize = 20;

#[derive(Clone, Debug, PartialEq)]
pub enum Part {
    Shared,
    Machine(String),
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    pub format: u32,
    pub created: String,
    pub machine: MachineInfo,
    pub items: Vec<Item>,
    #[serde(default)]
    pub plugins: Vec<PluginRecord>,
    #[serde(default)]
    pub last_sync: Option<String>,
}

pub struct Store {
    pub root: PathBuf,
}
impl Store {
    pub fn create(root: &Path) -> Result<Store> {
        io(std::fs::create_dir_all(root), root)?;
        let f = root.join("karry.json");
        if !f.exists() {
            atomic_write(&f, format!("{{\"format\":{FORMAT}}}\n").as_bytes(), None)?;
        }
        Store::open(root)
    }
    pub fn open(root: &Path) -> Result<Store> {
        let f = root.join("karry.json");
        let v: serde_json::Value = serde_json::from_str(&io(std::fs::read_to_string(&f), &f)?).map_err(|e| Error::Msg(format!("{}: {e}", f.display())))?;
        if v["format"].as_u64().map_or(true, |n| n > FORMAT as u64) {
            return err("unsupported backup format");
        }
        Ok(Store { root: root.into() })
    }
    fn dir(&self, p: &Part) -> Result<PathBuf> {
        Ok(match p {
            Part::Shared => self.root.join("shared"),
            Part::Machine(id) => {
                if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
                    return err(format!("bad machine id {id:?}"));
                }
                self.root.join("machines").join(id)
            }
        })
    }
    pub fn file(&self, p: &Part, sp: &str) -> Result<PathBuf> {
        safe_join(&self.dir(p)?.join("files"), sp)
    }
    pub fn write(&self, p: &Part, sp: &str, data: &[u8]) -> Result<()> {
        atomic_write(&self.file(p, sp)?, data, None)
    }
    pub fn read(&self, p: &Part, sp: &str) -> Result<Vec<u8>> {
        let f = self.file(p, sp)?;
        io(std::fs::read(&f), &f)
    }
    pub fn manifest(&self, p: &Part) -> Result<Option<Manifest>> {
        let f = self.dir(p)?.join("manifest.json");
        if !f.exists() {
            return Ok(None);
        }
        serde_json::from_str(&io(std::fs::read_to_string(&f), &f)?).map(Some).map_err(|e| Error::Msg(format!("{}: {e}", f.display())))
    }
    pub fn write_manifest(&self, p: &Part, m: &Manifest) -> Result<()> {
        atomic_write(&self.dir(p)?.join("manifest.json"), serde_json::to_string_pretty(m).unwrap().as_bytes(), None)
    }
}

#[derive(Serialize, Debug, Default)]
pub struct BackupReport {
    pub shared: usize,
    pub machine: usize,
    pub plugins: usize,
    pub skipped: Vec<Skipped>,
}

/// Replace this machine's backup in `store` (shared/ and machines/<id>/) with the current state.
/// Refuses to replace another machine's shared data (use sync instead) unless `force`.
pub fn backup(ctx: &Ctx, store: &Store, only: Option<&[String]>, force: bool) -> Result<BackupReport> {
    let inv = scan::scan(ctx)?;
    let me = MachineInfo::detect(ctx);
    if let Some(m) = store.manifest(&Part::Shared)?.filter(|m| m.machine.id != me.id && !force) {
        return err(format!("this setup belongs to {}; back up would overwrite it. Use sync to join it", m.machine.hostname));
    }
    let mid = Part::Machine(me.id.clone());
    for p in [&Part::Shared, &mid] {
        let d = store.dir(p)?.join("files");
        if d.exists() {
            io(std::fs::remove_dir_all(&d), &d)?;
        }
    }
    let (mut sh, mut ma) = (vec![], vec![]);
    for mut it in inv.items.into_iter().filter(|i| only.map_or(true, |o| o.contains(&i.category))) {
        let part = if it.scope == Scope::Shared { &Part::Shared } else { &mid };
        store.write(part, &it.store_path, &it.bytes()?)?;
        it.by = Some(me.hostname.clone());
        if it.scope == Scope::Shared { sh.push(it) } else { ma.push(it) }
    }
    let plugins = plugins::records(ctx);
    let (_, now) = now_stamp();
    let mk = |items, plugins| Manifest { format: FORMAT, created: now.clone(), machine: me.clone(), items, plugins, last_sync: None };
    let rep = BackupReport { shared: sh.len(), machine: ma.len(), plugins: plugins.len(), skipped: inv.skipped };
    store.write_manifest(&Part::Shared, &mk(sh, plugins))?;
    store.write_manifest(&mid, &mk(ma, vec![]))?;
    Ok(rep)
}

// ---- validation and loading
#[derive(Serialize, Debug, Default)]
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub enum Content {
    Bytes(Vec<u8>),
    File(PathBuf),
}
impl Content {
    pub fn read(&self) -> Result<Vec<u8>> {
        match self {
            Content::Bytes(b) => Ok(b.clone()),
            Content::File(p) => io(std::fs::read(p), p),
        }
    }
}
/// A file to put on this machine. kind/apply/reload come from the local catalog, never from the backup.
pub struct Desired {
    pub store_path: String,
    pub category: String,
    pub scope: Scope,
    pub kind: Kind,
    pub apply: Apply,
    pub reload: Vec<String>,
    pub content: Content,
}
#[derive(Default)]
pub struct Incoming {
    pub files: Vec<Desired>,
    pub plugins: Vec<PluginRecord>,
    pub source: Option<MachineInfo>,
    pub warnings: Vec<String>,
}

/// Checks everything before anything is written: paths, symlinks, hashes, catalog coverage, secrets, syntax.
pub fn validate(ctx: &Ctx, store: &Store) -> Report {
    let mut r = Report::default();
    let me = MachineInfo::detect(ctx);
    for d in ["shared", "machines"] {
        for e in WalkDir::new(store.root.join(d)).follow_links(false).into_iter().flatten() {
            if e.file_type().is_symlink() {
                r.errors.push(format!("symlink in backup: {}", e.path().display()));
            }
        }
    }
    let mut parts = vec![Part::Shared];
    if let Ok(rd) = std::fs::read_dir(store.root.join("machines")) {
        for m in rd.flatten() {
            let id = m.file_name().to_string_lossy().to_string();
            if id == me.id {
                parts.push(Part::Machine(id));
            } else {
                r.warnings.push(format!("machine files of {id} ignored (different machine)"));
            }
        }
    }
    for part in parts {
        let m = match store.manifest(&part) {
            Ok(Some(m)) => m,
            Ok(None) => continue,
            Err(e) => {
                r.errors.push(e.to_string());
                continue;
            }
        };
        if part == Part::Shared {
            if let (Some(a), Some(b)) = (m.machine.plasma.as_deref().and_then(plugins::major), me.plasma.as_deref().and_then(plugins::major)) {
                if a != b {
                    r.warnings.push(format!("backup is from Plasma {a}, this machine runs Plasma {b}"));
                }
            }
        }
        for it in &m.items {
            let sp = &it.store_path;
            let mut bad = |msg: &str| r.errors.push(format!("{sp}: {msg}"));
            if let Err(e) = check_sp(sp) {
                bad(&e.to_string());
                continue;
            }
            let want_scope = if part == Part::Shared { Scope::Shared } else { Scope::Machine };
            if it.scope != want_scope {
                bad("scope does not match its place in the backup");
                continue;
            }
            let entry = match ctx.catalog.find(sp) {
                Some(e) if e.scope != Scope::Secret => e,
                Some(_) => {
                    bad("secret path");
                    continue;
                }
                None => {
                    bad("not covered by the catalog");
                    continue;
                }
            };
            if sp.split('/').skip(1).any(|c| ctx.catalog.is_secret_name(c)) {
                bad("secret file name");
                continue;
            }
            if ctx.roots.abs(sp).is_err() {
                bad("bad target");
                continue;
            }
            let data = match store.read(&part, sp) {
                Ok(d) => d,
                Err(e) => {
                    bad(&e.to_string());
                    continue;
                }
            };
            if sha256(&data) != it.sha256 {
                bad("checksum mismatch");
            }
            if data.len() as u64 > 256 << 20 || (entry.kind == Kind::Kconfig && data.len() > 8 << 20) {
                bad("file too large");
            }
            match entry.kind {
                Kind::Kconfig if std::str::from_utf8(&data).ok().and_then(|t| crate::kconfig::KConfig::parse(t).ok()).is_none() => bad("not a valid KConfig file"),
                Kind::Json if serde_json::from_slice::<serde_json::Value>(&data).is_err() => bad("not valid JSON"),
                _ => {}
            }
            let target = ctx.roots.abs(sp).unwrap();
            let mut anc = target.parent();
            while let Some(a) = anc.filter(|a| !a.exists()) {
                anc = a.parent();
            }
            if anc.map_or(true, |a| !a.is_dir() || std::fs::metadata(a).map_or(true, |m| m.permissions().readonly())) {
                bad("target folder is not writable");
            }
        }
    }
    r
}

pub fn load(ctx: &Ctx, store: &Store, only: Option<&[String]>) -> Result<Incoming> {
    let rep = validate(ctx, store);
    if !rep.errors.is_empty() {
        return Err(Error::Validation(rep.errors));
    }
    let me = MachineInfo::detect(ctx);
    let mut inc = Incoming { warnings: rep.warnings, ..Default::default() };
    for part in [Part::Shared, Part::Machine(me.id.clone())] {
        let Some(m) = store.manifest(&part)? else { continue };
        if part == Part::Shared {
            inc.plugins = m.plugins.clone();
            inc.source = Some(m.machine.clone());
        }
        for it in m.items.iter().filter(|i| only.map_or(true, |o| o.contains(&ctx.catalog.find(&i.store_path).map(|e| e.category.clone()).unwrap_or_default()))) {
            let e = ctx.catalog.find(&it.store_path).unwrap();
            inc.files.push(Desired {
                store_path: it.store_path.clone(),
                category: e.category.clone(),
                scope: it.scope,
                kind: if e.kind == Kind::Dir { Kind::File } else { e.kind },
                apply: e.apply,
                reload: e.reload.clone(),
                content: Content::File(store.file(&part, &it.store_path)?),
            });
        }
    }
    Ok(inc)
}

// ---- snapshots
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SnapFile {
    pub store_path: String,
    pub existed: bool,
    pub sha256: String,
    pub mode: Option<u32>,
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Snapshot {
    pub id: String,
    pub created: String,
    pub label: String,
    pub files: Vec<SnapFile>,
}

fn snap_dir(ctx: &Ctx) -> PathBuf {
    ctx.roots.karry_state().join("snapshots")
}
pub fn pending_dir(ctx: &Ctx) -> PathBuf {
    ctx.roots.karry_state().join("pending")
}

/// Copy every listed file (as it is now) so `undo` can put it back exactly.
pub fn snapshot_create(ctx: &Ctx, label: &str, paths: &[PathBuf]) -> Result<Snapshot> {
    let (stamp, created) = now_stamp();
    let mut id = stamp.clone();
    let mut n = 1;
    while snap_dir(ctx).join(&id).exists() {
        n += 1;
        id = format!("{stamp}-{n}");
    }
    let dir = snap_dir(ctx).join(&id);
    let mut files = vec![];
    for p in paths {
        let Some(sp) = ctx.roots.to_store(p) else { return err(format!("{} is outside the known roots", p.display())) };
        let f = match std::fs::read(p) {
            Ok(data) => {
                atomic_write(&safe_join(&dir.join("files"), &sp)?, &data, None)?;
                SnapFile { store_path: sp, existed: true, sha256: sha256(&data), mode: file_mode(p) }
            }
            Err(_) => SnapFile { store_path: sp, existed: false, sha256: String::new(), mode: None },
        };
        files.push(f);
    }
    let s = Snapshot { id, created, label: label.into(), files };
    atomic_write(&dir.join("manifest.json"), serde_json::to_string_pretty(&s).unwrap().as_bytes(), None)?;
    let all = snapshots(ctx);
    for old in all.iter().skip(KEEP_SNAPSHOTS) {
        let _ = std::fs::remove_dir_all(snap_dir(ctx).join(&old.id));
    }
    Ok(s)
}

/// Newest first.
pub fn snapshots(ctx: &Ctx) -> Vec<Snapshot> {
    let mut v: Vec<Snapshot> = std::fs::read_dir(snap_dir(ctx)).into_iter().flatten().flatten().filter_map(|d| serde_json::from_str(&std::fs::read_to_string(d.path().join("manifest.json")).ok()?).ok()).collect();
    v.sort_by(|a, b| b.id.cmp(&a.id));
    v
}

pub fn undo(ctx: &Ctx, id: Option<&str>) -> Result<Snapshot> {
    let all = snapshots(ctx);
    let s = match id {
        Some(i) => all.into_iter().find(|s| s.id == i),
        None => all.into_iter().next(),
    }
    .ok_or_else(|| Error::Msg("no such snapshot".into()))?;
    if s.id.contains('/') || s.id.contains("..") {
        return err("bad snapshot id");
    }
    let dir = snap_dir(ctx).join(&s.id);
    // a staged logout restore must not run after an undo
    let _ = std::fs::remove_dir_all(pending_dir(ctx));
    let _ = std::fs::remove_file(ctx.roots.config.join(scan::PENDING_SCRIPT));
    let mut inc = Incoming::default();
    for f in &s.files {
        let target = ctx.roots.abs(&f.store_path)?;
        if !f.existed {
            if target.exists() {
                io(std::fs::remove_file(&target), &target)?;
            }
            continue;
        }
        let data = io(std::fs::read(safe_join(&dir.join("files"), &f.store_path)?), &dir)?;
        if sha256(&data) != f.sha256 {
            return err(format!("snapshot file {} is damaged", f.store_path));
        }
        let entry = ctx.catalog.find(&f.store_path);
        match entry.filter(|_| ctx.plasma_running()) {
            // Plasma is running: go through the same safe strategies as a restore
            Some(e) => {
                let mk = |scope, content| Desired { store_path: f.store_path.clone(), category: e.category.clone(), scope, kind: if e.kind == Kind::Dir { Kind::File } else { e.kind }, apply: e.apply, reload: e.reload.clone(), content: Content::Bytes(content) };
                match (e.kind == Kind::Kconfig).then(|| std::str::from_utf8(&data).ok().and_then(|t| KConfig::parse(t).ok())).flatten() {
                    Some(kc) => {
                        let (shared, machine) = scan::kconfig_parts(e, kc);
                        inc.files.push(mk(Scope::Shared, shared.to_text().into_bytes()));
                        inc.files.push(mk(Scope::Machine, machine.to_text().into_bytes()));
                    }
                    None => inc.files.push(mk(e.scope, data)),
                }
            }
            None => atomic_write(&target, &data, f.mode)?,
        }
    }
    if !inc.files.is_empty() {
        crate::apply::apply(ctx, &inc, &format!("before undo of {}", s.label), true, false)?;
    }
    Ok(s)
}
