use crate::{err, Error, Result, Roots};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Category {
    pub id: String,
    pub name: String,
    pub icon: String,
}
#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Kconfig,
    Json,
    File,
    Dir,
}
#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    Shared,
    Machine,
    Secret,
}
#[derive(Deserialize, Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Apply {
    Kwrite,
    Copy,
    Plasmashell,
    Logout,
    Package,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    category: String,
    id: Option<String>,
    path: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    system: Vec<String>,
    kind: Kind,
    scope: Scope,
    apply: Apply,
    #[serde(default)]
    reload: Vec<String>,
    #[serde(default)]
    machine_groups: Vec<String>,
    #[serde(default)]
    secret_keys: Vec<String>,
    note: Option<String>,
}
#[derive(Deserialize, Default)]
struct Patterns {
    names: Vec<String>,
}
#[derive(Deserialize, Default)]
struct AutoCfg {
    ignore: Vec<String>,
}
#[derive(Deserialize, Default)]
struct File {
    #[serde(default)]
    category: Vec<Category>,
    #[serde(default)]
    entry: Vec<Raw>,
    secret_patterns: Option<Patterns>,
    auto: Option<AutoCfg>,
    #[serde(default)]
    reload: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub category: String,
    pub paths: Vec<String>,
    pub system: Vec<String>,
    pub kind: Kind,
    pub scope: Scope,
    pub apply: Apply,
    pub reload: Vec<String>,
    pub machine_groups: Vec<Regex>,
    pub secret_keys: Vec<Regex>,
    pub note: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub categories: Vec<Category>,
    pub entries: Vec<Entry>,
    pub secret_names: Vec<Regex>,
    pub auto_ignore: Vec<Regex>,
    pub reload: BTreeMap<String, Vec<String>>,
}

pub fn glob_re(pat: &str) -> Regex {
    let mut s = String::from("^");
    for c in pat.chars() {
        match c {
            '*' => s.push_str(".*"),
            '?' => s.push('.'),
            c => s.push_str(&regex::escape(&c.to_string())),
        }
    }
    Regex::new(&(s + "$")).unwrap()
}

fn res(v: &[String]) -> Result<Vec<Regex>> {
    v.iter().map(|r| Regex::new(r).map_err(|e| Error::Msg(format!("bad regex {r}: {e}")))).collect()
}

impl Catalog {
    pub fn embedded() -> Result<Catalog> {
        let mut c = Catalog::default();
        c.add(include_str!("../../../data/catalog.toml"))?;
        c.validate()?;
        Ok(c)
    }
    /// Embedded catalog plus `$XDG_CONFIG_HOME/karry/catalog.d/*.toml` (entries appended).
    pub fn load(roots: &Roots) -> Result<Catalog> {
        let mut c = Catalog::default();
        c.add(include_str!("../../../data/catalog.toml"))?;
        if let Ok(rd) = std::fs::read_dir(roots.config.join("karry/catalog.d")) {
            let mut files: Vec<_> = rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "toml")).collect();
            files.sort();
            for f in files {
                let t = crate::io(std::fs::read_to_string(&f), &f)?;
                c.add(&t)?;
            }
        }
        c.validate()?;
        Ok(c)
    }
    fn add(&mut self, text: &str) -> Result<()> {
        let f: File = toml::from_str(text).map_err(|e| Error::Msg(format!("catalog: {e}")))?;
        self.categories.extend(f.category);
        for r in f.entry {
            let mut paths = r.paths;
            paths.extend(r.path);
            if paths.is_empty() {
                return err("catalog entry without path");
            }
            self.entries.push(Entry {
                id: r.id.unwrap_or_else(|| paths[0].clone()),
                category: r.category,
                paths,
                system: r.system,
                kind: r.kind,
                scope: r.scope,
                apply: r.apply,
                reload: r.reload,
                machine_groups: res(&r.machine_groups)?,
                secret_keys: res(&r.secret_keys)?,
                note: r.note,
            });
        }
        if let Some(p) = f.secret_patterns {
            self.secret_names.extend(res(&p.names)?);
        }
        if let Some(a) = f.auto {
            self.auto_ignore.extend(res(&a.ignore)?);
        }
        self.reload.extend(f.reload);
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        let mut errs = vec![];
        let mut seen = std::collections::BTreeSet::new();
        for e in &self.entries {
            if !self.categories.iter().any(|c| c.id == e.category) {
                errs.push(format!("{}: unknown category {}", e.id, e.category));
            }
            for r in &e.reload {
                if !self.reload.contains_key(r) {
                    errs.push(format!("{}: unknown reload {r}", e.id));
                }
            }
            for p in &e.paths {
                if !seen.insert(p.clone()) {
                    errs.push(format!("duplicate path {p}"));
                }
                if !p.contains(':') {
                    errs.push(format!("{p}: missing root"));
                }
                if e.scope != Scope::Secret && p.ends_with('/') != (e.kind == Kind::Dir) {
                    errs.push(format!("{p}: trailing / must match kind=dir"));
                }
            }
        }
        if errs.is_empty() { Ok(()) } else { Err(Error::Validation(errs)) }
    }
    pub fn is_secret_name(&self, name: &str) -> bool {
        self.secret_names.iter().any(|r| r.is_match(name))
    }
    /// The entry covering a store path ("config/kwinrc"), or the apps template for an unlisted `*rc`.
    pub fn find(&self, sp: &str) -> Option<&Entry> {
        for e in &self.entries {
            for spec in &e.paths {
                let s = spec.replacen(':', "/", 1);
                let s = s.trim_end_matches('/');
                let hit = if spec.ends_with('/') {
                    sp == s || sp.strip_prefix(s).is_some_and(|r| r.starts_with('/'))
                } else if let Some((dir, name)) = s.rsplit_once('/').filter(|(_, n)| n.contains('*')) {
                    sp.rsplit_once('/').is_some_and(|(d, n)| d == dir && glob_re(name).is_match(n))
                } else {
                    sp == s
                };
                if hit {
                    return Some(e);
                }
            }
        }
        let name = sp.strip_prefix("config/")?;
        if !name.contains('/') && name.ends_with("rc") && !self.auto_ignore.iter().any(|r| r.is_match(name)) {
            return self.entries.iter().find(|e| e.category == "apps" && e.kind == Kind::Kconfig);
        }
        None
    }
}
