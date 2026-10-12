use image::{Rgba, Rgba32FImage};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Bloom {
    pub amount: f32,
    pub radius: f32,
    pub threshold: f32,
    pub knee: f32,
}
impl Default for Bloom {
    fn default() -> Self {
        Self {
            amount: 0.,
            radius: 12.,
            threshold: 1.,
            knee: 0.5,
        }
    }
}
impl Bloom {
    pub fn valid(&self) -> bool {
        [self.amount, self.radius, self.threshold, self.knee]
            .iter()
            .all(|v| v.is_finite())
            && (0.0..=5.).contains(&self.amount)
            && (0.0..=100.).contains(&self.radius)
            && (0.0..=32.).contains(&self.threshold)
            && (0.0..=1.).contains(&self.knee)
    }
    #[cfg(test)]
    pub fn apply(&self, source: &Rgba32FImage) -> Rgba32FImage {
        self.apply_alpha(source, false)
    }
    pub fn apply_alpha(&self, source: &Rgba32FImage, expand: bool) -> Rgba32FImage {
        if self.amount == 0. || !self.valid() {
            return source.clone();
        }
        let threshold = f64::from(self.threshold);
        let knee = threshold * f64::from(self.knee);
        let bright = Rgba32FImage::from_fn(source.width(), source.height(), |x, y| {
            let p = source.get_pixel(x, y);
            let rgb = [p[0], p[1], p[2]].map(|v| f64::from(v).max(0.));
            let l = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
            let soft = if knee > 0. {
                (l - threshold + knee).clamp(0., 2. * knee).powi(2) / (4. * knee)
            } else {
                0.
            };
            let weight = (l - threshold).max(soft).max(0.) / l.max(1e-6);
            if expand {
                Rgba([
                    rgb[0] as f32,
                    rgb[1] as f32,
                    rgb[2] as f32,
                    (f64::from(p[3]) * weight).clamp(0., 1.) as f32,
                ])
            } else {
                Rgba([
                    crate::precision::float_channel(rgb[0] * weight),
                    crate::precision::float_channel(rgb[1] * weight),
                    crate::precision::float_channel(rgb[2] * weight),
                    p[3],
                ])
            }
        });
        let blurred = if self.radius > 0. {
            crate::precision::blur(&bright, self.radius)
        } else {
            bright
        };
        let mut output = source.clone();
        let amount = f64::from(self.amount);
        for (p, b) in output.pixels_mut().zip(blurred.pixels()) {
            if expand {
                let base_alpha = f64::from(p[3]);
                let glow_alpha = f64::from(b[3]) * amount;
                let coverage = glow_alpha.clamp(0., 1.);
                let alpha = base_alpha + (1. - base_alpha) * coverage;
                if alpha > 0. {
                    for c in 0..3 {
                        p[c] = crate::precision::float_channel(
                            (f64::from(p[c]) * base_alpha + f64::from(b[c]) * glow_alpha) / alpha,
                        );
                    }
                    p[3] = alpha.clamp(0., 1.) as f32;
                } else {
                    *p = Rgba([0.; 4]);
                }
            } else if p[3] > 0. {
                for c in 0..3 {
                    p[c] = crate::precision::float_channel(
                        f64::from(p[c]) + f64::from(b[c]) * f64::from(b[3]) * amount,
                    );
                }
            }
        }
        output
    }
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) {
        ui.collapsing("사진 블룸", |ui| {
            for (v, name, range) in [
                (&mut self.amount, "강도", 0.0..=5.0),
                (&mut self.radius, "반경 px", 0.0..=100.0),
                (&mut self.threshold, "밝기 기준", 0.0..=32.0),
                (&mut self.knee, "기준 경계 부드러움", 0.0..=1.0),
            ] {
                ui.add(crate::numeric_slider::Slider::new(v, range).text(name));
            }
            ui.label(
                "경계 밖 확산을 켜면 투명 영역에도 빛이 번집니다. 끄면 기존 알파를 유지합니다.",
            );
            if ui.button("블룸 초기화").clicked() {
                *self = Default::default();
            }
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn highlight_bloom_spreads_hdr_without_hidden_color_alpha_or_source_damage() {
        let mut source = Rgba32FImage::from_pixel(9, 9, Rgba([0., 0., 0., 1.]));
        *source.get_pixel_mut(4, 4) = Rgba([4., 2., 1., 1.]);
        let original = source.clone();
        assert_eq!(Bloom::default().apply(&source), source);
        let bloom = Bloom {
            amount: 1.,
            radius: 1.5,
            threshold: 1.,
            knee: 0.,
        };
        let out = bloom.apply(&source);
        assert!(out.get_pixel(3, 4)[0] > 0.);
        assert!(out.get_pixel(4, 4)[0] > 4.);
        assert_eq!(out.get_pixel(3, 4), out.get_pixel(5, 4));
        assert!(out.pixels().zip(source.pixels()).all(|(a, b)| a[3] == b[3]));
        assert_eq!(source, original);
        assert_eq!(
            Bloom {
                threshold: 10.,
                ..bloom.clone()
            }
            .apply(&source),
            source
        );
        let mut hidden = Rgba32FImage::from_pixel(9, 9, Rgba([-0.2, 0., 0., 0.5]));
        *hidden.get_pixel_mut(4, 4) = Rgba([100., 100., 100., 0.]);
        assert_eq!(bloom.apply(&hidden), hidden);
        let legacy: crate::adjustments::Adjustments = serde_json::from_str("{}").unwrap();
        assert_eq!(legacy.bloom, Bloom::default());
        let mut invalid = bloom.clone();
        invalid.radius = f32::NAN;
        assert!(!invalid.valid());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/bloom");
        std::fs::create_dir_all(&dir).unwrap();
        let doc = crate::precision::document(
            "bloom".into(),
            image::DynamicImage::ImageRgba32F(source.clone()),
        )
        .unwrap();
        let mut editor = crate::document::Editor::new(doc, false);
        editor.edit("bloom", |doc| {
            doc.active_layer_mut().unwrap().adjustments.bloom = bloom.clone()
        });
        assert!(!editor.document.local_preview());
        let image = crate::precision::composite(&editor.document);
        crate::files::save_project(&editor.document, &dir.join("bloom.rphoto")).unwrap();
        let restored = crate::files::open(&dir.join("bloom.rphoto"))
            .unwrap()
            .document;
        assert_eq!(restored.active_layer().unwrap().adjustments.bloom, bloom);
        assert_eq!(crate::precision::composite(&restored), image);
        assert_eq!(
            restored.active_layer().unwrap().precision.as_deref(),
            Some(&source)
        );
        editor.undo();
        assert_eq!(
            editor.document.active_layer().unwrap().adjustments.bloom,
            Bloom::default()
        );
        editor.redo();
        assert_eq!(crate::precision::composite(&editor.document), image);
    }
}

#[cfg(test)]
#[path = "bloom_hdr_tests.rs"]
mod hdr_tests;
