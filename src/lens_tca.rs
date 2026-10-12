use super::{Profile, cubic_minimum};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TcaModel {
    Linear,
    Poly3,
    Acm,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tca {
    pub model: TcaModel,
    // Channel order is [constant, r, r²] or [alpha0, alpha1..alpha5].
    pub red: [f64; 6],
    pub blue: [f64; 6],
    pub enabled: bool,
}
impl Tca {
    pub fn effect_enabled(&self) -> bool {
        self.enabled
            && (self.red != [1., 0., 0., 0., 0., 0.] || self.blue != [1., 0., 0., 0., 0., 0.])
    }
    pub fn valid_for(&self, profile: &Profile) -> bool {
        let r = profile.radius_bound(self.model == TcaModel::Acm);
        if !r.is_finite() {
            return false;
        }
        [self.red, self.blue].into_iter().all(|v| {
            if v.iter().any(|v| !v.is_finite() || v.abs() > 100.) || !(0.1..=10.).contains(&v[0]) {
                return false;
            }
            match self.model {
                TcaModel::Linear => v[1..].iter().all(|v| *v == 0.),
                TcaModel::Poly3 => {
                    v[3..].iter().all(|v| *v == 0.)
                        && cubic_minimum([v[0], 2. * v[1], 3. * v[2], 0.], r) > 0.01
                }
                TcaModel::Acm => {
                    let tangent = 6. * (v[4].abs() + v[5].abs()) * r;
                    let radial = cubic_minimum([1., 3. * v[1], 5. * v[2], 7. * v[3]], r * r);
                    let angular = cubic_minimum([1., v[1], v[2], v[3]], r * r);
                    radial.min(angular) > 0.01 + tangent
                }
            }
        })
    }
    pub fn point(&self, p: [f64; 2], size: [u32; 2], profile: &Profile, blue: bool) -> [f64; 2] {
        let short = f64::from(size[0].saturating_sub(1).min(size[1].saturating_sub(1)));
        let center = [
            f64::from(size[0]) / 2. + short / 2. * profile.center[0],
            f64::from(size[1]) / 2. + short / 2. * profile.center[1],
        ];
        let diag = f64::from(size[0]).hypot(f64::from(size[1]));
        let scale = if self.model == TcaModel::Acm {
            36f64.hypot(24.) / (profile.input_crop * diag * profile.real_focal)
        } else {
            2. * profile.crop * profile.aspect.hypot(1.) / (profile.input_crop * diag)
        };
        let [x, y] = [(p[0] - center[0]) * scale, (p[1] - center[1]) * scale];
        let [a, b, c, d, e, f] = if blue { self.blue } else { self.red };
        let q = x * x + y * y;
        let (dx, dy) = match self.model {
            TcaModel::Linear => (a * x, a * y),
            TcaModel::Poly3 => {
                let factor = a + b * q.sqrt() + c * q;
                (x * factor, y * factor)
            }
            TcaModel::Acm => {
                let factor = 1. + b * q + c * q * q + d * q * q * q + 2. * (e * y + f * x);
                (a * (x * factor + f * q), a * (y * factor + e * q))
            }
        };
        [center[0] + dx / scale, center[1] + dy / scale]
    }
}
pub fn parse(node: roxmltree::Node<'_, '_>) -> Result<Option<Tca>, String> {
    use super::xml::number;
    let model = match node.attribute("model") {
        Some("linear") => TcaModel::Linear,
        Some("poly3") => TcaModel::Poly3,
        Some("acm") => TcaModel::Acm,
        Some("none") => return Ok(None),
        _ => return Err("알려지지 않은 색수차 모델을 제외했습니다.".into()),
    };
    let mut red = [1., 0., 0., 0., 0., 0.];
    let mut blue = red;
    match model {
        TcaModel::Linear => {
            red[0] = number(node, "kr", 1.)?;
            blue[0] = number(node, "kb", 1.)?;
        }
        TcaModel::Poly3 => {
            for (i, (r, b)) in [("vr", "vb"), ("cr", "cb"), ("br", "bb")]
                .into_iter()
                .enumerate()
            {
                red[i] = number(node, r, if i == 0 { 1. } else { 0. })?;
                blue[i] = number(node, b, if i == 0 { 1. } else { 0. })?;
            }
        }
        TcaModel::Acm => {
            for i in 0..6 {
                red[i] = number(node, &format!("alpha{i}"), if i == 0 { 1. } else { 0. })?;
                blue[i] = number(node, &format!("beta{i}"), if i == 0 { 1. } else { 0. })?;
            }
        }
    }
    // The measured TCA is offered separately; choosing distortion never silently enables it.
    Ok(Some(Tca {
        model,
        red,
        blue,
        enabled: false,
    }))
}
