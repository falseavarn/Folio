//! `folio --install` adds Folio to the app menu and to the Files (Nautilus) right-click menu
//! under Scripts ▸ Add to Folio, and lists it in "Open With" for images. `folio --uninstall`
//! removes those files. Both point at the binary that ran them, so run them from where Folio stays.

use std::{fs, io, path::PathBuf};

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

    // Inside an AppImage the binary runs from a temporary mount; point at the .AppImage file instead.
    let exe = match std::env::var_os("APPIMAGE") {
        Some(appimage) => PathBuf::from(appimage),
        None => std::env::current_exe()?,
    };
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
    // The same entry the packages install, pointed at this binary.
    let entry = include_str!("../packaging/folio.desktop");
    fs::write(
        &desktop,
        entry.replace("Exec=folio ", &format!("Exec=\"{exe}\" ")),
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
    let _ = targets;
    Err(io::Error::other(
        "--install is only available on Linux so far",
    ))
}

#[cfg(test)]
mod tests {
    #[test]
    fn desktop_entry_has_one_exec_line_to_rewrite() {
        assert_eq!(
            include_str!("../packaging/folio.desktop")
                .matches("Exec=folio ")
                .count(),
            1
        );
    }
}
