//! Walk the catalog into an inventory; also the per-category status overview and the autostart check.
use crate::catalog::{glob_re, Apply, Entry, Kind, Scope};
use crate::kconfig::KConfig;
use crate::plugins::MachineInfo;
use crate::store::{Part, Store};
use crate::{io, sha256, Ctx, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

const MAX_FILE: u64 = 256 << 20;
pub const PENDING_SCRIPT: &str = "plasma-workspace/env/karry-pending.sh";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Item {
    pub store_path: String,
    #[serde(skip)]
    pub abs: PathBuf,
    pub category: String,
    pub entry_id: String,
    pub scope: Scope,
    pub kind: Kind,
    pub apply: Apply,
    pub sha256: String,
    pub size: u64,
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub by: Option<String>, // hostname that last published it (sync)
    #[serde(skip)]
    pub inline: Option<Vec<u8>>, // kconfig parts are built in memory; other files are read from `abs`
}
impl Item {
    pub fn bytes(&self) -> Result<Vec<u8>> {
        match &self.inline {
            Some(b) => Ok(b.clone()),
            None => io(std::fs::read(&self.abs), &self.abs),
        }
    }
}
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Skipped {
    pub path: String,
    pub reason: String,
}
#[derive(Serialize, Clone, Debug, Default)]
pub struct Inventory {
    pub items: Vec<Item>,
    pub skipped: Vec<Skipped>,
}

/// (shared part, machine part) of a kconfig file: secret keys dropped, machine groups split off.
pub fn kconfig_parts(e: &Entry, mut kc: KConfig) -> (KConfig, KConfig) {
    for r in &e.secret_keys {
        kc.strip_keys(r);
    }
    if e.scope == Scope::Machine {
        return (KConfig::default(), kc);
    }
    kc.split(&|g| e.machine_groups.iter().any(|r| r.is_match(g)))
}

fn expand(ctx: &Ctx, spec: &str) -> Vec<PathBuf> {
    let p = ctx.roots.resolve(spec);
    let name = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    if !name.contains('*') {
        return vec![p];
    }
    let re = glob_re(&name);
    let mut v: Vec<PathBuf> = std::fs::read_dir(p.parent().unwrap_or(Path::new("/"))).into_iter().flatten().flatten().filter(|f| re.is_match(&f.file_name().to_string_lossy())).map(|f| f.path()).collect();
    v.sort();
    v
}

fn is_desktop_kde_app(ctx: &Ctx, stem: &str) -> bool {
    ctx.roots.data_dirs().iter().any(|d| {
        [format!("{stem}.desktop"), format!("org.kde.{stem}.desktop")].iter().any(|n| {
            std::fs::read_to_string(d.join("applications").join(n)).is_ok_and(|t| t.contains("X-KDE-") || t.contains("KDE;"))
        })
    })
}

pub fn scan(ctx: &Ctx) -> Result<Inventory> {
    let mut inv = Inventory::default();
    let mut listed: BTreeSet<PathBuf> = BTreeSet::new();
    for e in &ctx.catalog.entries {
        for spec in &e.paths {
            if e.scope == Scope::Secret {
                inv.skipped.push(Skipped { path: spec.clone(), reason: "secret: never read".into() });
                listed.extend(expand(ctx, spec));
                continue;
            }
            for abs in expand(ctx, spec) {
                listed.insert(abs.clone());
                scan_path(ctx, e, &abs, &mut inv);
            }
        }
    }
    // auto rule: other KDE app rc files in ~/.config
    if let Some(tpl) = ctx.catalog.find("config/x-auto-probe-rc") {
        let mut names: Vec<_> = std::fs::read_dir(&ctx.roots.config).into_iter().flatten().flatten().map(|f| f.path()).collect();
        names.sort();
        for p in names {
            let n = p.file_name().unwrap().to_string_lossy().to_string();
            let Some(stem) = n.strip_suffix("rc") else { continue };
            if p.is_file() && !listed.contains(&p) && ctx.catalog.find(&format!("config/{n}")).is_some() && is_desktop_kde_app(ctx, stem) {
                scan_path(ctx, tpl, &p, &mut inv);
            }
        }
    }
    inv.items.sort_by(|a, b| (a.store_path.as_str(), a.scope as u8).cmp(&(b.store_path.as_str(), b.scope as u8)));
    Ok(inv)
}

fn scan_path(ctx: &Ctx, e: &Entry, abs: &Path, inv: &mut Inventory) {
    let Ok(meta) = std::fs::symlink_metadata(abs) else { return };
    let skip = |inv: &mut Inventory, p: &Path, r: &str| inv.skipped.push(Skipped { path: p.display().to_string(), reason: r.into() });
    if meta.file_type().is_symlink() {
        return skip(inv, abs, "symlink");
    }
    let files: Vec<PathBuf> = if meta.is_dir() {
        WalkDir::new(abs).follow_links(false).sort_by_file_name().into_iter().flatten().filter(|d| d.file_type().is_file() || d.file_type().is_symlink()).map(|d| d.into_path()).collect()
    } else {
        vec![abs.to_path_buf()]
    };
    for f in files {
        let Some(sp) = ctx.roots.to_store(&f) else { continue };
        let rel = f.strip_prefix(ctx.roots.base(sp.split('/').next().unwrap()).unwrap()).unwrap_or(&f);
        if rel == Path::new(PENDING_SCRIPT) {
            continue;
        }
        if rel.components().any(|c| ctx.catalog.is_secret_name(&c.as_os_str().to_string_lossy())) {
            skip(inv, &f, "secret file name");
            continue;
        }
        let m = std::fs::symlink_metadata(&f);
        if m.as_ref().map_or(true, |m| m.file_type().is_symlink()) {
            skip(inv, &f, "symlink");
            continue;
        }
        let size = m.unwrap().len();
        if size > MAX_FILE {
            skip(inv, &f, "too large");
            continue;
        }
        let Ok(bytes) = std::fs::read(&f) else {
            skip(inv, &f, "unreadable");
            continue;
        };
        let plugin_id = (e.apply == Apply::Package).then(|| {
            let base = e.paths.iter().map(|p| ctx.roots.resolve(p)).find(|b| f.starts_with(b));
            base.and_then(|b| f.strip_prefix(b).ok().and_then(|r| r.components().next()).map(|c| c.as_os_str().to_string_lossy().to_string()))
        }).flatten();
        let mk = |scope: Scope, kind: Kind, data: Vec<u8>, inline: bool| Item {
            store_path: sp.clone(),
            abs: f.clone(),
            category: e.category.clone(),
            entry_id: e.id.clone(),
            scope,
            kind,
            apply: e.apply,
            sha256: sha256(&data),
            size: data.len() as u64,
            plugin_id: plugin_id.clone(),
            by: None,
            inline: inline.then_some(data),
        };
        if e.kind == Kind::Kconfig {
            match std::str::from_utf8(&bytes).ok().and_then(|t| KConfig::parse(t).ok()) {
                None => skip(inv, &f, "not a valid KConfig file"),
                Some(kc) => {
                    let (shared, machine) = kconfig_parts(e, kc);
                    if e.scope != Scope::Machine {
                        inv.items.push(mk(Scope::Shared, Kind::Kconfig, shared.to_text().into_bytes(), true));
                    }
                    if !machine.flat().is_empty() {
                        inv.items.push(mk(Scope::Machine, Kind::Kconfig, machine.to_text().into_bytes(), true));
                    }
                }
            }
        } else {
            let kind = if e.kind == Kind::Dir { Kind::File } else { e.kind };
            inv.items.push(mk(e.scope, kind, bytes, false));
        }
    }
}

// ---- autostart check
/// If the program of an autostart .desktop is missing here, add Hidden=true (unless `opt_in`). Returns the warning.
pub fn autostart_fix(ctx: &Ctx, data: &[u8], opt_in: bool) -> (Vec<u8>, Option<String>) {
    let Some(mut kc) = std::str::from_utf8(data).ok().and_then(|t| KConfig::parse(t).ok()) else { return (data.to_vec(), None) };
    let g = "[Desktop Entry]";
    let prog = kc.get(g, "TryExec").or_else(|| kc.get(g, "Exec")).and_then(|e| {
        e.split_whitespace().find(|w| !w.contains('=') && w != &"env").map(|w| w.trim_matches('"').to_string())
    });
    let Some(p) = prog else { return (data.to_vec(), None) };
    let found = if p.starts_with('/') { ctx.roots.sys_path(&p).exists() } else { ctx.roots.path_dirs.iter().any(|d| d.join(&p).exists()) };
    if found || kc.get(g, "Hidden").as_deref() == Some("true") {
        return (data.to_vec(), None);
    }
    let msg = format!("autostart program '{p}' is not installed here");
    if opt_in {
        return (data.to_vec(), Some(msg));
    }
    kc.set(g, "Hidden", "true");
    (kc.to_text().into_bytes(), Some(format!("{msg}; copied disabled (Hidden=true)")))
}

// ---- status
#[derive(Serialize, Clone, Debug)]
pub struct CategoryStatus {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub items: usize,
    pub changed: usize, // differs from the last backup (all of them when there is none)
    pub issues: usize,
    pub machine_only: bool,
    pub skipped: usize,
}
#[derive(Serialize, Clone, Debug)]
pub struct Status {
    pub machine: MachineInfo,
    pub plasma_running: bool,
    pub backup_found: bool,
    pub categories: Vec<CategoryStatus>,
    pub last_snapshot: Option<crate::store::Snapshot>,
}

pub fn status(ctx: &Ctx, store: Option<&Store>) -> Result<Status> {
    let inv = scan(ctx)?;
    let machine = MachineInfo::detect(ctx);
    let mut known = std::collections::BTreeMap::new();
    let mut found = false;
    if let Some(s) = store {
        for part in [Part::Shared, Part::Machine(machine.id.clone())] {
            if let Ok(Some(m)) = s.manifest(&part) {
                found = true;
                for i in m.items {
                    known.insert((i.store_path, i.scope as u8), i.sha256);
                }
            }
        }
    }
    let mut issues: std::collections::BTreeMap<&str, usize> = Default::default();
    *issues.entry("shortcuts").or_default() += crate::shortcuts::diagnose_system(ctx).len();
    for i in inv.items.iter().filter(|i| i.store_path.starts_with("config/autostart/") && i.store_path.ends_with(".desktop")) {
        if autostart_fix(ctx, &i.bytes()?, false).1.is_some() {
            *issues.entry("autostart").or_default() += 1;
        }
    }
    let categories = ctx.catalog.categories.iter().map(|c| {
        let its: Vec<_> = inv.items.iter().filter(|i| i.category == c.id).collect();
        CategoryStatus {
            id: c.id.clone(),
            name: c.name.clone(),
            icon: c.icon.clone(),
            items: its.len(),
            changed: its.iter().filter(|i| known.get(&(i.store_path.clone(), i.scope as u8)) != Some(&i.sha256)).count(),
            issues: issues.get(c.id.as_str()).copied().unwrap_or(0),
            machine_only: !its.is_empty() && its.iter().all(|i| i.scope == Scope::Machine),
            skipped: inv.skipped.iter().filter(|s| ctx.catalog.entries.iter().any(|e| e.category == c.id && e.scope == Scope::Secret && e.paths.contains(&s.path))).count(),
        }
    }).collect();
    Ok(Status { machine, plasma_running: ctx.plasma_running(), backup_found: found, categories, last_snapshot: crate::store::snapshots(ctx).into_iter().next() })
}
