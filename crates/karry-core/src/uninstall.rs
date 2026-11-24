//! Remove Karry's own files. Never touches KDE settings, restored files, the sync folder or the git clone.
use crate::{scan, Ctx, Result};
use std::path::PathBuf;

/// What `uninstall` would remove (only paths that exist).
pub fn targets(ctx: &Ctx, keep_snapshots: bool) -> Vec<PathBuf> {
    let r = &ctx.roots;
    let mut v = vec![r.config.join("karry"), r.state_karry("base"), r.state_karry("pending"), r.config.join(scan::PENDING_SCRIPT)];
    if !keep_snapshots {
        v.push(r.state_karry("snapshots"));
    }
    v.extend(["applications/org.kde.karry.desktop", "icons/hicolor/scalable/apps/org.kde.karry.svg"].map(|p| r.data.join(p)));
    v.push(r.config.join("autostart/org.kde.karry.desktop"));
    v.retain(|p| p.symlink_metadata().is_ok());
    v
}

pub fn uninstall(ctx: &Ctx, keep_snapshots: bool) -> Result<Vec<PathBuf>> {
    let t = targets(ctx, keep_snapshots);
    for p in &t {
        crate::io(if p.is_dir() { std::fs::remove_dir_all(p) } else { std::fs::remove_file(p) }, p)?;
    }
    let _ = std::fs::remove_dir(ctx.roots.karry_state()); // only if nothing else is left in it
    Ok(t)
}
