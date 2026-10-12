use crate::{
    document::{Document, Selection},
    vector::Content,
};
use base64::Engine;
use eframe::egui::{self, Vec2};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    pub tile_size: [u32; 2],
    pub scale: [f32; 2],
    pub rotation: f32,
    pub offset: [f32; 2],
    pub smooth: bool,
    #[serde(default = "unit_scale")]
    pub canvas_scale: [f32; 2],
}
fn unit_scale() -> [f32; 2] {
    [1., 1.]
}
#[derive(Clone, Debug, PartialEq)]
pub struct Pattern {
    pub settings: Settings,
    pub data: String,
}
impl Pattern {
    pub fn from_tile(tile: &RgbaImage, size: [u32; 2]) -> Result<Self, String> {
        crate::document::validate_size(tile.width(), tile.height())?;
        crate::document::validate_size(size[0], size[1])?;
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(tile.clone())
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            settings: Settings {
                width: size[0],
                height: size[1],
                tile_size: [tile.width(), tile.height()],
                scale: [1., 1.],
                rotation: 0.,
                offset: [0.; 2],
                smooth: false,
                canvas_scale: [1., 1.],
            },
            data: format!(
                "data:image/png;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
            ),
        })
    }
    pub fn read(source: &str) -> Option<Self> {
        let doc = roxmltree::Document::parse(source).ok()?;
        let settings: Settings =
            serde_json::from_str(doc.root_element().attribute("data-photo-pattern")?).ok()?;
        let image = doc
            .descendants()
            .find(|n| n.has_tag_name("pattern"))?
            .descendants()
            .find(|n| n.has_tag_name("image"))?;
        let data = image
            .attribute("href")
            .or_else(|| image.attribute(("http://www.w3.org/1999/xlink", "href")))?
            .to_string();
        let pattern = Self { settings, data };
        pattern.validate().ok()?;
        Some(pattern)
    }
    pub fn validate(&self) -> Result<(), String> {
        crate::document::validate_size(self.settings.width, self.settings.height)?;
        crate::document::validate_size(self.settings.tile_size[0], self.settings.tile_size[1])?;
        if self
            .settings
            .scale
            .iter()
            .any(|v| !v.is_finite() || !(0.01..=100.).contains(v))
            || self
                .settings
                .canvas_scale
                .iter()
                .any(|v| !v.is_finite() || !(1e-6..=1e6).contains(v))
            || !self.settings.rotation.is_finite()
            || self.settings.rotation.abs() > 360.
            || self
                .settings
                .offset
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 1e9)
            || !self.data.starts_with("data:image/png;base64,")
        {
            return Err("패턴 배율·회전·위치·타일을 확인하세요.".into());
        }
        Ok(())
    }
    pub fn source(&self) -> Result<String, String> {
        self.validate()?;
        let s = &self.settings;
        let metadata = crate::vector::escape(&serde_json::to_string(s).map_err(|e| e.to_string())?);
        Ok(format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}' data-photo-pattern='{metadata}'><defs><pattern id='photo-pattern' patternUnits='userSpaceOnUse' width='{}' height='{}' patternTransform='translate({},{}) scale({},{}) rotate({}) scale({},{})'><image width='{}' height='{}' href='{}' image-rendering='{}'/></pattern></defs><rect width='100%' height='100%' fill='url(#photo-pattern)'/></svg>",
            s.width,
            s.height,
            s.tile_size[0],
            s.tile_size[1],
            s.offset[0],
            s.offset[1],
            s.canvas_scale[0],
            s.canvas_scale[1],
            s.rotation,
            s.scale[0],
            s.scale[1],
            s.tile_size[0],
            s.tile_size[1],
            crate::vector::escape(&self.data),
            if s.smooth { "auto" } else { "pixelated" }
        ))
    }
    pub fn content(&self) -> Result<Content, String> {
        Ok(Content::Svg {
            source: self.source()?,
        })
    }
    pub fn replace_tile(&mut self, tile: &RgbaImage) -> Result<(), String> {
        let next = Self::from_tile(tile, [self.settings.width, self.settings.height])?;
        self.data = next.data;
        self.settings.tile_size = next.settings.tile_size;
        Ok(())
    }
    pub fn resize(&mut self, new: [u32; 2]) {
        let factors = [
            new[0] as f32 / self.settings.width as f32,
            new[1] as f32 / self.settings.height as f32,
        ];
        for (i, f) in factors.into_iter().enumerate() {
            self.settings.canvas_scale[i] *= f;
            self.settings.offset[i] *= f;
        }
        self.settings.width = new[0];
        self.settings.height = new[1];
    }
}
pub fn preset(kind: usize, colors: [[u8; 4]; 2]) -> RgbaImage {
    RgbaImage::from_fn(16, 16, |x, y| {
        Rgba(
            colors[usize::from(match kind {
                1 => x >= 8,
                2 => (x as i32 - 8).pow(2) + (y as i32 - 8).pow(2) > 16,
                _ => (x / 8 + y / 8) % 2 == 1,
            })],
        )
    })
}
pub fn add(
    document: &mut Document,
    content: Content,
    selection: Option<&Selection>,
) -> Result<(), String> {
    let Content::Svg { ref source } = content else {
        return Err("패턴 요소가 아닙니다.".into());
    };
    Pattern::read(source).ok_or("패턴 설정이 유효하지 않습니다.")?;
    let pixels = content.render()?;
    document.add_layer("패턴 채우기".into(), pixels)?;
    let layer = document.active_layer_mut().unwrap();
    layer.content = Some(content);
    if let Some(selection) = selection {
        layer.mask = Some(std::sync::Arc::new(image::GrayImage::from_fn(
            layer.pixels.width(),
            layer.pixels.height(),
            |x, y| image::Luma([(selection.coverage(x as i32, y as i32) * 255.).round() as u8]),
        )));
    }
    Ok(())
}
#[derive(Clone, Copy)]
pub enum Source {
    Clipboard,
    Selection,
}
#[derive(Default)]
pub struct Panel {
    preview: Option<(Pattern, egui::TextureHandle)>,
}
impl Panel {
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        source: &mut String,
        colors: [[u8; 4]; 2],
        clipboard: bool,
        selection: bool,
    ) -> Option<Source> {
        let mut pattern = Pattern::read(source)?;
        let mut action = None;
        ui.heading("패턴 채우기");
        ui.horizontal_wrapped(|ui| {
            for (kind, label) in ["체크", "줄무늬", "점"].into_iter().enumerate() {
                if ui.button(label).clicked() {
                    let _ = pattern.replace_tile(&preset(kind, colors));
                }
            }
            if ui
                .add_enabled(clipboard, egui::Button::new("복사한 이미지"))
                .clicked()
            {
                action = Some(Source::Clipboard);
            }
            if ui
                .add_enabled(selection, egui::Button::new("현재 선택 영역"))
                .clicked()
            {
                action = Some(Source::Selection);
            }
        });
        ui.label("사진을 열어 복사하면 해당 이미지를 타일로 사용할 수 있습니다.");
        ui.label(format!(
            "원본 타일: {} × {} px · PNG/알파 보존",
            pattern.settings.tile_size[0], pattern.settings.tile_size[1]
        ));
        for (value, label) in pattern
            .settings
            .scale
            .iter_mut()
            .zip(["가로 배율", "세로 배율"])
        {
            let mut percent = *value * 100.;
            let changed = ui
                .horizontal(|ui| {
                    crate::numeric_slider::labeled_drag(
                        ui,
                        label,
                        &mut percent,
                        1.0..=10000.,
                        1.,
                        " %",
                    )
                    .changed()
                })
                .inner;
            if changed {
                *value = percent / 100.;
            }
        }
        ui.add(
            crate::numeric_slider::Slider::new(&mut pattern.settings.rotation, -180.0..=180.)
                .text("회전"),
        );
        ui.horizontal(|ui| {
            for (value, label) in pattern.settings.offset.iter_mut().zip(["X ", "Y "]) {
                ui.add(
                    egui::DragValue::new(value)
                        .range(-1e9..=1e9)
                        .prefix(label)
                        .suffix(" px"),
                );
            }
        });
        ui.checkbox(&mut pattern.settings.smooth, "타일 가장자리 부드럽게");
        if self.preview.as_ref().is_none_or(|(old, _)| old != &pattern) {
            let mut sample = pattern.clone();
            sample.settings.width = 160;
            sample.settings.height = 100;
            if let Ok(pixels) = sample.content().and_then(|content| content.render()) {
                let image = egui::ColorImage::from_rgba_unmultiplied([160, 100], pixels.as_raw());
                let texture =
                    ui.ctx()
                        .load_texture("pattern-preview", image, egui::TextureOptions::NEAREST);
                self.preview = Some((pattern.clone(), texture));
            }
        }
        if let Some((_, texture)) = &self.preview {
            ui.image((texture.id(), Vec2::new(160., 100.)));
        }
        if let Ok(next) = pattern.source() {
            *source = next;
        }
        action
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeating_alpha_transform_resize_edit_undo_and_project_svg_roundtrip() {
        let tile = RgbaImage::from_fn(2, 2, |x, y| {
            Rgba(if x == y {
                [255, 20, 40, 255]
            } else {
                [0, 0, 0, 0]
            })
        });
        let mut pattern = Pattern::from_tile(&tile, [8, 6]).unwrap();
        let content = pattern.content().unwrap();
        let pixels = content.render().unwrap();
        for (x, y, pixel) in pixels.enumerate_pixels() {
            assert_eq!(*pixel, *tile.get_pixel(x % 2, y % 2));
        }
        pattern.settings.offset = [1., 0.];
        let translated = pattern.content().unwrap().render().unwrap();
        assert_eq!(translated.get_pixel(1, 0), tile.get_pixel(0, 0));
        pattern.settings.scale = [2., 2.];
        pattern.settings.rotation = 90.;
        let mut content = pattern.content().unwrap();
        let data = pattern.data.clone();
        content.resize([8, 6], [16, 12]);
        let Content::Svg { source } = &content else {
            unreachable!()
        };
        let resized = Pattern::read(source).unwrap();
        assert_eq!(resized.settings.scale, [2., 2.]);
        assert_eq!(resized.settings.canvas_scale, [2., 2.]);
        assert_eq!(resized.settings.offset, [2., 0.]);
        assert_eq!(resized.data, data);
        let mut editor = crate::document::Editor::new(Document::new(16, 12, true).unwrap(), true);
        let selection = Selection {
            min: [2, 2],
            max: [12, 10],
            inverted: false,
            mask: None,
        };
        editor.edit("pattern", |doc| {
            add(doc, content.clone(), Some(&selection)).unwrap()
        });
        assert_eq!(editor.history().count(), 1);
        assert_eq!(
            editor
                .document
                .active_layer()
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .get_pixel(0, 0)[0],
            0
        );
        let image = editor.document.composite();
        editor.undo();
        assert_eq!(editor.document.layers.len(), 1);
        editor.redo();
        assert_eq!(editor.document.composite(), image);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/pattern");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("pattern.rphoto");
        crate::files::save_project(&editor.document, &path).unwrap();
        let restored = crate::files::load_project(&path).unwrap();
        assert_eq!(restored.active_layer().unwrap().content, Some(content));
        assert_eq!(restored.composite(), image);
        let svg = dir.join("pattern.svg");
        crate::svg_export::save(&restored, &svg).unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().contains("<pattern"));
        assert_eq!(
            crate::files::open(&svg).unwrap().document.composite(),
            image
        );
        pattern.settings.scale[0] = f32::NAN;
        assert!(pattern.content().is_err());
    }
}
