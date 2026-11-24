use karry_core::apply::{self, What};
use karry_core::store::{self, Store};
use karry_core::sync::{self, SyncOptions};
use karry_core::{scan, shortcuts, Ctx, Roots};
use std::path::Path;

const USAGE: &str = "usage: karry-cli [--json] <command>
  status | doctor | snapshots | undo [id]
  backup <dir> [--force] | export <file.karry> | restore <dir|file.karry> [--dry-run] [--only cat,..]
  sync [--dry-run] | init --folder <dir> | --git <url>
  uninstall [--keep-snapshots] [--yes]";

type R = Result<(), Box<dyn std::error::Error>>;

fn opt(a: &[String], name: &str) -> Option<String> {
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1)).cloned()
}

fn show<T: serde::Serialize>(json: bool, v: &T, text: impl FnOnce() -> String) {
    if json { println!("{}", serde_json::to_string_pretty(v).unwrap()) } else { println!("{}", text().trim_end()) }
}

fn plan_text(p: &apply::Plan) -> String {
    let mut s = String::new();
    for c in &p.changes {
        s += &match &c.what {
            What::SetKey { group, key, value, .. } => format!("set      {} {group} {key}={value}\n", c.store_path),
            What::DeleteKey { group, key, .. } => format!("delete   {} {group} {key}\n", c.store_path),
            What::WriteFile { existing, .. } => format!("{}  {}\n", if *existing { "replace " } else { "create  " }, c.store_path),
            What::InstallHint { id, command, .. } => format!("install  {id}: {command}\n"),
            What::NeedsLogout { reason } => format!("logout   {}: {reason}\n", c.store_path),
        };
    }
    for w in &p.warnings {
        s += &format!("warning: {w}\n");
    }
    for i in &p.shortcut_issues {
        s += &format!("shortcut: {}\n", i.message);
    }
    if s.is_empty() { "nothing to change\n".into() } else { s }
}

