use crate::{
    api::{Metadata, Query},
    assets::{self, Loaded, Source},
    settings::{Settings, Theme},
    system::{self, Appearance},
    worker::{DiskJob, Event, NetJob, Workers},
};
use eframe::egui::{self, Color32, RichText, TextureHandle, TextureOptions};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    path::PathBuf,
    time::{Duration, Instant},
};

const HISTORY_LIMIT: usize = 48;
const MEMORY_LIMIT: usize = 96 * 1024 * 1024;
#[derive(Clone)]
struct Item {
    meta: Metadata,
    source: Source,
    dimensions: [u32; 2],
    thumb: TextureHandle,
    saved: bool,
}
#[derive(PartialEq, Eq)]
enum View {
    Discover,
    Collection,
}

pub struct GalleryApp {
    base: PathBuf,
    settings: Settings,
    workers: Workers,
    appearance: Appearance,
    last_os_check: Instant,
    os_pending: bool,
    style_key: Option<(bool, [u8; 3])>,
    tags: Vec<String>,
    tag_search: String,
    tags_loading: bool,
    view: View,
    history: Vec<Item>,
    selected: Option<usize>,
    preview: Option<TextureHandle>,
    zoom: f32,
    epoch: u64,
    net_busy: bool,
    waiting: bool,
    ready: Option<Box<Loaded>>,
    open_epoch: u64,
    opening: bool,
    saving: HashSet<String>,
    library: Vec<PathBuf>,
    library_epoch: u64,
    scanning: bool,
    scan_due: bool,
    library_search: String,
    thumbs: HashMap<PathBuf, TextureHandle>,
    thumb_order: VecDeque<PathBuf>,
    thumb_pending: HashSet<PathBuf>,
    thumb_failed: HashSet<PathBuf>,
    settings_open: bool,
    dirty: Option<Instant>,
    status: String,
    error: bool,
    started: Instant,
    smoke: bool,
    smoke_phase: u8,
    smoke_path: Option<PathBuf>,
    closing: bool,
    capture_after: Instant,
}

