//! Shortcut doctor: finds duplicate, orphaned and shadowed bindings in kglobalshortcutsrc and offers fixes.
use crate::kconfig::{group_path, KConfig};
use crate::{err, Ctx, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Binding {
    pub group: String,
    pub action: String,
    pub friendly: String,
    pub key: String,
}
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueKind {
    Duplicate,
    OrphanApp,
    OrphanKwinAction,
    Shadowed,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum FixOp {
    Unbind { group: String, action: String, key: Option<String> },
    Reset { group: String, action: String },
    RemoveAction { group: String, action: String },
    RemoveGroup { group: String },
    Rebind { group: String, action: String }, // new keys are supplied when applying
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Fix {
    pub id: String,
    pub label: String,
    pub needs_input: bool,
    pub ops: Vec<FixOp>,
}
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ShortcutIssue {
    pub id: String,
    pub kind: IssueKind,
    pub message: String,
    pub bindings: Vec<Binding>,
    pub fixes: Vec<Fix>,
}

/// What is installed on the target machine.
#[derive(Default, Clone, Debug)]
pub struct Installed {
    pub desktop_files: BTreeSet<String>,
    pub kwin_script_actions: BTreeSet<String>,
}
impl Installed {
    pub fn scan(ctx: &Ctx) -> Installed {
        let mut i = Installed::default();
        for d in ctx.roots.data_dirs() {
            for e in walkdir::WalkDir::new(d.join("applications")).max_depth(4).into_iter().flatten() {
                i.desktop_files.insert(e.file_name().to_string_lossy().to_string());
            }
        }
        let kwinrc = std::fs::read_to_string(ctx.roots.config.join("kwinrc")).ok().and_then(|t| KConfig::parse(&t).ok());
        // JS scripts call registerShortcut("name", ...); QML scripts declare ShortcutHandler { name: "..." }
        let re = Regex::new(r#"registerShortcut\(\s*["']([^"']+)["']|ShortcutHandler\s*\{[^}]*?\bname:\s*["']([^"']+)["']"#).unwrap();
        for f in crate::plugins::installed(&ctx.roots, &ctx.catalog).into_iter().filter(|f| f.entry_id == "kwin-scripts") {
            let off = kwinrc.as_ref().and_then(|k| k.get("[Plugins]", &format!("{}Enabled", f.id))).as_deref() == Some("false");
            if off {
                continue;
            }
            for e in walkdir::WalkDir::new(&f.path).into_iter().flatten().filter(|e| e.path().extension().is_some_and(|x| x == "js" || x == "qml")) {
                if let Ok(t) = std::fs::read_to_string(e.path()) {
                    i.kwin_script_actions.extend(re.captures_iter(&t).filter_map(|c| c.get(1).or(c.get(2))).map(|m| m.as_str().to_string()));
                }
            }
        }
        i
    }
}

pub fn normalize(k: &str) -> String {
    let (mut meta, mut ctrl, mut alt, mut shift) = (false, false, false, false);
    let mut rest = k.trim();
    while let Some((h, t)) = rest.split_once('+') {
        match h.to_ascii_lowercase().as_str() {
            "meta" | "win" | "super" | "windows" => meta = true,
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            _ => break,
        }
        rest = t;
    }
    let mut out = String::new();
    for (on, n) in [(meta, "Meta"), (ctrl, "Ctrl"), (alt, "Alt"), (shift, "Shift")] {
        if on {
            out += n;
            out.push('+');
        }
    }
    out + rest
}

fn split_keys(field: &str) -> Vec<String> {
    field.replace("\\t", "\t").split('\t').map(|s| s.trim()).filter(|s| !s.is_empty() && *s != "none").map(String::from).collect()
}
fn fields(v: &str) -> (String, String, String) {
    let mut it = v.splitn(3, ',').map(String::from);
    (it.next().unwrap_or_default(), it.next().unwrap_or_default(), it.next().unwrap_or_default())
}
fn component(group: &str) -> String {
    let p = group_path(group);
    if p.first().is_some_and(|f| f == "services") { p.get(1) } else { p.first() }.cloned().unwrap_or_default()
}

pub fn diagnose(text: &str, inst: &Installed) -> Result<Vec<ShortcutIssue>> {
    let kc = KConfig::parse(text)?;
    // (group, action, active keys, default keys, friendly)
    let mut acts = vec![];
    for ((g, k), v) in kc.flat() {
        if g.is_empty() || g.contains("[$") || k.starts_with("_k_") {
            continue;
        }
        let (a, d, f) = fields(&v);
        acts.push((g, k, split_keys(&a).iter().map(|s| normalize(s)).collect::<Vec<_>>(), split_keys(&d).iter().map(|s| normalize(s)).collect::<Vec<_>>(), f));
    }
    let mut issues = vec![];
    let bind = |a: &(String, String, Vec<String>, Vec<String>, String), key: &str| Binding { group: a.0.clone(), action: a.1.clone(), friendly: if a.4.is_empty() { a.1.clone() } else { a.4.clone() }, key: key.into() };
    let fixes_for = |bs: &[Binding], dup: bool| -> Vec<Fix> {
        let mut fx = vec![];
        for (n, b) in bs.iter().enumerate() {
            if dup {
                let ops = bs.iter().filter(|o| *o != b).map(|o| FixOp::Unbind { group: o.group.clone(), action: o.action.clone(), key: Some(o.key.clone()) }).collect();
                fx.push(Fix { id: format!("keep-{n}"), label: format!("Keep it on \"{}\"", b.friendly), needs_input: false, ops });
            }
            fx.push(Fix { id: format!("reset-{n}"), label: format!("Reset \"{}\" to its default", b.friendly), needs_input: false, ops: vec![FixOp::Reset { group: b.group.clone(), action: b.action.clone() }] });
            fx.push(Fix { id: format!("rebind-{n}"), label: format!("Choose a new shortcut for \"{}\"", b.friendly), needs_input: true, ops: vec![FixOp::Rebind { group: b.group.clone(), action: b.action.clone() }] });
        }
        fx
    };
    // duplicates
    let mut by_key: BTreeMap<&str, Vec<Binding>> = BTreeMap::new();
    for a in &acts {
        for k in a.2.iter().collect::<BTreeSet<_>>() {
            by_key.entry(k).or_default().push(bind(a, k));
        }
    }
    for (k, bs) in &by_key {
        if bs.len() > 1 {
            issues.push(ShortcutIssue { id: format!("dup:{k}"), kind: IssueKind::Duplicate, message: format!("{k} is used by {} actions", bs.len()), fixes: fixes_for(bs, true), bindings: bs.clone() });
        }
    }
    // orphan apps
    let mut seen = BTreeSet::new();
    for a in &acts {
        let c = component(&a.0);
        if c.ends_with(".desktop") && !inst.desktop_files.contains(&c) && seen.insert(a.0.clone()) {
            let b = Binding { group: a.0.clone(), action: String::new(), friendly: c.clone(), key: String::new() };
            let fix = Fix { id: "remove".into(), label: format!("Remove the shortcuts of {c}"), needs_input: false, ops: vec![FixOp::RemoveGroup { group: a.0.clone() }] };
            issues.push(ShortcutIssue { id: format!("orphan-app:{}", a.0), kind: IssueKind::OrphanApp, message: format!("{c} is not installed"), bindings: vec![b], fixes: vec![fix] });
        }
    }
    // orphan KWin script actions: anything in [kwin] that is neither built in nor registered by an enabled script
    let builtin = Regex::new(r"^(Window|Switch|Walk|Activate|Show|Toggle|Expose|Overview|Kill|Suspend|Edit|Invert|Setup|Increase|Decrease|Grid|Cycle|Move|Track|Slide|Zoom|Magnify|Cube|Present|Desktop|Quick|Maximize|Minimize|Make|Hide|Peek|Lock|Mouse|Disable|Enable|Close|Cancel|Clear|Flip|Resize|Keep|Pack|Unpack|Block|Reset|Remove|Raise|Lower|Next|Previous|Focus|Fullscreen|Sticky|Screen|Tile|Cursor|Colorpicker|view_|Expo|MoveMouse|Kwin|disableInputCapture|Pointer|Add|Delete|Lock|Fill|Center|Sheet|Starfield|ShowDesktop|Slideback|ShowFPS|ZoomEffect|InvertWindow|HighlightWindow|Magic|DesktopGrid|Track Mouse|Toggle Night Color)").unwrap();
    for a in acts.iter().filter(|a| a.0 == "[kwin]") {
        if !builtin.is_match(&a.1) && !inst.kwin_script_actions.contains(&a.1) {
            let b = bind(a, a.2.first().map(|s| s.as_str()).unwrap_or(""));
            let fix = Fix { id: "remove".into(), label: format!("Remove \"{}\"", b.friendly), needs_input: false, ops: vec![FixOp::RemoveAction { group: a.0.clone(), action: a.1.clone() }] };
            issues.push(ShortcutIssue { id: format!("orphan-kwin:{}", a.1), kind: IssueKind::OrphanKwinAction, message: format!("\"{}\" belongs to a KWin script or effect that is not installed or enabled", a.1), bindings: vec![b], fixes: vec![fix] });
        }
    }
    // shadowed: bound key equals the default of another kwin/plasmashell action that is no longer on it
    for a in acts.iter().filter(|a| a.0 == "[kwin]" || a.0 == "[plasmashell]") {
        for k in a.3.iter().filter(|k| !a.2.contains(k)) {
            for o in acts.iter().filter(|o| o.2.contains(k) && (o.0 != a.0 || o.1 != a.1)) {
                let bs = vec![bind(o, k), bind(a, k)];
                issues.push(ShortcutIssue { id: format!("shadow:{}/{}:{k}", o.0, o.1), kind: IssueKind::Shadowed, message: format!("{k} is the default of \"{}\" but is used by \"{}\"", bs[1].friendly, bs[0].friendly), fixes: fixes_for(&bs[..1], false), bindings: bs });
            }
        }
    }
    Ok(issues)
}

/// The shortcuts this machine will have: a change staged for next login wins over the live file.
pub fn current_text(ctx: &Ctx) -> Option<String> {
    let staged = crate::store::pending_dir(ctx).join("files/config/kglobalshortcutsrc");
    std::fs::read_to_string(staged).or_else(|_| std::fs::read_to_string(ctx.roots.config.join("kglobalshortcutsrc"))).ok()
}

pub fn diagnose_system(ctx: &Ctx) -> Vec<ShortcutIssue> {
    current_text(ctx).and_then(|t| diagnose(&t, &Installed::scan(ctx)).ok()).unwrap_or_default()
}

/// Fix one issue through the normal snapshot + apply path (staged for next login while Plasma runs).
pub fn fix_system(ctx: &Ctx, issue: &str, fix: &str, input: Option<&str>) -> Result<crate::apply::ApplyReport> {
    let f = diagnose_system(ctx).into_iter().find(|i| i.id == issue).and_then(|i| i.fixes.into_iter().find(|f| f.id == fix)).ok_or_else(|| crate::Error::Msg("this problem is already gone".into()))?;
    let text = current_text(ctx).unwrap_or_default();
    crate::apply::apply_file(ctx, "config/kglobalshortcutsrc", apply_fix(&text, &f, input)?.into_bytes(), &format!("fix: {}", f.label))
}

/// Returns the kglobalshortcutsrc text with the fix applied. `input` = new keys for Rebind ops.
pub fn apply_fix(text: &str, fix: &Fix, input: Option<&str>) -> Result<String> {
    let mut kc = KConfig::parse(text)?;
    for op in &fix.ops {
        match op {
            FixOp::RemoveGroup { group } => kc.remove_group(group),
            FixOp::RemoveAction { group, action } => {
                kc.delete(group, action);
            }
            FixOp::Unbind { group, action, key } => {
                let Some(cur) = kc.get(group, action) else { continue };
                let (a, d, f) = fields(&cur);
                let keep: Vec<String> = split_keys(&a).into_iter().filter(|k| key.as_ref().is_some_and(|x| normalize(k) != *x)).collect();
                kc.set(group, action, &format!("{},{d},{f}", if keep.is_empty() { "none".into() } else { keep.join("\\t") }));
            }
            FixOp::Reset { group, action } => {
                let Some(cur) = kc.get(group, action) else { continue };
                let (_, d, f) = fields(&cur);
                kc.set(group, action, &format!("{},{d},{f}", if d.is_empty() { "none" } else { &d }));
            }
            FixOp::Rebind { group, action } => {
                let Some(keys) = input else { return err("rebind needs the new shortcut") };
                let (_, d, f) = fields(&kc.get(group, action).unwrap_or_default());
                kc.set(group, action, &format!("{},{d},{f}", keys.replace('\t', "\\t")));
            }
        }
    }
    Ok(kc.to_text())
}
