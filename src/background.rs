//! Keeps one Folio running. A second launch (`folio`, `folio --add <image>`, the Files right-click
//! action) hands its request to the running instance over a per-user Unix socket, so two
//! processes never save the same project files. On Linux a tray icon keeps Folio reachable
//! after its window closes.

use eframe::egui;
use std::{path::PathBuf, sync::mpsc::Sender};

#[derive(Debug, PartialEq)]
pub enum Msg {
    Show,
    NewLink,
    NewNote,
    AddImage(PathBuf),
    Quit,
}

impl Msg {
    /// One line per request; also used as the tray menu item ids.
    fn encode(&self) -> Option<String> {
        Some(match self {
            Msg::Show => "show".into(),
            Msg::NewLink => "link".into(),
            Msg::NewNote => "note".into(),
            Msg::Quit => "quit".into(),
            Msg::AddImage(path) => {
                format!("image\t{}", path.to_str().filter(|p| !p.contains('\n'))?)
            }
        })
    }

    fn decode(line: &str) -> Option<Msg> {
        Some(match line.split_once('\t') {
            Some(("image", path)) => Msg::AddImage(path.into()),
            None if line == "show" => Msg::Show,
            None if line == "link" => Msg::NewLink,
            None if line == "note" => Msg::NewNote,
            None if line == "quit" => Msg::Quit,
            _ => return None,
        })
    }
}

#[cfg(unix)]
fn socket_path() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(dir) => PathBuf::from(dir).join("folio.sock"),
        None => std::env::temp_dir().join(format!(
            "folio-{}.sock",
            std::env::var("USER").unwrap_or_default()
        )),
    }
}

/// Sends the requests to an already running Folio. Returns false when none is running.
#[cfg(unix)]
pub fn hand_off(msgs: &[Msg]) -> bool {
    use std::io::Write;
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(socket_path()) else {
        return false;
    };
    for line in msgs.iter().filter_map(Msg::encode) {
        let _ = writeln!(stream, "{line}");
    }
    true
}

/// Accepts requests from later launches until the process exits.
#[cfg(unix)]
pub fn listen(tx: Sender<Msg>, ctx: egui::Context) -> std::io::Result<()> {
    use std::io::BufRead;
    let path = socket_path();
    // A socket file left by a crash; hand_off already showed nothing is listening on it.
    let _ = std::fs::remove_file(&path);
    let listener = std::os::unix::net::UnixListener::bind(&path)?;
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            for line in std::io::BufReader::new(stream)
                .lines()
                .map_while(Result::ok)
            {
                let Some(msg) = Msg::decode(&line) else {
                    continue;
                };
                if tx.send(msg).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        }
    });
    Ok(())
}

#[cfg(unix)]
pub fn stop_listening() {
    let _ = std::fs::remove_file(socket_path());
}

// shortcut: no single-instance handoff on Windows; add a named pipe before shipping a Windows build.
#[cfg(not(unix))]
pub fn hand_off(_: &[Msg]) -> bool {
    false
}
#[cfg(not(unix))]
pub fn listen(_: Sender<Msg>, _: egui::Context) -> std::io::Result<()> {
    Ok(())
}
#[cfg(not(unix))]
pub fn stop_listening() {}

/// The tray icon. Dropping it removes the icon. None if the desktop has no tray.
#[cfg(target_os = "linux")]
pub fn tray(tx: Sender<Msg>, ctx: egui::Context) -> Option<tray_icon::TrayIcon> {
    use tray_icon::{
        Icon, TrayIconBuilder,
        menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    };
    let png = image::load_from_memory(include_bytes!("../icons/64x64.png"))
        .ok()?
        .into_rgba8();
    let (w, h) = png.dimensions();
    let icon = Icon::from_rgba(png.into_raw(), w, h).ok()?;
    let item = |msg: Msg, text: &str| {
        MenuItem::with_id(msg.encode().unwrap_or_default(), text, true, None)
    };
    let menu = Menu::with_items(&[
        &item(Msg::Show, "Open Folio"),
        &item(Msg::NewLink, "Add link…"),
        &item(Msg::NewNote, "Add note…"),
        &PredefinedMenuItem::separator(),
        &item(Msg::Quit, "Quit Folio"),
    ])
    .ok()?;
    MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
        if let Some(msg) = Msg::decode(&e.id.0) {
            let _ = tx.send(msg);
            ctx.request_repaint();
        }
    }));
    TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_icon(icon)
        .with_tooltip("Folio")
        .build()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_round_trip() {
        for msg in [
            Msg::Show,
            Msg::NewLink,
            Msg::NewNote,
            Msg::Quit,
            Msg::AddImage("/tmp/a b.png".into()),
        ] {
            assert_eq!(Msg::decode(&msg.encode().unwrap()), Some(msg));
        }
        assert_eq!(Msg::AddImage("/tmp/a\nb.png".into()).encode(), None);
        assert_eq!(Msg::decode("rm -rf"), None);
    }
}
