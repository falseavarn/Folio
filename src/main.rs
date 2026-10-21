mod background;
mod install;
mod store;

use background::Msg;
use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Layout, RichText, Stroke, vec2};
use std::{collections::VecDeque, path::PathBuf, process::ExitCode, sync::mpsc};
use store::{Asset, Kind, Store};

// FalseAvarn palette (falseavarn/src/index.css) plus Folio's identity color, moss.
const OBSIDIAN: Color32 = Color32::from_rgb(0x11, 0x13, 0x15);
const RAISED: Color32 = Color32::from_rgb(0x1A, 0x1C, 0x1F);
const ACTIVE: Color32 = Color32::from_rgb(0x26, 0x28, 0x2C);
const SUNK: Color32 = Color32::from_rgb(0x0B, 0x0C, 0x0E);
const CANVAS: Color32 = Color32::from_rgb(0xFA, 0xFA, 0xF9);
// avzinc (#71717A) is under 4.5:1 on obsidian, so secondary text uses a lighter zinc.
const MUTED: Color32 = Color32::from_rgb(0xA1, 0xA1, 0xAA);
const MOSS: Color32 = Color32::from_rgb(0x8B, 0xC3, 0x4A);
const DANGER: Color32 = Color32::from_rgb(0xF8, 0x71, 0x71);

const USAGE: &str = "Usage: folio [--add <image>...] [--install | --uninstall]";

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    let msgs: Vec<Msg> = match first.as_ref().map(|a| a.to_string_lossy()).as_deref() {
        None => vec![Msg::Show],
        Some("--add") => {
            let images: Vec<Msg> = args
                .filter_map(|a| std::path::absolute(a).ok())
                .map(Msg::AddImage)
                .collect();
            if images.is_empty() {
                vec![Msg::Show]
            } else {
                images
            }
        }
        Some(flag @ ("--install" | "--uninstall")) => {
            return match install::run(flag == "--install") {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("folio {flag}: {e}");
                    ExitCode::FAILURE
                }
            };
        }
        Some(_) => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if background::hand_off(&msgs) {
        return ExitCode::SUCCESS;
    }

    let mut store = match Store::open(Store::default_root()) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("Folio could not open its library: {e}");
            eprintln!("{msg}");
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Folio")
                .set_description(msg)
                .show();
            return ExitCode::FAILURE;
        }
    };
    // One context for every window Folio opens, so styling and loaded images carry over.
    let ctx = egui::Context::default();
    style(&ctx);
    egui_extras::install_image_loaders(&ctx);

    let (tx, rx) = mpsc::channel();
    for msg in msgs {
        let _ = tx.send(msg);
    }
    if let Err(e) = background::listen(tx.clone(), ctx.clone()) {
        eprintln!("Folio will not receive requests from other launches: {e}");
    }
    #[cfg(target_os = "linux")]
    let tray = background::tray(tx.clone(), ctx.clone());
    #[cfg(target_os = "linux")]
    let stays_in_background = tray.is_some();
    // shortcut: tray only on Linux (its KSNI backend runs on its own thread); Windows and macOS need their tray on the main event loop.
    #[cfg(not(target_os = "linux"))]
    let stays_in_background = false;

    let (mut quit, mut next) = (false, None);
    loop {
        // With no window open, wait here for the tray or another launch.
        let msg = match next.take() {
            Some(msg) => msg,
            None => match rx.recv() {
                Ok(msg) => msg,
                Err(_) => break,
            },
        };
        if msg == Msg::Quit {
            break;
        }
        let popup = msg != Msg::Show;
        let viewport = egui::ViewportBuilder::default()
            .with_app_id("folio")
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!("../icons/128x128@2x.png"))
                    .expect("bundled icon is a valid PNG"),
            );
        let viewport = if popup {
            viewport
                .with_title("Add to Folio")
                .with_inner_size([440.0, 640.0])
                .with_min_inner_size([360.0, 420.0])
        } else {
            viewport
                .with_title("Folio")
                .with_inner_size([1120.0, 720.0])
                .with_min_inner_size([820.0, 480.0])
        };
        let options = eframe::NativeOptions {
            viewport,
            ..Default::default()
        };
        let app = App::new(&mut store, &rx, &mut quit, &mut next, popup, msg);
        if let Err(e) = eframe::run_native_ext(
            "Folio",
            options,
            Some(ctx.clone()),
            Box::new(|_| Ok(Box::new(app))),
        ) {
            eprintln!("Folio's window failed: {e}");
            background::stop_listening();
            return ExitCode::FAILURE;
        }
        if quit || (!stays_in_background && next.is_none()) {
            break;
        }
    }
    background::stop_listening();
    ExitCode::SUCCESS
}

fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = OBSIDIAN;
    v.window_fill = RAISED;
    v.extreme_bg_color = SUNK;
    v.faint_bg_color = RAISED;
    v.hyperlink_color = MOSS;
    v.selection.bg_fill = Color32::from_rgb(0x3A, 0x52, 0x1F);
    v.selection.stroke = Stroke::new(1.0, MOSS);
    v.text_cursor.stroke.color = MOSS;
    v.widgets.noninteractive.fg_stroke.color = CANVAS;
    v.widgets.noninteractive.bg_stroke.color = ACTIVE;
    // FalseAvarn is dark-only, so pin the theme instead of following the OS.
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, |s| {
        s.visuals = v;
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.button_padding = vec2(12.0, 6.0);
        for (text, size) in [
            (egui::TextStyle::Body, 15.0),
            (egui::TextStyle::Button, 15.0),
            (egui::TextStyle::Heading, 22.0),
        ] {
            if let Some(font) = s.text_styles.get_mut(&text) {
                font.size = size;
            }
        }
    });
}

#[derive(Clone, Copy, PartialEq)]
enum Filter {
    All,
    Links,
    Images,
    Notes,
}

impl Filter {
    const ALL: [(Filter, &str); 4] = [
        (Filter::All, "All"),
        (Filter::Links, "Links"),
        (Filter::Images, "Images"),
        (Filter::Notes, "Notes"),
    ];

    fn matches(self, kind: &Kind) -> bool {
        matches!(
            (self, kind),
            (Filter::All, _)
                | (Filter::Links, Kind::Link { .. })
                | (Filter::Images, Kind::Image { .. })
                | (Filter::Notes, Kind::Note { .. })
        )
    }
}

/// An asset being created (`id == None`) or edited in the side editor or the popup.
struct Draft {
    id: Option<u64>,
    kind: Kind,
    description: String,
    projects: Vec<u64>,
    picked: Option<PathBuf>,
    error: Option<String>,
}

/// The new-project form in the sidebar. `dir` is None until the user picks a folder.
struct NewProject {
    name: String,
    dir: Option<PathBuf>,
}

enum Confirm {
    Project(u64),
    Asset(u64),
}

/// One window: the full library, or the small "Add to Folio" popup (`popup`).
/// It borrows what outlives the window from `main`.
struct App<'a> {
    store: &'a mut Store,
    rx: &'a mpsc::Receiver<Msg>,
    quit: &'a mut bool,
    /// What `main` should open after this window closes.
    next: &'a mut Option<Msg>,
    popup: bool,
    queue: VecDeque<Msg>,
    project: Option<u64>,
    filter: Filter,
    new_project: Option<NewProject>,
    rename: Option<String>,
    draft: Option<Draft>,
    confirm: Option<Confirm>,
    error: Option<String>,
    /// A success message for the status bar, such as where an export was saved.
    notice: Option<String>,
}

impl<'a> App<'a> {
    fn new(
        store: &'a mut Store,
        rx: &'a mpsc::Receiver<Msg>,
        quit: &'a mut bool,
        next: &'a mut Option<Msg>,
        popup: bool,
        first: Msg,
    ) -> Self {
        let error = (!store.warnings.is_empty()).then(|| store.warnings.join(" "));
        let mut app = Self {
            store,
            rx,
            quit,
            next,
            popup,
            queue: VecDeque::new(),
            project: None,
            filter: Filter::All,
            new_project: None,
            rename: None,
            draft: None,
            confirm: None,
            error,
            notice: None,
        };
        app.open_request(first);
        app
    }

