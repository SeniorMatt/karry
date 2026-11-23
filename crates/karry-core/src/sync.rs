//! Sync through a folder or a git repo: key-level 3-way merge against the last synced copy.
use crate::apply::{self, Plan};
use crate::catalog::{Kind, Scope};
use crate::kconfig::{diff, merge3, Conflict, KConfig};
use crate::plugins::{self, MachineInfo};
use crate::scan::{self, Item};
use crate::store::{self, Content, Desired, Incoming, Manifest, Part, Snapshot, Store};
use crate::{atomic_write, err, io, now_stamp, sha256, Cmd, Ctx, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub trait Backend {
    fn root(&self) -> &Path;
    /// Fetch remote changes. Returns conflicts the backend already merged itself (git).
    fn pull(&self, ctx: &Ctx) -> Result<Vec<Conflict>>;
    /// Publish local changes; false = nothing to push.
    fn push(&self, ctx: &Ctx, msg: &str) -> Result<bool>;
    /// Conflict copies made by file-sync tools (Syncthing, Nextcloud, Dropbox).
    fn conflict_files(&self) -> Vec<String> {
        walkdir::WalkDir::new(self.root()).into_iter().flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.contains(".sync-conflict-") || n.contains("conflicted copy")).collect()
    }
}

pub struct FolderBackend {
    pub root: PathBuf,
}
impl Backend for FolderBackend {
    fn root(&self) -> &Path {
        &self.root
    }
    fn pull(&self, _: &Ctx) -> Result<Vec<Conflict>> {
        Ok(vec![])
    }
    fn push(&self, _: &Ctx, _: &str) -> Result<bool> {
        Ok(false)
    }
}

pub struct GitBackend {
    pub root: PathBuf,
}
impl GitBackend {
    fn git(&self, ctx: &Ctx, args: &[&str]) -> Result<crate::Output> {
        let mut a = vec!["-C", self.root.to_str().unwrap_or(".")];
        a.extend(args);
        ctx.runner.run(&Cmd::new("git", &a)).map_err(|e| Error::Msg(format!("git: {e}")))
    }
    fn has_remote(&self, ctx: &Ctx) -> bool {
        self.git(ctx, &["remote"]).map_or(false, |o| o.code == 0 && !o.stdout.trim().is_empty())
    }
}
impl Backend for GitBackend {
    fn root(&self) -> &Path {
        &self.root
    }
    fn pull(&self, ctx: &Ctx) -> Result<Vec<Conflict>> {
        if !self.has_remote(ctx) {
            return Ok(vec![]);
        }
        let _ = self.git(ctx, &["fetch", "origin"]);
        if self.git(ctx, &["rev-parse", "--verify", "-q", "@{u}"])?.code != 0 {
            return Ok(vec![]); // nothing published yet
        }
        if self.git(ctx, &["pull", "--rebase", "--autostash"])?.code == 0 {
            return Ok(vec![]);
        }
        // Rebase stopped on conflicting files: merge kconfig files key by key, keep our copy of the rest.
        let mut conflicts = vec![];
        let files = self.git(ctx, &["diff", "--name-only", "--diff-filter=U"])?.stdout;
        for f in files.lines() {
            let show = |n: u8| self.git(ctx, &["show", &format!(":{n}:{f}")]).ok().filter(|o| o.code == 0).map(|o| o.stdout);
            let (base, remote, local) = (show(1), show(2), show(3)); // while rebasing, :2 is upstream, :3 is our commit
            let parse = |t: &Option<String>| t.as_deref().map(KConfig::parse).transpose().ok();
            let merged = match (parse(&base), parse(&remote), parse(&local)) {
                (Some(b), Some(Some(r)), Some(Some(l))) => {
                    let (m, mut c) = merge3(&b.unwrap_or_default(), &l, &r, &|_, _| false);
                    for x in &mut c {
                        x.store_path = f.to_string();
                    }
                    conflicts.extend(c);
                    m.to_text()
                }
                _ => {
                    conflicts.push(Conflict { store_path: f.into(), group: String::new(), key: String::new(), base: None, local: None, remote: None });
                    local.unwrap_or_default()
                }
            };
            let p = crate::safe_join(&self.root, f)?;
            atomic_write(&p, merged.as_bytes(), None)?;
            self.git(ctx, &["add", f])?;
        }
        if self.git(ctx, &["-c", "core.editor=true", "rebase", "--continue"])?.code != 0 {
            let _ = self.git(ctx, &["rebase", "--abort"]);
            return err("git could not merge the remote changes");
        }
        Ok(conflicts)
    }
    fn push(&self, ctx: &Ctx, msg: &str) -> Result<bool> {
        self.git(ctx, &["add", "-A"])?;
        let _ = self.git(ctx, &["commit", "-q", "-m", msg]);
        if !self.has_remote(ctx) {
            return Ok(false);
        }
        let o = self.git(ctx, &["push", "-u", "origin", "HEAD"])?;
        if o.code != 0 {
            return err(format!("git push failed: {}", o.stderr.trim()));
        }
        Ok(true)
    }
}

