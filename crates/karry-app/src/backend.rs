//! The one QObject the QML talks to. Data goes out as JSON strings; long jobs run on a thread.
#[cxx_qt::bridge]
pub mod qobject {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
    }

    extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, busy)]
        #[qproperty(bool, configured)]
        #[qproperty(QString, error)]
        #[qproperty(QString, status)]
        #[qproperty(QString, snapshots)]
        #[qproperty(QString, issues)]
        #[qproperty(QString, preview)]
        type Backend = super::BackendRust;

        #[qsignal]
        fn finished(self: Pin<&mut Backend>, task: &QString, result: &QString);

        #[qinvokable]
        #[cxx_name = "debugPage"]
        fn debug_page(self: &Backend) -> QString;
        #[qinvokable]
        fn refresh(self: Pin<&mut Backend>);
        #[qinvokable]
        #[cxx_name = "initFolder"]
        fn init_folder(self: Pin<&mut Backend>, path: &QString);
        #[qinvokable]
        #[cxx_name = "initGit"]
        fn init_git(self: Pin<&mut Backend>, url: &QString);
        #[qinvokable]
        fn backup(self: Pin<&mut Backend>, only: &QString);
        #[qinvokable]
        #[cxx_name = "previewRestore"]
        fn preview_restore(self: Pin<&mut Backend>);
        #[qinvokable]
        #[cxx_name = "previewSync"]
        fn preview_sync(self: Pin<&mut Backend>);
        #[qinvokable]
        #[cxx_name = "applyRestore"]
        fn apply_restore(self: Pin<&mut Backend>);
        #[qinvokable]
        #[cxx_name = "applySync"]
        fn apply_sync(self: Pin<&mut Backend>, remote_wins: &QString);
        #[qinvokable]
        fn fix(self: Pin<&mut Backend>, issue: &QString, fix: &QString, input: &QString);
        #[qinvokable]
        fn undo(self: Pin<&mut Backend>, id: &QString);
    }

    impl cxx_qt::Threading for Backend {}
}

use core::pin::Pin;
use cxx_qt::Threading;
use cxx_qt_lib::QString;
use karry_core::apply::{self, Plan};
use karry_core::store::{self, Part, Store};
use karry_core::sync::{self, SyncOptions};
use karry_core::{scan, shortcuts, Ctx, Error, Roots};
use serde_json::{json, Value};
use std::path::Path;

pub struct BackendRust {
    busy: bool,
    configured: bool,
    error: QString,
    status: QString,
    snapshots: QString,
    issues: QString,
    preview: QString,
}
impl Default for BackendRust {
    fn default() -> Self {
        let q = QString::default;
        Self {
            busy: false,
            configured: Roots::from_env().config.join("karry/karry.toml").exists(),
            error: q(),
            status: q(),
            snapshots: q(),
            issues: q(),
            preview: q(),
        }
    }
}