fn run() -> R {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let json = args.iter().any(|a| a == "--json");
    args.retain(|a| a != "--json");
    let Some(cmd) = args.first().cloned() else {
        println!("{USAGE}");
        return Ok(());
    };
    let dry = args.iter().any(|a| a == "--dry-run");
    let ctx = Ctx::new(Roots::from_env())?;
    let config = || sync::load_config(&ctx).and_then(|c| c.ok_or_else(|| karry_core::Error::Msg("not set up: run `karry-cli init`".into())));
    match cmd.as_str() {
        "status" => {
            let cfg = sync::load_config(&ctx)?;
            let store = cfg.and_then(|c| Store::open(&c.path).ok());
            let s = scan::status(&ctx, store.as_ref())?;
            show(json, &s, || {
                let mut t = format!("{} ({}), plasma {}\n", s.machine.hostname, s.machine.distro, s.machine.plasma.clone().unwrap_or("?".into()));
                for c in &s.categories {
                    t += &format!("{:<28} {:>4} files {:>4} changed {:>3} issues{}\n", c.name, c.items, c.changed, c.issues, if c.machine_only { "  (this machine only)" } else { "" });
                }
                if let Some(sn) = &s.last_snapshot {
                    t += &format!("last snapshot: {} ({})\n", sn.id, sn.label);
                }
                t
            });
        }
        "doctor" => {
            let v = shortcuts::diagnose_system(&ctx);
            show(json, &v, || if v.is_empty() { "no shortcut problems".into() } else { v.iter().map(|i| format!("{:?}: {}\n", i.kind, i.message)).collect() });
        }
        "snapshots" => {
            let v = store::snapshots(&ctx);
            show(json, &v, || v.iter().map(|s| format!("{}  {}  {} files\n", s.id, s.label, s.files.len())).collect());
        }
        "undo" => {
            let s = store::undo(&ctx, args.get(1).map(|s| s.as_str()))?;
            show(json, &s, || format!("restored snapshot {} ({} files)", s.id, s.files.len()));
        }
        "backup" => {
            let dir = args.get(1).filter(|a| !a.starts_with("--")).map(|d| Path::new(d).to_path_buf());
            let only: Option<Vec<String>> = opt(&args, "--only").map(|s| s.split(',').map(String::from).collect());
            let r = store::backup(&ctx, &Store::create(&match dir { Some(d) => d, None => config()?.path })?, only.as_deref(), args.iter().any(|a| a == "--force"))?;
            show(json, &r, || format!("backed up {} shared and {} machine files, {} plugin records; {} secret/unsafe items skipped", r.shared, r.machine, r.plugins, r.skipped.len()));
        }
        "export" => {
            let (f, c) = (args.get(1).ok_or(USAGE)?, config()?);
            store::export(&Store::open(&c.path)?, Path::new(f))?;
            show(json, &serde_json::json!({ "file": f }), || format!("saved {f}"));
        }
        "restore" => {
            let dir = args.get(1).filter(|a| !a.starts_with("--"));
            let only: Option<Vec<String>> = opt(&args, "--only").map(|s| s.split(',').map(String::from).collect());
            let st = match dir {
                Some(d) if d.ends_with(".karry") => store::import(Path::new(d))?,
                Some(d) => Store::open(Path::new(d))?,
                None => Store::open(&config()?.path)?,
            };
            let inc = store::load(&ctx, &st, only.as_deref())?;
            let r = apply::apply(&ctx, &inc, "restore", false, dry)?;
            show(json, &r, || {
                let mut t = plan_text(&r.plan);
                if let Some(s) = &r.snapshot {
                    t += &format!("snapshot {} taken; undo with `karry-cli undo`\n", s.id);
                }
                if r.plan.needs_logout {
                    t += "Log out to finish.\n";
                }
                t
            });
        }
        "sync" => {
            let cfg = config()?;
            let r = sync::sync(&ctx, &Store::create(&cfg.path)?, sync::backend(&cfg).as_ref(), &SyncOptions { dry_run: dry, remote_wins: opt(&args, "--remote-wins").map(|j| serde_json::from_str(&j)).transpose()?.unwrap_or_default(), ..Default::default() })?;
            show(json, &r, || {
                let mut t: String = r.incoming.iter().map(|i| format!("{}\n", i.summary)).collect();
                t += &plan_text(&r.plan);
                t += &format!("published {} files; {} conflicts{}\n", r.published.len(), r.conflicts.len(), if r.pushed { "; pushed" } else { "" });
                for c in &r.conflicts {
                    t += &format!("conflict: {} {} {} (local {:?}, remote {:?})\n", c.store_path, c.group, c.key, c.local, c.remote);
                }
                t
            });
        }
        "init" => {
            let c = if args.iter().any(|a| a == "--local") { sync::init_folder(&ctx, &ctx.roots.home.join("Karry"))? } else if let Some(d) = opt(&args, "--folder") { sync::init_folder(&ctx, Path::new(&d))? } else if let Some(u) = opt(&args, "--git") { sync::init_git(&ctx, &u)? } else { return Err(USAGE.into()) };
            show(json, &c, || format!("saved {}", sync::config_path(&ctx).display()));
        }
        "info" => {
            let cfg = sync::load_config(&ctx)?;
            let source = cfg.as_ref().and_then(|c| Store::open(&c.path).ok()).and_then(|s| s.manifest(&store::Part::Shared).ok().flatten()).map(|m| m.machine).filter(|m| m.id != karry_core::plugins::MachineInfo::detect(&ctx).id);
            let v = serde_json::json!({ "machine": karry_core::plugins::MachineInfo::detect(&ctx), "config": cfg, "local_only": cfg.as_ref().is_some_and(|c| c.kind == "folder" && c.path == ctx.roots.home.join("Karry")), "source": source });
            show(json, &v, || format!("{}\n", v));
        }
        "fix" => {
            let (i, f) = (args.get(1).ok_or(USAGE)?, args.get(2).ok_or(USAGE)?);
            let r = shortcuts::fix_system(&ctx, i, f, args.get(3).map(|s| s.as_str()))?;
            show(json, &r, || format!("fixed; log out to finish: {}\n", r.plan.needs_logout));
        }
        "uninstall" => {
            let keep = args.iter().any(|a| a == "--keep-snapshots");
            let t = if args.iter().any(|a| a == "--yes") { karry_core::uninstall::uninstall(&ctx, keep)? } else { karry_core::uninstall::targets(&ctx, keep) };
            show(json, &t, || {
                let done = args.iter().any(|a| a == "--yes");
                let mut s: String = t.iter().map(|p| format!("{} {}\n", if done { "removed" } else { "would remove" }, p.display())).collect();
                s += if done { "Your KDE settings and the sync folder were not touched.\n" } else { "Nothing removed. Run again with --yes. Your KDE settings and the sync folder are never touched.\n" };
                s
            });
        }
        _ => println!("{USAGE}"),
    }
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        if std::env::args().any(|a| a == "--json") {
            println!("{}", serde_json::json!({ "error": e.to_string() }));
        } else {
            eprintln!("karry-cli: {e}");
        }
        std::process::exit(1);
    }
}
