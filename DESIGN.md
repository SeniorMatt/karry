# Karry — design

Karry backs up, restores and syncs a whole KDE Plasma 6 setup across machines and distros.
KDE only. Rust (cxx-qt) + Kirigami/QML, KClock-style UI.

## Why Rust + cxx-qt
KDE documents cxx-qt for Kirigami apps; the logic (parsing, merging, diffing, sync) is pure Rust and
unit-testable without Qt. Qt/KDE APIs we need are reached through CLI tools and D-Bus
(kwriteconfig6, qdbus6, plasma-apply-*), so no C++ bindings are required.

## Layout
```
crates/karry-core   pure Rust library, all logic, no Qt. Heavily tested.
crates/karry-cli    `karry-cli` binary: backup / restore / sync / doctor / undo / status (headless + test harness)
crates/karry-app    cxx-qt bridge + QML (Kirigami) app, binary `karry`
data/catalog.toml   what to back up (data-driven, see comments in the file)
data/packages.toml  distro families, package-owner queries, cross-distro package names
packaging/arch/PKGBUILD
```
Data files are embedded with `include_str!` and can be overridden by `~/.config/karry/catalog.d/*.toml`
(entries appended) for user extensions.

## karry-core modules
- `paths`    resolve `config:`/`data:`/`state:`/`home:` roots from XDG env; globbing; relative "store paths".
- `catalog`  load + validate catalog (no duplicate paths, known categories/kinds/scopes, regexes compile).
- `kconfig`  lossless KConfig parser/writer: nested groups `[A][B]`, `key[locale]`, `$e`/`$i` flags,
             comments, `[$i]` immutables, escapes. Round-trip byte-identical for unchanged files.
             Key-level diff and 3-way merge (base/local/remote) with conflict list.
- `scan`     walk catalog -> `Inventory` of items {store_path, abs_path, category, scope, kind, sha256, size}.
             Applies `machine_groups` (split into shared/machine part), `secret_keys` (dropped),
             `secret_patterns` (file skipped). Never reads `secret` scope entries.
- `plugins`  for `apply=package` entries: list plugin ids (dir names), user-vs-system, read
             metadata.json (KPlugin.Id, Name, Version, X-KDE-…); find plugins *referenced* by configs
             (appletsrc `plugin=`, kwinrc `[Plugins] xEnabled=true`, kdeglobals/plasmarc theme names,
             kcminputrc cursorTheme, …) and resolve them: user-installed -> copied; system -> owning package via
             `packages.toml` family `owner` command; missing on target -> "install X with <cmd>".
- `distro`   /etc/os-release ID / ID_LIKE -> family; Plasma version (`plasmashell --version`), KF version.
- `store`    on-disk format of a backup:
             ```
             <root>/karry.json                 format version
             <root>/shared/files/<store_path>  shared files (kconfig files hold only shared groups)
             <root>/shared/manifest.json       items + hashes + plugin/package records + source machine
             <root>/machines/<machine-id>/files/...     machine-scope files + machine groups
             <root>/machines/<machine-id>/manifest.json hostname, distro, plasma version, last sync time
             ```
             machine-id = /etc/machine-id (hashed, never stored raw) + friendly hostname.
- `snapshot` before any restore/sync-apply: copy every file that will change into
             `$XDG_STATE_HOME/karry/snapshots/<ts>/` + manifest. `undo(snapshot)` restores via the
             same safe applier. Keep the last 20.
- `validate` before writing: kconfig parses, JSON parses, size sane, no secret pattern, target parent
             writable, store path cannot escape its root (no `..`, no symlink escape), Plasma major version
             compatible (warn on 5->6).
- `apply`    builds a `Plan` (list of `Change`: SetKey/DeleteKey/WriteFile/InstallHint/NeedsLogout) and
             executes it with a `Runner` trait (real: spawns processes; test: records calls).
             Strategies from catalog `apply`:
             kwrite      -> `kwriteconfig6 --file <abs> --group A --group B --key k <v>` (or --delete), then reload
             copy        -> atomic write (tmp + fsync + rename)
             plasmashell -> stop `plasma-plasmashell.service` (fallback `kquitapp6 plasmashell`), atomic write, start
             logout      -> stage into `$XDG_STATE_HOME/karry/pending/` + autostart-phase script
                            `~/.config/plasma-workspace/env/karry-pending.sh` that applies before Plasma starts and
                            removes itself; UI says "Log out to finish".
             package     -> copy user plugin dirs; system ones become InstallHint.
             Shortcut changes: `kglobalshortcutsrc` uses `logout` by default; if kglobalaccel D-Bus is present,
             live-apply single bindings via `org.kde.KGlobalAccel` (`setForeignShortcutKeys`).
             If Plasma is not running (TTY / tests), everything is a plain atomic write.
- `shortcuts` the shortcut doctor (see below).
- `sync`     `Backend` trait: `FolderBackend` (any synced folder; detects Syncthing `*.sync-conflict-*`,
             Nextcloud `(conflicted copy)`, Dropbox `(… conflicted copy …)`) and `GitBackend` (shells out to git:
             pull --rebase, commit per machine "karry: <host> <ts>", push; conflicts -> our 3-way merge).
             Sync = publish local shared state into `shared/` with key-level 3-way merge against the
             per-machine base copy at `$XDG_STATE_HOME/karry/base/`; incoming changes are shown as a
             per-machine diff ("thinkpad changed 3 keys in kwinrc") and applied through `apply`.
             Machine scope never leaves `machines/<id>/`.
- `status`   overview for the UI: per category {items, changed since last backup, issues}.

## Shortcut doctor
Input: kglobalshortcutsrc (value format `active\tactive2,default,Friendly name`, `none` = unbound) plus the
target's installed components:
- duplicate binding: same key sequence on two actions (after normalising `Meta+Shift+A` == `Shift+Meta+A`)
- orphan app: `[services][<app>.desktop]` or component whose .desktop file is not installed
  (search XDG_DATA_DIRS/applications and ~/.local/share/applications)
- orphan KWin action: a `kwin` action from a KWin script/effect that is not installed/enabled
- shadowed: binding equal to a KWin/plasmashell default of another action
Fixes offered per issue: keep A / keep B (unbind the other), reset to default, remove orphan, rebind
(UI key recorder). Doctor runs on restore and on incoming sync, results shown before applying.

## Autostart check
`.desktop` in autostart whose `Exec`/`TryExec` binary is not on PATH on the target -> warn, copy with
`Hidden=true` unless user opts in.

## UI (Kirigami, KClock style)
Pages: Overview (cards per category with state chips, big "Back up now" / "Sync" buttons, last snapshot +
Undo), Shortcuts (doctor issues list with fix buttons), Sync (machines list, incoming changes per machine,
conflicts with side-by-side values), Snapshots (timeline, one-click undo), Settings.
First run wizard: Welcome -> where to keep it (synced folder | git remote, equal choice) -> what to include
(categories, displays shown as locked "this machine only") -> first backup with progress -> done.
Subtle animations: Kirigami.Units.longDuration fades/slides, number tickers, card hover lift.

## Never break anything (rules)
1. Secrets never read. 2. Machine files never synced. 3. Snapshot before every write, one-click undo.
4. Validate before write. 5. Never overwrite a file a running owner will write back (use kwrite /
restart owner / stage for logout). 6. Dry-run plan always available and shown before applying.
