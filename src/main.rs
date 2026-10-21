mod store;

use eframe::egui::{self, Align, Color32, CornerRadius, Frame, Layout, RichText, Stroke, vec2};
use std::path::PathBuf;
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

fn main() -> eframe::Result {
    let store = match Store::open(Store::default_root()) {
        Ok(s) => s,
        Err(e) => {
            let msg = format!("Folio could not open its library: {e}");
            eprintln!("{msg}");
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("Folio")
                .set_description(msg)
                .show();
            std::process::exit(1);
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Folio")
            .with_inner_size([1120.0, 720.0])
            .with_min_inner_size([820.0, 480.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Folio",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            style(&cc.egui_ctx);
            Ok(Box::new(App::new(store)))
        }),
    )
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

/// An asset being created (`id == None`) or edited in the side editor.
struct Draft {
    id: Option<u64>,
    kind: Kind,
    description: String,
    projects: Vec<u64>,
    picked: Option<PathBuf>,
    error: Option<String>,
}

enum Confirm {
    Project(u64),
    Asset(u64),
}

struct App {
    store: Store,
    project: Option<u64>,
    filter: Filter,
    new_project: Option<String>,
    rename: Option<String>,
    draft: Option<Draft>,
    confirm: Option<Confirm>,
    error: Option<String>,
}

impl App {
    fn new(store: Store) -> Self {
        Self {
            store,
            project: None,
            filter: Filter::All,
            new_project: None,
            rename: None,
            draft: None,
            confirm: None,
            error: None,
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
                projects: self.project.into_iter().collect(),
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
                    mark(ui);
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
                            self.new_project = Some(String::new());
                        }
                    }
                    Some(name) => {
                        ui.label("Project name");
                        let edit =
                            ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                        if !edit.has_focus() && name.is_empty() {
                            edit.request_focus();
                        }
                        let enter =
                            edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        ui.horizontal(|ui| {
                            let ok = !name.trim().is_empty();
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
        if create {
            let name = self.new_project.take().unwrap_or_default();
            if let Some(id) = report(&mut self.error, self.store.add_project(&name)) {
                select = Some(Some(id));
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
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Library: {}", self.store.root().display()))
                            .color(MUTED)
                            .small(),
                    );
                    if let Some(e) = &self.error {
                        ui.label(RichText::new(e).color(DANGER));
                    }
                });
            });
    }

    fn editor_panel(&mut self, ui: &mut egui::Ui) {
        let (mut save, mut cancel, mut delete) = (false, false, None);
        let store = &self.store;
        let Some(d) = &mut self.draft else { return };
        egui::Panel::right("editor")
            .resizable(false)
            .exact_size(360.0)
            .frame(Frame::NONE.fill(RAISED).inner_margin(20.0))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.heading(if d.id.is_some() {
                        "Edit asset"
                    } else {
                        "New asset"
                    });
                    ui.add_space(8.0);
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
                                let on = std::mem::discriminant(&d.kind)
                                    == std::mem::discriminant(&kind);
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
                        Kind::Image { file } => {
                            ui.label("Image");
                            let shown = d
                                .picked
                                .clone()
                                .or_else(|| (!file.is_empty()).then(|| store.image_path(file)));
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
                                    .add_filter("Images", &["png", "jpg", "jpeg", "gif", "webp"])
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
                });
            });
        if save {
            let r = self.store.put_asset(
                d.id,
                d.kind.clone(),
                &d.description,
                d.projects.clone(),
                d.picked.as_deref(),
            );
            match r {
                Ok(()) => self.draft = None,
                Err(e) => d.error = Some(e.to_string()),
            }
        }
        if cancel {
            self.draft = None;
        }
        if delete.is_some() {
            self.confirm = delete.map(Confirm::Asset);
        }
    }

    fn assets_panel(&mut self, ui: &mut egui::Ui) {
        let mut edit = None;
        let (mut add, mut rename_to, mut delete) = (false, None, false);
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
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                for (f, label) in Filter::ALL {
                    if ui.selectable_label(self.filter == f, label).clicked() {
                        self.filter = f;
                    }
                }
            });
            ui.add_space(12.0);

            let shown: Vec<&Asset> =
                lib.assets.iter().rev().filter(|a| self.project.is_none_or(|p| a.projects.contains(&p)) && self.filter.matches(&a.kind)).collect();
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
                    if asset_row(ui, &self.store, a) {
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
    }

    fn confirm_window(&mut self, ui: &mut egui::Ui) {
        let Some(c) = &self.confirm else { return };
        let (title, body) = match *c {
            Confirm::Project(id) => (
                "Delete project?",
                format!(
                    "“{}” will be deleted, along with {} tagged only with it and their image files. Assets that also belong to other projects are kept.",
                    self.store.project_name(id),
                    match self.store.only_in(id) {
                        1 => "1 asset".to_string(),
                        n => format!("{n} assets"),
                    }
                ),
            ),
            Confirm::Asset(_) => ("Delete asset?", "This asset will be deleted. If it is an image, its file in the library is deleted too.".into()),
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
                        self.draft = None;
                    }
                }
            }
        }
        if go || close {
            self.confirm = None;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.projects_panel(ui);
        self.status_bar(ui);
        self.editor_panel(ui);
        self.assets_panel(ui);
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

/// Folio's mark: two stacked sheets.
fn mark(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(22.0, 22.0), egui::Sense::hover());
    let sheet =
        |dx: f32, dy: f32| egui::Rect::from_min_size(rect.min + vec2(dx, dy), vec2(16.0, 18.0));
    ui.painter().rect_filled(
        sheet(6.0, 0.0),
        CornerRadius::same(2),
        MOSS.gamma_multiply(0.45),
    );
    ui.painter()
        .rect_filled(sheet(0.0, 4.0), CornerRadius::same(2), MOSS);
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
                    if let Kind::Image { file } = &a.kind {
                        ui.add(
                            egui::Image::new(format!(
                                "file://{}",
                                store.image_path(file).display()
                            ))
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
