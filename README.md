# Karry

Back up, restore and sync your KDE Plasma setup between machines.

Karry saves your themes, panels, widgets, shortcuts, KWin settings and KDE app configs. You can restore
them on another machine, even if it runs a different distro. It won't break the machine you restore to:

- your monitor layout stays on each machine
- passwords, wallets and accounts are never copied
- it takes a snapshot before every restore, so you can undo it
- it checks your shortcuts for clashes and for apps that aren't installed
- for themes or plugins that come from distro packages, it tells you what to install

## Install

Arch / CachyOS:

```sh
cd packaging/arch && makepkg -si
```

From source (needs Rust, CMake, Qt 6 and KDE Frameworks 6):

```sh
cargo build --release -p karry-cli
cmake -B build -G Ninja && cmake --build build
```

## Usage

Open Karry and pick a folder that Syncthing, Nextcloud or a similar tool already syncs, or a git repo.
That's it.

From a terminal:

```sh
karry-cli backup ~/karry-backup
karry-cli restore ~/karry-backup --dry-run
karry-cli sync
karry-cli undo
```

## License

GPL-3.0-or-later
