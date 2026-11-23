use karry_core::apply::{self, What};
use karry_core::shortcuts::{self, Installed, IssueKind};
use karry_core::store::{self, Part, Store};
use karry_core::sync::{self, Backend, FolderBackend, SyncOptions};
use karry_core::{Cmd, Ctx, RecordingRunner, Roots};
use std::path::{Path, PathBuf};
use std::sync::Arc;

struct World {
    ctx: Ctx,
    rec: Arc<RecordingRunner>,
}
impl World {
    fn new(base: &Path, machine_id: &str, plasma: bool) -> World {
        let roots = Roots::under(base);
        for (f, c) in [("etc/machine-id", format!("{machine_id}\n")), ("etc/hostname", format!("host-{}\n", &machine_id[..4])), ("etc/os-release", "ID=arch\n".into())] {
            put(&roots.sys.join(f), c.as_bytes());
        }
        let rec = Arc::new(RecordingRunner::new());
        let ctx = Ctx::new(roots).unwrap().with_runner(rec.clone(), Some(plasma));
        World { ctx, rec }
    }
    fn cfg(&self, name: &str) -> PathBuf {
        self.ctx.roots.config.join(name)
    }
    fn read(&self, name: &str) -> String {
        std::fs::read_to_string(self.cfg(name)).unwrap()
    }
    fn put(&self, name: &str, c: &str) {
        put(&self.cfg(name), c.as_bytes())
    }
}
fn put(p: &Path, c: &[u8]) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, c).unwrap();
}
fn tree(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut v: Vec<_> = walkdir::WalkDir::new(root).into_iter().flatten().filter(|e| e.file_type().is_file()).map(|e| (e.path().strip_prefix(root).unwrap().display().to_string(), std::fs::read(e.path()).unwrap())).collect();
    v.sort();
    v
}
fn all_store_text(root: &Path) -> String {
    tree(root).into_iter().map(|(p, b)| format!("{p}\n{}", String::from_utf8_lossy(&b))).collect()
}
fn source(dir: &Path) -> World {
    let w = World::new(dir, "aaaa-source-machine", false);
    w.put("kdeglobals", "[General]\nColorScheme=BreezeDark\n\n[KScreen]\nScreenScaleFactors=eDP-1=1.5;\n");
    w.put("kwinrc", "[Windows]\nBorderSnapZone=10\n[Compositing]\nBackend=OpenGL\n");
    w.put("dolphinrc", "[General]\nShowFullPath=true\npassword=hunter2\nmyToken=abc123\n");
    w.put("kwinoutputconfig.json", "{\"outputs\":[]}");
    w.put("kglobalshortcutsrc", "[kwin]\nOverview=Meta+W,Meta+W,Overview\n");
    w.put("gtk-3.0/settings.ini", "[Settings]\ngtk-theme-name=Breeze\n");
    put(&w.ctx.roots.data.join("kwalletd/kdewallet.kwl"), b"SECRETWALLET");
    put(&w.ctx.roots.data.join("konsole/profile.profile"), b"[General]\nName=x\n");
    put(&w.ctx.roots.data.join("konsole/token.txt"), b"SECRETTOKEN");
    w
}

#[test]
fn secrets_never_read_or_stored() {
    let (a, out) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let w = source(a.path());
    let s = Store::create(out.path()).unwrap();
    store::backup(&w.ctx, &s, None).unwrap();
    let text = all_store_text(out.path());
    for bad in ["hunter2", "abc123", "SECRETWALLET", "SECRETTOKEN", "kdewallet", "token.txt"] {
        assert!(!text.contains(bad), "{bad} leaked");
    }
    assert!(text.contains("ShowFullPath=true") && text.contains("profile.profile"));
    assert!(!all_store_text(out.path()).contains("aaaa-source-machine"), "raw machine id stored");
}

