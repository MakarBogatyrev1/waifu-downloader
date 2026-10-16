use eframe::egui::{self, Color32, Pos2, Response, RichText, Stroke, Ui, Vec2, Widget};
#[derive(Clone, Copy)]
pub enum Icon {
    Image,
    Collection,
    Heart,
    HeartFilled,
    Folder,
    Settings,
    User,
    Download,
    Shuffle,
    Refresh,
    Check,
}
pub fn for_label(label: &str) -> Icon {
    let s = label.to_lowercase();
    if s.contains("избранном") {
        Icon::HeartFilled
    } else if s.contains("избран") {
        Icon::Heart
    } else if s.contains("сохранено") {
        Icon::Check
    } else if s.contains("сохран") {
        Icon::Download
    } else if s.contains("настрой") {
        Icon::Settings
    } else if s.contains("синхрон") || s.contains("обнов") {
        Icon::Refresh
    } else if s.contains("аккаунт") || s.contains("войти") || s.contains("вход") {
        Icon::User
    } else {
        Icon::Shuffle
    }
}
pub struct IconButton {
    label: String,
    icon: Icon,
    fill: Option<Color32>,
    color: Option<Color32>,
    size: Vec2,
    strong: bool,
}
pub fn button(label: impl Into<String>, icon: Icon) -> IconButton {
    IconButton {
        label: label.into(),
        icon,
        fill: None,
        color: None,
        size: Vec2::ZERO,
        strong: false,
    }
}
impl IconButton {
    pub fn fill(mut self, c: Color32) -> Self {
        self.fill = Some(c);
        self
    }
    pub fn color(mut self, c: Color32) -> Self {
        self.color = Some(c);
        self
    }
    pub fn strong(mut self) -> Self {
        self.strong = true;
        self
    }
    pub fn min_size(mut self, s: Vec2) -> Self {
        self.size = s;
        self
    }
}
impl Widget for IconButton {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut text = RichText::new(format!("        {}", self.label));
        if self.strong {
            text = text.strong()
        }
        if let Some(c) = self.color {
            text = text.color(c)
        }
        let mut button = egui::Button::new(text).min_size(self.size);
        if let Some(c) = self.fill {
            button = button.fill(c)
        }
        let response = ui.add(button);
        let color = self
            .color
            .unwrap_or_else(|| ui.style().interact(&response).fg_stroke.color);
        paint(
            ui,
            self.icon,
            Pos2::new(response.rect.left() + 19.0, response.rect.center().y),
            18.0,
            if ui.is_enabled() {
                color
            } else {
                color.gamma_multiply(0.45)
            },
        );
        response
    }
}
pub fn paint(ui: &Ui, icon: Icon, center: Pos2, size: f32, color: Color32) {
    let p = ui.painter();
    let point = |x: f32, y: f32| center + egui::vec2(x - 12.0, y - 12.0) * (size / 24.0);
    let stroke = Stroke::new((size / 12.0).max(1.2), color);
    let line = |points: &[(f32, f32)]| {
        p.add(egui::Shape::line(
            points.iter().map(|(x, y)| point(*x, *y)).collect(),
            stroke,
        ));
    };
    let rect = |a: (f32, f32), b: (f32, f32)| {
        p.rect_stroke(
            egui::Rect::from_min_max(point(a.0, a.1), point(b.0, b.1)),
            3,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    match icon {
        Icon::Image => {
            rect((3., 3.), (21., 21.));
            p.circle_stroke(point(8., 8.), 1.8 * size / 24., stroke);
            line(&[(4., 18.), (10., 12.), (14., 16.), (17., 12.), (21., 17.)]);
        }
        Icon::Collection => {
            rect((6., 5.), (22., 21.));
            line(&[(17., 2.), (2., 2.), (2., 17.)]);
            line(&[(7., 18.), (12., 12.), (16., 16.), (19., 12.), (21., 15.)]);
        }
        Icon::Folder => line(&[
            (3., 7.),
            (3., 20.),
            (21., 20.),
            (21., 7.),
            (12., 7.),
            (9., 4.),
            (3., 4.),
            (3., 7.),
        ]),
        Icon::Download => {
            line(&[(12., 3.), (12., 15.)]);
            line(&[(7., 10.), (12., 15.), (17., 10.)]);
            line(&[(3., 15.), (3., 21.), (21., 21.), (21., 15.)]);
        }
        Icon::Check => line(&[(4., 12.), (9., 17.), (20., 6.)]),
        Icon::Shuffle => {
            line(&[(3., 5.), (7., 5.), (17., 19.), (22., 19.)]);
            line(&[(3., 19.), (7., 19.), (17., 5.), (22., 5.)]);
            line(&[(18., 1.), (22., 5.), (18., 9.)]);
            line(&[(18., 15.), (22., 19.), (18., 23.)]);
        }
        Icon::Refresh => {
            line(&[
                (20., 8.),
                (17., 4.),
                (11., 3.),
                (5., 6.),
                (3., 12.),
                (6., 18.),
                (12., 21.),
                (18., 18.),
            ]);
            line(&[(20., 2.), (20., 8.), (14., 8.)]);
        }
        Icon::User => {
            p.circle_stroke(point(12., 7.), 4. * size / 24., stroke);
            line(&[
                (3., 22.),
                (4., 17.),
                (8., 14.),
                (16., 14.),
                (20., 17.),
                (21., 22.),
            ]);
        }
        Icon::Settings => {
            p.circle_stroke(center, 4. * size / 24., stroke);
            p.circle_stroke(center, 8. * size / 24., stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.;
                let v = egui::vec2(a.cos(), a.sin());
                p.line_segment(
                    [
                        center + v * (7. * size / 24.),
                        center + v * (11. * size / 24.),
                    ],
                    Stroke::new(size / 6., color),
                );
            }
        }
        Icon::Heart | Icon::HeartFilled => {
            let mut points = vec![];
            for i in 0..49 {
                let t = i as f32 * std::f32::consts::TAU / 48.;
                let x = 16. * t.sin().powi(3);
                let y = 13. * t.cos() - 5. * (2. * t).cos() - 2. * (3. * t).cos() - (4. * t).cos();
                points.push(center + egui::vec2(x, -y - 1.) * (size / 35.));
            }
            if matches!(icon, Icon::HeartFilled) {
                p.circle_filled(point(8., 9.), 4.6 * size / 24., color);
                p.circle_filled(point(16., 9.), 4.6 * size / 24., color);
                p.add(egui::Shape::convex_polygon(
                    vec![point(3.5, 10.), point(20.5, 10.), point(12., 21.)],
                    color,
                    Stroke::NONE,
                ));
            } else {
                p.add(egui::Shape::line(points, stroke));
            }
        }
    }
}
