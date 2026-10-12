use eframe::egui::{self, Color32, Rect, Sense, TextureHandle, Vec2};
use image::{Rgba, Rgba32FImage, RgbaImage};
use std::sync::Arc;
pub fn thumbnail(source: &Rgba32FImage) -> RgbaImage {
    let (w, h) = source.dimensions();
    let scale = (512. / f64::from(w)).min(256. / f64::from(h)).min(1.);
    let tw = (f64::from(w) * scale).floor().max(1.) as u32;
    let th = (f64::from(h) * scale).floor().max(1.) as u32;
    let sx = f64::from(w) / f64::from(tw);
    let sy = f64::from(h) / f64::from(th);
    RgbaImage::from_fn(tw, th, |x, y| {
        if tw == w && th == h {
            return display(source.get_pixel(x, y).0);
        }
        let (left, right, top, bottom) = (
            f64::from(x) * sx,
            f64::from(x + 1) * sx,
            f64::from(y) * sy,
            f64::from(y + 1) * sy,
        );
        let mut sum = [0f64; 4];
        let mut total = 0.;
        for py in top.floor() as u32..(bottom.ceil() as u32).min(h) {
            for px in left.floor() as u32..(right.ceil() as u32).min(w) {
                let weight = (right.min(f64::from(px + 1)) - left.max(f64::from(px)))
                    * (bottom.min(f64::from(py + 1)) - top.max(f64::from(py)));
                let p = source
                    .get_pixel(px, py)
                    .0
                    .map(|v| f64::from(v).clamp(0., 1.));
                for c in 0..3 {
                    sum[c] += p[c] * p[3] * weight;
                }
                sum[3] += p[3] * weight;
                total += weight;
            }
        }
        Rgba([
            if sum[3] > 0. {
                (sum[0] / sum[3] * 255.).round() as u8
            } else {
                0
            },
            if sum[3] > 0. {
                (sum[1] / sum[3] * 255.).round() as u8
            } else {
                0
            },
            if sum[3] > 0. {
                (sum[2] / sum[3] * 255.).round() as u8
            } else {
                0
            },
            (sum[3] / total * 255.).round() as u8,
        ])
    })
}
fn display(p: [f32; 4]) -> Rgba<u8> {
    Rgba(p.map(|v| (v.clamp(0., 1.) * 255.).round() as u8))
}
pub fn detail(source: &Rgba32FImage, focus: [f32; 2]) -> RgbaImage {
    let size = [source.width().min(160), source.height().min(160)];
    let dimensions = [source.width(), source.height()];
    let start: [u32; 2] = std::array::from_fn(|i| {
        ((focus[i].clamp(0., 1.) * dimensions[i].saturating_sub(1) as f32).round() as u32)
            .saturating_sub(size[i] / 2)
            .min(dimensions[i] - size[i])
    });
    RgbaImage::from_fn(size[0], size[1], |x, y| {
        display(source.get_pixel(x + start[0], y + start[1]).0)
    })
}
fn texture(
    ctx: &egui::Context,
    label: &str,
    image: &RgbaImage,
    options: egui::TextureOptions,
) -> TextureHandle {
    ctx.load_texture(
        label,
        egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        ),
        options,
    )
}
pub struct View {
    before: Arc<Rgba32FImage>,
    after: Option<Arc<Rgba32FImage>>,
    before_fit: Option<TextureHandle>,
    after_fit: Option<TextureHandle>,
    detail_textures: Option<[TextureHandle; 2]>,
    focus: [f32; 2],
}
impl View {
    pub fn new(before: Arc<Rgba32FImage>) -> Self {
        Self {
            before,
            after: None,
            before_fit: None,
            after_fit: None,
            detail_textures: None,
            focus: [0.5; 2],
        }
    }
    pub fn before_ready(&mut self, ctx: &egui::Context, preview: &RgbaImage) {
        self.before_fit = Some(texture(
            ctx,
            "raw-before",
            preview,
            egui::TextureOptions::LINEAR,
        ));
    }
    pub fn after_ready(
        &mut self,
        ctx: &egui::Context,
        pixels: Arc<Rgba32FImage>,
        preview: &RgbaImage,
    ) {
        self.after = Some(pixels);
        self.after_fit = Some(texture(
            ctx,
            "raw-after",
            preview,
            egui::TextureOptions::LINEAR,
        ));
        self.detail_textures = None;
    }
    pub fn restore_before(&mut self) {
        self.after = Some(self.before.clone());
        self.after_fit = self.before_fit.clone();
        self.detail_textures = None;
    }
    pub fn clear_after(&mut self) {
        self.after = None;
        self.after_fit = None;
        self.detail_textures = None;
    }
    #[cfg(test)]
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let _ = self.ui_pick(ui, false);
    }
    #[cfg(test)]
    pub fn ui_pick(&mut self, ui: &mut egui::Ui, picking: bool) -> Option<[f32; 2]> {
        self.ui_pick_sized(ui, picking, 180.)
    }
    pub fn ui_pick_sized(
        &mut self,
        ui: &mut egui::Ui,
        picking: bool,
        fit_height: f32,
    ) -> Option<[f32; 2]> {
        let mut sample = None;
        ui.label("전체 보기에서 클릭하면 같은 위치를 100%로 비교합니다.");
        let mut picked = None;
        ui.columns(2, |columns| {
            for (i, t) in [self.before_fit.as_ref(), self.after_fit.as_ref()]
                .into_iter()
                .enumerate()
            {
                columns[i].label(if i == 0 { "원본" } else { "현상 결과" });
                if let Some(t) = t {
                    let size = t.size_vec2();
                    let scale = (columns[i].available_width() / size.x)
                        .min(fit_height.clamp(48., 512.) / size.y)
                        .min(1.);
                    let (response, image_rect) = paint(&mut columns[i], t, size * scale);
                    let response = if picking
                        && i == 1
                        && columns[i].input(|input| {
                            input
                                .pointer
                                .hover_pos()
                                .is_some_and(|pos| image_rect.contains(pos))
                        }) {
                        response.on_hover_cursor(egui::CursorIcon::Crosshair)
                    } else {
                        response
                    };
                    if response.clicked()
                        && let Some(p) = response.interact_pointer_pos()
                        && image_rect.contains(p)
                    {
                        let v = (p - image_rect.min) / image_rect.size();
                        picked = Some([v.x.clamp(0., 1.), v.y.clamp(0., 1.)]);
                        if picking && i == 1 {
                            sample = picked;
                        }
                    }
                } else {
                    columns[i].weak("미리보기 준비 중…");
                }
            }
        });
        if let Some(focus) = picked {
            self.focus = focus;
            self.detail_textures = None;
        }
        if self.detail_textures.is_none()
            && let Some(after) = &self.after
        {
            self.detail_textures = Some([
                texture(
                    ui.ctx(),
                    "raw-before-detail",
                    &detail(&self.before, self.focus),
                    egui::TextureOptions::NEAREST,
                ),
                texture(
                    ui.ctx(),
                    "raw-after-detail",
                    &detail(after, self.focus),
                    egui::TextureOptions::NEAREST,
                ),
            ]);
        }
        if let Some(details) = &self.detail_textures {
            ui.label(format!(
                "비교 위치 {:.1}% · {:.1}%",
                self.focus[0] * 100.,
                self.focus[1] * 100.
            ));
            ui.horizontal_wrapped(|ui| {
                for (i, t) in details.iter().enumerate() {
                    ui.vertical(|ui| {
                        ui.label(if i == 0 {
                            "원본 100%"
                        } else {
                            "현상 결과 100%"
                        });
                        paint(ui, t, t.size_vec2() / ui.ctx().pixels_per_point());
                    });
                }
            });
        }
        sample
    }
}
fn paint(ui: &mut egui::Ui, t: &TextureHandle, size: Vec2) -> (egui::Response, Rect) {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);
    let cell = 8.;
    let cols = (size.x / cell).ceil() as u32;
    let rows = (size.y / cell).ceil() as u32;
    for y in 0..rows {
        for x in 0..cols {
            let at = rect.min + Vec2::new(x as f32 * cell, y as f32 * cell);
            painter.rect_filled(
                Rect::from_min_size(at, Vec2::splat(cell)).intersect(rect),
                0.,
                if (x + y) % 2 == 0 {
                    Color32::from_gray(160)
                } else {
                    Color32::from_gray(210)
                },
            );
        }
    }
    painter.image(
        t.id(),
        rect,
        Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1., 1.)),
        Color32::WHITE,
    );
    (response, rect)
}
#[cfg(test)]
#[path = "raw_preview_tests.rs"]
mod tests;
