use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorMixer {
    pub hue: [f32; 8],
    pub saturation: [f32; 8],
    pub luminance: [f32; 8],
}
pub const NAMES: [&str; 8] = [
    "빨강",
    "주황",
    "노랑",
    "초록",
    "청록",
    "파랑",
    "보라",
    "마젠타",
];
const CENTERS: [f64; 8] = [0., 30., 60., 120., 180., 240., 270., 300.];
fn lum(rgb: [f64; 3]) -> f64 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}
fn hue(rgb: [f64; 3], low: f64, range: f64) -> f64 {
    let c = rgb.map(|v| (v - low) / range);
    let angle = if c[0] >= c[1] && c[0] >= c[2] {
        c[1] - c[2]
    } else if c[1] >= c[2] {
        2. + c[2] - c[0]
    } else {
        4. + c[0] - c[1]
    };
    (angle * 60.).rem_euclid(360.)
}
fn basis(h: f64) -> [f64; 3] {
    let h = h.rem_euclid(360.) / 60.;
    let x = 1. - (h % 2. - 1.).abs();
    match h as u8 {
        0 => [1., x, 0.],
        1 => [x, 1., 0.],
        2 => [0., 1., x],
        3 => [0., x, 1.],
        4 => [x, 0., 1.],
        _ => [1., 0., x],
    }
}
fn weights(h: f64) -> [f64; 8] {
    let left = CENTERS.partition_point(|c| *c <= h).saturating_sub(1);
    let right = (left + 1) % 8;
    let span = if right == 0 {
        360. - CENTERS[left]
    } else {
        CENTERS[right] - CENTERS[left]
    };
    let u = ((h - CENTERS[left]) / span).clamp(0., 1.);
    let w = u * u * (3. - 2. * u);
    let mut out = [0.; 8];
    out[left] = 1. - w;
    out[right] = w;
    out
}
impl ColorMixer {
    pub fn disabled(&self) -> bool {
        *self == Self::default()
    }
    pub fn valid(&self) -> bool {
        self.hue
            .iter()
            .all(|v| v.is_finite() && (-180.0..=180.).contains(v))
            && self
                .saturation
                .iter()
                .chain(&self.luminance)
                .all(|v| v.is_finite() && (-1.0..=1.).contains(v))
    }
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if self.disabled() {
            return rgb;
        }
        let c = rgb.map(f64::from);
        let low = c.into_iter().fold(f64::INFINITY, f64::min);
        let high = c.into_iter().fold(f64::NEG_INFINITY, f64::max);
        let range = high - low;
        if range <= f64::EPSILON * high.abs().max(1.) {
            return rgb;
        }
        let h = hue(c, low, range);
        let w = weights(h);
        let delta =
            |values: [f32; 8]| -> f64 { w.iter().zip(values).map(|(w, v)| w * f64::from(v)).sum() };
        let rotation = delta(self.hue);
        let saturation = delta(self.saturation);
        let brightness = delta(self.luminance);
        if rotation == 0. && saturation == 0. && brightness == 0. {
            return rgb;
        }
        let color = basis(h + rotation);
        let y = lum(c);
        let mean = lum(color);
        let chroma = range * (1. + saturation);
        let mut centered = color.map(|v| (v - mean) * chroma);
        // Preserve working-RGB luminosity; fit chroma to the display gamut only
        // for in-gamut source pixels. HDR/negative source values remain extended.
        if c.iter().all(|v| (0.0..=1.).contains(v)) {
            let mut scale = 1f64;
            for v in centered {
                if v > 0. {
                    scale = scale.min((1. - y) / v);
                } else if v < 0. {
                    scale = scale.min(-y / v);
                }
            }
            centered = centered.map(|v| v * scale.clamp(0., 1.));
        }
        centered.map(|v| {
            ((y + v) * (1. + brightness)).clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32
        })
    }
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) {
        ui.collapsing("색상 믹서",|ui| {
            let id=ui.id().with("mixer-band");let mut index=ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0).min(7);
            ui.horizontal_wrapped(|ui| {for (i,name) in NAMES.iter().enumerate() {if ui.selectable_label(index==i,*name).clicked() {index=i;}}});ui.data_mut(|d| d.insert_temp(id,index));
            ui.add(crate::numeric_slider::Slider::new(&mut self.hue[index],-180.0..=180.).text("색조 이동"));
            ui.add(crate::numeric_slider::Slider::new(&mut self.saturation[index],-1.0..=1.).text("색상별 채도"));
            ui.add(crate::numeric_slider::Slider::new(&mut self.luminance[index],-1.0..=1.).text("색상별 밝기"));
            ui.label("이웃 색상은 부드럽게 함께 조절합니다. 색조·채도는 밝기를 유지하며, 밝기는 선택한 색상을 어둡거나 밝게 조절합니다.");
            if ui.button("이 색상 초기화").clicked() {self.hue[index]=0.;self.saturation[index]=0.;self.luminance[index]=0.;}
            if ui.button("색상 믹서 전체 초기화").clicked() {*self=Self::default();}
        });
    }
}
#[cfg(test)]
#[path = "color_mixer_tests.rs"]
mod tests;
