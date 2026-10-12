use crate::{canvas::View, document::Layer};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
#[derive(PartialEq)]
struct Key {
    layer: u64,
    mask: usize,
    revision: u64,
    bounds: Rect,
    size: [usize; 2],
}
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    view: View,
    key: Option<Key>,
    texture: Option<egui::TextureHandle>,
    layer: Option<(u64, [u32; 2])>,
}
fn pixels(mask: &image::GrayImage, bounds: Rect, size: [usize; 2]) -> egui::ColorImage {
    let values = (0..size[1])
        .flat_map(|y| {
            (0..size[0]).map(move |x| {
                let u = bounds.left() + (x as f32 + 0.5) * bounds.width() / size[0] as f32;
                let v = bounds.top() + (y as f32 + 0.5) * bounds.height() / size[1] as f32;
                if u >= 0. && v >= 0. && u < mask.width() as f32 && v < mask.height() as f32 {
                    Color32::from_gray(mask.get_pixel(u.floor() as u32, v.floor() as u32)[0])
                } else {
                    Color32::from_gray(38)
                }
            })
        })
        .collect();
    egui::ColorImage::new(size, values)
}
impl Panel {
    pub fn ui(&mut self, ctx: &egui::Context, layer: Option<&Layer>, revision: u64, editing: bool) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("마스크 원본 보기")
            .open(&mut open)
            .default_width(340.)
            .resizable(true)
            .show(ctx, |ui| {
                let Some(layer) = layer else {
                    ui.label("문서를 열어 마스크가 있는 레이어를 선택하세요.");
                    return;
                };
                let Some(mask) = &layer.mask else {
                    ui.label("선택한 레이어에 마스크가 없습니다.");
                    return;
                };
                let layer_key = (layer.id, [mask.width(), mask.height()]);
                if self.layer != Some(layer_key) {
                    self.view = View::default();
                    self.layer = Some(layer_key);
                    self.key = None;
                }
                ui.label(format!(
                    "{} · {} × {} px",
                    layer.name,
                    mask.width(),
                    mask.height()
                ));
                ui.label("흰색은 표시, 검정은 숨김입니다. 원본 마스크 좌표로 표시합니다.");
                if !layer.mask_enabled {
                    ui.colored_label(Color32::LIGHT_YELLOW, "합성에서 비활성화된 마스크입니다.");
                }
                let (area, response) = ui.allocate_exact_size(
                    Vec2::new(ui.available_width().max(120.), 240.),
                    Sense::click_and_drag(),
                );
                let dimensions = [mask.width(), mask.height()];
                let dpi = ctx.pixels_per_point();
                if self.view.fit {
                    self.view.fit_to(area, dimensions, dpi);
                }
                if response.dragged() {
                    self.view.pan += response.drag_delta();
                }
                if response.hovered()
                    && let Some(pointer) = response.hover_pos()
                {
                    let scroll = ctx.input(|i| i.smooth_scroll_delta.y);
                    if scroll != 0. {
                        self.view.zoom_at(area, pointer, (scroll * 0.003).exp());
                    }
                }
                let min = self.view.document_pos(area, dimensions, dpi, area.min);
                let max = self.view.document_pos(area, dimensions, dpi, area.max);
                let bounds =
                    Rect::from_min_max(Pos2::new(min[0], min[1]), Pos2::new(max[0], max[1]));
                let size = [
                    (area.width() * dpi).ceil().clamp(1., 512.) as usize,
                    (area.height() * dpi).ceil().clamp(1., 512.) as usize,
                ];
                let key = Key {
                    layer: layer.id,
                    mask: std::sync::Arc::as_ptr(mask) as usize,
                    revision,
                    bounds,
                    size,
                };
                // Stroke pixels can change in place before the editor increments its revision.
                if self.key.as_ref() != Some(&key) || editing {
                    let image = pixels(mask, bounds, size);
                    if let Some(texture) = &mut self.texture {
                        texture.set(image, egui::TextureOptions::NEAREST);
                    } else {
                        self.texture = Some(ctx.load_texture(
                            "mask-view",
                            image,
                            egui::TextureOptions::NEAREST,
                        ));
                    }
                    self.key = Some(key);
                }
                if let Some(texture) = &self.texture {
                    ui.painter_at(area).image(
                        texture.id(),
                        area,
                        Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
                        Color32::WHITE,
                    );
                }
                if let Some(pointer) = response.hover_pos() {
                    let p = self.view.document_pos(area, dimensions, dpi, pointer);
                    if p[0] >= 0.
                        && p[1] >= 0.
                        && p[0] < mask.width() as f32
                        && p[1] < mask.height() as f32
                    {
                        let (x, y) = (p[0].floor() as u32, p[1].floor() as u32);
                        ui.label(format!(
                            "X: {x}  Y: {y} · 값: {} / 255",
                            mask.get_pixel(x, y)[0]
                        ));
                    }
                }
                ui.horizontal(|ui| {
                    if ui.button("−").clicked() {
                        self.view.zoom_at(area, area.center(), 1. / 1.25);
                    }
                    if ui.button("+").clicked() {
                        self.view.zoom_at(area, area.center(), 1.25);
                    }
                    if ui.button("100%").clicked() {
                        let factor = 1. / self.view.zoom;
                        self.view.zoom_at(area, area.center(), factor);
                    }
                    if ui.button("맞춤").clicked() {
                        self.view.fit = true;
                    }
                    ui.label(format!("{:.0}%", self.view.zoom * 100.));
                });
            });
        self.open = open;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn visible_source_pixels_zoom_empty_space_and_view_updates_do_not_edit_document() {
        let mask =
            image::GrayImage::from_fn(1024, 512, |x, y| image::Luma([((x + y) % 256) as u8]));
        let rect = Rect::from_min_max(Pos2::new(2., 4.), Pos2::new(6., 6.));
        let image = pixels(&mask, rect, [4, 2]);
        assert_eq!(image.pixels[0], Color32::from_gray(6));
        assert_eq!(image.pixels[7], Color32::from_gray(10));
        let image = pixels(
            &mask,
            Rect::from_min_size(Pos2::new(-2., 0.), Vec2::new(2., 2.)),
            [2, 2],
        );
        assert!(image.pixels.iter().all(|p| *p == Color32::from_gray(38)));
        let ctx = egui::Context::default();
        let mut doc = crate::document::Document::new(1024, 512, true).unwrap();
        doc.active_layer_mut().unwrap().mask = Some(std::sync::Arc::new(mask));
        let original = doc.clone();
        let mut panel = Panel {
            open: true,
            ..Default::default()
        };
        for editing in [false, true] {
            let mut output = ctx.run_ui(egui::RawInput::default(), |_| {
                panel.ui(&ctx, doc.active_layer(), 0, editing)
            });
            output.textures_delta.clear();
        }
        assert!(
            panel
                .texture
                .as_ref()
                .unwrap()
                .size()
                .iter()
                .all(|v| *v <= 512)
        );
        assert_eq!(doc.layers[0].pixels, original.layers[0].pixels);
        assert_eq!(doc.layers[0].mask, original.layers[0].mask);
        let old_key = panel.key.as_ref().unwrap().mask;
        std::sync::Arc::make_mut(doc.active_layer_mut().unwrap().mask.as_mut().unwrap()).put_pixel(
            2,
            4,
            image::Luma([255]),
        );
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {
            panel.ui(&ctx, doc.active_layer(), 0, true)
        });
        output.textures_delta.clear();
        assert_ne!(panel.key.as_ref().unwrap().mask, old_key);
        let pointer = panel.key.as_ref().unwrap().mask;
        std::sync::Arc::make_mut(doc.active_layer_mut().unwrap().mask.as_mut().unwrap())
            .as_mut()
            .fill(200);
        let mut output = ctx.run_ui(egui::RawInput::default(), |_| {
            panel.ui(&ctx, doc.active_layer(), 0, true)
        });
        assert_eq!(panel.key.as_ref().unwrap().mask, pointer);
        assert!(!output.textures_delta.set.is_empty());
        output.textures_delta.clear();
    }
}
