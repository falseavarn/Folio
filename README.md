# Folio

A desktop app for collecting research assets (links, images and notes) and tagging each one with the research projects it belongs to. Part of the FalseAvarn suite. Identity color: moss `#8BC34A`.

## Install

Download a package from [Releases](https://github.com/falseavarn/Folio/releases). Folio needs a 64-bit Linux with glibc 2.39 or newer (Fedora 40+, Ubuntu 24.04+, Debian 13+).

- **Fedora, openSUSE:** `sudo dnf install ./folio-*.x86_64.rpm`
- **Ubuntu, Debian:** `sudo apt install ./folio_*_amd64.deb`
- **Any distribution:** `chmod +x Folio-*-x86_64.AppImage` and run it.

Then run `folio --install` (or `./Folio-*.AppImage --install`) once to add **Scripts ▸ Add to Folio** to the Files right-click menu.

To check a download, run `sha256sum -c SHA256SUMS --ignore-missing` in the download folder, or verify that GitHub built it from this repository with `gh attestation verify <file> -R falseavarn/Folio`.

## Run from source

Requires a Rust toolchain (stable, edition 2024).

```sh
cargo run --release
cargo test
```

## Where your data lives

Everything stays on your machine.

- **Each project has its own folder.** You choose it when you create the project (the default is `~/Folio/<project name>`). Folio keeps its files in a hidden `.folio` folder inside it: `.folio/project.json` (the project's name, links, notes and descriptions) and a copy of each of its images. The project folder can be backed up, moved or shared on its own, and anything else you keep there is left alone.
- **An asset in several projects is saved in every one of their folders.** Folio keeps the copies in step when you edit.
- **`~/Folio/.folio/projects.json` lists your project folders.** Set `FOLIO_DIR` to keep it somewhere else. Creating a project in a folder that already holds `.folio/project.json` adds that project to your list.

Files are written to a temporary file first and then renamed, so a crash during a save does not corrupt them. If a project folder is missing or unreadable at startup, Folio shows a warning, leaves that folder untouched and keeps it in the list.

## Running in the background

On Linux, Folio adds a tray icon with Open Folio, Add link…, Add note… and Quit. Closing the window keeps Folio running in the tray; Quit stops it. GNOME needs the AppIndicator extension to show tray icons. Launching Folio again, or adding an image from Files, goes to the copy that is already running.

## Add images from Files

```sh
cargo build --release
./target/release/folio --install
```

This adds Folio to the app menu, lists it under Open With for images, and adds **Scripts ▸ Add to Folio** to the right-click menu in Files (Nautilus). Choosing it opens a small window to add a description and pick projects. The entries point at the binary you ran, so run `--install` again if you move it, and `folio --uninstall` to remove them.

## Use your research with AI agents

- **Export for AI…** on a project saves a Markdown file (by default `<project name>.md` in the project folder) listing every link, image path, note and description. Attach it or paste it into a chat to give an assistant the project's context.
- **[skills/folio-research](skills/folio-research/SKILL.md)** is an [Agent Skill](https://agentskills.io/specification) that teaches coding agents (Claude Code, Codex, Copilot and others) to find and read Folio projects directly, read-only. Copy the `folio-research` folder into your agent's skills folder, for example `~/.claude/skills/` for Claude Code or `.agents/skills/` in a repository.

## Behavior

- Every asset needs at least one project. An asset can belong to several.
- Deleting a project removes its `.folio` folder (project file and image copies), and deletes the assets that were tagged only with it. Anything else you put in the folder stays. Folio asks before deleting.
- Links must start with `http://` or `https://`. They open in your browser.

## Stack

Rust with [egui/eframe](https://github.com/emilk/egui) 0.37, `egui_extras` for image loading (PNG, JPEG, GIF, WebP), [`rfd`](https://github.com/PolyMeilex/rfd) for the native file picker, `serde_json` for storage, and [`tray-icon`](https://github.com/tauri-apps/tray-icon) (D-Bus StatusNotifierItem backend) for the Linux tray.

## Releasing

1. Set the new version in `Cargo.toml` and commit it.
2. Tag the commit with the same version and push the tag: `git tag v0.2.0 && git push origin v0.2.0`.

The [Release workflow](.github/workflows/release.yml) runs only for tags of the form `vMAJOR.MINOR.PATCH`. It checks that the tag matches `Cargo.toml`, runs the tests, builds the `.deb`, `.rpm` and AppImage with [packaging/build.sh](packaging/build.sh), attests them, and publishes them with a `SHA256SUMS` file as a GitHub release. To build the packages locally, install `cargo-deb`, `cargo-generate-rpm` and `appimagetool`, then run `packaging/build.sh`.

## License

[MIT](LICENSE)