// ---- config (karry.toml)
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Config {
    pub kind: String, // "folder" | "git"
    pub path: PathBuf,
    pub url: Option<String>,
}
pub fn config_path(ctx: &Ctx) -> PathBuf {
    ctx.roots.config.join("karry/karry.toml")
}
pub fn load_config(ctx: &Ctx) -> Result<Option<Config>> {
    let p = config_path(ctx);
    match std::fs::read_to_string(&p) {
        Ok(t) => toml::from_str(&t).map(Some).map_err(|e| Error::Msg(format!("{}: {e}", p.display()))),
        Err(_) => Ok(None),
    }
}
pub fn init_folder(ctx: &Ctx, dir: &Path) -> Result<Config> {
    Store::create(dir)?;
    save(ctx, Config { kind: "folder".into(), path: dir.into(), url: None })
}
pub fn init_git(ctx: &Ctx, url: &str) -> Result<Config> {
    let dir = ctx.roots.karry_state().join("repo");
    if !dir.exists() {
        io(std::fs::create_dir_all(dir.parent().unwrap()), &dir)?;
        let d = dir.to_str().unwrap();
        let ok = ctx.runner.run(&Cmd::new("git", &["clone", "-q", url, d])).map_or(false, |o| o.code == 0);
        if !ok && !dir.exists() {
            io(std::fs::create_dir_all(&dir), &dir)?;
            for a in [&["init", "-q"][..], &["remote", "add", "origin", url]] {
                let mut v = vec!["-C", d];
                v.extend(a);
                ctx.runner.run(&Cmd::new("git", &v)).map_err(|e| Error::Msg(e.to_string()))?;
            }
        }
    }
    Store::create(&dir)?;
    save(ctx, Config { kind: "git".into(), path: dir, url: Some(url.into()) })
}
fn save(ctx: &Ctx, c: Config) -> Result<Config> {
    atomic_write(&config_path(ctx), toml::to_string(&c).unwrap().as_bytes(), None)?;
    Ok(c)
}
pub fn backend(c: &Config) -> Box<dyn Backend> {
    if c.kind == "git" { Box::new(GitBackend { root: c.path.clone() }) } else { Box::new(FolderBackend { root: c.path.clone() }) }
}

// ---- sync
#[derive(Default, Clone)]
pub struct SyncOptions {
    pub dry_run: bool,
    /// "store_path|group|key" entries where the other machine's value wins (whole file: "store_path||").
    pub remote_wins: Vec<String>,
    pub autostart_opt_in: bool,
}
#[derive(Serialize, Clone, Debug)]
pub struct IncomingChange {
    pub machine: String,
    pub store_path: String,
    pub keys: usize,
    pub summary: String,
}
#[derive(Serialize, Debug, Default)]
pub struct SyncReport {
    pub dry_run: bool,
    pub published: Vec<String>,
    pub incoming: Vec<IncomingChange>,
    pub conflicts: Vec<Conflict>,
    pub plan: Plan,
    pub snapshot: Option<Snapshot>,
    pub folder_conflicts: Vec<String>,
    pub pushed: bool,
    pub warnings: Vec<String>,
}

