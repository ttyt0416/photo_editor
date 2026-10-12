use crate::{document::Layer, matrix};
use eframe::egui::{self, Color32, Pos2, Rect, Vec2};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub color: [u8; 3],
    pub opacity: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            color: [255, 50, 70],
            opacity: 0.4,
        }
    }
}
impl Settings {
    pub fn valid(&self) -> bool {
        self.opacity.is_finite() && (0.0..=1.).contains(&self.opacity)
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(&mut self.enabled, "마스크 오버레이");
        if self.enabled {
            ui.horizontal(|ui| {
                ui.label("숨겨지는 영역 색");
                ui.color_edit_button_srgb(&mut self.color);
            });
            ui.add(
                crate::numeric_slider::Slider::new(&mut self.opacity, 0.0..=1.)
                    .text("표시 불투명도"),
            );
        }
    }
}
#[derive(Clone, PartialEq)]
struct Key {
    layer: u64,
    mask: usize,
    revision: u64,
    inverse: matrix::Matrix,
    sampling: crate::sampling::Sampling,
    bounds: Rect,
    size: [usize; 2],
    settings: Settings,
}
#[derive(Default)]
pub struct Overlay {
    key: Option<Key>,
    texture: Option<egui::TextureHandle>,
}
pub struct Frame {
    pub image: Rect,
    pub area: Rect,
    pub scale: f32,
    pub revision: u64,
    pub editing: bool,
}
fn inverse(layer: &Layer) -> Option<matrix::Matrix> {
    matrix::inverse(crate::masks::world(layer))
}
fn coverage(layer: &Layer, inverse: matrix::Matrix, point: [f32; 2]) -> f32 {
    let Some(mask) = &layer.mask else {
        return 1.;
    };
    let local = matrix::point(inverse, point);
    crate::sampling::gray(local, crate::masks::sampling(layer), |x, y| {
        if x >= 0 && y >= 0 && x < mask.width() as i32 && y < mask.height() as i32 {
            mask.get_pixel(x as u32, y as u32)[0] as f32 / 255.
        } else {
            0.
        }
    })
}
fn pixels(layer: &Layer, key: &Key) -> egui::ColorImage {
    let mut pixels = Vec::with_capacity(key.size[0] * key.size[1]);
    for y in 0..key.size[1] {
        for x in 0..key.size[0] {
            let point = key.bounds.min
                + Vec2::new(
                    (x as f32 + 0.5) * key.bounds.width() / key.size[0] as f32,
                    (y as f32 + 0.5) * key.bounds.height() / key.size[1] as f32,
                );
            let mask = coverage(layer, key.inverse, [point.x, point.y]);
            let alpha = ((1. - mask) * key.settings.opacity * 255.).round() as u8;
            pixels.push(Color32::from_rgba_unmultiplied(
                key.settings.color[0],
                key.settings.color[1],
                key.settings.color[2],
                alpha,
            ));
        }
    }
    egui::ColorImage::new(key.size, pixels)
}
impl Overlay {
    pub fn draw(
        &mut self,
        painter: &egui::Painter,
        layer: Option<&Layer>,
        settings: &Settings,
        frame: Frame,
    ) {
        if !settings.enabled || !settings.valid() || layer.is_none_or(|layer| layer.mask.is_none())
        {
            self.texture = None;
            self.key = None;
            return;
        }
        let layer = layer.unwrap();
        let Some(inverse) = inverse(layer) else {
            return;
        };
        let visible = frame.image.intersect(frame.area);
        if !visible.is_positive() || frame.scale <= 0. {
            return;
        }
        let bounds = Rect::from_min_max(
            ((visible.min - frame.image.min) / frame.scale).to_pos2(),
            ((visible.max - frame.image.min) / frame.scale).to_pos2(),
        );
        let dpi = painter.ctx().pixels_per_point();
        let size = [
            (visible.width() * dpi).ceil().clamp(1., 512.) as usize,
            (visible.height() * dpi).ceil().clamp(1., 512.) as usize,
        ];
        let key = Key {
            layer: layer.id,
            mask: std::sync::Arc::as_ptr(layer.mask.as_ref().unwrap()) as usize,
            revision: frame.revision,
            inverse,
            sampling: crate::masks::sampling(layer),
            bounds,
            size,
            settings: settings.clone(),
        };
        if self.key.as_ref() != Some(&key) || frame.editing {
            let image = pixels(layer, &key);
            if let Some(texture) = &mut self.texture {
                texture.set(image, egui::TextureOptions::NEAREST);
            } else {
                self.texture = Some(painter.ctx().load_texture(
                    "mask-overlay",
                    image,
                    egui::TextureOptions::NEAREST,
                ));
            }
            self.key = Some(key);
        }
        if let Some(texture) = &self.texture {
            painter.with_clip_rect(visible).image(
                texture.id(),
                visible,
                Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.)),
                Color32::WHITE,
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[test]
    fn independent_mask_overlay_stays_fixed_when_layer_moves() {
        let mut doc = crate::document::Document::new(8, 4, true).unwrap();
        let l = doc.active_layer_mut().unwrap();
        l.mask = Some(Arc::new(image::GrayImage::from_fn(8, 4, |x, _| {
            image::Luma([if x < 4 { 255 } else { 0 }])
        })));
        crate::masks::set_linked(l, false).unwrap();
        l.offset = [4, 0];
        let inverse = inverse(l).unwrap();
        assert_eq!(coverage(l, inverse, [1.5, 1.5]), 1.);
        assert_eq!(coverage(l, inverse, [5.5, 1.5]), 0.);
    }
    #[test]
    fn overlay_maps_transformed_pixel_and_group_masks_and_refreshes_in_place_without_edits() {
        let mut doc = crate::document::Document::new(32, 24, true).unwrap();
        let layer = doc.active_layer_mut().unwrap();
        layer.pixels = Arc::new(image::RgbaImage::new(8, 4));
        layer.mask = Some(Arc::new(image::GrayImage::from_fn(8, 4, |x, _| {
            image::Luma([if x < 4 { 255 } else { 0 }])
        })));
        layer.offset = [10, 5];
        layer.transform.angle = 90.;
        layer.transform.scale = [-1., 1.];
        for group in [false, true] {
            layer.group = group;
            let inverse = inverse(layer).unwrap();
            for (point, expected) in [([1.5, 1.5], 1.), ([6.5, 1.5], 0.)] {
                let world = layer.transform.to_document(point, layer.offset, [8, 4]);
                assert_eq!(coverage(layer, inverse, world), expected);
            }
        }
        layer.group = false;
        layer.transform = Default::default();
        layer.offset = [0, 0];
        layer.mask_enabled = false;
        let key = Key {
            layer: layer.id,
            mask: 0,
            revision: 0,
            inverse: matrix::IDENTITY,
            sampling: crate::sampling::Sampling::Nearest,
            bounds: Rect::from_min_size(Pos2::ZERO, egui::vec2(8., 4.)),
            size: [8, 4],
            settings: Settings {
                enabled: true,
                opacity: 0.5,
                ..Default::default()
            },
        };
        let image = pixels(layer, &key);
        assert_eq!(image.pixels[1].a(), 0);
        assert_eq!(image.pixels[6].a(), 128);
        let unchanged = layer.mask.clone();
        let source = layer.pixels.clone();
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(2.);
        let mut overlay = Overlay::default();
        let draw = |overlay: &mut Overlay, layer: &Layer, editing| {
            let mut out = ctx.run_ui(Default::default(), |ui| {
                overlay.draw(
                    ui.painter(),
                    Some(layer),
                    &key.settings,
                    Frame {
                        image: Rect::from_min_size(Pos2::ZERO, egui::vec2(1024., 512.)),
                        area: Rect::from_min_size(Pos2::ZERO, egui::vec2(640., 480.)),
                        scale: 1.,
                        revision: 0,
                        editing,
                    },
                )
            });
            let changed = !out.textures_delta.set.is_empty();
            out.textures_delta.clear();
            changed
        };
        assert!(draw(&mut overlay, layer, false));
        assert!(overlay.key.as_ref().unwrap().size.iter().all(|v| *v <= 512));
        assert!(!draw(&mut overlay, layer, false));
        assert_eq!(layer.mask, unchanged);
        assert_eq!(layer.pixels, source);
        drop(unchanged);
        let pointer = Arc::as_ptr(layer.mask.as_ref().unwrap());
        Arc::make_mut(layer.mask.as_mut().unwrap()).fill(128);
        assert_eq!(Arc::as_ptr(layer.mask.as_ref().unwrap()), pointer);
        assert!(draw(&mut overlay, layer, true));
        assert_eq!(layer.pixels, source);
    }
}
