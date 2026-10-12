use image::{Rgba, Rgba32FImage};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Geometry {
    pub angle: f32,
    pub perspective: [f32; 2],
    pub zoom: f32,
    pub aspect: f32,
    pub shift: [f32; 2],
    pub sampling: crate::sampling::Sampling,
}
impl Default for Geometry {
    fn default() -> Self {
        Self {
            angle: 0.,
            perspective: [0.; 2],
            zoom: 1.,
            aspect: 1.,
            shift: [0.; 2],
            sampling: crate::sampling::Sampling::Bilinear,
        }
    }
}
impl Geometry {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn enabled(&self) -> bool {
        self.angle != 0.
            || self.perspective != [0.; 2]
            || self.zoom != 1.
            || self.aspect != 1.
            || self.shift != [0.; 2]
    }
    pub fn valid(&self) -> bool {
        self.angle.is_finite()
            && (-180. ..=180.).contains(&self.angle)
            && self
                .perspective
                .iter()
                .all(|v| v.is_finite() && (-0.4..=0.4).contains(v))
            && self.zoom.is_finite()
            && (0.25..=4.).contains(&self.zoom)
            && self.aspect.is_finite()
            && (0.25..=4.).contains(&self.aspect)
            && self
                .shift
                .iter()
                .all(|v| v.is_finite() && (-1. ..=1.).contains(v))
    }
    fn mapping(&self, frame: [u32; 2]) -> crate::matrix::Matrix {
        use crate::matrix as m;
        let c = frame.map(|v| f64::from(v) * 0.5);
        let (sin, cos) = f64::from(self.angle).to_radians().sin_cos();
        let sx = f64::from(self.zoom) * f64::from(self.aspect);
        let sy = f64::from(self.zoom);
        let affine = [cos * sx, -sin * sy, 0., sin * sx, cos * sy, 0., 0., 0., 1.];
        // Source-normalized perspective keeps the forward denominator >= 0.2.
        let perspective = [
            1.,
            0.,
            0.,
            0.,
            1.,
            0.,
            f64::from(self.perspective[0]) / c[0],
            f64::from(self.perspective[1]) / c[1],
            1.,
        ];
        m::multiply(
            m::translate(
                c[0] + f64::from(self.shift[0]) * f64::from(frame[0]),
                c[1] + f64::from(self.shift[1]) * f64::from(frame[1]),
            ),
            m::multiply(affine, m::multiply(perspective, m::translate(-c[0], -c[1]))),
        )
    }
    pub fn apply_frame(&self, source: &Rgba32FImage, frame: [u32; 2], pad: u32) -> Rgba32FImage {
        if !self.enabled() || !self.valid() || frame.contains(&0) {
            return source.clone();
        }
        let Some(inverse) = crate::matrix::inverse(self.mapping(frame)) else {
            return source.clone();
        };
        Rgba32FImage::from_fn(source.width(), source.height(), |x, y| {
            let p = [
                f64::from(x) + 0.5 - f64::from(pad),
                f64::from(y) + 0.5 - f64::from(pad),
            ];
            let w = inverse[6] * p[0] + inverse[7] * p[1] + inverse[8];
            // The other projective sheet is outside the visible source image.
            if !w.is_finite() || w <= 1e-12 {
                return Rgba([0.; 4]);
            }
            let point = [
                ((inverse[0] * p[0] + inverse[1] * p[1] + inverse[2]) / w + f64::from(pad)) as f32,
                ((inverse[3] * p[0] + inverse[4] * p[1] + inverse[5]) / w + f64::from(pad)) as f32,
            ];
            Rgba(crate::sampling::rgba(point, self.sampling, |sx, sy| {
                if sx < 0 || sy < 0 || sx >= source.width() as i32 || sy >= source.height() as i32 {
                    [0.; 4]
                } else {
                    source.get_pixel(sx as u32, sy as u32).0
                }
            }))
        })
    }
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) {
        use eframe::egui;
        ui.collapsing("기하 보정", |ui| {
            ui.add(crate::numeric_slider::Slider::new(&mut self.angle, -180. ..=180.).text("회전 °"));
            ui.add(crate::numeric_slider::Slider::new(&mut self.perspective[0], -0.4..=0.4).text("수평 원근"));
            ui.add(crate::numeric_slider::Slider::new(&mut self.perspective[1], -0.4..=0.4).text("수직 원근"));
            ui.add(crate::numeric_slider::Slider::new(&mut self.zoom, 0.25..=4.).text("기하 확대"));
            ui.add(crate::numeric_slider::Slider::new(&mut self.aspect, 0.25..=4.).text("가로세로 비율"));
            ui.add(crate::numeric_slider::Slider::new(&mut self.shift[0], -1. ..=1.).text("수평 이동"));
            ui.add(crate::numeric_slider::Slider::new(&mut self.shift[1], -1. ..=1.).text("수직 이동"));
            egui::ComboBox::from_id_salt("geometry-sampling").selected_text(self.sampling.label()).show_ui(ui, |ui| {
                for mode in [crate::sampling::Sampling::Nearest, crate::sampling::Sampling::Bilinear, crate::sampling::Sampling::Bicubic] {
                    ui.selectable_value(&mut self.sampling, mode, mode.label());
                }
            });
            ui.label("원본 크기의 경계 안에서 보정합니다. 빈 가장자리는 투명하며 확대 또는 자르기로 정리할 수 있습니다. 이동은 사진 폭·높이에 대한 비율입니다.");
            if !self.valid() { ui.colored_label(egui::Color32::YELLOW, "기하 보정 값이 유효하지 않습니다."); }
            if ui.button("기하 초기화").clicked() { *self = Self::default(); }
        });
    }
}
#[cfg(test)]
#[path = "photo_geometry_tests.rs"]
mod tests;
