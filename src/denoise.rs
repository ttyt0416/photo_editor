use image::Rgba32FImage;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Denoise {
    pub luminance: f32,
    pub color: f32,
    pub radius: u32,
    pub threshold: f32,
}
impl Default for Denoise {
    fn default() -> Self {
        Self {
            luminance: 0.,
            color: 0.,
            radius: 2,
            threshold: 0.1,
        }
    }
}
fn lum(c: [f64; 3]) -> f64 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}
impl Denoise {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn enabled(&self) -> bool {
        self.luminance > 0. || self.color > 0.
    }
    pub fn valid(&self) -> bool {
        self.luminance.is_finite()
            && (0.0..=1.).contains(&self.luminance)
            && self.color.is_finite()
            && (0.0..=1.).contains(&self.color)
            && (1..=8).contains(&self.radius)
            && self.threshold.is_finite()
            && (0.001..=1.).contains(&self.threshold)
    }
    pub fn apply(&self, source: &Rgba32FImage) -> Rgba32FImage {
        if !self.enabled() || !self.valid() {
            return source.clone();
        }
        let horizontal = self.pass(source, false);
        self.pass(&horizontal, true)
    }
    fn pass(&self, source: &Rgba32FImage, vertical: bool) -> Rgba32FImage {
        let mut output = source.clone();
        let sigma = f64::from(self.radius).max(1.) / 2.;
        let threshold = f64::from(self.threshold);
        let radius = self.radius as i32;
        let spatial_weights: Vec<f64> = (-radius..=radius)
            .map(|d| (-f64::from(d * d) / (2. * sigma * sigma)).exp())
            .collect();
        for (x, y, p) in output.enumerate_pixels_mut() {
            if p[3] <= 0. {
                continue;
            }
            let c = [f64::from(p[0]), f64::from(p[1]), f64::from(p[2])];
            let yc = lum(c);
            let cc = c.map(|v| v - yc);
            let mut sum_y = 0.;
            let mut sum_c = [0.; 3];
            let mut total_y = 0.;
            let mut total_c = 0.;
            for (i, spatial) in spatial_weights.iter().enumerate() {
                let d = i as i32 - radius;
                let (nx, ny) = if vertical {
                    (x as i32, y as i32 + d)
                } else {
                    (x as i32 + d, y as i32)
                };
                if nx < 0 || ny < 0 || nx >= source.width() as i32 || ny >= source.height() as i32 {
                    continue;
                }
                let q = source.get_pixel(nx as u32, ny as u32);
                if q[3] <= 0. {
                    continue;
                }
                let rgb = [f64::from(q[0]), f64::from(q[1]), f64::from(q[2])];
                let yq = lum(rgb);
                let cq = rgb.map(|v| v - yq);
                let delta = (yq - yc) / threshold;
                let wy = *spatial * (-0.5 * delta * delta).exp() * f64::from(q[3]).clamp(0., 1.);
                let distance = cq
                    .iter()
                    .zip(cc)
                    .map(|(a, b)| ((a - b) / threshold).powi(2))
                    .sum::<f64>()
                    / 3.;
                let wc = wy * (-0.5 * distance).exp();
                sum_y += yq * wy;
                total_y += wy;
                for k in 0..3 {
                    sum_c[k] += cq[k] * wc;
                }
                total_c += wc;
            }
            if total_y <= 0. || total_c <= 0. {
                continue;
            }
            let yout = yc + (sum_y / total_y - yc) * f64::from(self.luminance);
            for k in 0..3 {
                let chroma = cc[k] + (sum_c[k] / total_c - cc[k]) * f64::from(self.color);
                p[k] = (yout + chroma).clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32;
            }
        }
        output
    }
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) {
        ui.collapsing("노이즈 감소",|ui| {
 ui.add(crate::numeric_slider::Slider::new(&mut self.luminance,0.0..=1.).text("밝기 노이즈 감소"));ui.add(crate::numeric_slider::Slider::new(&mut self.color,0.0..=1.).text("색상 노이즈 감소"));ui.add(crate::numeric_slider::Slider::new(&mut self.radius,1..=8).text("노이즈 감소 반경 px"));ui.add(crate::numeric_slider::Slider::new(&mut self.threshold,0.001..=1.).logarithmic(true).text("노이즈 허용 차이"));ui.label("허용 차이를 낮추면 경계를 더 보존합니다. 크게 올리면 세부까지 부드러워질 수 있습니다. 투명 픽셀의 숨은 색은 주변에 섞지 않습니다.");if ui.button("노이즈 감소 초기화").clicked(){*self=Self::default();}});
    }
}
#[cfg(test)]
#[path = "denoise_tests.rs"]
mod tests;
