use crate::{color_model::Model, document::Document, files};
use eframe::egui::{self, Color32, Sense, Vec2};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::PathBuf,
};

// The footprint is in document pixels, including for transformed source layers.
pub fn sample(document: &Document, point: [f32; 2], size: u32, merged: bool) -> Option<[u8; 4]> {
    if !point.iter().all(|v| v.is_finite())
        || point[0] < 0.0
        || point[1] < 0.0
        || point[0] >= document.width as f32
        || point[1] >= document.height as f32
    {
        return None;
    }
    let center = point.map(|v| v.floor() as i32);
    let radius = (size.clamp(1, 101) / 2) as i32;
    let min = std::array::from_fn::<_, 2, _>(|i| (center[i] - radius).max(0) as u32);
    let bounds = [document.width, document.height];
    let max =
        std::array::from_fn::<_, 2, _>(|i| (center[i] + radius + 1).min(bounds[i] as i32) as u32);
    let layer = document.active_layer()?;
    if !merged && layer.canvas_space() {
        return None;
    }
    let regional = document.local_preview();
    let image = merged.then(|| {
        if regional {
            document.composite_region(crate::document::PixelRegion { min, max })
        } else {
            document.composite()
        }
    });
    let mut sum = [0.0f64; 4];
    let mut count = 0u32;
    for y in min[1]..max[1] {
        for x in min[0]..max[0] {
            let p = if let Some(image) = &image {
                image
                    .get_pixel(
                        if regional { x - min[0] } else { x },
                        if regional { y - min[1] } else { y },
                    )
                    .0
            } else {
                let p = layer.transform.to_local(
                    [x as f32 + 0.5, y as f32 + 0.5],
                    layer.offset,
                    [layer.pixels.width(), layer.pixels.height()],
                );
                if p[0] < 0.0
                    || p[1] < 0.0
                    || p[0] >= layer.pixels.width() as f32
                    || p[1] >= layer.pixels.height() as f32
                {
                    [0; 4]
                } else {
                    layer
                        .pixels
                        .get_pixel(p[0].floor() as u32, p[1].floor() as u32)
                        .0
                }
            };
            let alpha = f64::from(p[3]);
            for c in 0..3 {
                sum[c] += f64::from(p[c]) * alpha;
            }
            sum[3] += alpha;
            count += 1;
        }
    }
    Some(std::array::from_fn(|c| {
        if c == 3 {
            (sum[3] / f64::from(count)).round() as u8
        } else if sum[3] > 0.0 {
            (sum[c] / sum[3]).round() as u8
        } else {
            0
        }
    }))
}

pub fn hex(color: [u8; 4]) -> String {
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        color[0], color[1], color[2], color[3]
    )
}
pub fn parse_hex(value: &str) -> Option<[u8; 4]> {
    let text = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if !matches!(text.len(), 6 | 8) || !text.is_ascii() {
        return None;
    }
    let mut color = [255; 4];
    for (i, c) in color.iter_mut().take(text.len() / 2).enumerate() {
        *c = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(color)
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    version: u8,
    pub foreground: [u8; 4],
    pub background: [u8; 4],
    recent: Vec<[u8; 4]>,
    palette: Vec<[u8; 4]>,
    #[serde(default)]
    model: Model,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            version: 1,
            foreground: [90, 153, 245, 255],
            background: [255; 4],
            recent: vec![],
            model: Model::Rgb,
            palette: vec![
                [0, 0, 0, 255],
                [255; 4],
                [230, 60, 60, 255],
                [250, 170, 40, 255],
                [250, 230, 50, 255],
                [60, 190, 90, 255],
                [40, 180, 200, 255],
                [70, 110, 240, 255],
                [150, 80, 210, 255],
                [230, 90, 170, 255],
            ],
        }
    }
}
impl Library {
    fn remember(&mut self, color: [u8; 4]) {
        self.recent.retain(|c| *c != color);
        self.recent.insert(0, color);
        self.recent.truncate(16);
    }
    fn valid(&self) -> bool {
        self.version == 1 && self.recent.len() <= 16 && self.palette.len() <= 256
    }
}

