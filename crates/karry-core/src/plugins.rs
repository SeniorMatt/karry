//! Distro detection, cross-distro package names, plugin discovery and resolution.
use crate::kconfig::KConfig;
use crate::{Ctx, Error, Result, Roots};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Deserialize, Clone, Debug)]
pub struct Family {
    pub ids: Vec<String>,
    pub install: String,
    pub owner: String,
}
#[derive(Deserialize, Clone, Debug)]
pub struct Packages {
    pub families: BTreeMap<String, Family>,
    pub map: BTreeMap<String, BTreeMap<String, String>>,
}
impl Packages {
    pub fn embedded() -> Result<Packages> {
        toml::from_str(include_str!("../../../data/packages.toml")).map_err(|e| Error::Msg(format!("packages.toml: {e}")))
    }
    pub fn family_of(&self, id: &str, id_like: &[String]) -> Option<String> {
        std::iter::once(id).chain(id_like.iter().map(|s| s.as_str())).find_map(|i| self.families.iter().find(|(_, f)| f.ids.iter().any(|x| x == i)).map(|(n, _)| n.clone()))
    }
    pub fn install_cmd(&self, family: &str, pkgs: &[&str]) -> Option<String> {
        self.families.get(family).map(|f| f.install.replace("{pkgs}", &pkgs.join(" ")))
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct MachineInfo {
    pub id: String, // first 16 hex of sha256(/etc/machine-id); the raw id is never stored
    pub hostname: String,
    pub distro: String,
    pub family: Option<String>,
    pub plasma: Option<String>,
    pub frameworks: Option<String>,
}
impl MachineInfo {
    pub fn detect(ctx: &Ctx) -> MachineInfo {
        let r = &ctx.roots;
        let raw = std::fs::read_to_string(r.sys_path("/etc/machine-id")).unwrap_or_else(|_| "unknown-machine".into());
        let osr = std::fs::read_to_string(r.sys_path("/etc/os-release")).unwrap_or_default();
        let field = |k: &str| osr.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(|v| v.trim().trim_matches('"').to_string());
        let id = field("ID").unwrap_or_default();
        let like: Vec<String> = field("ID_LIKE").unwrap_or_default().split_whitespace().map(String::from).collect();
        let host = std::fs::read_to_string(r.sys_path("/etc/hostname")).map(|s| s.trim().to_string()).ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "unknown".into());
        let ver = |cmd: &str, re: &str| {
            ctx.run(cmd).and_then(|o| regex::Regex::new(re).ok()?.captures(&o.stdout)?.get(1).map(|m| m.as_str().to_string()))
        };
        MachineInfo {
            id: crate::sha256(raw.trim().as_bytes())[..16].to_string(),
            hostname: host,
            family: ctx.packages.family_of(&id, &like),
            distro: id,
            plasma: ver("plasmashell --version", r"(\d+\.\d+(?:\.\d+)?)"),
            frameworks: ver("kreadconfig6 --version", r"Frameworks:\s*(\S+)"),
        }
    }
}
pub fn major(v: &str) -> Option<u32> {
    v.split('.').next()?.parse().ok()
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    User,
    System,
}
#[derive(Clone, Debug)]
pub struct Found {
    pub entry_id: String,
    pub id: String,       // KPlugin.Id from metadata.json, else the dir/file stem
    pub dir_name: String,
    pub origin: Origin,
    pub path: PathBuf,
    pub name: Option<String>,
    pub version: Option<String>,
}

fn metadata(dir: &std::path::Path) -> (Option<String>, Option<String>, Option<String>) {
    let v: Option<serde_json::Value> = std::fs::read_to_string(dir.join("metadata.json")).ok().and_then(|t| serde_json::from_str(&t).ok());
    let s = |k: &str| v.as_ref().and_then(|v| v["KPlugin"][k].as_str().map(String::from));
    (s("Id"), s("Name"), s("Version"))
}

/// Plugins found under every `apply = package` entry, user dirs and system dirs.
pub fn installed(roots: &Roots, cat: &crate::catalog::Catalog) -> Vec<Found> {
    let mut out = vec![];
    for e in cat.entries.iter().filter(|e| e.apply == crate::catalog::Apply::Package) {
        let dirs = e.paths.iter().map(|p| (roots.resolve(p), Origin::User)).chain(e.system.iter().map(|s| (roots.sys_path(s), Origin::System)));
        for (d, origin) in dirs {
            let Ok(rd) = std::fs::read_dir(&d) else { continue };
            for f in rd.flatten() {
                let dir_name = f.file_name().to_string_lossy().to_string();
                if dir_name.starts_with('.') {
                    continue;
                }
                let stem = if f.path().is_dir() { dir_name.clone() } else { dir_name.rsplit_once('.').map_or(dir_name.clone(), |(s, _)| s.to_string()) };
                let (id, name, version) = metadata(&f.path());
                out.push(Found { entry_id: e.id.clone(), id: id.unwrap_or(stem), dir_name, origin, path: f.path(), name, version });
            }
        }
    }
    out
}

/// (candidate entry ids, plugin id) that the user's configs point at.
pub fn references(roots: &Roots) -> Vec<(Vec<&'static str>, String)> {
    let table: [(&str, &str, &str, &str); 8] = [
        ("kdeglobals", "[General]", "ColorScheme", "color-schemes"),
        ("kdeglobals", "[Icons]", "Theme", "icons-cursors"),
        ("kdeglobals", "[KDE]", "LookAndFeelPackage", "global-themes"),
        ("plasmarc", "[Theme]", "name", "plasma-themes"),
        ("kcminputrc", "[Mouse]", "cursorTheme", "icons-cursors"),
        ("kwinrc", "[org.kde.kdecoration2]", "theme", "decorations"),
        ("ksplashrc", "[KSplash]", "Theme", "global-themes"),
        ("ksplashrc", "[KSplash]", "Engine", "global-themes"),
    ];
    let load = |f: &str| std::fs::read_to_string(roots.config.join(f)).ok().and_then(|t| KConfig::parse(&t).ok());
    let mut out = vec![];
    for (f, g, k, e) in table {
        if let Some(v) = load(f).and_then(|c| c.get(g, k)).filter(|v| !v.is_empty()) {
            out.push((vec![e], v));
        }
    }
    if let Some(c) = load("kwinrc") {
        for ((g, k), v) in c.flat() {
            if g == "[Plugins]" && v == "true" {
                if let Some(id) = k.strip_suffix("Enabled") {
                    out.push((vec!["kwin-effects", "kwin-scripts"], id.to_string()));
                }
            }
        }
    }
    if let Some(c) = load("plasma-org.kde.plasma.desktop-appletsrc") {
        for ((_, k), v) in c.flat() {
            if k == "plugin" {
                out.push((vec!["plasmoids"], v));
            }
        }
    }
    out
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PluginRecord {
    pub id: String,
    pub entry_id: String,
    pub origin: Origin,
    pub name: Option<String>,
    pub version: Option<String>,
    pub package: Option<String>,
    pub packages: BTreeMap<String, String>, // family -> package name
}

/// User-installed plugins plus system plugins the configs reference (with their owning package).
pub fn records(ctx: &Ctx) -> Vec<PluginRecord> {
    let all = installed(&ctx.roots, &ctx.catalog);
    let family = MachineInfo::detect(ctx).family;
    let mut out: Vec<PluginRecord> = vec![];
    let mk = |f: &Found| PluginRecord { id: f.id.clone(), entry_id: f.entry_id.clone(), origin: f.origin, name: f.name.clone(), version: f.version.clone(), package: None, packages: BTreeMap::new() };
    for f in all.iter().filter(|f| f.origin == Origin::User) {
        out.push(mk(f));
    }
    for (entries, id) in references(&ctx.roots) {
        if out.iter().any(|r| r.id == id && entries.contains(&r.entry_id.as_str())) {
            continue;
        }
        let hit = all.iter().filter(|f| f.origin == Origin::System && entries.contains(&f.entry_id.as_str())).find(|f| f.id.eq_ignore_ascii_case(&id) || f.dir_name.eq_ignore_ascii_case(&id));
        let Some(f) = hit else { continue };
        let mut r = mk(f);
        if let Some(m) = ctx.packages.map.get(&r.id) {
            r.packages = m.clone();
        }
        if let Some(fam) = &family {
            let cmd = ctx.packages.families[fam].owner.replace("{path}", &f.path.to_string_lossy());
            let owner = ctx.run(&cmd).and_then(|o| o.stdout.lines().next().map(|l| l.split(':').next().unwrap_or("").trim().to_string())).filter(|s| !s.is_empty());
            if let Some(p) = owner {
                r.package = Some(p.clone());
                r.packages.insert(fam.clone(), p);
            }
        }
        out.push(r);
    }
    out
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Hint {
    pub id: String,
    pub package: Option<String>,
    pub command: String,
}
/// System plugins from a backup that this machine lacks: what to install.
pub fn hints(ctx: &Ctx, recs: &[PluginRecord]) -> Vec<Hint> {
    let have = installed(&ctx.roots, &ctx.catalog);
    let me = MachineInfo::detect(ctx);
    let mut out = vec![];
    for r in recs.iter().filter(|r| r.origin == Origin::System) {
        if have.iter().any(|f| f.id == r.id && f.entry_id == r.entry_id) {
            continue;
        }
        let pkg = me.family.as_ref().and_then(|f| r.packages.get(f)).cloned();
        let command = match (&pkg, &me.family) {
            (Some(p), Some(f)) => ctx.packages.install_cmd(f, &[p]).unwrap_or_default(),
            _ => format!("search your package manager for \"{}\"", r.id),
        };
        out.push(Hint { id: r.id.clone(), package: pkg, command });
    }
    out
}

