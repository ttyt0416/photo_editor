use super::{Profile, cubic_minimum};
use image::{Rgba, Rgba32FImage};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VignetteModel {
    Pa,
    Acm,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vignetting {
    pub model: VignetteModel,
    pub aperture: f64,
    pub distance: f64,
    pub coefficients: [f64; 3],
}
impl Vignetting {
    pub fn valid(&self) -> bool {
        self.aperture.is_finite()
            && (0.1..=1024.).contains(&self.aperture)
            && self.distance.is_finite()
            && (0.001..=1e6).contains(&self.distance)
            && self
                .coefficients
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 100.)
    }
    pub fn valid_for(&self, p: &Profile) -> bool {
        if !self.valid() {
            return false;
        }
        let center = 1. + p.center[0].abs().max(p.center[1].abs());
        let r = if self.model == VignetteModel::Pa {
            center * p.crop / p.input_crop
        } else {
            center * 36f64.hypot(24.) / (2. * p.input_crop * p.real_focal)
        };
        let [a, b, c] = self.coefficients;
        cubic_minimum([1., a, b, c], r * r) > 1e-4
    }
    pub fn divisor(&self, point: [f64; 2], size: [u32; 2], p: &Profile) -> f64 {
        let point = super::orientation::inverse(point, size, p.orientation);
        let size = super::orientation::size(size, p.orientation);
        let short = f64::from(size[0].saturating_sub(1).min(size[1].saturating_sub(1)));
        let center = [
            f64::from(size[0]) / 2. + short / 2. * p.center[0],
            f64::from(size[1]) / 2. + short / 2. * p.center[1],
        ];
        let diag = f64::from(size[0]).hypot(f64::from(size[1]));
        // PA uses the calibration half diagonal, unlike the distortion/TCA half short edge.
        let scale = if self.model == VignetteModel::Pa {
            2. * p.crop / (p.input_crop * diag)
        } else {
            36f64.hypot(24.) / (p.input_crop * diag * p.real_focal)
        };
        let x = (point[0] - center[0]) * scale;
        let y = (point[1] - center[1]) * scale;
        let q = x * x + y * y;
        let [a, b, c] = self.coefficients;
        1. + a * q + b * q * q + c * q * q * q
    }
    pub fn apply(
        &self,
        source: &Rgba32FImage,
        frame: [u32; 2],
        pad: u32,
        p: &Profile,
    ) -> Rgba32FImage {
        if self.coefficients == [0.; 3] {
            return source.clone();
        }
        Rgba32FImage::from_fn(source.width(), source.height(), |x, y| {
            let mut color = source.get_pixel(x, y).0;
            let point = [
                f64::from(x) + 0.5 - f64::from(pad),
                f64::from(y) + 0.5 - f64::from(pad),
            ];
            // Expanded filter spill is not camera sensor coverage.
            if color[3] <= 0.
                || point[0] < 0.
                || point[1] < 0.
                || point[0] >= f64::from(frame[0])
                || point[1] >= f64::from(frame[1])
            {
                return Rgba(color);
            }
            let divisor = self.divisor(point, frame, p);
            if divisor == 1. {
                return Rgba(color);
            }
            for v in &mut color[..3] {
                let encoded = f64::from(*v);
                let linear = if encoded <= 0.04045 {
                    encoded / 12.92
                } else {
                    ((encoded + 0.055) / 1.055).powf(2.4)
                };
                let linear = linear / divisor;
                let encoded = if linear <= 0.0031308 {
                    12.92 * linear
                } else {
                    1.055 * linear.powf(1. / 2.4) - 0.055
                };
                *v = encoded.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32;
            }
            Rgba(color)
        })
    }
}
pub fn parse(node: roxmltree::Node<'_, '_>) -> Result<Option<Vignetting>, String> {
    use super::xml::number;
    let model = match node.attribute("model") {
        Some("pa") => VignetteModel::Pa,
        Some("acm") => VignetteModel::Acm,
        Some("none") => return Ok(None),
        _ => return Err("알려지지 않은 주변부 밝기 모델을 제외했습니다.".into()),
    };
    let names = if model == VignetteModel::Pa {
        ["k1", "k2", "k3"]
    } else {
        ["alpha1", "alpha2", "alpha3"]
    };
    let mut coefficients = [0.; 3];
    for (i, key) in names.into_iter().enumerate() {
        coefficients[i] = number(node, key, 0.)?;
    }
    Ok(Some(Vignetting {
        model,
        aperture: number(node, "aperture", 0.)?,
        distance: number(node, "distance", 0.)?,
        coefficients,
    }))
}
