use image::Rgba32FImage;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DetailSharpen {
    pub amount: f32,
    pub radius: f32,
    pub threshold: f32,
    pub masking: f32,
}
impl Default for DetailSharpen {
    fn default() -> Self {
        Self {
            amount: 0.,
            radius: 1.,
            threshold: 0.,
            masking: 0.,
        }
    }
}
fn luminance(c: [f32; 4]) -> f64 {
    f64::from(c[0]) * 0.2126 + f64::from(c[1]) * 0.7152 + f64::from(c[2]) * 0.0722
}
impl DetailSharpen {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn enabled(&self) -> bool {
        self.amount > 0.
    }
    pub fn valid(&self) -> bool {
        self.amount.is_finite()
            && (0.0..=3.).contains(&self.amount)
            && self.radius.is_finite()
            && (0.3..=8.).contains(&self.radius)
            && self.threshold.is_finite()
            && (0.0..=1.).contains(&self.threshold)
            && self.masking.is_finite()
            && (0.0..=1.).contains(&self.masking)
    }
    fn base(&self, source: &Rgba32FImage) -> Vec<f64> {
        let radius = (self.radius * 3.).ceil() as i32;
        let sigma = f64::from(self.radius);
        let kernel: Vec<f64> = (-radius..=radius)
            .map(|d| (-f64::from(d * d) / (2. * sigma * sigma)).exp())
            .collect();
        // Separable premultiplied luminance convolution uses f64 accumulators
        // so valid extended float source values cannot overflow in the blur.
        let width = source.width() as usize;
        let height = source.height() as usize;
        let mut horizontal = vec![[0f64; 2]; width * height];
        for y in 0..height {
            for x in 0..width {
                for (i, weight) in kernel.iter().enumerate() {
                    let nx = x as i32 + i as i32 - radius;
                    if nx < 0 || nx >= width as i32 {
                        continue;
                    }
                    let p = source.get_pixel(nx as u32, y as u32).0;
                    if p[3] <= 0. {
                        continue;
                    }
                    let alpha = f64::from(p[3]).clamp(0., 1.);
                    horizontal[y * width + x][0] += luminance(p) * alpha * weight;
                    horizontal[y * width + x][1] += alpha * weight;
                }
            }
        }
        let mut base = vec![0.; width * height];
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0.; 2];
                for (i, weight) in kernel.iter().enumerate() {
                    let ny = y as i32 + i as i32 - radius;
                    if ny < 0 || ny >= height as i32 {
                        continue;
                    }
                    let p = horizontal[ny as usize * width + x];
                    sum[0] += p[0] * weight;
                    sum[1] += p[1] * weight;
                }
                base[y * width + x] = if sum[1] > 0. {
                    sum[0] / sum[1]
                } else {
                    luminance(source.get_pixel(x as u32, y as u32).0)
                };
            }
        }
        base
    }
    pub fn apply(&self, source: &Rgba32FImage) -> Rgba32FImage {
        if !self.enabled() || !self.valid() {
            return source.clone();
        }
        let base = self.base(source);
        let mut output = source.clone();
        for (x, y, p) in output.enumerate_pixels_mut() {
            if p[3] <= 0. {
                continue;
            }
            let center = luminance(p.0);
            let detail = center - base[y as usize * source.width() as usize + x as usize];
            let threshold = f64::from(self.threshold);
            let detail = detail.signum() * (detail.abs() - threshold).max(0.);
            if detail == 0. {
                continue;
            }
            let mut gain = f64::from(self.amount);
            if self.masking > 0. {
                let mut edge = 0f64;
                for (dx, dy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                    let nx = x as i32 + dx;
                    let ny = y as i32 + dy;
                    if nx < 0
                        || ny < 0
                        || nx >= source.width() as i32
                        || ny >= source.height() as i32
                    {
                        continue;
                    }
                    let q = source.get_pixel(nx as u32, ny as u32).0;
                    if q[3] > 0. {
                        edge = edge.max((luminance(q) - center).abs());
                    }
                }
                let t = (edge / (f64::from(self.masking) * 0.25)).clamp(0., 1.);
                gain *= t * t * (3. - 2. * t);
            }
            let delta = detail * gain;
            if delta == 0. {
                continue;
            }
            for channel in &mut p.0[..3] {
                *channel = (f64::from(*channel) + delta)
                    .clamp(-f64::from(f32::MAX), f64::from(f32::MAX))
                    as f32;
            }
        }
        output
    }
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) {
        ui.collapsing("세부 선명화", |ui| {
            ui.add(crate::numeric_slider::Slider::new(&mut self.amount,0.0..=3.).text("세부 선명화 강도"));
            ui.add(crate::numeric_slider::Slider::new(&mut self.radius,0.3..=8.).text("선명화 반경 px"));
            ui.add(crate::numeric_slider::Slider::new(&mut self.threshold,0.0..=1.).text("선명화 임계값"));
            ui.add(crate::numeric_slider::Slider::new(&mut self.masking,0.0..=1.).text("경계만 선명하게"));
            ui.label("임계값은 작은 잡음의 강화를 줄입니다. 경계 제한을 올리면 평탄한 영역보다 강한 경계를 선명하게 합니다.");
            if ui.button("세부 선명화 초기화").clicked() {*self=Self::default();}
        });
    }
}
#[cfg(test)]
#[path = "detail_sharpen_tests.rs"]
mod tests;
