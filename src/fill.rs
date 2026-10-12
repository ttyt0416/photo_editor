use crate::{
    document::{Document, Selection},
    vector::Content,
};
use eframe::egui;
use std::sync::Arc;

pub fn add(
    doc: &mut Document,
    content: Content,
    selection: Option<&Selection>,
) -> Result<(), String> {
    let name = match content {
        Content::Solid { .. } => "색상 채우기",
        Content::Gradient { .. } => "그라디언트 채우기",
        _ => return Err("채우기 요소가 아닙니다.".into()),
    };
    doc.add_layer(name.into(), content.render()?)?;
    let layer = doc.active_layer_mut().unwrap();
    layer.content = Some(content);
    if let Some(selection) = selection {
        layer.mask = Some(Arc::new(image::GrayImage::from_fn(
            layer.pixels.width(),
            layer.pixels.height(),
            |x, y| image::Luma([(selection.coverage(x as i32, y as i32) * 255.).round() as u8]),
        )));
    }
    Ok(())
}

#[derive(Default)]
pub struct Panel {
    color: Option<[u8; 4]>,
    hex: String,
    preview: Option<(Content, egui::TextureHandle)>,
}
impl Panel {
    pub fn color_ui(&mut self, ui: &mut egui::Ui, color: &mut [u8; 4]) {
        ui.heading("색상 채우기");
        let mut candidate = *color;
        if ui
            .color_edit_button_srgba_unmultiplied(&mut candidate)
            .changed()
        {
            *color = candidate;
        }
        ui.horizontal(|ui| {
            for (i, label) in ["R ", "G ", "B ", "A "].into_iter().enumerate() {
                ui.add(
                    egui::DragValue::new(&mut color[i])
                        .range(0..=255)
                        .prefix(label),
                );
            }
        });
        let id = ui.id().with("fill-hex");
        if self.color != Some(*color) && !ui.memory(|m| m.has_focus(id)) {
            self.hex = color.iter().map(|v| format!("{v:02X}")).collect();
        }
        ui.horizontal(|ui| {
            ui.label("RGBA #");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.hex)
                    .id(id)
                    .desired_width(100.),
            );
            let value = self.hex.trim().trim_start_matches('#');
            let decoded: Option<Vec<u8>> = if value.len() == 8 && value.is_ascii() {
                (0..4)
                    .map(|i| u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).ok())
                    .collect()
            } else {
                None
            };
            if response.changed()
                && let Some(bytes) = decoded.as_ref()
            {
                color.copy_from_slice(bytes);
            }
            if decoded.is_none() {
                ui.label("8자리 색상/알파 값을 입력하세요.");
            }
        });
        self.color = Some(*color);
    }
    pub fn preview(&mut self, ui: &mut egui::Ui, content: &Content) {
        let size = match content {
            Content::Solid { width, height, .. } | Content::Gradient { width, height, .. } => {
                [*width, *height]
            }
            _ => return,
        };
        if self
            .preview
            .as_ref()
            .is_none_or(|(saved, _)| saved != content)
        {
            let scale = (200. / size[0] as f32).min(120. / size[1] as f32).min(1.);
            let new = [
                (size[0] as f32 * scale).round().max(1.) as u32,
                (size[1] as f32 * scale).round().max(1.) as u32,
            ];
            let mut small = content.clone();
            small.resize(size, new);
            match small.render() {
                Ok(image) => {
                    let texture = ui.ctx().load_texture(
                        "fill-preview",
                        egui::ColorImage::from_rgba_unmultiplied(
                            [new[0] as usize, new[1] as usize],
                            image.as_raw(),
                        ),
                        egui::TextureOptions::LINEAR,
                    );
                    self.preview = Some((content.clone(), texture));
                }
                Err(error) => {
                    ui.label(error);
                    return;
                }
            }
        }
        let Some((_, texture)) = &self.preview else {
            return;
        };
        let (rect, _) = ui.allocate_exact_size(texture.size_vec2(), egui::Sense::hover());
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        for y in 0..(rect.height() / 8.).ceil() as i32 {
            for x in 0..(rect.width() / 8.).ceil() as i32 {
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        rect.min + egui::vec2(x as f32 * 8., y as f32 * 8.),
                        egui::Vec2::splat(8.),
                    ),
                    0.,
                    egui::Color32::from_gray(if (x + y) % 2 == 0 { 65 } else { 90 }),
                );
            }
        }
        ui.painter().image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1., 1.)),
            egui::Color32::WHITE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn solid_and_gradient_fill_preserve_alpha_mask_source_and_undo_across_project_svg() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/fill");
        std::fs::create_dir_all(&dir).unwrap();
        let mut editor = crate::document::Editor::new(Document::new(8, 6, true).unwrap(), false);
        let color = [23, 45, 67, 89];
        let content = Content::Solid {
            width: 8,
            height: 6,
            color,
        };
        let selection = Selection {
            min: [2, 1],
            max: [6, 5],
            mask: None,
            inverted: false,
        };
        editor.begin("fill");
        add(&mut editor.document, content.clone(), Some(&selection)).unwrap();
        editor.finish();
        let layer = editor.document.active_layer().unwrap();
        assert!(layer.pixels.pixels().all(|p| p.0 == color));
        assert_eq!(layer.mask.as_ref().unwrap().get_pixel(0, 0)[0], 0);
        assert_eq!(layer.mask.as_ref().unwrap().get_pixel(3, 2)[0], 255);
        assert_eq!(editor.history().count(), 1);
        let project = dir.join("solid.rphoto");
        crate::files::save_project(&editor.document, &project).unwrap();
        let restored = crate::files::open(&project).unwrap().document;
        assert_eq!(
            restored.active_layer().unwrap().content.as_ref(),
            Some(&content)
        );
        assert_eq!(restored.composite(), editor.document.composite());
        let svg = dir.join("solid.svg");
        crate::svg_export::save(&restored, &svg).unwrap();
        let reopened = crate::files::open(&svg).unwrap().document.composite();
        for (a, b) in reopened.pixels().zip(restored.composite().pixels()) {
            assert!(a[3].abs_diff(b[3]) <= 1);
            for c in 0..3 {
                assert!(
                    (u16::from(a[c]) * u16::from(a[3]) / 255)
                        .abs_diff(u16::from(b[c]) * u16::from(b[3]) / 255)
                        <= 1
                );
            }
        }
        editor.undo();
        assert_eq!(editor.document.layers.len(), 1);
        editor.redo();
        assert_eq!(
            editor.document.active_layer().unwrap().content.as_ref(),
            Some(&content)
        );
        let mut resized = content.clone();
        resized.resize([8, 6], [16, 9]);
        assert_eq!(
            resized.render().unwrap(),
            image::RgbaImage::from_pixel(16, 9, image::Rgba(color))
        );
        let gradient = Content::Gradient {
            width: 8,
            height: 6,
            start: [0., 0.],
            end: [8., 0.],
            colors: [color, [200, 150, 10, 230]],
            stops: vec![],
            interpolation: Default::default(),
        };
        add(&mut editor.document, gradient.clone(), None).unwrap();
        assert!(editor.document.active_layer().unwrap().mask.is_none());
        assert_eq!(
            editor.document.active_layer().unwrap().content.as_ref(),
            Some(&gradient)
        );
        crate::files::save_project(&editor.document, &dir.join("gradient.rphoto")).unwrap();
        assert_eq!(
            crate::files::open(&dir.join("gradient.rphoto"))
                .unwrap()
                .document
                .active_layer()
                .unwrap()
                .content
                .as_ref(),
            Some(&gradient)
        );
    }
    #[test]
    fn showing_solid_controls_keeps_unpremultiplied_rgba_without_input() {
        let ctx = egui::Context::default();
        let mut panel = Panel::default();
        let mut color = [23, 45, 67, 89];
        for _ in 0..3 {
            let mut output = ctx.run_ui(Default::default(), |ui| {
                panel.color_ui(ui, &mut color);
            });
            output.textures_delta.clear();
            assert_eq!(color, [23, 45, 67, 89]);
        }
        assert_eq!(panel.hex, "172D4359");
    }
}