    /// Handles a request from the tray or another launch while this window is open.
    fn handle(&mut self, ctx: &egui::Context, msg: Msg) {
        match msg {
            Msg::Quit => {
                *self.quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Msg::Show if self.popup => {
                *self.next = Some(Msg::Show);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Msg::Show => ctx.send_viewport_cmd(egui::ViewportCommand::Focus),
            add if self.draft.is_some() => self.queue.push_back(add),
            add => {
                self.open_request(add);
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
    }

    fn open_request(&mut self, msg: Msg) {
        let (kind, picked) = match msg {
            Msg::NewLink => (Kind::Link { url: String::new() }, None),
            Msg::NewNote => (
                Kind::Note {
                    body: String::new(),
                },
                None,
            ),
            Msg::AddImage(path) if store::is_image(&path) && path.is_file() => (
                Kind::Image {
                    file: String::new(),
                },
                Some(path),
            ),
            Msg::AddImage(path) => {
                self.error = Some(format!(
                    "{} is not a PNG, JPEG, GIF or WebP image.",
                    path.display()
                ));
                return;
            }
            Msg::Show | Msg::Quit => return,
        };
        self.open_draft(None);
        if let Some(d) = &mut self.draft {
            d.kind = kind;
            d.picked = picked;
        }
    }

    /// Closes the editor, then opens the next queued request, or closes the popup when none is left.
    fn close_draft(&mut self, ctx: &egui::Context) {
        self.draft = None;
        if let Some(msg) = self.queue.pop_front() {
            self.open_request(msg);
        } else if self.popup {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn open_draft(&mut self, asset: Option<&Asset>) {
        self.draft = Some(match asset {
            Some(a) => Draft {
                id: Some(a.id),
                kind: a.kind.clone(),
                description: a.description.clone(),
                projects: a.projects.clone(),
                picked: None,
                error: None,
            },
            None => Draft {
                id: None,
                kind: Kind::Link { url: String::new() },
                description: String::new(),
                // The open project, or the only one there is.
                projects: match (self.project, self.store.lib.projects.as_slice()) {
                    (Some(p), _) => vec![p],
                    (None, [only]) => vec![only.id],
                    _ => Vec::new(),
                },
                picked: None,
                error: None,
            },
        });
    }

    fn projects_panel(&mut self, ui: &mut egui::Ui) {
        let mut select = None;
        let (mut create, mut cancel_new) = (false, false);
        egui::Panel::left("projects")
            .resizable(false)
            .exact_size(240.0)
            .frame(Frame::NONE.fill(RAISED).inner_margin(16.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Image::new(egui::include_image!("../icons/64x64.png"))
                            .fit_to_exact_size(vec2(24.0, 24.0)),
                    );
                    ui.label(RichText::new("Folio").size(20.0).strong().color(CANVAS));
                });
                ui.add_space(20.0);
                ui.label(RichText::new("Projects").color(MUTED));
                let lib = &self.store.lib;
                if nav_row(ui, self.project.is_none(), "All assets", lib.assets.len()) {
                    select = Some(None);
                }
                for p in &lib.projects {
                    let n = lib
                        .assets
                        .iter()
                        .filter(|a| a.projects.contains(&p.id))
                        .count();
                    if nav_row(ui, self.project == Some(p.id), &p.name, n) {
                        select = Some(Some(p.id));
                    }
                }
                ui.add_space(12.0);
                match &mut self.new_project {
                    None => {
                        if ui.button("New project").clicked() {
                            self.new_project = Some(NewProject {
                                name: String::new(),
                                dir: None,
                            });
                        }
                    }
                    Some(form) => {
                        ui.label("Project name");
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut form.name).desired_width(f32::INFINITY),
                        );
                        if !edit.has_focus() && form.name.is_empty() && form.dir.is_none() {
                            edit.request_focus();
                        }
                        let enter =
                            edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.label("Saved in");
                        let dir = form
                            .dir
                            .clone()
                            .unwrap_or_else(|| self.store.suggested_dir(&form.name));
                        ui.label(
                            RichText::new(dir.display().to_string())
                                .color(MUTED)
                                .size(13.0),
                        );
                        if ui.button("Choose folder…").clicked()
                            && let Some(picked) = rfd::FileDialog::new()
                                .set_directory(dir.parent().unwrap_or(&dir))
                                .pick_folder()
                        {
                            form.dir = Some(picked);
                        }
                        if dir.join(store::PROJECT_FILE).exists() {
                            ui.label(
                                RichText::new(
                                    "This folder already holds a Folio project. Creating opens it.",
                                )
                                .color(MUTED)
                                .size(13.0),
                            );
                        }
                        ui.horizontal(|ui| {
                            let ok = !form.name.trim().is_empty() || form.dir.is_some();
                            create =
                                ui.add_enabled(ok, primary("Create")).clicked() || (ok && enter);
                            cancel_new = ui.button("Cancel").clicked();
                        });
                    }
                }
            });
        if cancel_new {
            self.new_project = None;
        }
        if create && let Some(form) = self.new_project.take() {
            let dir = form
                .dir
                .unwrap_or_else(|| self.store.suggested_dir(&form.name));
            match report(
                &mut self.error,
                self.store.add_project(&form.name, dir.clone()),
            ) {
                Some(id) => select = Some(Some(id)),
                // Keep the form open with what was entered so the error can be fixed.
                None => {
                    self.new_project = Some(NewProject {
                        name: form.name,
                        dir: Some(dir),
                    })
                }
            }
        }
        if let Some(p) = select {
            self.project = p;
            self.rename = None;
        }
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status")
            .frame(Frame::NONE.fill(RAISED).inner_margin(8.0))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("Library: {}", self.store.root().display()))
                            .color(MUTED)
                            .small(),
                    );
                    if let Some(e) = &self.error {
                        ui.label(RichText::new(e).color(DANGER));
                    } else if let Some(n) = &self.notice {
                        ui.label(RichText::new(n).color(MUTED).small());
                    }
                });
            });
    }

    fn editor_panel(&mut self, ui: &mut egui::Ui) {
        if self.draft.is_none() {
            return;
        }
        egui::Panel::right("editor")
            .resizable(false)
            .exact_size(360.0)
            .frame(Frame::NONE.fill(RAISED).inner_margin(20.0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| self.editor_form(ui));
            });
    }

    /// The small window opened from the tray or the Files right-click menu.
    fn popup_panel(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(RAISED).inner_margin(20.0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Image::new(egui::include_image!("../icons/64x64.png"))
                                .fit_to_exact_size(vec2(24.0, 24.0)),
                        );
                        ui.label(
                            RichText::new("Add to Folio")
                                .size(20.0)
                                .strong()
                                .color(CANVAS),
                        );
                    });
                    ui.add_space(12.0);
                    if let Some(e) = &self.error {
                        ui.label(RichText::new(e).color(DANGER));
                    }
                    if self.store.lib.projects.is_empty() {
                        ui.label("Create a project in Folio first, then add to it from here.");
                        if ui.add(primary("Open Folio")).clicked() {
                            *self.next = Some(Msg::Show);
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    } else if self.draft.is_some() {
                        self.editor_form(ui);
                    } else if ui.button("Close").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });
    }

    fn editor_form(&mut self, ui: &mut egui::Ui) {
        let (mut save, mut cancel, mut delete) = (false, false, None);
        let store = &*self.store;
        let Some(d) = &mut self.draft else { return };
        if !self.popup {
            ui.heading(if d.id.is_some() {
                "Edit asset"
            } else {
                "New asset"
            });
            ui.add_space(8.0);
        }
        if d.id.is_none() {
            ui.horizontal(|ui| {
                for (label, kind) in [
                    ("Link", Kind::Link { url: String::new() }),
                    (
                        "Image",
                        Kind::Image {
                            file: String::new(),
                        },
                    ),
                    (
                        "Note",
                        Kind::Note {
                            body: String::new(),
                        },
                    ),
                ] {
                    let on = std::mem::discriminant(&d.kind) == std::mem::discriminant(&kind);
                    if ui.selectable_label(on, label).clicked() && !on {
                        d.kind = kind;
                        d.error = None;
                    }
                }
            });
            ui.add_space(8.0);
        }
        match &mut d.kind {
            Kind::Link { url } => {
                ui.label("Link");
                ui.add(
                    egui::TextEdit::singleline(url)
                        .hint_text("https://")
                        .desired_width(f32::INFINITY),
                );
            }
            Kind::Image { .. } => {
                ui.label("Image");
                let saved =
                    d.id.and_then(|id| store.lib.assets.iter().find(|a| a.id == id))
                        .and_then(|a| store.image_of(a));
                let shown = d.picked.clone().or(saved);
                if let Some(path) = &shown {
                    ui.add(
                        egui::Image::new(format!("file://{}", path.display()))
                            .max_width(ui.available_width())
                            .max_height(220.0),
                    );
                }
                if ui
                    .button(if shown.is_some() {
                        "Replace image…"
                    } else {
                        "Choose image…"
                    })
                    .clicked()
                    && let Some(p) = rfd::FileDialog::new()
                        .add_filter("Images", &store::IMAGE_EXTENSIONS)
                        .pick_file()
                {
                    d.picked = Some(p);
                }
            }
            Kind::Note { body } => {
                ui.label("Note");
                ui.add(
                    egui::TextEdit::multiline(body)
                        .desired_rows(8)
                        .desired_width(f32::INFINITY),
                );
            }
        }
        ui.add_space(8.0);
        ui.label("Description (optional)");
        ui.add(
            egui::TextEdit::multiline(&mut d.description)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        ui.add_space(8.0);
        ui.label("Projects");
        if store.lib.projects.is_empty() {
            ui.label(RichText::new("Create a project first.").color(MUTED));
        }
        for p in &store.lib.projects {
            let mut on = d.projects.contains(&p.id);
            if ui.checkbox(&mut on, &p.name).changed() {
                if on {
                    d.projects.push(p.id);
                } else {
                    d.projects.retain(|&id| id != p.id);
                }
            }
        }
        if let Some(e) = &d.error {
            ui.add_space(8.0);
            ui.label(RichText::new(e).color(DANGER));
        }
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            save = ui.add(primary("Save")).clicked();
            cancel = ui.button("Cancel").clicked();
            if let Some(id) = d.id {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button(RichText::new("Delete").color(DANGER)).clicked() {
                        delete = Some(id);
                    }
                });
            }
        });
        if save {
            match self.store.put_asset(
                d.id,
                d.kind.clone(),
                &d.description,
                d.projects.clone(),
                d.picked.as_deref(),
            ) {
                Ok(()) => self.close_draft(ui.ctx()),
                Err(e) => d.error = Some(e.to_string()),
            }
        } else if cancel {
            self.close_draft(ui.ctx());
        } else if delete.is_some() {
            self.confirm = delete.map(Confirm::Asset);
        }
    }

    fn assets_panel(&mut self, ui: &mut egui::Ui) {
        let mut edit = None;
        let (mut add, mut rename_to, mut delete, mut export) = (false, None, false, None);
        egui::CentralPanel::default().frame(Frame::NONE.fill(OBSIDIAN).inner_margin(24.0)).show(ui, |ui| {
            let lib = &self.store.lib;
            ui.horizontal(|ui| {
                match (self.project, &mut self.rename) {
                    (None, _) => {
                        ui.heading("All assets");
                    }
                    (Some(_), Some(name)) => {
                        let edit = ui.add(egui::TextEdit::singleline(name).font(egui::TextStyle::Heading));
                        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if ui.add_enabled(!name.trim().is_empty(), primary("Save")).clicked() || enter {
                            rename_to = Some(name.clone());
                        }
                        if ui.button("Cancel").clicked() {
                            self.rename = None;
                        }
                    }
                    (Some(id), rename) => {
                        let name = self.store.project_name(id);
                        ui.heading(name);
                        if ui.button("Rename").clicked() {
                            *rename = Some(name.to_string());
                        }
                        if ui.button("Export for AI…").clicked() {
                            export = Some(id);
                        }
                        if ui.button("Open folder").clicked()
                            && let Some(p) = self.store.project(id)
                        {
                            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("file://{}", p.dir.display())));
                        }
                        if ui.button(RichText::new("Delete").color(DANGER)).clicked() {
                            delete = true;
                        }
                    }
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let r = ui.add_enabled(!lib.projects.is_empty(), primary("Add asset"));
                    add = r.on_disabled_hover_text("Create a project first.").clicked();
                });
            });
            if let Some(p) = self.project.and_then(|id| self.store.project(id)) {
                ui.label(RichText::new(p.dir.display().to_string()).color(MUTED).size(13.0));
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (f, label) in Filter::ALL {
                    if ui.selectable_label(self.filter == f, label).clicked() {
                        self.filter = f;
                    }
                }
            });
            ui.add_space(12.0);

            let mut shown: Vec<&Asset> =
                lib.assets.iter().filter(|a| self.project.is_none_or(|p| a.projects.contains(&p)) && self.filter.matches(&a.kind)).collect();
            shown.sort_by_key(|a| std::cmp::Reverse(a.added));
            if shown.is_empty() {
                let msg = if lib.projects.is_empty() {
                    "Create a project in the sidebar, then add the links, images and notes you collect for it."
                } else if lib.assets.is_empty() {
                    "Nothing collected yet. Add a link, an image or a note."
                } else {
                    "Nothing here matches this project and filter."
                };
                ui.add_space(24.0);
                ui.label(RichText::new(msg).color(MUTED));
                return;
            }
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for a in shown {
                    if asset_row(ui, self.store, a) {
                        edit = Some(a.id);
                    }
                    ui.add_space(4.0);
                }
            });
        });
        if add {
            self.open_draft(None);
        }
        if let Some(id) = edit {
            let asset = self.store.lib.assets.iter().find(|a| a.id == id).cloned();
            self.open_draft(asset.as_ref());
        }
        if let (Some(name), Some(id)) = (rename_to, self.project)
            && report(&mut self.error, self.store.rename_project(id, &name)).is_some()
        {
            self.rename = None;
        }
        if delete {
            self.confirm = self.project.map(Confirm::Project);
        }
        if let Some(id) = export {
            self.export(id);
        }
    }

    /// Saves the project's Markdown summary where the user chooses, by default inside the project folder.
    fn export(&mut self, id: u64) {
        let (Some(p), Some(md)) = (self.store.project(id), self.store.export_markdown(id)) else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .set_directory(&p.dir)
            .set_file_name(format!("{}.md", p.name.replace(['/', '\\'], "-")))
            .add_filter("Markdown", &["md"])
            .save_file()
        else {
            return;
        };
        if report(&mut self.error, std::fs::write(&path, md)).is_some() {
            self.notice = Some(format!("Exported {}", path.display()));
        }
    }

    fn confirm_window(&mut self, ui: &mut egui::Ui) {
        let Some(c) = &self.confirm else { return };
        let (title, body) = match *c {
            Confirm::Project(id) => (
                "Delete project?",
                format!(
                    "“{}” will be deleted, along with {} tagged only with it. Folio removes its project file and image copies from {}; anything else you put in that folder stays. Assets that also belong to other projects are kept.",
                    self.store.project_name(id),
                    match self.store.only_in(id) {
                        1 => "1 asset".to_string(),
                        n => format!("{n} assets"),
                    },
                    self.store.project(id).map(|p| p.dir.display().to_string()).unwrap_or_default(),
                ),
            ),
            Confirm::Asset(_) => {
                ("Delete asset?", "This asset will be deleted. If it is an image, its copies in the project folders are deleted too.".into())
            }
        };
        let (mut go, mut close) = (false, ui.input(|i| i.key_pressed(egui::Key::Escape)));
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ui.ctx(), |ui| {
                ui.set_max_width(380.0);
                ui.label(body);
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    go = ui
                        .add(
                            egui::Button::new(RichText::new("Delete").color(OBSIDIAN)).fill(DANGER),
                        )
                        .clicked();
                    close |= ui.button("Cancel").clicked();
                });
            });
        if go {
            match *c {
                Confirm::Project(id) => {
                    if report(&mut self.error, self.store.delete_project(id)).is_some() {
                        self.project = None;
                        self.draft = None;
                    }
                }
                Confirm::Asset(id) => {
                    if report(&mut self.error, self.store.delete_asset(id)).is_some() {
                        self.close_draft(ui.ctx());
                    }
                }
            }
        }
        if go || close {
            self.confirm = None;
        }
    }
}

