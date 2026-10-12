use image::Rgba32FImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TonalContrast {
    pub amount: f32,
    pub radius: f32,
    // Local contrast strengths for shadows, midtones and highlights.
    pub strengths: [f32; 3],
}
impl Default for TonalContrast {
    fn default() -> Self {
        Self {
            amount: 0.,
            radius: 16.,
            strengths: [40., 60., 30.],
        }
    }
}
fn luminance(color: [f32; 4]) -> f64 {
    f64::from(color[0]) * 0.2126 + f64::from(color[1]) * 0.7152 + f64::from(color[2]) * 0.0722
}
impl TonalContrast {
    pub fn enabled(&self) -> bool {
        self.amount != 0. && self.strengths != [0.; 3]
    }
    pub fn valid(&self) -> bool {
        self.amount.is_finite()
            && (0.0..=100.).contains(&self.amount)
            && self.radius.is_finite()
            && (1.0..=100.).contains(&self.radius)
            && self
                .strengths
                .iter()
                .all(|v| v.is_finite() && (-100.0..=100.).contains(v))
    }
    pub fn apply(&self, source: &Rgba32FImage) -> Rgba32FImage {
        if !self.enabled() || !self.valid() {
            return source.clone();
        }
        // Premultiplied blur ignores hidden RGB while maintaining detail near transparent edges.
        let base = crate::precision::blur(source, self.radius);
        let mut output = source.clone();
        for (p, b) in output.pixels_mut().zip(base.pixels()) {
            if p[3] <= 0. {
                continue;
            }
            let lum = luminance(p.0);
            let position = lum.clamp(0., 1.);
            let shadow = (1. - 2. * position).max(0.);
            let highlight = (2. * position - 1.).max(0.);
            let weights = [shadow, 1. - shadow - highlight, highlight];
            let gain = f64::from(self.amount) / 100.
                * self
                    .strengths
                    .into_iter()
                    .zip(weights)
                    .map(|(s, w)| f64::from(s) * w / 100.)
                    .sum::<f64>();
            let detail = (lum - luminance(b.0)) * gain;
            // Equal channel shifts retain chromatic differences; no SDR clamp destroys HDR or negatives.
            for c in 0..3 {
                p[c] = crate::precision::float_channel(f64::from(p[c]) + detail);
            }
        }
        output
    }
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) {
        ui.collapsing("톤별 대비", |ui| {
            ui.add(
                crate::numeric_slider::Slider::new(&mut self.amount, 0.0..=100.)
                    .text("전체 강도 %"),
            );
            for (v, label) in self.strengths.iter_mut().zip([
                "그림자 대비 %",
                "중간톤 대비 %",
                "하이라이트 대비 %",
            ]) {
                ui.add(crate::numeric_slider::Slider::new(v, -100.0..=100.).text(label));
            }
            ui.add(
                crate::numeric_slider::Slider::new(&mut self.radius, 1.0..=100.)
                    .logarithmic(true)
                    .text("세부 반경 px"),
            );
            ui.label(
                "양수는 세부 대비를 높이고 음수는 부드럽게 합니다. 알파와 HDR 범위를 유지합니다.",
            );
            if ui.button("톤 대비 초기화").clicked() {
                *self = Default::default();
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adjustments::Adjustments,
        document::{PixelRegion, Selection},
        precision,
    };
    use image::Rgba;
    use std::sync::Arc;
    fn source() -> Rgba32FImage {
        Rgba32FImage::from_fn(12, 6, |x, y| {
            let value = [0.1, 0.25, 0.4, 0.6, 0.8, 1.8][x as usize % 6] + y as f32 * 0.01;
            Rgba([
                value,
                value - 0.15,
                value + 0.2,
                if x == 0 { 0. } else { 0.75 },
            ])
        })
    }
    fn approx(a: &Rgba32FImage, b: &Rgba32FImage) {
        for (a, b) in a.pixels().zip(b.pixels()) {
            for c in 0..4 {
                assert!((a[c] - b[c]).abs() < 2e-5, "{} != {}", a[c], b[c]);
            }
        }
    }
    #[test]
    fn independent_tone_gains_positive_negative_neutral_and_alpha_hdr() {
        let source = source();
        let original = source.clone();
        let tone = TonalContrast {
            amount: 80.,
            radius: 1.5,
            strengths: [100., -50., 25.],
        };
        let out = tone.apply(&source);
        let base = precision::blur(&source, tone.radius);
        for (p, (s, b)) in out.pixels().zip(source.pixels().zip(base.pixels())) {
            assert_eq!(p[3], s[3]);
            if s[3] == 0. {
                assert_eq!(p, s);
                continue;
            }
            let l = s[0] * 0.2126 + s[1] * 0.7152 + s[2] * 0.0722;
            let shadow = (1. - 2. * l.clamp(0., 1.)).max(0.);
            let high = (2. * l.clamp(0., 1.) - 1.).max(0.);
            let gain = 0.8 * (shadow - 0.5 * (1. - shadow - high) + 0.25 * high);
            let delta = (l - (b[0] * 0.2126 + b[1] * 0.7152 + b[2] * 0.0722)) * gain;
            for c in 0..3 {
                assert!((p[c] - (s[c] + delta)).abs() < 1e-6);
            }
            assert!(((p[0] - p[1]) - (s[0] - s[1])).abs() < 1e-6);
        }
        assert!(out.pixels().any(|p| p[0] > 1.));
        assert!(out.pixels().any(|p| p[1] < 0.));
        assert_eq!(source, original);
        assert_eq!(TonalContrast::default().apply(&source), source);
        assert_eq!(
            TonalContrast {
                strengths: [0.; 3],
                ..tone.clone()
            }
            .apply(&source),
            source
        );
        let flat = Rgba32FImage::from_pixel(4, 3, Rgba([0.4, 0.25, 1.3, 0.5]));
        approx(&tone.apply(&flat), &flat);
        let mut invalid = tone;
        invalid.radius = f32::NAN;
        assert!(!invalid.valid());
        invalid.radius = 0.;
        assert!(!invalid.valid());
        invalid.radius = 1.;
        invalid.strengths[2] = 101.;
        assert!(!invalid.valid());
        let old: Adjustments = serde_json::from_str("{}").unwrap();
        assert_eq!(old.tonal_contrast, TonalContrast::default());
        let mut a = old;
        a.tonal_contrast.radius = 50.;
        assert!(!a.active());
        assert!(a.local());
    }
    #[test]
    fn tone_bands_are_independent_and_hidden_rgb_cannot_contaminate_visible_detail() {
        let source = Rgba32FImage::from_fn(9, 3, |x, _| {
            Rgba([[0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9][x as usize]; 4])
        });
        let a = TonalContrast {
            amount: 100.,
            radius: 1.,
            strengths: [100., 0., 0.],
        };
        let positive = a.apply(&source);
        let negative = TonalContrast {
            strengths: [-100., 0., 0.],
            ..a.clone()
        }
        .apply(&source);
        assert_ne!(positive.get_pixel(0, 1), source.get_pixel(0, 1));
        for x in 0..9 {
            let s = source.get_pixel(x, 1);
            let p = positive.get_pixel(x, 1);
            let n = negative.get_pixel(x, 1);
            for c in 0..3 {
                assert!((p[c] + n[c] - 2. * s[c]).abs() < 1e-6);
            }
            if x >= 4 {
                assert_eq!(p, s);
                assert_eq!(n, s);
            }
        }
        let mut hidden = source.clone();
        *hidden.get_pixel_mut(4, 1) = Rgba([100., -100., 100., 0.]);
        let mut clean = hidden.clone();
        *clean.get_pixel_mut(4, 1) = Rgba([0.; 4]);
        let a = TonalContrast {
            strengths: [100.; 3],
            ..a
        };
        let h = a.apply(&hidden);
        let c = a.apply(&clean);
        for (h, c) in h.pixels().zip(c.pixels()) {
            if h[3] > 0. {
                assert_eq!(h, c);
            }
        }
    }
    #[test]
    fn selected_tonal_adjustment_project_bake_region_history_and_photoshop_outputs() {
        let mut doc = precision::document(
            "tone HDR".into(),
            image::DynamicImage::ImageRgba32F(source()),
        )
        .unwrap();
        let source = doc.layers[0].precision.clone().unwrap();
        let selection = Selection {
            mask: Some(Arc::new(image::GrayImage::from_fn(6, 4, |x, _| {
                image::Luma([if x % 2 == 0 { 128 } else { 255 }])
            }))),
            ..Selection::rectangle([3, 1], [9, 5])
        };
        let scope =
            crate::adjustment_scope::Scope::capture(&doc.layers[0], &selection, [12, 6]).unwrap();
        let tone = TonalContrast {
            amount: 75.,
            radius: 2.,
            strengths: [80., -30., 60.],
        };
        doc.layers[0].adjustments = Adjustments {
            tonal_contrast: tone.clone(),
            scope: Some(scope),
            ..Default::default()
        };
        assert!(!doc.local_preview());
        let expected = precision::composite(&doc);
        let full = tone.apply(&source);
        for (x, y, p) in expected.enumerate_pixels() {
            if source.get_pixel(x, y)[3] > 0. {
                let weight = selection.coverage(x as i32, y as i32);
                for c in 0..3 {
                    let value = source.get_pixel(x, y)[c]
                        + (full.get_pixel(x, y)[c] - source.get_pixel(x, y)[c]) * weight;
                    assert!((p[c] - value).abs() < 2e-6);
                }
            }
            assert_eq!(p[3], source.get_pixel(x, y)[3]);
        }
        let region = PixelRegion {
            min: [2, 1],
            max: [10, 5],
        };
        approx(
            &precision::composite_region(&doc, region),
            &image::imageops::crop_imm(&expected, 2, 1, 8, 4).to_image(),
        );
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/tonal-contrast");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selected-hdr.rphoto");
        crate::files::save_project(&doc, &path).unwrap();
        let restored = crate::files::open(&path).unwrap().document;
        assert_eq!(restored.layers[0].adjustments.tonal_contrast, tone);
        approx(&precision::composite(&restored), &expected);
        for ext in ["psd", "psb"] {
            let path = dir.join(format!("selected-hdr.{ext}"));
            crate::files::save_photoshop(&doc, &path).unwrap();
            approx(
                &precision::composite(&crate::files::open(&path).unwrap().document),
                &expected,
            );
        }
        let mut editor = crate::document::Editor::new(doc, false);
        let id = editor.document.active;
        editor.edit("tone bake", |doc| {
            crate::adjustment_scope::bake(doc, id).unwrap()
        });
        approx(&precision::composite(&editor.document), &expected);
        editor.undo();
        assert!(Arc::ptr_eq(
            &source,
            editor.document.layers[0].precision.as_ref().unwrap()
        ));
        editor.redo();
        approx(&precision::composite(&editor.document), &expected);
    }
    #[test]
    fn standalone_and_group_tonal_contrast_apply_to_stack_without_duplicate_source() {
        let mut doc = precision::document(
            "tone stack".into(),
            image::DynamicImage::ImageRgba32F(source()),
        )
        .unwrap();
        let source = doc.layers[0].precision.clone().unwrap();
        crate::adjustment_layer::add(&mut doc).unwrap();
        let tone = TonalContrast {
            amount: 60.,
            radius: 1.,
            strengths: [50., -20., 75.],
        };
        doc.layers[1].adjustments.tonal_contrast = tone.clone();
        assert_eq!(doc.layers[1].pixels.dimensions(), (1, 1));
        let expected = tone.apply(&source);
        let rendered = precision::composite(&doc);
        for (p, e) in rendered.pixels().zip(expected.pixels()) {
            if e[3] > 0. {
                for c in 0..4 {
                    assert!((p[c] - e[c]).abs() < 1e-5);
                }
            }
        }
        assert!(Arc::ptr_eq(
            &source,
            doc.layers[0].precision.as_ref().unwrap()
        ));
        doc.layers.pop();
        doc.active = doc.layers[0].id;
        doc.group_active().unwrap();
        let layer = doc.active_layer_mut().unwrap();
        layer.pass_through = false;
        layer.adjustments.tonal_contrast = tone;
        approx(&precision::composite(&doc), &rendered);
    }
}

#[cfg(test)]
#[path = "tonal_hdr_tests.rs"]
mod hdr_tests;