impl GalleryApp {
    pub fn new(cc: &eframe::CreationContext<'_>, base: PathBuf, smoke: bool) -> Self {
        let (settings, warning) = Settings::load(&base);
        let workers = Workers::start(cc.egui_ctx.clone());
        let _ = workers.net.try_send(NetJob::Tags);
        let _ = workers.disk.try_send(DiskJob::Appearance);
        let mut app = Self {
            base,
            settings,
            workers,
            appearance: Appearance {
                light: false,
                accent: None,
            },
            last_os_check: Instant::now(),
            os_pending: true,
            style_key: None,
            tags: vec!["waifu".into()],
            tag_search: String::new(),
            tags_loading: true,
            view: View::Discover,
            history: vec![],
            selected: None,
            preview: None,
            zoom: 1.0,
            epoch: 0,
            net_busy: false,
            waiting: false,
            ready: None,
            open_epoch: 0,
            opening: false,
            saving: HashSet::new(),
            library: vec![],
            library_epoch: 0,
            scanning: false,
            scan_due: false,
            library_search: String::new(),
            thumbs: HashMap::new(),
            thumb_order: VecDeque::new(),
            thumb_pending: HashSet::new(),
            thumb_failed: HashSet::new(),
            settings_open: false,
            dirty: None,
            status: warning
                .clone()
                .unwrap_or("Выбери тег. Найди что-то своё.".into()),
            error: warning.is_some(),
            started: Instant::now(),
            smoke,
            smoke_phase: 0,
            smoke_path: None,
            closing: false,
            capture_after: Instant::now(),
        };
        app.scan();
        app.apply_style(&cc.egui_ctx);
        app
    }
    fn query(&self) -> Query {
        Query {
            mode: self.settings.mode.clone(),
            tag: self.settings.category.clone(),
            custom_url: if self.settings.use_custom_url {
                Some(self.settings.custom_url.trim().into())
            } else {
                None
            },
        }
    }
    fn accent(&self) -> [u8; 3] {
        palette(&self.settings, self.appearance).1
    }
    fn ac_text(&self) -> Color32 {
        let [r, g, b] = system::readable_accent(self.accent(), self.dark());
        Color32::from_rgb(r, g, b)
    }
    fn dark(&self) -> bool {
        palette(&self.settings, self.appearance).0
    }
    fn background(&self) -> Color32 {
        if self.dark() {
            Color32::from_rgb(14, 16, 23)
        } else {
            Color32::from_rgb(244, 245, 250)
        }
    }
    fn panel(&self) -> Color32 {
        if self.dark() {
            Color32::from_rgb(20, 23, 32)
        } else {
            Color32::WHITE
        }
    }
    fn card(&self) -> Color32 {
        if self.dark() {
            Color32::from_rgb(27, 31, 43)
        } else {
            Color32::from_rgb(236, 238, 246)
        }
    }
    fn muted(&self) -> Color32 {
        if self.dark() {
            Color32::from_rgb(153, 162, 184)
        } else {
            Color32::from_rgb(93, 103, 125)
        }
    }
    fn ac(&self) -> Color32 {
        let [r, g, b] = self.accent();
        Color32::from_rgb(r, g, b)
    }
    fn message(&mut self, text: impl Into<String>, error: bool) {
        self.status = text.into();
        self.error = error;
    }
    fn changed(&mut self) {
        self.dirty = Some(Instant::now());
    }
    fn apply_style(&mut self, ctx: &egui::Context) {
        let key = (self.dark(), self.accent());
        if self.style_key == Some(key) {
            return;
        }
        let mut visuals = if key.0 {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        visuals.panel_fill = self.panel();
        visuals.window_fill = self.panel();
        visuals.extreme_bg_color = self.background();
        visuals.faint_bg_color = self.card();
        visuals.selection.bg_fill = self.ac();
        let [r, g, b] = system::contrast_text(self.accent());
        visuals.selection.stroke = egui::Stroke::new(1.0, Color32::from_rgb(r, g, b));
        visuals.widgets.inactive.bg_fill = self.card();
        visuals.widgets.hovered.bg_fill = self.ac().gamma_multiply(0.35);
        visuals.widgets.active.bg_fill = self.ac().gamma_multiply(0.5);
        visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(9);
        visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(9);
        visuals.widgets.active.corner_radius = egui::CornerRadius::same(9);
        visuals.window_corner_radius = egui::CornerRadius::same(16);
        ctx.set_visuals(visuals);
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(if key.0 {
            egui::SystemTheme::Dark
        } else {
            egui::SystemTheme::Light
        }));
        ctx.style_mut(|s| {
            s.spacing.item_spacing = egui::vec2(10.0, 10.0);
            s.spacing.button_padding = egui::vec2(14.0, 10.0);
            s.text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(15.0));
            s.text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
            s.text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(26.0));
        });
        self.style_key = Some(key);
    }
    fn invalidate(&mut self) {
        self.epoch += 1;
        self.ready = None;
        self.waiting = false;
        self.changed();
        self.scan();
        self.message("Фильтры обновлены. Нажми «Следующая».", false);
    }
    fn fetch(&mut self) {
        if self.net_busy {
            return;
        }
        let query = self.query();
        if query.custom_url.as_ref().is_some_and(|s| s.is_empty()) {
            self.waiting = false;
            self.message("Укажи URL API в настройках.", true);
            return;
        }
        if self
            .workers
            .net
            .try_send(NetJob::Image(self.epoch, query))
            .is_ok()
        {
            self.net_busy = true;
        } else {
            self.waiting = false;
            self.message("Очередь загрузки занята. Попробуй ещё раз.", true);
        }
    }
    fn next(&mut self, ctx: &egui::Context) {
        if self.waiting {
            return;
        }
        self.view = View::Discover;
        if let Some(image) = self.ready.take() {
            self.present(ctx, *image);
            self.prefetch();
        } else {
            self.waiting = true;
            self.fetch();
            self.message("Ищем следующую находку…", false);
        }
    }
    fn prefetch(&mut self) {
        if self.settings.prefetch && self.ready.is_none() {
            self.fetch();
        }
    }
    fn present(&mut self, ctx: &egui::Context, loaded: Loaded) {
        self.open_epoch += 1;
        self.opening = false;
        let key = loaded.meta.key.clone();
        self.history.retain(|item| item.meta.key != key);
        let preview = ctx.load_texture("current-preview", loaded.preview, TextureOptions::LINEAR);
        let thumb = ctx.load_texture(
            format!("thumb-{key}"),
            loaded.thumbnail,
            TextureOptions::LINEAR,
        );
        let saved = matches!(loaded.source, Source::Disk(_));
        self.history.insert(
            0,
            Item {
                meta: loaded.meta,
                source: loaded.source,
                dimensions: loaded.dimensions,
                thumb,
                saved,
            },
        );
        self.selected = Some(0);
        self.preview = Some(preview);
        self.zoom = 1.0;
        self.waiting = false;
        trim_history(&mut self.history);
        self.message("Готово. Сохрани то, что нравится.", false);
    }
    fn select(&mut self, index: usize) {
        if index >= self.history.len() {
            return;
        }
        self.open_epoch += 1;
        let item = &self.history[index];
        if self
            .workers
            .disk
            .try_send(DiskJob::Open(
                self.open_epoch,
                item.source.clone(),
                item.meta.clone(),
            ))
            .is_ok()
        {
            self.selected = Some(index);
            self.opening = true;
            self.preview = None;
            self.zoom = 1.0;
        } else {
            self.message("Завершаем текущую операцию. Попробуй ещё раз.", true);
        }
    }
    fn open_local(&mut self, path: PathBuf) {
        self.waiting = false;
        self.open_epoch += 1;
        if self
            .workers
            .disk
            .try_send(DiskJob::Open(
                self.open_epoch,
                Source::Disk(path.clone()),
                assets::disk_metadata(&path),
            ))
            .is_ok()
        {
            self.view = View::Discover;
            self.selected = None;
            self.preview = None;
            self.opening = true;
        } else {
            self.message("Очередь коллекции занята.", true);
        }
    }
    fn save_current(&mut self) {
        let Some(index) = self.selected else {
            return;
        };
        let item = &self.history[index];
        if item.saved || self.saving.contains(&item.meta.key) {
            return;
        }
        let folder = self
            .settings
            .save_folder(&self.base, &item.meta.mode, &item.meta.tag);
        let key = item.meta.key.clone();
        if self
            .workers
            .disk
            .try_send(DiskJob::Save(key.clone(), item.source.clone(), folder))
            .is_ok()
        {
            self.saving.insert(key);
            self.message("Сохраняем полный размер…", false);
        } else {
            self.message("Очередь сохранения занята.", true);
        }
    }
    fn scan(&mut self) {
        self.library_epoch += 1;
        self.scan_due = true;
    }
    fn events(&mut self, ctx: &egui::Context) {
        while let Ok(event) = self.workers.events.try_recv() {
            match event {
                Event::Tags(result) => {
                    self.tags_loading = false;
                    match result {
                        Ok(tags) => {
                            self.tags = tags;
                            self.tags.sort_by(|a, b| {
                                (a.as_str() != "waifu", a).cmp(&(b.as_str() != "waifu", b))
                            });
                            if !self.tags.contains(&self.settings.category) {
                                self.settings.category = if self.tags.iter().any(|s| s == "waifu") {
                                    "waifu".into()
                                } else {
                                    self.tags[0].clone()
                                };
                                self.invalidate();
                            }
                        }
                        Err(e) => {
                            self.message(format!("Теги недоступны. Доступен waifu. {e}"), true)
                        }
                    }
                }
                Event::Image(epoch, result) => {
                    self.net_busy = false;
                    if epoch != self.epoch {
                        if self.waiting {
                            self.fetch();
                        }
                        continue;
                    }
                    match result {
                        Ok(loaded) => {
                            if self.waiting {
                                self.present(ctx, *loaded);
                                self.prefetch();
                            } else if self.settings.prefetch {
                                self.ready = Some(loaded);
                            }
                        }
                        Err(e) => {
                            let was_waiting = self.waiting;
                            self.waiting = false;
                            if was_waiting {
                                self.message(e, true);
                            }
                        }
                    }
                }
                Event::Open(epoch, result) => {
                    if epoch != self.open_epoch {
                        continue;
                    }
                    self.opening = false;
                    match result {
                        Ok(loaded) => {
                            if let Some(index) = self.selected {
                                self.history[index].dimensions = loaded.dimensions;
                                self.preview = Some(ctx.load_texture(
                                    "current-preview",
                                    loaded.preview,
                                    TextureOptions::LINEAR,
                                ));
                            } else {
                                self.present(ctx, *loaded);
                            }
                        }
                        Err(e) => self.message(e, true),
                    }
                }
                Event::Saved(key, result) => {
                    self.saving.remove(&key);
                    match result {
                        Ok(path) => {
                            for item in &mut self.history {
                                if item.meta.key == key {
                                    item.saved = true;
                                    item.source = Source::Disk(path.clone());
                                }
                            }
                            self.smoke_path = Some(path.clone());
                            self.message(format!("Сохранено: {}", path.display()), false);
                            self.scan();
                        }
                        Err(e) => self.message(format!("Не удалось сохранить: {e}"), true),
                    }
                }
                Event::Scan(epoch, result) => {
                    self.scanning = false;
                    if epoch != self.library_epoch {
                        continue;
                    }
                    self.scanning = false;
                    match result {
                        Ok(paths) => {
                            self.library = paths;
                        }
                        Err(e) => self.message(format!("Коллекция: {e}"), true),
                    }
                }
                Event::Thumb(path, result) => {
                    self.thumb_pending.remove(&path);
                    match result {
                        Ok(color) => {
                            while self.thumbs.len() >= 128 {
                                if let Some(old) = self.thumb_order.pop_front() {
                                    self.thumbs.remove(&old);
                                } else {
                                    break;
                                }
                            }
                            self.thumb_order.push_back(path.clone());
                            self.thumbs.insert(
                                path.clone(),
                                ctx.load_texture(
                                    path.to_string_lossy(),
                                    color,
                                    TextureOptions::LINEAR,
                                ),
                            );
                        }
                        Err(_) => {
                            self.thumb_failed.insert(path);
                        }
                    }
                }
                Event::Settings(result) => {
                    if let Err(e) = result {
                        self.message(format!("Настройки не сохранены: {e}"), true);
                    }
                }
                Event::Appearance(value) => {
                    self.appearance = value;
                    self.os_pending = false;
                }
            }
        }
    }
    fn primary(&self, ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
        let [r, g, b] = system::contrast_text(self.accent());
        ui.add_enabled(
            enabled,
            egui::Button::new(
                RichText::new(text)
                    .color(Color32::from_rgb(r, g, b))
                    .strong(),
            )
            .fill(self.ac())
            .min_size(egui::vec2(158.0, 42.0)),
        )
    }
    fn sidebar(&mut self, ctx: &egui::Context) {
        egui::SidePanel::left("sidebar")
            .exact_width(238.0)
            .resizable(false)
            .frame(egui::Frame::new().fill(self.panel()).inner_margin(20))
            .show(ctx, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    egui::Frame::new()
                        .fill(self.ac())
                        .corner_radius(11)
                        .inner_margin(10)
                        .show(ui, |ui| {
                            let [r, g, b] = system::contrast_text(self.accent());
                            ui.label(
                                RichText::new("W")
                                    .strong()
                                    .size(22.0)
                                    .color(Color32::from_rgb(r, g, b)),
                            );
                        });
                    ui.vertical(|ui| {
                        ui.label(RichText::new("waifu").size(24.0).strong());
                        ui.label(
                            RichText::new("G A L L E R Y")
                                .size(10.0)
                                .color(self.muted()),
                        );
                    });
                });
                ui.add_space(24.0);
                for (view, label) in [
                    (View::Discover, "Обзор"),
                    (View::Collection, "Моя коллекция"),
                ] {
                    let active = self.view == view;
                    if ui
                        .add_sized(
                            [ui.available_width(), 42.0],
                            egui::Button::new(RichText::new(label).strong()).fill(if active {
                                self.ac().gamma_multiply(0.22)
                            } else {
                                Color32::TRANSPARENT
                            }),
                        )
                        .clicked()
                    {
                        self.view = view;
                    }
                }
                ui.add_space(22.0);
                ui.label(
                    RichText::new("ФИЛЬТРЫ")
                        .size(11.0)
                        .color(self.muted())
                        .strong(),
                );
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    for mode in ["SFW", "NSFW"] {
                        if ui
                            .add_sized(
                                [89.0, 36.0],
                                egui::Button::new(RichText::new(mode).strong().color(
                                    if self.settings.mode == mode {
                                        let [r, g, b] = system::contrast_text(self.accent());
                                        Color32::from_rgb(r, g, b)
                                    } else {
                                        ui.visuals().text_color()
                                    },
                                ))
                                .fill(if self.settings.mode == mode {
                                    self.ac()
                                } else {
                                    self.card()
                                })
                                .stroke(egui::Stroke::NONE)
                                .corner_radius(9),
                            )
                            .clicked()
                            && self.settings.mode != mode
                        {
                            self.settings.mode = mode.into();
                            self.invalidate();
                        }
                    }
                });
                if self.settings.use_custom_url {
                    ui.label(
                        RichText::new("Свой API · фильтры задаются URL")
                            .size(11.0)
                            .color(self.muted()),
                    );
                }
                ui.add_space(8.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.tag_search)
                        .hint_text("Найти тег…")
                        .desired_width(f32::INFINITY),
                );
                if self.tags_loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.small("Загружаем теги");
                    });
                }
                let tags: Vec<_> = self
                    .tags
                    .iter()
                    .filter(|tag| tag.contains(&self.tag_search.to_lowercase()))
                    .cloned()
                    .collect();
                let height = (ui.available_height() - 130.0).max(100.0);
                egui::ScrollArea::vertical()
                    .id_salt("tags")
                    .max_height(height)
                    .show(ui, |ui| {
                        for tag in tags {
                            let active = self.settings.category == tag;
                            let text = tag.replace('-', " ");
                            let (rect, response) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 32.0),
                                egui::Sense::click(),
                            );
                            if active || response.hovered() {
                                ui.painter().rect_filled(
                                    rect,
                                    8,
                                    if active {
                                        self.ac().gamma_multiply(0.15)
                                    } else {
                                        self.card()
                                    },
                                );
                            }
                            ui.painter().text(
                                rect.left_center() + egui::vec2(12.0, 0.0),
                                egui::Align2::LEFT_CENTER,
                                text,
                                egui::FontId::proportional(14.0),
                                if active {
                                    self.ac_text()
                                } else {
                                    ui.visuals().text_color()
                                },
                            );
                            if response
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .clicked()
                                && !active
                            {
                                self.settings.category = tag;
                                self.invalidate();
                            }
                        }
                    });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.label(
                        RichText::new("Изображения · Waifu.im")
                            .size(10.0)
                            .color(self.muted()),
                    );
                    ui.add_space(10.0);
                    if ui
                        .add_sized([ui.available_width(), 38.0], egui::Button::new("Настройки"))
                        .clicked()
                    {
                        self.settings_open = true;
                    }
                });
            });
    }
    fn header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(if self.view == View::Discover {
                        "DISCOVER  /  WAIFU.IM"
                    } else {
                        "LIBRARY  /  ТВОЯ КОЛЛЕКЦИЯ"
                    })
                    .size(10.0)
                    .strong()
                    .color(self.muted()),
                );
                ui.label(
                    RichText::new(if self.view == View::Discover {
                        "Твоя следующая находка"
                    } else {
                        "Всё, что вдохновляет"
                    })
                    .size(27.0)
                    .strong(),
                );
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self
                    .primary(
                        ui,
                        if self.waiting {
                            "Загрузка…"
                        } else {
                            "Следующая"
                        },
                        !self.waiting,
                    )
                    .clicked()
                {
                    self.next(ctx);
                }
                let (enabled, label) = self
                    .selected
                    .map(|i| {
                        let item = &self.history[i];
                        (
                            !item.saved && !self.saving.contains(&item.meta.key),
                            if item.saved {
                                "Сохранено"
                            } else if self.saving.contains(&item.meta.key) {
                                "Сохраняем…"
                            } else {
                                "Сохранить"
                            },
                        )
                    })
                    .unwrap_or((false, "Сохранить"));
                if ui
                    .add_enabled(
                        enabled,
                        egui::Button::new(label).min_size(egui::vec2(130.0, 42.0)),
                    )
                    .clicked()
                {
                    self.save_current();
                }
            });
        });
        ui.add_space(16.0);
    }
    fn viewer(&mut self, ui: &mut egui::Ui) {
        let film_height = if self.history.is_empty() { 0.0 } else { 116.0 };
        let canvas_height = (ui.available_height() - film_height - 68.0).max(230.0);
        egui::Frame::new()
            .fill(self.panel())
            .corner_radius(18)
            .inner_margin(12)
            .show(ui, |ui| {
                ui.set_min_height(canvas_height);
                if self.opening || (self.preview.is_none() && self.waiting) {
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), canvas_height),
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add_space(canvas_height * 0.4);
                            ui.spinner();
                            ui.label("Готовим изображение…");
                        },
                    );
                } else if let Some(texture) = &self.preview {
                    let available = egui::vec2(ui.available_width(), canvas_height);
                    let original = texture.size_vec2();
                    let fit = (available.x / original.x)
                        .min(available.y / original.y)
                        .min(1.0);
                    let size = original * fit * self.zoom;
                    egui::ScrollArea::both()
                        .id_salt("preview")
                        .max_height(canvas_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let width = ui.available_width().max(size.x);
                            ui.allocate_ui_with_layout(
                                egui::vec2(width, canvas_height.max(size.y)),
                                egui::Layout::top_down(egui::Align::Center),
                                |ui| {
                                    ui.add_space(((canvas_height - size.y) / 2.0).max(0.0));
                                    ui.add(
                                        egui::Image::new((texture.id(), size)).corner_radius(10),
                                    );
                                },
                            );
                        });
                } else {
                    let available = egui::vec2(ui.available_width(), canvas_height);
                    ui.allocate_ui_with_layout(
                        available,
                        egui::Layout::top_down(egui::Align::Center),
                        |ui| {
                            ui.add_space((canvas_height * 0.22).max(16.0));
                            egui::Frame::new()
                                .fill(self.ac().gamma_multiply(0.15))
                                .corner_radius(24)
                                .inner_margin(22)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new("W")
                                            .size(50.0)
                                            .strong()
                                            .color(self.ac_text()),
                                    );
                                });
                            ui.add_space(16.0);
                            ui.label(RichText::new("Место для вдохновения").size(25.0).strong());
                            ui.label(
                                RichText::new("Выбери тег и нажми «Следующая».")
                                    .color(self.muted()),
                            );
                            ui.label(
                                RichText::new("Или просто нажми пробел.")
                                    .size(12.0)
                                    .color(self.muted()),
                            );
                        },
                    );
                }
            });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if let Some(index) = self.selected {
                let item = &self.history[index];
                ui.label(
                    RichText::new(format!("{} × {}", item.dimensions[0], item.dimensions[1]))
                        .strong(),
                );
                ui.label(
                    RichText::new(format!("{}  /  {}", item.meta.mode, item.meta.tag))
                        .color(self.muted()),
                );
                if !item.meta.artist.is_empty() {
                    ui.label(RichText::new(&item.meta.artist).color(self.muted()));
                }
                if let Some(source) = &item.meta.source {
                    ui.hyperlink_to("Источник ↗", source);
                }
            } else {
                ui.label(
                    RichText::new("Оригинальное разрешение при сохранении")
                        .size(12.0)
                        .color(self.muted()),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.preview.is_some() {
                    if ui.small_button("Вписать").clicked() {
                        self.zoom = 1.0;
                    }
                    ui.add(
                        egui::Slider::new(&mut self.zoom, 0.5..=3.0)
                            .show_value(false)
                            .text("Масштаб"),
                    );
                }
            });
        });
        if !self.history.is_empty() {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("НЕДАВНОЕ")
                        .size(10.0)
                        .strong()
                        .color(self.muted()),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if self.ready.is_some() {
                        ui.label(
                            RichText::new("Следующая уже готова")
                                .size(11.0)
                                .color(self.ac_text()),
                        );
                    } else if self.net_busy && !self.waiting {
                        ui.label(
                            RichText::new("Предзагрузка…")
                                .size(11.0)
                                .color(self.muted()),
                        );
                    }
                });
            });
            let mut clicked = None;
            egui::ScrollArea::horizontal()
                .id_salt("history")
                .max_height(88.0)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (index, item) in self.history.iter().enumerate() {
                            let size = egui::vec2(86.0, 68.0);
                            let response = ui
                                .add(
                                    egui::ImageButton::new(
                                        egui::Image::new((item.thumb.id(), item.thumb.size_vec2()))
                                            .fit_to_exact_size(size)
                                            .maintain_aspect_ratio(true),
                                    )
                                    .selected(self.selected == Some(index))
                                    .frame(true),
                                )
                                .on_hover_text(format!(
                                    "{} × {} · {}",
                                    item.dimensions[0], item.dimensions[1], item.meta.tag
                                ));
                            if response.clicked() {
                                clicked = Some(index);
                            }
                        }
                    });
                });
            if let Some(index) = clicked {
                self.select(index);
            }
        }
    }
    fn collection(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.library_search)
                    .hint_text("Поиск по имени файла…")
                    .desired_width(300.0),
            );
            if ui.button("Обновить").clicked() {
                self.thumbs.clear();
                self.thumb_order.clear();
                self.thumb_failed.clear();
                self.scan();
            }
            if self.scanning {
                ui.spinner();
            }
            ui.label(RichText::new(format!("Файлов: {}", self.library.len())).color(self.muted()));
        });
        ui.add_space(12.0);
        if self.library.is_empty() {
            ui.add_space(90.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("Здесь будут твои находки")
                        .size(25.0)
                        .strong(),
                );
                ui.label("Сохрани изображение из обзора — оно появится в коллекции.");
                ui.label(RichText::new("Папку можно изменить в настройках.").color(self.muted()));
            });
            return;
        }
        let filtered: Vec<_> = self
            .library
            .iter()
            .filter(|p| {
                p.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase()
                    .contains(&self.library_search.to_lowercase())
            })
            .cloned()
            .collect();
        let columns = (ui.available_width() / 194.0).floor().max(1.0) as usize;
        let tile_width = (ui.available_width() - (columns - 1) as f32 * 10.0) / columns as f32;
        let rows = filtered.len().div_ceil(columns);
        let mut open = None;
        egui::ScrollArea::vertical()
            .id_salt("library")
            .show_rows(ui, 205.0, rows, |ui, range| {
                for row in range {
                    ui.horizontal(|ui| {
                        for path in filtered.iter().skip(row * columns).take(columns) {
                            ui.allocate_ui_with_layout(
                                egui::vec2(tile_width, 203.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    egui::Frame::new()
                                        .fill(self.panel())
                                        .corner_radius(12)
                                        .inner_margin(9)
                                        .show(ui, |ui| {
                                            ui.set_width((tile_width - 18.0).max(100.0));
                                            ui.set_height(185.0);
                                            let (rect, response) = ui.allocate_exact_size(
                                                egui::vec2(ui.available_width(), 140.0),
                                                egui::Sense::click(),
                                            );
                                            ui.painter().rect_filled(rect, 8, self.card());
                                            if let Some(texture) = self.thumbs.get(path) {
                                                let size = texture.size_vec2();
                                                let scale = (rect.width() / size.x)
                                                    .min(rect.height() / size.y);
                                                let image_rect = egui::Rect::from_center_size(
                                                    rect.center(),
                                                    size * scale,
                                                );
                                                ui.painter().image(
                                                    texture.id(),
                                                    image_rect,
                                                    egui::Rect::from_min_max(
                                                        egui::pos2(0.0, 0.0),
                                                        egui::pos2(1.0, 1.0),
                                                    ),
                                                    Color32::WHITE,
                                                );
                                            } else {
                                                ui.painter().text(
                                                    rect.center(),
                                                    egui::Align2::CENTER_CENTER,
                                                    if self.thumb_failed.contains(path) {
                                                        "Не читается"
                                                    } else {
                                                        "…"
                                                    },
                                                    egui::FontId::proportional(18.0),
                                                    self.muted(),
                                                );
                                                if !self.thumb_failed.contains(path)
                                                    && !self.thumb_pending.contains(path)
                                                    && self
                                                        .workers
                                                        .disk
                                                        .try_send(DiskJob::Thumb(path.clone()))
                                                        .is_ok()
                                                {
                                                    self.thumb_pending.insert(path.clone());
                                                }
                                            }
                                            if response
                                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                                .clicked()
                                            {
                                                open = Some(path.clone());
                                            }
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(
                                                        path.file_name()
                                                            .unwrap_or_default()
                                                            .to_string_lossy(),
                                                    )
                                                    .size(11.0),
                                                )
                                                .truncate(),
                                            )
                                            .on_hover_text(path.display().to_string());
                                            ui.label(
                                                RichText::new("Открыть изображение")
                                                    .size(10.0)
                                                    .color(self.muted()),
                                            );
                                        });
                                },
                            );
                        }
                    });
                }
            });
        if let Some(path) = open {
            self.open_local(path);
        }
    }

    fn settings_window(&mut self, ctx: &egui::Context) {
        if !self.settings_open {
            return;
        }
        let before = serde_json::to_string(&self.settings).unwrap_or_default();
        let old_query = self.query();
        let old_roots = self.settings.library_roots(&self.base);
        let mut open = self.settings_open;
        egui::Window::new("Настройки").open(&mut open).default_width(510.0).default_height(680.0).default_pos(ctx.screen_rect().center()-egui::vec2(260.0,340.0)).resizable(false)
            .collapsible(false).show(ctx,|ui|{
                egui::ScrollArea::vertical().scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible).max_height(580.0).show(ui,|ui|{
                    ui.label(RichText::new("Сделай галерею своей").size(23.0).strong());
                    ui.label(RichText::new("Оформление применяется сразу.").color(self.muted()));
                    ui.add_space(14.0);ui.strong("Тема");
                    ui.horizontal(|ui|{
                        for (value,label) in [(Theme::Light,"Светлая"),(Theme::Dark,"Тёмная"),(Theme::System,"Как в Windows")]{
                            ui.selectable_value(&mut self.settings.theme,value,label);
                        }
                    });
                    ui.add_space(12.0);ui.strong("Акцентный цвет");
                    ui.checkbox(&mut self.settings.system_accent,"Использовать цвет Windows");
                    ui.horizontal(|ui|{
                        for rgb in [[159,123,255],[235,110,171],[76,164,255],[55,187,157],[245,169,77],[233,103,104]] {
                            let button=egui::Button::new("   ").fill(Color32::from_rgb(rgb[0],rgb[1],rgb[2]))
                                .min_size(egui::vec2(38.0,32.0));
                            if ui.add(button).on_hover_text(format!("#{:02X}{:02X}{:02X}",rgb[0],rgb[1],rgb[2])).clicked(){
                                self.settings.accent=rgb;self.settings.system_accent=false;
                            }
                        }
                        if ui.color_edit_button_srgb(&mut self.settings.accent).changed(){self.settings.system_accent=false;}
                    });
                    ui.add_space(12.0);ui.separator();ui.add_space(4.0);
                    ui.strong("Сохранение");
                    ui.horizontal(|ui|{
                        ui.selectable_value(&mut self.settings.save_mode,"A".into(),"По режиму и тегу");
                        ui.selectable_value(&mut self.settings.save_mode,"B".into(),"В одну папку");
                        ui.selectable_value(&mut self.settings.save_mode,"C".into(),"Своя папка");
                    });
                    if self.settings.save_mode=="C" {
                        ui.horizontal(|ui|{
                            ui.add(egui::TextEdit::singleline(&mut self.settings.custom_save_folder).hint_text("Путь к папке").desired_width(330.0));
                            if ui.button("Выбрать…").clicked(){
                                if let Some(path)=rfd::FileDialog::new().pick_folder(){self.settings.custom_save_folder=path.to_string_lossy().into();}
                            }
                        });
                    }
                    ui.label(RichText::new("PNG в полном разрешении. Существующие файлы не заменяются.").size(12.0).color(self.muted()));
                    ui.add_space(10.0);ui.strong("Локальная коллекция");
                    egui::ComboBox::from_id_salt("load-mode")
                        .selected_text(match self.settings.load_saved_mode{1=>"Текущий тег",2=>"Все теги текущего режима",_=>"Все изображения"})
                        .show_ui(ui,|ui|{
                            ui.selectable_value(&mut self.settings.load_saved_mode,1,"Текущий тег");
                            ui.selectable_value(&mut self.settings.load_saved_mode,2,"Все теги текущего режима");
                            ui.selectable_value(&mut self.settings.load_saved_mode,3,"Все изображения");
                        });
                    ui.add_space(12.0);ui.separator();ui.add_space(4.0);
                    ui.strong("Быстродействие");
                    ui.checkbox(&mut self.settings.prefetch,"Предзагружать следующую картинку");
                    ui.label(RichText::new("Один кадр вперёд. Использует дополнительный трафик.").size(12.0).color(self.muted()));
                    ui.label(RichText::new("История: до 48 кадров / 96 МБ оригиналов. Миниатюры: до 128 в кэше.").size(11.0).color(self.muted()));
                    ui.add_space(12.0);ui.collapsing("Свой API",|ui|{
                        ui.checkbox(&mut self.settings.use_custom_url,"Использовать пользовательский URL");
                        ui.add_enabled(self.settings.use_custom_url,egui::TextEdit::singleline(&mut self.settings.custom_url)
                            .hint_text("https://…").desired_width(f32::INFINITY));
                        ui.label(RichText::new("JSON с items[0].url или url. Фильтры задаются в URL.").size(12.0).color(self.muted()));
                    });
                    ui.add_space(12.0);ui.separator();
                    ui.label(RichText::new("Пробел / N — следующая   ·   S — сохранить
← / → — история   ·   Ctrl+, — настройки").size(12.0).color(self.muted()));
                    ui.add_space(6.0);
                    if ui.button("Сбросить оформление").clicked(){
                        self.settings.theme=Theme::System;self.settings.accent=[159,123,255];self.settings.system_accent=true;
                    }
                });
            });
        self.settings_open = open;
        if serde_json::to_string(&self.settings).unwrap_or_default() != before {
            self.changed();
            if self.query() != old_query {
                self.invalidate();
            } else if self.settings.library_roots(&self.base) != old_roots {
                self.scan();
            }
            if !self.settings.prefetch {
                self.ready = None;
            }
        }
    }
    fn housekeeping(&mut self, ctx: &egui::Context) {
        if self.scan_due
            && !self.scanning
            && self
                .workers
                .disk
                .try_send(DiskJob::Scan(
                    self.library_epoch,
                    self.settings.library_roots(&self.base),
                ))
                .is_ok()
        {
            self.scanning = true;
            self.scan_due = false;
        }
        if self.last_os_check.elapsed() > Duration::from_secs(2)
            && !self.os_pending
            && self.workers.disk.try_send(DiskJob::Appearance).is_ok()
        {
            self.os_pending = true;
            self.last_os_check = Instant::now();
        }
        if self
            .dirty
            .is_some_and(|t| t.elapsed() > Duration::from_millis(400))
            && self
                .workers
                .disk
                .try_send(DiskJob::Settings(self.settings.clone(), self.base.clone()))
                .is_ok()
        {
            self.dirty = None;
        }
        ctx.request_repaint_after(Duration::from_secs(2));
        if self.dirty.is_some() {
            ctx.request_repaint_after(Duration::from_millis(400));
        }
    }
    fn smoke(&mut self, ctx: &egui::Context) {
        if !self.smoke {
            return;
        }
        if self.started.elapsed() > Duration::from_secs(90) {
            let _ = std::fs::write(
                self.base.join("smoke-result.txt"),
                format!("FAILED: {}", self.status),
            );
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ctx.request_repaint_after(Duration::from_millis(50));
        let screenshot = ctx.input(|i| {
            i.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = screenshot {
            let name = match self.smoke_phase {
                4 => "preview-dark.png",
                6 => "preview-light.png",
                8 => "preview-settings.png",
                10 => "preview-collection.png",
                _ => "preview.png",
            };
            let rgba: Vec<u8> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
            let _ = image::save_buffer(
                self.base.join(name),
                &rgba,
                image.width() as u32,
                image.height() as u32,
                image::ColorType::Rgba8,
            );
            match self.smoke_phase {
                4 => {
                    self.settings.theme = Theme::Light;
                    self.smoke_phase = 5;
                }
                6 => {
                    self.settings_open = true;
                    self.capture_after = Instant::now() + Duration::from_millis(400);
                    self.smoke_phase = 7;
                }
                8 => {
                    self.settings_open = false;
                    self.view = View::Collection;
                    self.smoke_phase = 9;
                }
                10 => {
                    self.open_local(self.smoke_path.as_ref().unwrap().clone());
                    self.smoke_phase = 11;
                }
                _ => {}
            }
            ctx.request_repaint();
            return;
        }
        match self.smoke_phase {
            0 if !self.tags_loading => {
                self.next(ctx);
                self.smoke_phase = 1;
            }
            1 if self.selected.is_some() && !self.waiting => {
                self.save_current();
                self.smoke_phase = 2;
            }
            2 if self.smoke_path.is_some() && !self.scanning && !self.scan_due => {
                self.settings.theme = Theme::Dark;
                self.settings.system_accent = false;
                self.smoke_phase = 3;
                ctx.request_repaint();
            }
            3 if self.style_key.is_some_and(|s| s.0) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_phase = 4;
            }
            5 if self.style_key.is_some_and(|s| !s.0) => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_phase = 6;
            }
            7 if Instant::now() >= self.capture_after => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_phase = 8;
            }
            9 if self
                .library
                .iter()
                .take(12)
                .all(|p| self.thumbs.contains_key(p)) =>
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
                self.smoke_phase = 10;
            }
            11 if !self.opening && self.selected.is_some() => {
                let item = &self.history[self.selected.unwrap()];
                let path = self.smoke_path.as_ref().unwrap();
                let ok = image::image_dimensions(path)
                    .is_ok_and(|d| d == (item.dimensions[0], item.dimensions[1]))
                    && self.library.contains(path);
                let _ = std::fs::write(
                    self.base.join("smoke-result.txt"),
                    format!(
                        "{}: tags={}, dimensions={:?}, saved={}, collection_opened=true",
                        if ok { "OK" } else { "FAILED" },
                        self.tags.len(),
                        item.dimensions,
                        path.display()
                    ),
                );
                self.smoke_phase = 12;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }
}

impl eframe::App for GalleryApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.events(ctx);
        if ctx.input(|i| i.viewport().close_requested()) && !self.saving.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.closing = true;
            self.message("Завершаем сохранение перед закрытием…", false);
        }
        if self.closing && self.saving.is_empty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        self.housekeeping(ctx);
        self.apply_style(ctx);
        if !ctx.wants_keyboard_input() {
            if ctx.input(|i| i.key_pressed(egui::Key::Space) || i.key_pressed(egui::Key::N)) {
                self.next(ctx);
            }
            if ctx.input(|i| i.key_pressed(egui::Key::S)) {
                self.save_current();
            }
            if let Some(index) = self.selected {
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
                    self.select((index + 1).min(self.history.len() - 1));
                }
                if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
                    self.select(index.saturating_sub(1));
                }
            }
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Comma)) {
            self.settings_open = !self.settings_open;
        }
        self.sidebar(ctx);
        egui::TopBottomPanel::bottom("status")
            .frame(
                egui::Frame::new()
                    .fill(self.background())
                    .inner_margin(egui::Margin::symmetric(22, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let color = if self.error {
                        if self.dark() {
                            Color32::from_rgb(235, 110, 120)
                        } else {
                            Color32::from_rgb(174, 40, 65)
                        }
                    } else {
                        self.muted()
                    };
                    ui.add(
                        egui::Label::new(RichText::new(&self.status).size(11.0).color(color))
                            .truncate(),
                    )
                    .on_hover_text(&self.status);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("SPACE  следующая     S  сохранить")
                                .size(10.0)
                                .color(self.muted()),
                        );
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(self.background()).inner_margin(22))
            .show(ctx, |ui| {
                self.header(ui, ctx);
                if self.view == View::Discover {
                    self.viewer(ui);
                } else {
                    self.collection(ui);
                }
            });
        self.settings_window(ctx);
        self.smoke(ctx);
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        // Keep settings writes serialized with the disk worker.
        let (tx, rx) = std::sync::mpsc::channel();
        if self
            .workers
            .disk
            .send(DiskJob::Settings(self.settings.clone(), self.base.clone()))
            .is_ok()
            && self.workers.disk.send(DiskJob::Flush(tx)).is_ok()
        {
            let _ = rx.recv_timeout(Duration::from_secs(30));
        }
    }
}

