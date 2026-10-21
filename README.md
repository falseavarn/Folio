# Folio

A desktop app for collecting research assets (links, images and notes) and tagging each one with the research projects it belongs to. Part of the FalseAvarn suite. Identity color: moss `#8BC34A`.

## Run

Requires a Rust toolchain (stable, edition 2024).

```sh
cargo run --release
cargo test
```

## Where your data lives

Everything stays on your machine, in `~/Folio` (set `FOLIO_DIR` to use another folder):

- `library.json` holds the projects and asset records (link URLs, note text, descriptions, project tags).
- `files/` holds copies of the images you add. The original file is not moved or changed.

The library is written to a temporary file first and then renamed, so a crash during a save does not corrupt it. If `library.json` cannot be read, Folio refuses to start rather than replace it with an empty library.

## Behavior

- Every asset needs at least one project. An asset can belong to several.
- Deleting a project also deletes the assets that were tagged only with that project, and their image files. Assets that belong to other projects too are kept. Folio asks before deleting.
- Links must start with `http://` or `https://`. They open in your browser.

## Stack

Rust with [egui/eframe](https://github.com/emilk/egui) 0.37, `egui_extras` for image loading (PNG, JPEG, GIF, WebP), [`rfd`](https://github.com/PolyMeilex/rfd) for the native file picker, and `serde_json` for storage.