#[derive(Default)]
pub struct Panel {
    pub open: bool,
    background: bool,
    value: String,
    displayed_color: Option<[u8; 4]>,
    model_color: Option<[u8; 3]>,
    model_mode: Option<Model>,
    model_target: bool,
    model_values: [f64; 3],
    pub library: Library,
    path: Option<PathBuf>,
    saved: Library,
}
impl Panel {
    pub fn load(path: PathBuf) -> Result<Self, String> {
        let library = if path.exists() {
            let file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
            if file.metadata().map_err(|e| e.to_string())?.len() > 64 * 1024 {
                return Err("색상 설정이 너무 큽니다.".into());
            }
            let mut bytes = Vec::new();
            file.take(64 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|e| e.to_string())?;
            let library: Library = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
            if !library.valid() {
                return Err("지원하지 않는 색상 설정입니다. 기존 파일을 유지합니다.".into());
            }
            library
        } else {
            Library::default()
        };
        Ok(Self {
            saved: library.clone(),
            library,
            path: Some(path),
            ..Default::default()
        })
    }
    pub fn show(&mut self, foreground: [u8; 4], background: [u8; 4]) {
        self.open = true;
        self.value = hex(if self.background {
            background
        } else {
            foreground
        });
    }
    pub fn stop_persistence(&mut self) {
        self.path = None;
    }
    pub fn observe(
        &mut self,
        foreground: [u8; 4],
        background: [u8; 4],
        finished: bool,
    ) -> Result<(), String> {
        if !finished {
            return Ok(());
        }
        for (old, new) in [
            (self.library.foreground, foreground),
            (self.library.background, background),
        ] {
            if old != new {
                self.library.remember(new);
            }
        }
        self.library.foreground = foreground;
        self.library.background = background;
        if self.library == self.saved {
            return Ok(());
        }
        if let Some(path) = &self.path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let bytes = serde_json::to_vec(&self.library).map_err(|e| e.to_string())?;
            files::atomic_write(path, |file| {
                file.write_all(&bytes).map_err(|e| e.to_string())
            })?;
        }
        self.saved = self.library.clone();
        Ok(())
    }
    pub fn ui(&mut self, ctx: &egui::Context, foreground: &mut [u8; 4], background: &mut [u8; 4]) {
        if !self.open {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("colors-panel")).show(ctx, |ui| {
            let width = (ctx.content_rect().width() - 64.).clamp(120., 440.);
            ui.set_width(width);
            egui::ScrollArea::both()
                .id_salt("color-panel-body")
                .max_width(width)
                .max_height((ctx.content_rect().height() - 120.).clamp(70., 560.))
                .show(ui, |ui| {
                    ui.set_max_width(width);
                    self.contents(ui, foreground, background);
                });
            ui.separator();
            if ui.button("닫기").clicked() {
                self.open = false;
            }
        });
        if response.should_close() {
            self.open = false;
        }
    }
    pub fn contents(
        &mut self,
        ui: &mut egui::Ui,
        foreground: &mut [u8; 4],
        background: &mut [u8; 4],
    ) {
        ui.heading("색상");
        let before = self.background;
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.background, false, "전경색");
            ui.selectable_value(&mut self.background, true, "배경색");
        });
        let color = if self.background {
            background
        } else {
            foreground
        };
        if before != self.background
            || (self.displayed_color != Some(*color)
                && !ui.ctx().memory(|m| m.has_focus(ui.id().with("hex-value"))))
        {
            self.value = hex(*color);
        }
        let mut candidate = *color;
        let mut changed = ui
            .color_edit_button_srgba_unmultiplied(&mut candidate)
            .changed();
        if changed {
            *color = candidate;
        }
        ui.horizontal_wrapped(|ui| {
            for model in Model::ALL {
                ui.selectable_value(&mut self.library.model, model, model.label());
            }
        });
        let model = self.library.model;
        let rgb = [color[0], color[1], color[2]];
        if self.model_color != Some(rgb)
            || self.model_mode != Some(model)
            || self.model_target != self.background
        {
            self.model_values = model.components(*color);
        }
        if model == Model::Rgb {
            ui.horizontal_wrapped(|ui| {
                for (index, label) in ["R", "G", "B", "A"].into_iter().enumerate() {
                    changed |= ui
                        .add(
                            egui::DragValue::new(&mut color[index])
                                .range(0..=255)
                                .prefix(format!("{label} ")),
                        )
                        .changed();
                }
            });
        } else {
            let labels = if model == Model::Hsv {
                ["색조 °", "채도 %", "밝기 %"]
            } else {
                ["색조 °", "채도 %", "명도 %"]
            };
            let before = self.model_values;
            let mut model_changed = false;
            for (i, label) in labels.into_iter().enumerate() {
                model_changed |= ui
                    .add(
                        crate::numeric_slider::Slider::new(
                            &mut self.model_values[i],
                            0. ..=if i == 0 { 360. } else { 100. },
                        )
                        .text(label),
                    )
                    .changed();
            }
            if model_changed {
                if let Some(rgb) = model.rgb(self.model_values) {
                    color[..3].copy_from_slice(&rgb);
                    changed = true;
                } else {
                    self.model_values = before;
                }
            }
            changed |= ui
                .add(
                    egui::DragValue::new(&mut color[3])
                        .range(0..=255)
                        .prefix("A "),
                )
                .changed();
        }
        self.model_color = Some([color[0], color[1], color[2]]);
        self.model_mode = Some(model);
        self.model_target = self.background;
        if changed {
            self.value = hex(*color);
        }
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.value)
                    .id(ui.id().with("hex-value"))
                    .desired_width(140.0)
                    .hint_text("#RRGGBB 또는 #RRGGBBAA"),
            );
            let parsed = parse_hex(&self.value);
            let apply = ui
                .add_enabled(parsed.is_some(), egui::Button::new("값 적용"))
                .clicked()
                || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if apply && let Some(value) = parsed {
                *color = value;
                self.value = hex(value);
            }
        });
        if parse_hex(&self.value).is_none() {
            ui.colored_label(
                Color32::LIGHT_RED,
                "6자리 RGB 또는 8자리 RGBA 색상 값을 입력하세요.",
            );
        }
        for (title, colors) in [
            ("최근 색상", &self.library.recent),
            ("팔레트", &self.library.palette),
        ] {
            ui.label(title);
            egui::ScrollArea::vertical()
                .id_salt(title)
                .max_height(110.0)
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (index, c) in colors.iter().enumerate() {
                            let response = ui
                                .push_id((title, index), |ui| {
                                    let (rect, response) =
                                        ui.allocate_exact_size(Vec2::splat(24.0), Sense::click());
                                    ui.painter().rect_filled(
                                        rect,
                                        2.0,
                                        Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]),
                                    );
                                    ui.painter().rect_stroke(
                                        rect,
                                        2.0,
                                        egui::Stroke::new(1.0, Color32::GRAY),
                                        egui::StrokeKind::Inside,
                                    );
                                    response.on_hover_text(hex(*c))
                                })
                                .inner;
                            if response.clicked() {
                                *color = *c;
                                self.value = hex(*c);
                            }
                        }
                    });
                });
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.library.palette.len() < 256 && !self.library.palette.contains(color),
                    egui::Button::new("팔레트에 추가"),
                )
                .clicked()
            {
                self.library.palette.push(*color);
            }
            if ui
                .add_enabled(
                    self.library.palette.contains(color),
                    egui::Button::new("팔레트에서 제거"),
                )
                .clicked()
            {
                self.library.palette.retain(|c| c != color);
            }
        });
        self.displayed_color = Some(*color);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};
    use std::sync::Arc;
    include!("color_model_ui_tests.rs");

    #[test]
    fn docked_color_controls_sync_external_changes_without_mutating_document_or_color() {
        let ctx = egui::Context::default();
        let mut panel = Panel::default();
        let mut background = [255; 4];
        for color in [[90, 153, 245, 255], [23, 45, 67, 89]] {
            let mut foreground = color;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::new(220., 600.),
                    )),
                    ..Default::default()
                },
                |ui| panel.contents(ui, &mut foreground, &mut background),
            );
            output.textures_delta.clear();
            assert_eq!(foreground, color);
            assert_eq!(background, [255; 4]);
            assert_eq!(panel.value, hex(color));
            assert!(!panel.open);
        }
    }
    #[test]
    fn averaging_ignores_transparent_rgb_and_clips_at_edges_in_document_space() {
        let mut doc = Document::new(3, 3, true).unwrap();
        let mut pixels = RgbaImage::from_pixel(3, 3, Rgba([0, 255, 0, 0]));
        pixels.put_pixel(1, 1, Rgba([255, 0, 0, 255]));
        pixels.put_pixel(0, 0, Rgba([0, 0, 255, 128]));
        doc.layers[0].pixels = Arc::new(pixels);
        assert_eq!(sample(&doc, [1.8, 1.6], 1, false), Some([255, 0, 0, 255]));
        assert_eq!(sample(&doc, [1.0, 1.0], 101, false), Some([170, 0, 85, 43]));
        assert_eq!(sample(&doc, [0.0, 0.0], 3, false), Some([170, 0, 85, 96]));
        assert_eq!(sample(&doc, [-0.1, 1.0], 1, true), None);
        assert_eq!(sample(&doc, [f32::NAN, 1.0], 1, true), None);
        let mut doc = Document::new(4, 2, true).unwrap();
        doc.layers[0].pixels = Arc::new(RgbaImage::from_fn(2, 2, |x, _| {
            Rgba(if x == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            })
        }));
        doc.layers[0].offset = [1, 0];
        doc.layers[0].transform.scale = [-1.0, 1.0];
        doc.layers[0].opacity = 0.5;
        assert_eq!(sample(&doc, [2.3, 0.3], 1, false), Some([255, 0, 0, 255]));
        assert_eq!(sample(&doc, [1.3, 0.3], 1, false), Some([0, 0, 255, 255]));
        assert_eq!(sample(&doc, [2.3, 0.3], 1, true), Some([255, 0, 0, 128]));
        doc.layers[0].pixels = Arc::new(RgbaImage::from_pixel(2, 2, Rgba([0, 255, 0, 255])));
        assert_eq!(sample(&doc, [2.3, 0.3], 1, true), Some([0, 255, 0, 128]));
        let mut doc = Document::new(9, 9, true).unwrap();
        doc.layers[0].pixels = Arc::new(RgbaImage::from_fn(9, 9, |x, y| {
            Rgba([if x == 5 { 255 } else { 0 }, (y * 20) as u8, 50, 255])
        }));
        doc.layers[0].adjustments.blur = 2.0;
        assert!(!doc.local_preview());
        let expected = doc.composite().get_pixel(5, 6).0;
        assert_ne!(expected, doc.layers[0].pixels.get_pixel(5, 6).0);
        assert_eq!(sample(&doc, [5.2, 6.3], 1, true), Some(expected));
    }

    #[test]
    fn color_values_recent_palette_and_committed_settings_roundtrip_without_replacing_bad_input() {
        assert_eq!(parse_hex(" #Aa2200 "), Some([170, 34, 0, 255]));
        assert_eq!(parse_hex("00000000"), Some([0; 4]));
        for s in ["#GG1122", "abcd", "あいう", "123456789"] {
            assert_eq!(parse_hex(s), None);
        }
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/colors/settings.json");
        let mut panel = Panel {
            path: Some(path.clone()),
            ..Default::default()
        };
        for v in 0..20 {
            panel.library.remember([v, 0, 0, 255]);
        }
        panel.library.remember([19, 0, 0, 255]);
        assert_eq!(panel.library.recent.len(), 16);
        panel.library.palette.push([40, 60, 80, 32]);
        let original = panel.library.foreground;
        panel.observe([1, 2, 3, 4], [255; 4], false).unwrap();
        assert_eq!(panel.library.foreground, original);
        panel
            .observe([10, 20, 30, 40], [80, 90, 100, 110], true)
            .unwrap();
        let reloaded = Panel::load(path.clone()).unwrap();
        assert!(reloaded.library == panel.library);
        assert_eq!(reloaded.library.recent[0], [80, 90, 100, 110]);
        assert!(!reloaded.library.recent.contains(&[1, 2, 3, 4]));
        let invalid = path.with_file_name("invalid.json");
        std::fs::write(&invalid, b"{\"version\":99}").unwrap();
        assert!(Panel::load(invalid.clone()).is_err());
        assert_eq!(std::fs::read(invalid).unwrap(), b"{\"version\":99}");
    }
}
