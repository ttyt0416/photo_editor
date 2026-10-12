use image::{Rgba, Rgba32FImage};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Lens {
    pub radial: [f32; 2],
    pub center: [f32; 2],
    pub zoom: f32,
    pub chromatic: [f32; 2],
    pub sampling: crate::sampling::Sampling,
}
impl Default for Lens {
    fn default() -> Self {
        Self {
            radial: [0.; 2],
            center: [0.5; 2],
            zoom: 1.,
            chromatic: [0.; 2],
            sampling: crate::sampling::Sampling::Bilinear,
        }
    }
}
impl Lens {
    pub fn enabled(&self) -> bool {
        self.radial != [0.; 2] || self.zoom != 1. || self.chromatic != [0.; 2]
    }
    pub fn valid(&self) -> bool {
        if self.radial.iter().any(|v| !v.is_finite() || v.abs() > 0.5)
            || self
                .center
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.).contains(v))
            || !self.zoom.is_finite()
            || !(0.25..=4.).contains(&self.zoom)
            || self
                .chromatic
                .iter()
                .any(|v| !v.is_finite() || v.abs() > 0.05)
        {
            return false;
        }
        // The radial map must remain monotonic for every supported aspect ratio.
        let end = 4.
            * self
                .center
                .iter()
                .map(|v| v.max(1. - v).powi(2))
                .fold(0., f32::max)
            / self.zoom.powi(2);
        let derivative = |t: f32| 1. + 3. * self.radial[0] * t + 5. * self.radial[1] * t * t;
        let vertex = if self.radial[1] != 0. {
            (-3. * self.radial[0] / (10. * self.radial[1])).clamp(0., end)
        } else {
            0.
        };
        [0., end, vertex].into_iter().all(|t| derivative(t) > 0.01)
    }
    fn point(&self, point: [f32; 2], size: [u32; 2], channel_scale: f32) -> [f32; 2] {
        let center = [
            self.center[0] * size[0] as f32,
            self.center[1] * size[1] as f32,
        ];
        let radius = (size[0] as f32).hypot(size[1] as f32) * 0.5;
        let p = [
            (point[0] - center[0]) / (radius * self.zoom),
            (point[1] - center[1]) / (radius * self.zoom),
        ];
        let r2 = p[0] * p[0] + p[1] * p[1];
        let factor = (1. + self.radial[0] * r2 + self.radial[1] * r2 * r2) * channel_scale;
        [
            center[0] + p[0] * radius * factor,
            center[1] + p[1] * radius * factor,
        ]
    }
    #[cfg(test)]
    pub fn apply(&self, source: &Rgba32FImage) -> Rgba32FImage {
        self.apply_frame(source, [source.width(), source.height()], 0)
    }
    pub fn apply_frame(&self, source: &Rgba32FImage, frame: [u32; 2], pad: u32) -> Rgba32FImage {
        if !self.enabled() || !self.valid() {
            return source.clone();
        }
        let size = [source.width(), source.height()];
        let sample = |point| {
            crate::sampling::rgba(point, self.sampling, |x, y| {
                if x < 0 || y < 0 || x >= size[0] as i32 || y >= size[1] as i32 {
                    [0.; 4]
                } else {
                    source.get_pixel(x as u32, y as u32).0
                }
            })
        };
        let mapping = |p, scale| self.point(p, frame, scale).map(|v| v + pad as f32);
        Rgba32FImage::from_fn(size[0], size[1], |x, y| {
            let p = [x as f32 + 0.5 - pad as f32, y as f32 + 0.5 - pad as f32];
            let mut out = sample(mapping(p, 1.));
            if self.chromatic[0] != 0. {
                out[0] = sample(mapping(p, 1. + self.chromatic[0]))[0];
            }
            if self.chromatic[1] != 0. {
                out[2] = sample(mapping(p, 1. + self.chromatic[1]))[2];
            }
            if out[3] == 0. {
                out = [0.; 4];
            }
            Rgba(out)
        })
    }
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) {
        use eframe::egui;
        ui.collapsing("수동 렌즈 보정", |ui| {
            ui.add(
                crate::numeric_slider::Slider::new(&mut self.radial[0], -0.5..=0.5)
                    .text("방사 왜곡 k1"),
            );
            ui.add(
                crate::numeric_slider::Slider::new(&mut self.radial[1], -0.5..=0.5)
                    .text("주변부 왜곡 k2"),
            );
            ui.add(crate::numeric_slider::Slider::new(&mut self.zoom, 0.25..=4.).text("확대"));
            for (i, label) in ["중심 X", "중심 Y"].into_iter().enumerate() {
                ui.add(
                    crate::numeric_slider::Slider::new(&mut self.center[i], 0.0..=1.).text(label),
                );
            }
            for (i, label) in ["빨강 색수차", "파랑 색수차"].into_iter().enumerate() {
                ui.add(
                    crate::numeric_slider::Slider::new(&mut self.chromatic[i], -0.05..=0.05)
                        .text(label),
                );
            }
            egui::ComboBox::from_id_salt("lens-sampling")
                .selected_text(self.sampling.label())
                .show_ui(ui, |ui| {
                    for mode in [
                        crate::sampling::Sampling::Nearest,
                        crate::sampling::Sampling::Bilinear,
                        crate::sampling::Sampling::Bicubic,
                    ] {
                        ui.selectable_value(&mut self.sampling, mode, mode.label());
                    }
                });
            if !self.valid() {
                ui.colored_label(
                    egui::Color32::YELLOW,
                    "이 설정은 영상을 접어 겹치게 합니다. 왜곡을 줄이거나 확대하세요.",
                );
            }
            if ui.button("렌즈 초기화").clicked() {
                *self = Default::default();
            }
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn radial_chromatic_zoom_sampling_and_project_preserve_hdr_source_and_history() {
        let source = Rgba32FImage::from_fn(9, 9, |x, y| {
            Rgba([
                x as f32 / 2. - 0.25,
                y as f32 / 2.,
                (x + y) as f32 / 3.,
                if x == 0 { 0. } else { 0.75 },
            ])
        });
        assert_eq!(Lens::default().apply(&source), source);
        let lens = Lens {
            radial: [0.1, 0.05],
            zoom: 1.2,
            chromatic: [0.03, -0.04],
            ..Default::default()
        };
        assert!(lens.valid());
        assert_eq!(lens.point([4.5, 4.5], [9, 9], 1.), [4.5, 4.5]);
        let point = lens.point([7.5, 4.5], [9, 9], 1.);
        let r2 = 2.5f32.powi(2) / 40.5;
        assert!((point[0] - (4.5 + 2.5 * (1. + 0.1 * r2 + 0.05 * r2 * r2))).abs() < 1e-5);
        assert_eq!(point[1], 4.5);
        let out = lens.apply(&source);
        assert_eq!(out.get_pixel(4, 4), source.get_pixel(4, 4));
        assert_ne!(out, source);
        assert!(out.pixels().all(|p| p.0.iter().all(|v| v.is_finite())));
        assert!(out.pixels().any(|p| p[0] > 1.));
        let mut invalid = lens.clone();
        invalid.radial = [-0.5, 0.];
        invalid.zoom = 0.25;
        assert!(!invalid.valid());
        assert_eq!(invalid.apply(&source), source);
        invalid = lens.clone();
        invalid.center[0] = f32::INFINITY;
        assert!(!invalid.valid());
        let legacy: crate::adjustments::Adjustments = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.lens, Lens::default());
        let doc = crate::precision::document(
            "lens".into(),
            image::DynamicImage::ImageRgba32F(source.clone()),
        )
        .unwrap();
        let mut editor = crate::document::Editor::new(doc, false);
        editor.edit("lens", |doc| {
            doc.active_layer_mut().unwrap().adjustments.lens = lens.clone()
        });
        assert!(!editor.document.local_preview());
        let expected = crate::precision::composite(&editor.document);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/lens");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lens.rphoto");
        crate::files::save_project(&editor.document, &path).unwrap();
        let restored = crate::files::open(&path).unwrap().document;
        assert_eq!(restored.active_layer().unwrap().adjustments.lens, lens);
        assert_eq!(
            restored.active_layer().unwrap().precision.as_deref(),
            Some(&source)
        );
        assert_eq!(crate::precision::composite(&restored), expected);
        editor.undo();
        assert_eq!(
            editor.document.active_layer().unwrap().adjustments.lens,
            Lens::default()
        );
        editor.redo();
        assert_eq!(crate::precision::composite(&editor.document), expected);
    }
}