#[test]
fn machine_files_stay_in_machine_dir_and_restore_only_there() {
    let (a, out, b, c) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let w = source(a.path());
    let s = Store::create(out.path()).unwrap();
    store::backup(&w.ctx, &s, None).unwrap();
    let shared: String = tree(&out.path().join("shared")).into_iter().map(|(p, b)| format!("{p}\n{}", String::from_utf8_lossy(&b))).collect();
    assert!(!shared.contains("kwinoutputconfig") && !shared.contains("[KScreen]") && !shared.contains("ScreenScaleFactors") && !shared.contains("[Compositing]"));
    let id = karry_core::plugins::MachineInfo::detect(&w.ctx).id;
    assert_eq!(id.len(), 16);
    let mtext = all_store_text(&out.path().join("machines").join(&id));
    assert!(mtext.contains("kwinoutputconfig") && mtext.contains("ScreenScaleFactors"));
    // other machine: machine files not offered
    let other = World::new(b.path(), "bbbb-other-machine", false);
    let inc = store::load(&other.ctx, &s, None).unwrap();
    assert!(inc.files.iter().all(|f| f.scope == karry_core::catalog::Scope::Shared));
    assert!(inc.warnings.iter().any(|w| w.contains("different machine")));
    // same machine id (reinstall): machine files restored
    let same = World::new(c.path(), "aaaa-source-machine", false);
    let inc = store::load(&same.ctx, &s, None).unwrap();
    assert!(inc.files.iter().any(|f| f.store_path == "config/kwinoutputconfig.json"));
    // a file that has a shared and a machine part lands whole on a fresh machine
    apply::apply(&same.ctx, &inc, "r", false, false).unwrap();
    let k = same.read("kwinrc");
    assert!(k.contains("BorderSnapZone=10") && k.contains("[Compositing]"));
}