/// Property updates plus the text handed to the `finished` signal.
type Out = (Vec<(&'static str, String)>, String);

fn js(v: Value) -> String {
    v.to_string()
}
fn cfg(ctx: &Ctx) -> Result<sync::Config, Error> {
    sync::load_config(ctx)?.ok_or_else(|| Error::Msg("Karry is not set up yet".into()))
}
fn refreshed(ctx: &Ctx, extra: Value) -> Result<Out, Error> {
    let config = sync::load_config(ctx)?;
    let store = config.as_ref().and_then(|c| Store::open(&c.path).ok());
    let mut status = serde_json::to_value(scan::status(ctx, store.as_ref())?).unwrap_or_default();
    status["config"] = json!(config);
    let me = status["machine"].clone();
    let host = me["hostname"].as_str().unwrap_or_default();
    let pretty = ctx.run("hostnamectl --pretty").map(|o| o.stdout.trim().to_string()).filter(|s| !s.is_empty());
    let hashy = host.len() >= 12 && host.chars().all(|c| c.is_ascii_hexdigit()) || host == "unknown";
    status["machine"]["hostname"] = json!(pretty.unwrap_or_else(|| if hashy { "This computer".into() } else { host.into() }));
    if let Some(m) = store.as_ref().and_then(|s| s.manifest(&Part::Shared).ok().flatten()) {
        status["synced"] = json!(m.last_sync.clone().unwrap_or(m.created.clone()));
        if m.machine.id != me["id"].as_str().unwrap_or_default() {
            status["found"] = json!(m.machine);
        }
    }
    let ups = vec![
        ("configured", (config.is_some() as u8).to_string()),
        ("status", js(status)),
        ("snapshots", js(json!(store::snapshots(ctx)))),
        ("issues", js(json!(shortcuts::diagnose_system(ctx)))),
    ];
    Ok((ups, js(extra)))
}
fn previewed(kind: &str, mut v: Value, plan: &Plan) -> Result<Out, Error> {
    v["kind"] = json!(kind);
    v["plan"] = json!(plan);
    Ok((vec![("preview", js(v))], String::new()))
}
fn only_list(s: &str) -> Option<Vec<String>> {
    serde_json::from_str::<Vec<String>>(s).ok().filter(|v| !v.is_empty())
}

impl qobject::Backend {
    /// Run `f` on a worker thread, then apply its property updates and emit `finished` on the Qt thread.
    fn job<F>(mut self: Pin<&mut Self>, task: &'static str, f: F)
    where
        F: FnOnce(&Ctx) -> Result<Out, Error> + Send + 'static,
    {
        if *self.busy() {
            return;
        }
        self.as_mut().set_busy(true);
        self.as_mut().set_error(QString::default());
        let qt = self.qt_thread();
        std::thread::spawn(move || {
            let out = Ctx::new(Roots::from_env()).and_then(|c| f(&c)).map_err(|e| e.to_string());
            let _ = qt.queue(move |mut q| {
                let mut result = String::new();
                match out {
                    Ok((ups, res)) => {
                        result = res;
                        for (k, v) in ups {
                            let s = QString::from(&v);
                            match k {
                                "configured" => q.as_mut().set_configured(v == "1"),
                                "status" => q.as_mut().set_status(s),
                                "snapshots" => q.as_mut().set_snapshots(s),
                                "issues" => q.as_mut().set_issues(s),
                                _ => q.as_mut().set_preview(s),
                            }
                        }
                    }
                    Err(e) => q.as_mut().set_error(QString::from(&e)),
                }
                q.as_mut().set_busy(false);
                q.as_mut().finished(&QString::from(task), &QString::from(&result));
            });
        });
    }

    pub fn debug_page(&self) -> QString {
        QString::from(&std::env::var("KARRY_PAGE").unwrap_or_default())
    }
    pub fn refresh(self: Pin<&mut Self>) {
        self.job("refresh", |c| refreshed(c, json!({})));
    }
    pub fn init_folder(self: Pin<&mut Self>, path: &QString) {
        let p = path.to_string();
        self.job("init", move |c| {
            sync::init_folder(c, Path::new(&p))?;
            refreshed(c, json!({}))
        });
    }
    pub fn init_git(self: Pin<&mut Self>, url: &QString) {
        let u = url.to_string();
        self.job("init", move |c| {
            sync::init_git(c, &u)?;
            refreshed(c, json!({}))
        });
    }
    pub fn backup(self: Pin<&mut Self>, only: &QString) {
        let only = only_list(&only.to_string());
        self.job("backup", move |c| {
            let r = store::backup(c, &Store::create(&cfg(c)?.path)?, only.as_deref(), false)?;
            refreshed(c, json!(r))
        });
    }
    pub fn preview_restore(self: Pin<&mut Self>) {
        self.job("preview", |c| {
            let inc = store::load(c, &Store::open(&cfg(c)?.path)?, None)?;
            let r = apply::apply(c, &inc, "restore", false, true)?;
            previewed("restore", json!({ "conflicts": [], "incoming": [] }), &r.plan)
        });
    }
    pub fn preview_sync(self: Pin<&mut Self>) {
        self.job("preview", |c| {
            let g = cfg(c)?;
            let r = sync::sync(c, &Store::create(&g.path)?, sync::backend(&g).as_ref(), &SyncOptions { dry_run: true, ..Default::default() })?;
            previewed("sync", json!({ "conflicts": r.conflicts, "incoming": r.incoming, "warnings": r.warnings }), &r.plan)
        });
    }
    pub fn apply_restore(self: Pin<&mut Self>) {
        self.job("restore", |c| {
            let inc = store::load(c, &Store::open(&cfg(c)?.path)?, None)?;
            let r = apply::apply(c, &inc, "restore", false, false)?;
            refreshed(c, json!({ "logout": r.plan.needs_logout, "warnings": r.warnings }))
        });
    }
    pub fn apply_sync(self: Pin<&mut Self>, remote_wins: &QString) {
        let wins: Vec<String> = serde_json::from_str(&remote_wins.to_string()).unwrap_or_default();
        self.job("sync", move |c| {
            let g = cfg(c)?;
            let r = sync::sync(c, &Store::create(&g.path)?, sync::backend(&g).as_ref(), &SyncOptions { remote_wins: wins, ..Default::default() })?;
            refreshed(c, json!({ "logout": r.plan.needs_logout, "pushed": r.pushed, "published": r.published.len() }))
        });
    }
    pub fn fix(self: Pin<&mut Self>, issue: &QString, fix: &QString, input: &QString) {
        let (issue, fix, input) = (issue.to_string(), fix.to_string(), input.to_string());
        self.job("fix", move |c| {
            let r = shortcuts::fix_system(c, &issue, &fix, Some(input.as_str()).filter(|s| !s.is_empty()))?;
            refreshed(c, json!({ "logout": r.plan.needs_logout }))
        });
    }
    pub fn undo(self: Pin<&mut Self>, id: &QString) {
        let id = id.to_string();
        self.job("undo", move |c| {
            let s = store::undo(c, Some(&id).filter(|s| !s.is_empty()).map(|s| s.as_str()))?;
            refreshed(c, json!({ "label": s.label }))
        });
    }
}