fn base_dir(ctx: &Ctx) -> PathBuf {
    ctx.roots.karry_state().join("base")
}

pub fn sync(ctx: &Ctx, store: &Store, backend: &dyn Backend, opts: &SyncOptions) -> Result<SyncReport> {
    let mut rep = SyncReport { dry_run: opts.dry_run, ..Default::default() };
    rep.conflicts = backend.pull(ctx)?;
    rep.folder_conflicts = backend.conflict_files();
    let v = store::validate(ctx, store);
    if !v.errors.is_empty() {
        return Err(Error::Validation(v.errors));
    }
    rep.warnings = v.warnings;
    let me = MachineInfo::detect(ctx);
    let inv = scan::scan(ctx)?;
    let remote_m = store.manifest(&Part::Shared)?;
    let remote_items: BTreeMap<String, Item> = remote_m.iter().flat_map(|m| m.items.clone()).map(|i| (i.store_path.clone(), i)).collect();
    let bdir = base_dir(ctx);
    let index_p = bdir.join("index.json");
    let mut index: BTreeMap<String, String> = std::fs::read_to_string(&index_p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let local: BTreeMap<String, &Item> = inv.items.iter().filter(|i| i.scope == Scope::Shared).map(|i| (i.store_path.clone(), i)).collect();
    let wins = |sp: &str, g: &str, k: &str| opts.remote_wins.iter().any(|w| *w == format!("{sp}|{g}|{k}"));

    let mut incoming = Incoming { source: remote_m.as_ref().map(|m| m.machine.clone()), plugins: remote_m.as_ref().map(|m| m.plugins.clone()).unwrap_or_default(), warnings: vec![], files: vec![] };
    let mut publish: Vec<(Item, Vec<u8>)> = vec![];
    let mut new_base: Vec<(String, String, Option<String>)> = vec![]; // path, sha, kconfig text
    let paths: BTreeSet<&String> = local.keys().chain(remote_items.keys()).collect();
    for sp in paths {
        let entry = ctx.catalog.find(sp);
        let (Some(entry), l, r) = (entry, local.get(sp), remote_items.get(sp)) else { continue };
        let lb = l.map(|i| i.bytes()).transpose()?;
        let rb = r.map(|_| store.read(&Part::Shared, sp)).transpose()?;
        let mk_desired = |data: Vec<u8>| Desired { store_path: sp.clone(), category: entry.category.clone(), scope: Scope::Shared, kind: if entry.kind == Kind::Dir { Kind::File } else { entry.kind }, apply: entry.apply, reload: entry.reload.clone(), content: Content::Bytes(data) };
        let (final_bytes, take_local, take_remote): (Vec<u8>, bool, bool);
        match (&lb, &rb) {
            (Some(lb), None) => (final_bytes, take_local, take_remote) = (lb.clone(), true, false),
            (None, Some(rb)) => (final_bytes, take_local, take_remote) = (rb.clone(), false, true),
            (None, None) => continue,
            (Some(lb), Some(rb)) if lb == rb => (final_bytes, take_local, take_remote) = (lb.clone(), false, false),
            (Some(lb), Some(rb)) if entry.kind == Kind::Kconfig => {
                let p = |b: &[u8]| KConfig::parse(&String::from_utf8_lossy(b));
                let base = std::fs::read_to_string(bdir.join("files").join(sp)).ok().and_then(|t| KConfig::parse(&t).ok()).unwrap_or_default();
                let (m, mut c) = merge3(&base, &p(lb)?, &p(rb)?, &|g, k| wins(sp, g, k));
                for x in &mut c {
                    x.store_path = sp.clone();
                }
                rep.conflicts.extend(c);
                let mb = m.to_text().into_bytes();
                (take_local, take_remote) = (mb != *rb, mb != *lb);
                final_bytes = mb;
            }
            (Some(lb), Some(rb)) => {
                let (lh, rh) = (sha256(lb), sha256(rb));
                match index.get(sp) {
                    Some(b) if *b == lh => (final_bytes, take_local, take_remote) = (rb.clone(), false, true),
                    Some(b) if *b == rh => (final_bytes, take_local, take_remote) = (lb.clone(), true, false),
                    _ if wins(sp, "", "") => (final_bytes, take_local, take_remote) = (rb.clone(), false, true),
                    _ => {
                        rep.conflicts.push(Conflict { store_path: sp.clone(), group: String::new(), key: String::new(), base: None, local: Some(lh), remote: Some(rh) });
                        (final_bytes, take_local, take_remote) = (lb.clone(), true, false);
                    }
                }
            }
        }
        if take_remote {
            let machine = r.and_then(|i| i.by.clone()).unwrap_or_else(|| "another machine".into());
            let keys = match (&lb, entry.kind == Kind::Kconfig) {
                (Some(lb), true) => diff(&KConfig::parse(&String::from_utf8_lossy(lb))?, &KConfig::parse(&String::from_utf8_lossy(&final_bytes))?).len(),
                _ => 1,
            };
            let name = sp.rsplit('/').next().unwrap_or(sp);
            let summary = if entry.kind == Kind::Kconfig { format!("{machine} changed {keys} key{} in {name}", if keys == 1 { "" } else { "s" }) } else { format!("{machine} changed {name}") };
            rep.incoming.push(IncomingChange { machine, store_path: sp.clone(), keys, summary });
            incoming.files.push(mk_desired(final_bytes.clone()));
        }
        if take_local {
            let mut it = (*l.unwrap()).clone();
            it.sha256 = sha256(&final_bytes);
            it.size = final_bytes.len() as u64;
            it.by = Some(me.hostname.clone());
            rep.published.push(sp.clone());
            publish.push((it, final_bytes.clone()));
        }
        let text = (entry.kind == Kind::Kconfig).then(|| String::from_utf8_lossy(&final_bytes).to_string());
        new_base.push((sp.clone(), sha256(&final_bytes), text));
    }
    // this machine's own files (never leave machines/<id>/)
    let own: Vec<&Item> = inv.items.iter().filter(|i| i.scope == Scope::Machine).collect();

    let ap = apply::apply(ctx, &incoming, "sync", opts.autostart_opt_in, opts.dry_run)?;
    rep.plan = ap.plan;
    rep.snapshot = ap.snapshot;
    rep.warnings.extend(ap.warnings);
    if opts.dry_run {
        return Ok(rep);
    }
    let (_, now) = now_stamp();
    for (it, data) in &publish {
        store.write(&Part::Shared, &it.store_path, data)?;
    }
    let mut items = remote_items;
    for (it, _) in publish {
        items.insert(it.store_path.clone(), it);
    }
    store.write_manifest(&Part::Shared, &Manifest { format: store::FORMAT, created: now.clone(), machine: me.clone(), items: items.into_values().collect(), plugins: plugins::records(ctx), last_sync: Some(now.clone()) })?;
    let mid = Part::Machine(me.id.clone());
    let mut mitems = vec![];
    for it in own {
        store.write(&mid, &it.store_path, &it.bytes()?)?;
        mitems.push(it.clone());
    }
    store.write_manifest(&mid, &Manifest { format: store::FORMAT, created: now.clone(), machine: me.clone(), items: mitems, plugins: vec![], last_sync: Some(now.clone()) })?;
    for (sp, h, text) in new_base {
        index.insert(sp.clone(), h);
        if let Some(t) = text {
            atomic_write(&bdir.join("files").join(&sp), t.as_bytes(), None)?;
        }
    }
    atomic_write(&index_p, serde_json::to_string(&index).unwrap().as_bytes(), None)?;
    rep.pushed = backend.push(ctx, &format!("karry: {} {now}", me.hostname))?;
    Ok(rep)
}
