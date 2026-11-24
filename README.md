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

Any distro: download `Karry-x86_64.AppImage` from the [latest release](https://github.com/SeniorMatt/karry/releases/latest), `chmod +x` it and run it.

From source (needs Rust, CMake, Qt 6 and KDE Frameworks 6):

```sh
cargo build --release -p karry-cli
cmake -B build -G Ninja && cmake --build build
```

## Usage

Open Karry. It works out of the box on this computer (backups go to `~/Karry`, no account needed).
To sync with other computers, point it at a folder that Syncthing or Nextcloud already syncs, or at a git repo.
You can also export your whole setup to a single `.karry` file and restore it anywhere.

From a terminal:

```sh
karry-cli backup ~/karry-backup
karry-cli restore ~/karry-backup --dry-run
karry-cli sync
karry-cli undo
```

## License

GPL-3.0-or-later
