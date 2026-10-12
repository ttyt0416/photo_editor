use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorBalance {
    // Rows: shadows, midtones, highlights. Columns: cyan/red, magenta/green, yellow/blue.
    pub shifts: [[f32; 3]; 3],
    pub preserve_luminosity: bool,
}
impl Default for ColorBalance {
    fn default() -> Self {
        Self {
            shifts: [[0.; 3]; 3],
            preserve_luminosity: true,
        }
    }
}
#[cfg(test)]
fn lum(c: [f32; 3]) -> f32 {
    c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114
}
impl ColorBalance {
    pub fn valid(&self) -> bool {
        self.shifts
            .iter()
            .flatten()
            .all(|v| v.is_finite() && (-100.0..=100.).contains(v))
    }
    pub fn apply(&self, color: [f32; 3]) -> [f32; 3] {
        if self.shifts == [[0.; 3]; 3] {
            return color;
        }
        let input = color;
        let color = color.map(f64::from);
        let lum = |c: [f64; 3]| c[0] * 0.299 + c[1] * 0.587 + c[2] * 0.114;
        let luminance = lum(color);
        let position = luminance.clamp(0., 1.);
        let shadow = (1. - 2. * position).max(0.);
        let highlight = (2. * position - 1.).max(0.);
        let weights = [shadow, 1. - shadow - highlight, highlight];
        let mut result = color;
        for (channel, value) in result.iter_mut().enumerate() {
            *value += (0..3)
                .map(|tone| f64::from(self.shifts[tone][channel]) * weights[tone] / 200.)
                .sum::<f64>();
        }
        if self.preserve_luminosity {
            let correction = lum(std::array::from_fn(|i| result[i] - color[i]));
            for value in &mut result {
                *value -= correction;
            }
            // Compress chroma into SDR gamut, keeping luminance. HDR values retain their range.
            if color.iter().all(|v| (0.0..=1.).contains(v)) {
                let mut chroma = 1f64;
                for value in result {
                    let delta = value - luminance;
                    if delta > 0. {
                        chroma = chroma.min((1. - luminance) / delta);
                    } else if delta < 0. {
                        chroma = chroma.min(-luminance / delta);
                    }
                }
                result = result.map(|v| luminance + (v - luminance) * chroma);
            }
        }
        if result == color {
            input
        } else {
            result.map(crate::precision::float_channel)
        }
    }
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) {
        ui.collapsing("색상 균형", |ui| {
            ui.checkbox(&mut self.preserve_luminosity, "광도 보존");
            for (values, label) in self
                .shifts
                .iter_mut()
                .zip(["그림자", "중간톤", "하이라이트"])
            {
                ui.label(label);
                for (value, label) in
                    values
                        .iter_mut()
                        .zip(["시안 ↔ 빨강", "마젠타 ↔ 초록", "노랑 ↔ 파랑"])
                {
                    ui.add(crate::numeric_slider::Slider::new(value, -100.0..=100.).text(label));
                }
            }
            if ui.button("색상 균형 초기화").clicked() {
                *self = Self::default();
            }
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tone_weights_luminance_gamut_hdr_source_project_and_undo_are_preserved() {
        let mut balance = ColorBalance {
            preserve_luminosity: false,
            ..Default::default()
        };
        balance.shifts[0][0] = 100.;
        balance.shifts[1][1] = 100.;
        balance.shifts[2][2] = 100.;
        assert_eq!(balance.apply([0.; 3]), [0.5, 0., 0.]);
        assert_eq!(balance.apply([0.5; 3]), [0.5, 1., 0.5]);
        assert_eq!(balance.apply([1.; 3]), [1., 1., 1.5]);
        balance.preserve_luminosity = true;
        for c in [[0.1, 0.2, 0.3], [0.8, 0.7, 0.9], [2.125, -0.2, 0.4]] {
            let out = balance.apply(c);
            assert!((lum(out) - lum(c)).abs() < 1e-6);
            if c.iter().all(|v| (0.0..=1.).contains(v)) {
                assert!(out.iter().all(|v| (-1e-6..=1.000001).contains(v)));
            }
        }
        let input = image::Rgba32FImage::from_pixel(2, 2, image::Rgba([2.125, -0.2, 0.4, 0.8]));
        let mut doc = crate::precision::document(
            "balance".into(),
            image::DynamicImage::ImageRgba32F(input.clone()),
        )
        .unwrap();
        doc.active_layer_mut().unwrap().adjustments.color_balance = balance.clone();
        assert_eq!(
            doc.active_layer()
                .unwrap()
                .precision
                .as_ref()
                .unwrap()
                .as_ref(),
            &input
        );
        let output = crate::precision::composite(&doc);
        assert!(output.get_pixel(0, 0)[0] > 1.);
        assert_eq!(output.get_pixel(0, 0)[3], 0.8);
        let mut editor = crate::document::Editor::new(doc, true);
        editor.edit("balance reset", |doc| {
            doc.active_layer_mut().unwrap().adjustments.color_balance = Default::default()
        });
        editor.undo();
        assert_eq!(crate::precision::composite(&editor.document), output);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/color-balance");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("balance.rphoto");
        crate::files::save_project(&editor.document, &path).unwrap();
        let restored = crate::files::load_project(&path).unwrap();
        assert_eq!(
            restored.active_layer().unwrap().adjustments.color_balance,
            balance
        );
        assert_eq!(crate::precision::composite(&restored), output);
        let old: crate::adjustments::Adjustments =
            serde_json::from_str("{\"brightness\":0.1}").unwrap();
        assert_eq!(old.color_balance, ColorBalance::default());
        balance.shifts[0][0] = f32::NAN;
        assert!(!balance.valid());
    }
}