fn palette(settings: &Settings, appearance: Appearance) -> (bool, [u8; 3]) {
    let dark = match settings.theme {
        Theme::Dark => true,
        Theme::Light => false,
        Theme::System => !appearance.light,
    };
    let accent = if settings.system_accent {
        appearance.accent.unwrap_or(settings.accent)
    } else {
        settings.accent
    };
    (dark, accent)
}
fn trim_history(history: &mut Vec<Item>) {
    while history.len() > HISTORY_LIMIT
        || (history.len() > 1
            && history
                .iter()
                .map(|i| i.source.memory_bytes())
                .sum::<usize>()
                > MEMORY_LIMIT)
    {
        history.pop();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn system_theme_and_accent_follow_os_changes() {
        let mut settings = Settings::default();
        let dark = Appearance {
            light: false,
            accent: Some([10, 20, 30]),
        };
        let light = Appearance {
            light: true,
            accent: Some([200, 100, 80]),
        };
        assert_eq!(palette(&settings, dark), (true, [10, 20, 30]));
        assert_eq!(palette(&settings, light), (false, [200, 100, 80]));
        settings.theme = Theme::Dark;
        settings.system_accent = false;
        settings.accent = [120, 80, 240];
        assert_eq!(palette(&settings, light), (true, [120, 80, 240]));
        assert_eq!(palette(&settings, dark), (true, [120, 80, 240]));
        settings.system_accent = true;
        assert_eq!(
            palette(
                &settings,
                Appearance {
                    light: false,
                    accent: None
                }
            )
            .1,
            settings.accent
        );
    }
    #[test]
    fn history_releases_oldest_encoded_images() {
        let ctx = egui::Context::default();
        let thumb = ctx.load_texture(
            "test",
            egui::ColorImage::new([1, 1], Color32::BLACK),
            TextureOptions::LINEAR,
        );
        let source = Source::Memory(std::sync::Arc::new(vec![0; 4 * 1024 * 1024]));
        let mut history: Vec<Item> = (0..100)
            .map(|i| Item {
                meta: Metadata {
                    key: i.to_string(),
                    source: None,
                    artist: String::new(),
                    mode: "SFW".into(),
                    tag: "waifu".into(),
                },
                source: source.clone(),
                dimensions: [1, 1],
                thumb: thumb.clone(),
                saved: false,
            })
            .collect();
        trim_history(&mut history);
        assert_eq!(history.len(), 24);
        assert_eq!(history[0].meta.key, "0");
        assert_eq!(history.last().unwrap().meta.key, "23");
    }
}
