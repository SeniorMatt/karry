//! Turn incoming files into a Plan, then execute it (snapshot first, strategies per catalog `apply`).
use crate::catalog::{Apply, Kind, Scope};
use crate::kconfig::{self, diff, group_path, KConfig, KeyChange};
use crate::plugins::{self, MachineInfo};
use crate::scan::{self, kconfig_parts};
use crate::shortcuts::{self, ShortcutIssue};
use crate::store::{pending_dir, snapshot_create, Incoming, Snapshot};
use crate::{atomic_write, io, sha256, Cmd, Ctx, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type")]
pub enum What {
    SetKey { group: String, key: String, value: String, old: Option<String> },
    DeleteKey { group: String, key: String, old: String },
    WriteFile { size: u64, sha256: String, existing: bool },
    InstallHint { id: String, package: Option<String>, command: String },
    NeedsLogout { reason: String },
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Change {
    pub category: String,
    pub store_path: String,
    pub abs: String,
    pub apply: Apply,
    pub reload: Vec<String>,
    #[serde(flatten)]
    pub what: What,
}
impl Change {
    fn is_file_op(&self) -> bool {
        !matches!(self.what, What::InstallHint { .. } | What::NeedsLogout { .. })
    }
}
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Plan {
    pub changes: Vec<Change>,
    pub warnings: Vec<String>,
    pub needs_logout: bool,
    pub shortcut_issues: Vec<ShortcutIssue>,
}
#[derive(Serialize, Debug)]
pub struct ApplyReport {
    pub plan: Plan,
    pub snapshot: Option<Snapshot>,
    pub warnings: Vec<String>,
}

pub fn plan(ctx: &Ctx, inc: &Incoming, autostart_opt_in: bool) -> Result<Plan> {
    let plasma = ctx.plasma_running();
    let mut p = Plan { warnings: inc.warnings.clone(), ..Default::default() };
    for d in &inc.files {
        let abs = ctx.roots.abs(&d.store_path)?;
        let existing = std::fs::read(&abs).ok();
        let mut bytes = d.content.read()?;
        if d.store_path.starts_with("config/autostart/") && d.store_path.ends_with(".desktop") {
            let (b, w) = scan::autostart_fix(ctx, &bytes, autostart_opt_in);
            bytes = b;
            p.warnings.extend(w.map(|w| format!("{}: {w}", d.store_path)));
        }
        let mk = |what| Change { category: d.category.clone(), store_path: d.store_path.clone(), abs: abs.display().to_string(), apply: d.apply, reload: d.reload.clone(), what };
        let write = |bytes: &[u8]| What::WriteFile { size: bytes.len() as u64, sha256: sha256(bytes), existing: existing.is_some() };
        let mut changes = vec![];
        let mut final_text = None;
        let entry = ctx.catalog.find(&d.store_path);
        let local = (d.kind == Kind::Kconfig).then(|| existing.as_ref().and_then(|e| std::str::from_utf8(e).ok().and_then(|t| KConfig::parse(t).ok()))).flatten();
        match (d.kind == Kind::Kconfig, local, entry, std::str::from_utf8(&bytes).ok().and_then(|t| KConfig::parse(t).ok())) {
            (true, Some(local), Some(e), Some(want)) => {
                let (shared, machine) = kconfig_parts(e, local.clone());
                let view = if d.scope == Scope::Machine { machine } else { shared };
                let ch = diff(&view, &want);
                let mut fin = local;
                fin.apply(&ch);
                final_text = Some(fin.to_text());
                for c in ch {
                    changes.push(mk(match c.new {
                        Some(v) => What::SetKey { group: c.group, key: c.key, value: v, old: c.old },
                        None => What::DeleteKey { group: c.group, key: c.key, old: c.old.unwrap_or_default() },
                    }));
                }
            }
            _ => {
                let dup = p.changes.iter().any(|c| c.store_path == d.store_path && matches!(c.what, What::WriteFile { .. }));
                if existing.as_deref() != Some(&bytes[..]) && !dup {
                    changes.push(mk(write(&bytes)));
                }
                if d.kind == Kind::Kconfig {
                    final_text = String::from_utf8(bytes.clone()).ok();
                }
            }
        }
        if changes.is_empty() {
            continue;
        }
        if d.store_path == "config/kglobalshortcutsrc" {
            if let Some(t) = &final_text {
                p.shortcut_issues = shortcuts::diagnose(t, &shortcuts::Installed::scan(ctx)).unwrap_or_default();
            }
        }
        p.changes.extend(changes);
        if d.apply == Apply::Logout && plasma {
            p.needs_logout = true;
            p.changes.push(mk(What::NeedsLogout { reason: "staged; applied at next login".into() }));
        }
    }
    // KWin scripts this plan copies will register their shortcuts, so their actions are not orphans yet
    if p.changes.iter().any(|c| c.store_path.starts_with("data/kwin/scripts/")) {
        p.shortcut_issues.retain(|i| i.kind != shortcuts::IssueKind::OrphanKwinAction);
    }
    for h in plugins::hints(ctx, &inc.plugins) {
        p.changes.push(Change { category: "plugins".into(), store_path: String::new(), abs: String::new(), apply: Apply::Package, reload: vec![], what: What::InstallHint { id: h.id, package: h.package, command: h.command } });
    }
    if let (Some(a), Some(b)) = (inc.source.as_ref().and_then(|s| s.plasma.as_deref()).and_then(plugins::major), MachineInfo::detect(ctx).plasma.as_deref().and_then(plugins::major)) {
        if a != b {
            p.warnings.push(format!("backup is from Plasma {a}, this machine runs Plasma {b}"));
        }
    }
    Ok(p)
}

/// Snapshot, then execute. With `dry_run` only the plan is returned.
pub fn apply(ctx: &Ctx, inc: &Incoming, label: &str, autostart_opt_in: bool, dry_run: bool) -> Result<ApplyReport> {
    let plan = plan(ctx, inc, autostart_opt_in)?;
    if dry_run || !plan.changes.iter().any(|c| c.is_file_op()) {
        return Ok(ApplyReport { plan, snapshot: None, warnings: vec![] });
    }
    let mut paths: Vec<_> = plan.changes.iter().filter(|c| c.is_file_op()).map(|c| std::path::PathBuf::from(&c.abs)).collect();
    paths.push(ctx.roots.config.join(scan::PENDING_SCRIPT));
    paths.sort();
    paths.dedup();
    let snapshot = snapshot_create(ctx, label, &paths)?;
    let warnings = execute(ctx, &plan, inc)?;
    Ok(ApplyReport { plan, snapshot: Some(snapshot), warnings })
}

fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn execute(ctx: &Ctx, plan: &Plan, inc: &Incoming) -> Result<Vec<String>> {
    let plasma = ctx.plasma_running();
    let mut warns = vec![];
    let run = |warns: &mut Vec<String>, cmd: Cmd| -> bool {
        match ctx.runner.run(&cmd) {
            Ok(o) if o.code == 0 => true,
            _ => {
                warns.push(format!("command failed: {}", cmd.line()));
                false
            }
        }
    };
    let svc = |verb: &str| Cmd::new("systemctl", &["--user", verb, "plasma-plasmashell.service"]);
    let restart_shell = plasma && plan.changes.iter().any(|c| c.apply == Apply::Plasmashell && c.is_file_op());
    if restart_shell && !run(&mut vec![], svc("stop")) {
        run(&mut warns, Cmd::new("kquitapp6", &["plasmashell"]));
    }
    let mut by_file: BTreeMap<&str, Vec<&Change>> = BTreeMap::new();
    for c in plan.changes.iter().filter(|c| c.is_file_op()) {
        by_file.entry(&c.store_path).or_default().push(c);
    }
    let mut staged = vec![];
    for (sp, chs) in &by_file {
        let abs = ctx.roots.abs(sp)?;
        let strat = chs[0].apply;
        let keys: Vec<&Change> = chs.iter().copied().filter(|c| !matches!(c.what, What::WriteFile { .. })).collect();
        if plasma && strat == Apply::Kwrite && keys.len() == chs.len() {
            for c in keys {
                let mut a: Vec<String> = vec!["--file".into(), c.abs.clone()];
                let (g, k, v) = match &c.what {
                    What::SetKey { group, key, value, .. } => (group, key, Some(value)),
                    What::DeleteKey { group, key, .. } => (group, key, None),
                    _ => continue,
                };
                for g in group_path(g) {
                    a.extend(["--group".into(), g]);
                }
                let k = k.rfind("[$").filter(|_| k.ends_with(']')).map_or(k.as_str(), |i| &k[..i]);
                a.extend(["--key".into(), k.into()]);
                match v {
                    Some(v) => a.push(kconfig::unescape(v)),
                    None => a.push("--delete".into()),
                }
                run(&mut warns, Cmd { program: "kwriteconfig6".into(), args: a });
            }
            continue;
        }
        let data: Vec<u8> = if chs.iter().any(|c| matches!(c.what, What::WriteFile { .. })) {
            // a new kconfig file may arrive as a shared part plus a machine part: write both together
            let mut all = vec![];
            for d in inc.files.iter().filter(|d| &d.store_path == sp) {
                let b = d.content.read()?;
                if !all.is_empty() && all.last() != Some(&b'\n') {
                    all.push(b'\n');
                }
                all.extend(b);
            }
            all
        } else {
            let mut kc = KConfig::parse(&io(std::fs::read_to_string(&abs), &abs)?)?;
            let ch: Vec<KeyChange> = keys.iter().filter_map(|c| match &c.what {
                What::SetKey { group, key, value, old } => Some(KeyChange { group: group.clone(), key: key.clone(), old: old.clone(), new: Some(value.clone()) }),
                What::DeleteKey { group, key, old } => Some(KeyChange { group: group.clone(), key: key.clone(), old: Some(old.clone()), new: None }),
                _ => None,
            }).collect();
            kc.apply(&ch);
            kc.to_text().into_bytes()
        };
        let data = if sp.starts_with("config/autostart/") && sp.ends_with(".desktop") { scan::autostart_fix(ctx, &data, false).0 } else { data };
        if plasma && strat == Apply::Logout {
            atomic_write(&pending_dir(ctx).join("files").join(sp), &data, None)?;
            staged.push((pending_dir(ctx).join("files").join(sp), abs));
        } else {
            atomic_write(&abs, &data, None)?;
        }
    }
    if !staged.is_empty() {
        let mut s = String::from("#!/bin/sh\n# Applies a staged restore before Plasma starts, then removes itself.\n");
        for (from, to) in &staged {
            let dir = to.parent().map(|p| p.display().to_string()).unwrap_or_default();
            s += &format!("mkdir -p {} && cp -f {} {}\n", sh_quote(&dir), sh_quote(&from.display().to_string()), sh_quote(&to.display().to_string()));
        }
        let script = ctx.roots.config.join(scan::PENDING_SCRIPT);
        s += &format!("rm -rf {}\nrm -f {}\n", sh_quote(&pending_dir(ctx).display().to_string()), sh_quote(&script.display().to_string()));
        atomic_write(&script, s.as_bytes(), Some(0o755))?;
    }
    if restart_shell && !run(&mut vec![], svc("start")) {
        run(&mut warns, Cmd::new("kstart", &["plasmashell"]));
    }
    if plasma {
        let mut names: Vec<&str> = plan.changes.iter().filter(|c| c.is_file_op() && c.apply != Apply::Logout).flat_map(|c| c.reload.iter().map(|s| s.as_str())).collect();
        names.sort();
        names.dedup();
        for n in names.into_iter().filter(|n| !(restart_shell && *n == "plasmashell")) {
            for line in ctx.catalog.reload.get(n).into_iter().flatten() {
                run(&mut warns, Cmd::parse(line));
            }
        }
    }
    Ok(warns)
}

/// Put one catalogued file onto this machine through the normal snapshot + strategy path.
pub fn apply_file(ctx: &Ctx, store_path: &str, data: Vec<u8>, label: &str) -> Result<ApplyReport> {
    let e = ctx.catalog.find(store_path).ok_or_else(|| crate::Error::Msg(format!("{store_path} is not in the catalog")))?;
    let d = crate::store::Desired { store_path: store_path.into(), category: e.category.clone(), scope: e.scope, kind: if e.kind == Kind::Dir { Kind::File } else { e.kind }, apply: e.apply, reload: e.reload.clone(), content: crate::store::Content::Bytes(data) };
    apply(ctx, &Incoming { files: vec![d], ..Default::default() }, label, true, false)
}