#[test]
fn snapshot_then_undo_is_byte_identical() {
    let (a, out, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s = Store::create(out.path()).unwrap();
    store::backup(&source(a.path()).ctx, &s, None).unwrap();
    let t = World::new(b.path(), "bbbb", false);
    t.put("kdeglobals", "# mine\n[General]\nColorScheme=Other\nExtra=1\n[KScreen]\nx=y\n");
    put(&t.cfg("gtk-3.0/settings.ini"), &[0u8, 255, 1, 2, 13, 10]);
    let before = tree(&b.path().join("home"));
    let inc = store::load(&t.ctx, &s, None).unwrap();
    let r = apply::apply(&t.ctx, &inc, "restore", false, false).unwrap();
    let id = r.snapshot.unwrap().id;
    assert_ne!(tree(&b.path().join("home")).len() + 100, before.len());
    assert!(t.read("kdeglobals").contains("ColorScheme=BreezeDark") && t.read("kdeglobals").contains("[KScreen]\nx=y"), "local machine group kept");
    assert!(!t.read("kdeglobals").contains("Extra=1"));
    store::undo(&t.ctx, Some(&id)).unwrap();
    let after: Vec<_> = tree(&b.path().join("home")).into_iter().filter(|(p, _)| !p.contains(".local/state/karry")).collect();
    assert_eq!(after, before);
    // dry run changes nothing
    let r = apply::apply(&t.ctx, &inc, "x", false, true).unwrap();
    assert!(r.snapshot.is_none() && !r.plan.changes.is_empty());
}

#[test]
fn undo_while_plasma_runs_uses_safe_strategies() {
    let (a, out, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s = Store::create(out.path()).unwrap();
    store::backup(&source(a.path()).ctx, &s, None).unwrap();
    let t = World::new(b.path(), "bbbb", false);
    t.put("kwinrc", "[Windows]\nBorderSnapZone=5\n");
    let id = apply::apply(&t.ctx, &store::load(&t.ctx, &s, None).unwrap(), "restore", false, false).unwrap().snapshot.unwrap().id;
    assert!(t.read("kwinrc").contains("BorderSnapZone=10"));
    let t = World { ctx: t.ctx.with_runner(t.rec.clone(), Some(true)), rec: t.rec };
    store::undo(&t.ctx, Some(&id)).unwrap();
    let calls: Vec<String> = t.rec.calls().iter().map(Cmd::line).collect();
    let kw = t.cfg("kwinrc").display().to_string();
    assert!(calls.contains(&format!("kwriteconfig6 --file {kw} --group Windows --key BorderSnapZone 5")), "{calls:?}");
    assert!(t.read("kwinrc").contains("BorderSnapZone=10"), "kwrite file must not be overwritten while Plasma runs");
}

#[test]
fn malicious_backup_is_rejected_before_any_write() {
    let (out, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let t = World::new(b.path(), "bbbb", false);
    t.put("kwinrc", "[A]\nk=v\n");
    let before = tree(&b.path().join("home"));
    let manifest = |sp: &str| {
        let me = karry_core::plugins::MachineInfo::detect(&t.ctx);
        format!(r#"{{"format":1,"created":"x","machine":{},"items":[{{"store_path":"{sp}","category":"windows","entry_id":"x","scope":"Shared","kind":"file","apply":"copy","sha256":"00","size":1,"plugin_id":null}}]}}"#, serde_json::to_string(&me).unwrap())
    };
    let outside = tempfile::tempdir().unwrap();
    put(&outside.path().join("passwd"), b"root");
    for (name, sp) in [("dotdot", "config/../../evil"), ("abs", "/etc/passwd"), ("uncatalogued", "home/.bashrc"), ("secret", "data/kwalletd/x"), ("secretname", "data/konsole/id_rsa"), ("link", "config/kwinrc")] {
        let root = out.path().join(name);
        std::fs::create_dir_all(root.join("shared/files/config")).unwrap();
        std::fs::write(root.join("karry.json"), "{\"format\":1}").unwrap();
        std::fs::write(root.join("shared/manifest.json"), manifest(sp)).unwrap();
        if name == "link" {
            std::os::unix::fs::symlink(outside.path().join("passwd"), root.join("shared/files/config/kwinrc")).unwrap();
        }
        let e = store::load(&t.ctx, &Store::open(&root).unwrap(), None);
        assert!(e.is_err(), "{name} accepted");
    }
    assert_eq!(tree(&b.path().join("home")), before);
}

#[test]
fn strategies_run_the_right_commands() {
    let (a, out, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let s = Store::create(out.path()).unwrap();
    store::backup(&source(a.path()).ctx, &s, None).unwrap();
    // Plasma running
    let t = World::new(b.path(), "bbbb", true);
    t.put("kdeglobals", "[General]\nColorScheme=Old\n");
    t.put("kwinrc", "[Windows]\nBorderSnapZone=5\n");
    t.put("kglobalshortcutsrc", "[kwin]\nOverview=Meta+X,Meta+W,Overview\n");
    t.put("gtk-3.0/settings.ini", "old");
    let inc = store::load(&t.ctx, &s, None).unwrap();
    let r = apply::apply(&t.ctx, &inc, "restore", false, false).unwrap();
    let calls: Vec<String> = t.rec.calls().iter().map(Cmd::line).collect();
    let kd = t.cfg("kdeglobals").display().to_string();
    assert!(calls.contains(&format!("kwriteconfig6 --file {kd} --group General --key ColorScheme BreezeDark")), "{calls:?}");
    let kw = t.cfg("kwinrc").display().to_string();
    assert!(calls.contains(&format!("kwriteconfig6 --file {kw} --group Windows --key BorderSnapZone 10")));
    assert_eq!(t.read("kdeglobals"), "[General]\nColorScheme=Old\n", "kwrite file must not be overwritten");
    assert!(calls.contains(&"qdbus6 org.kde.KWin /KWin reconfigure".to_string()));
    // logout: staged, target untouched, login script written
    assert_eq!(t.read("kglobalshortcutsrc"), "[kwin]\nOverview=Meta+X,Meta+W,Overview\n");
    assert!(r.plan.needs_logout);
    let script = t.read("plasma-workspace/env/karry-pending.sh");
    assert!(script.contains("kglobalshortcutsrc") && script.contains("rm -f"));
    // copy: plain atomic write
    assert!(t.read("gtk-3.0/settings.ini").contains("Breeze"));
    // not running: plain write of keys, no commands
    let b2 = tempfile::tempdir().unwrap();
    let t2 = World::new(b2.path(), "cccc", false);
    t2.put("kdeglobals", "[General]\nColorScheme=Old\n");
    t2.put("kglobalshortcutsrc", "[kwin]\nOverview=Meta+X,Meta+W,Overview\n");
    apply::apply(&t2.ctx, &store::load(&t2.ctx, &s, None).unwrap(), "r", false, false).unwrap();
    assert_eq!(t2.read("kdeglobals"), "[General]\nColorScheme=BreezeDark\n");
    assert!(t2.read("kglobalshortcutsrc").contains("Meta+W,Meta+W"));
    assert!(t2.rec.calls().iter().all(|c| c.program != "kwriteconfig6" && c.program != "systemctl"));
}

#[test]
fn plasmashell_strategy_stops_writes_starts_and_packages_hint() {
    let (a, out, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let src = source(a.path());
    src.put("plasma-org.kde.plasma.desktop-appletsrc", "[Containments][1]\nplugin=org.kde.panel\n[Containments][1][Applets][2]\nplugin=org.kde.plasma.weather\n");
    put(&src.ctx.roots.sys.join("usr/share/plasma/plasmoids/org.kde.plasma.weather/metadata.json"), br#"{"KPlugin":{"Id":"org.kde.plasma.weather"}}"#);
    src.rec.respond("pacman -Qoq", 0, "kdeplasma-addons\n");
    let s = Store::create(out.path()).unwrap();
    store::backup(&src.ctx, &s, None).unwrap();
    let m = s.manifest(&Part::Shared).unwrap().unwrap();
    assert!(m.plugins.iter().any(|p| p.id == "org.kde.plasma.weather" && p.package.as_deref() == Some("kdeplasma-addons")));
    let t = World::new(b.path(), "bbbb", true);
    t.put("plasma-org.kde.plasma.desktop-appletsrc", "[Containments][1]\nplugin=old\n");
    let r = apply::apply(&t.ctx, &store::load(&t.ctx, &s, None).unwrap(), "r", false, false).unwrap();
    let calls: Vec<String> = t.rec.calls().iter().map(Cmd::line).collect();
    let (stop, start) = (calls.iter().position(|c| c == "systemctl --user stop plasma-plasmashell.service").unwrap(), calls.iter().position(|c| c == "systemctl --user start plasma-plasmashell.service").unwrap());
    assert!(stop < start);
    assert!(t.read("plasma-org.kde.plasma.desktop-appletsrc").contains("org.kde.panel"));
    assert!(r.plan.changes.iter().any(|c| matches!(&c.what, What::InstallHint { id, command, .. } if id == "org.kde.plasma.weather" && command == "sudo pacman -S --needed kdeplasma-addons")));
}

#[test]
fn shortcut_doctor_finds_and_fixes() {
    let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/kglobalshortcutsrc")).unwrap();
    assert_eq!(karry_core::kconfig::KConfig::parse(&text).unwrap().to_text(), text);
    let inst = Installed {
        desktop_files: ["org.kde.dolphin.desktop", "org.kde.konsole.desktop", "org.kde.krunner.desktop", "firefox.desktop"].map(String::from).into(),
        ..Default::default()
    };
    let issues = shortcuts::diagnose(&text, &inst).unwrap();
    let ids: Vec<&str> = issues.iter().map(|i| i.id.as_str()).collect();
    // Shift+Meta+W (konsole) == Meta+Shift+W (custom.desktop)
    let dup = issues.iter().find(|i| i.id == "dup:Meta+Shift+W").unwrap_or_else(|| panic!("{ids:?}"));
    assert_eq!(dup.kind, IssueKind::Duplicate);
    assert_eq!(dup.bindings.len(), 2);
    // Alt+Space\tAlt+F2 is one action with two bindings: not a duplicate; "none" is ignored
    assert!(!ids.iter().any(|i| i.contains("Alt+Space") || i.contains("none")));
    assert!(ids.contains(&"orphan-app:[org.kde.oldapp.desktop]"));
    assert!(ids.contains(&"orphan-app:[services][custom.desktop]"));
    assert!(ids.contains(&"orphan-kwin:Krohnkite: Monocle Layout"));
    assert!(!ids.iter().any(|i| i.starts_with("orphan-app:[org.kde.dolphin")));
    // shadowed: Meta+Shift+Left... "Window to Next Screen" default Meta+Shift+Right is unused, so no issue; plasmashell Alt+F1 default is unused
    assert!(issues.iter().all(|i| i.kind != IssueKind::Shadowed));
    // keep fix: unbinds only the other action's key
    let keep = dup.fixes.iter().find(|f| f.id == "keep-0").unwrap();
    let fixed = shortcuts::apply_fix(&text, keep, None).unwrap();
    assert_eq!(shortcuts::diagnose(&fixed, &inst).unwrap().iter().filter(|i| i.kind == IssueKind::Duplicate).count(), 0);
    assert!(fixed.contains("_launch=none,none,Custom") || fixed.contains("NewTab=none,none,New tab"));
    // multi-binding unbind keeps the other key
    let two = shortcuts::Fix { id: "x".into(), label: "".into(), needs_input: false, ops: vec![shortcuts::FixOp::Unbind { group: "[kwin]".into(), action: "Switch to Desktop 1".into(), key: Some("Meta+1".into()) }] };
    assert!(shortcuts::apply_fix(&text, &two, None).unwrap().contains("Switch to Desktop 1=Ctrl+F1,Ctrl+F1,Switch to Desktop 1"));
    // reset, remove, rebind
    let reset = shortcuts::Fix { id: "r".into(), label: "".into(), needs_input: false, ops: vec![shortcuts::FixOp::Reset { group: "[plasmashell]".into(), action: "activate application launcher".into() }] };
    assert!(shortcuts::apply_fix(&text, &reset, None).unwrap().contains("activate application launcher=Alt+F1,Alt+F1,Activate Application Launcher"));
    let orphan = issues.iter().find(|i| i.id == "orphan-app:[org.kde.oldapp.desktop]").unwrap();
    let t2 = shortcuts::apply_fix(&text, &orphan.fixes[0], None).unwrap();
    assert!(!t2.contains("oldapp"));
    let orphan = issues.iter().find(|i| i.kind == IssueKind::OrphanKwinAction).unwrap();
    assert!(!shortcuts::apply_fix(&text, &orphan.fixes[0], None).unwrap().contains("Krohnkite"));
    let rb = dup.fixes.iter().find(|f| f.needs_input).unwrap();
    assert!(shortcuts::apply_fix(&text, rb, None).is_err());
    assert!(shortcuts::apply_fix(&text, rb, Some("Meta+Alt+9")).unwrap().contains("Meta+Alt+9"));
}

fn machine(dir: &Path, id: &str) -> World {
    let w = World::new(dir, id, false);
    w
}

#[test]
fn folder_sync_merges_keys_and_reports_conflicts() {
    let (shared, a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (wa, wb) = (machine(a.path(), "aaaa"), machine(b.path(), "bbbb"));
    let st = Store::create(shared.path()).unwrap();
    let be = FolderBackend { root: shared.path().into() };
    let go = |w: &World| sync::sync(&w.ctx, &st, &be, &SyncOptions::default()).unwrap();
    wa.put("kwinrc", "[Windows]\nx=1\ny=1\nz=1\n");
    wb.put("kwinrc", "[Windows]\nx=1\ny=1\nz=1\n");
    go(&wa);
    go(&wb);
    wa.put("kwinrc", "[Windows]\nx=2\ny=1\nz=1\n");
    wb.put("kwinrc", "[Windows]\nx=1\ny=3\nz=1\n");
    assert_eq!(go(&wa).published, vec!["config/kwinrc"]);
    let r = go(&wb);
    assert!(r.conflicts.is_empty());
    assert_eq!(r.incoming[0].summary, "host-aaaa changed 1 key in kwinrc");
    assert_eq!(wb.read("kwinrc"), "[Windows]\nx=2\ny=3\nz=1\n");
    assert_eq!(go(&wa).incoming.len(), 1);
    assert_eq!(wa.read("kwinrc"), "[Windows]\nx=2\ny=3\nz=1\n");
    // same key changed on both sides = conflict, local value kept
    wa.put("kwinrc", "[Windows]\nx=7\ny=3\nz=1\n");
    go(&wa);
    wb.put("kwinrc", "[Windows]\nx=8\ny=3\nz=1\n");
    let r = go(&wb);
    assert_eq!((r.conflicts.len(), r.conflicts[0].key.as_str()), (1, "x"));
    assert!(wb.read("kwinrc").contains("x=8"));
    // dry run touches nothing and takes no snapshot
    let before = wa.read("kwinrc");
    let r = sync::sync(&wa.ctx, &st, &be, &SyncOptions { dry_run: true, ..Default::default() }).unwrap();
    assert!(r.snapshot.is_none() && wa.read("kwinrc") == before);
    std::fs::write(shared.path().join("kwinrc.sync-conflict-20260101-abc"), "x").unwrap();
    assert_eq!(be.conflict_files().len(), 1);
}

#[test]
fn git_sync_between_two_machines() {
    if std::process::Command::new("git").arg("--version").output().is_err() {
        return;
    }
    for (k, v) in [("GIT_AUTHOR_NAME", "t"), ("GIT_AUTHOR_EMAIL", "t@example.com"), ("GIT_COMMITTER_NAME", "t"), ("GIT_COMMITTER_EMAIL", "t@example.com")] {
        std::env::set_var(k, v);
    }
    let (remote, a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    assert!(std::process::Command::new("git").args(["init", "-q", "--bare", remote.path().to_str().unwrap()]).status().unwrap().success());
    let url = remote.path().to_str().unwrap();
    let real = |d: &Path, id: &str| {
        let w = World::new(d, id, false);
        World { ctx: w.ctx.with_runner(Arc::new(karry_core::RealRunner), Some(false)), rec: w.rec }
    };
    let (wa, wb) = (real(a.path(), "aaaa"), real(b.path(), "bbbb"));
    let run = |w: &World| {
        let cfg = sync::load_config(&w.ctx).unwrap().unwrap();
        sync::sync(&w.ctx, &Store::create(&cfg.path).unwrap(), sync::backend(&cfg).as_ref(), &SyncOptions::default()).unwrap()
    };
    wa.put("kwinrc", "[W]\nx=1\ny=1\n");
    sync::init_git(&wa.ctx, url).unwrap();
    assert!(run(&wa).pushed);
    sync::init_git(&wb.ctx, url).unwrap();
    wb.put("kwinrc", "[W]\nx=1\ny=1\n");
    run(&wb);
    wa.put("kwinrc", "[W]\nx=2\ny=1\n");
    wb.put("kwinrc", "[W]\nx=1\ny=2\n");
    run(&wa);
    let r = run(&wb);
    assert!(r.conflicts.is_empty(), "{:?}", r.conflicts);
    assert_eq!(wb.read("kwinrc"), "[W]\nx=2\ny=2\n");
}

#[test]
fn catalog_and_packages_load() {
    let c = karry_core::catalog::Catalog::embedded().unwrap();
    assert!(c.find("config/kwinrc").is_some() && c.find("config/somethingrc").is_some() && c.find("home/.bashrc").is_none());
    assert!(karry_core::plugins::Packages::embedded().unwrap().family_of("cachyos", &["arch".into()]).as_deref() == Some("arch"));
}