impl eframe::App for App<'_> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(msg) = self.rx.try_recv() {
            self.handle(&ui.ctx().clone(), msg);
        }
        if self.popup {
            self.popup_panel(ui);
        } else {
            self.projects_panel(ui);
            self.status_bar(ui);
            self.editor_panel(ui);
            self.assets_panel(ui);
        }
        self.confirm_window(ui);
    }
}

/// Shows a failed save in the status bar; clears it on success.
fn report<T>(error: &mut Option<String>, r: std::io::Result<T>) -> Option<T> {
    *error = r.as_ref().err().map(|e| e.to_string());
    r.ok()
}

fn primary(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).color(OBSIDIAN).strong()).fill(MOSS)
}

/// A full-width sidebar row; the active one gets a neutral fill, not the identity color.
fn nav_row(ui: &mut egui::Ui, active: bool, name: &str, count: usize) -> bool {
    let text =
        RichText::new(format!("{name}   {count}")).color(if active { CANVAS } else { MUTED });
    let fill = if active { ACTIVE } else { Color32::TRANSPARENT };
    ui.add(
        egui::Button::new(text)
            .fill(fill)
            .min_size(vec2(ui.available_width(), 30.0)),
    )
    .clicked()
}

/// One asset in the list. Returns true when its Edit button was pressed.
fn asset_row(ui: &mut egui::Ui, store: &Store, a: &Asset) -> bool {
    let mut edit = false;
    Frame::NONE
        .fill(RAISED)
        .inner_margin(12.0)
        .corner_radius(CornerRadius::same(4))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                edit = ui.button("Edit").clicked();
                ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
                    if let Some(path) = store.image_of(a) {
                        ui.add(
                            egui::Image::new(format!("file://{}", path.display()))
                                .max_width(160.0)
                                .max_height(100.0),
                        );
                    }
                    ui.vertical(|ui| {
                        let tags: Vec<&str> =
                            a.projects.iter().map(|&p| store.project_name(p)).collect();
                        let kind = match a.kind {
                            Kind::Link { .. } => "Link",
                            Kind::Image { .. } => "Image",
                            Kind::Note { .. } => "Note",
                        };
                        ui.label(
                            RichText::new(format!("{kind} · {}", tags.join(", ")))
                                .color(MUTED)
                                .size(13.0),
                        );
                        match &a.kind {
                            Kind::Link { url } => {
                                ui.hyperlink_to(url, url);
                            }
                            Kind::Note { body } => {
                                let mut preview: String = body.chars().take(280).collect();
                                if body.chars().count() > 280 {
                                    preview.push('…');
                                }
                                ui.label(RichText::new(preview).color(CANVAS));
                            }
                            Kind::Image { .. } => {}
                        }
                        if !a.description.is_empty() {
                            ui.label(RichText::new(&a.description).color(MUTED));
                        }
                    });
                });
            });
        });
    edit
}
