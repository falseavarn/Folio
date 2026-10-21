//! `folio --install` adds Folio to the app menu and to the Files (Nautilus) right-click menu
//! under Scripts ▸ Add to Folio, and lists it in "Open With" for images. `folio --uninstall`
//! removes those files. Both point at the binary that ran them, so run them from where Folio stays.

use std::{fs, io, path::PathBuf};

const MIME_TYPES: &str = "image/png;image/jpeg;image/gif;image/webp;";

fn targets() -> (PathBuf, PathBuf, PathBuf) {
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    (
        data.join("applications/folio.desktop"),
        data.join("icons/hicolor/256x256/apps/folio.png"),
        data.join("nautilus/scripts/Add to Folio"),
    )
}

#[cfg(target_os = "linux")]
pub fn run(install: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let (desktop, icon, script) = targets();
    if !install {
        for path in [&desktop, &icon, &script] {
            match fs::remove_file(path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => println!("Removed {}", path.display()),
            }
        }
        return Ok(());
    }

    let exe = std::env::current_exe()?;
    let exe = exe
        .to_str()
        .ok_or_else(|| io::Error::other("Folio's path is not valid UTF-8"))?;
    // The path is quoted in both files; refuse characters those quotes would not protect.
    if exe.contains(['"', '`', '$', '\\', '\n']) {
        return Err(io::Error::other(format!(
            "move Folio to a path without quotes, $, ` or \\ first: {exe}"
        )));
    }
    for path in [&desktop, &icon, &script] {
        fs::create_dir_all(path.parent().unwrap_or(path))?;
    }
    fs::write(
        &desktop,
        format!(
            "[Desktop Entry]\nType=Application\nName=Folio\nComment=Classify research links, images and notes by project\n\
             Exec=\"{exe}\" --add %F\nIcon=folio\nTerminal=false\nCategories=Office;\nMimeType={MIME_TYPES}\n\
             StartupWMClass=folio\n"
        ),
    )?;
    fs::write(&icon, include_bytes!("../icons/128x128@2x.png"))?;
    // Nautilus passes the selection as newline-separated absolute paths; split on newlines only, no globbing.
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nset -f\nIFS='\n'\nexec \"{exe}\" --add $NAUTILUS_SCRIPT_SELECTED_FILE_PATHS\n"
        ),
    )?;
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755))?;
    for path in [&desktop, &icon, &script] {
        println!("Wrote {}", path.display());
    }
    println!("In Files, right-click an image and choose Scripts ▸ Add to Folio.");
    Ok(())
}

// shortcut: desktop integration is Linux-only; add Windows registry verbs and a macOS Service when those builds ship.
#[cfg(not(target_os = "linux"))]
pub fn run(_: bool) -> io::Result<()> {
    let _ = (targets, MIME_TYPES);
    Err(io::Error::other(
        "--install is only available on Linux so far",
    ))
}
