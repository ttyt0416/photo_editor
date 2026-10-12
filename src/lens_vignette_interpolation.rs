use super::{Profile, VignetteModel, Vignetting, xml};
use std::sync::Arc;
fn focal_span(profile: &Profile, rows: &[&Profile]) -> f64 {
    let measured = || {
        let min = rows.iter().map(|p| p.focal).fold(f64::INFINITY, f64::min);
        let max = rows
            .iter()
            .map(|p| p.focal)
            .fold(f64::NEG_INFINITY, f64::max);
        max - min
    };
    let Ok(document) = roxmltree::Document::parse(&profile.xml) else {
        return measured();
    };
    let Some(node) = document
        .root_element()
        .children()
        .find(|n| n.has_tag_name("focal"))
    else {
        return measured();
    };
    let min = node
        .attribute("min")
        .or_else(|| node.text())
        .and_then(|s| s.trim().parse::<f64>().ok());
    let max = node
        .attribute("max")
        .or_else(|| node.text())
        .and_then(|s| s.trim().parse::<f64>().ok());
    match (min, max) {
        (Some(min), Some(max))
            if min.is_finite()
                && max.is_finite()
                && (0.1..=5000.).contains(&min)
                && (min..=5000.).contains(&max) =>
        {
            max - min
        }
        _ => measured(),
    }
}
impl Profile {
    pub fn at_optics(&self, focal: f64, aperture: f64, distance: f64) -> Result<Profile, String> {
        if !self.valid()
            || !focal.is_finite()
            || !(0.1..=5000.).contains(&focal)
            || !aperture.is_finite()
            || !(0.1..=1024.).contains(&aperture)
            || !distance.is_finite()
            || !(0.001..=1e6).contains(&distance)
        {
            return Err("보정 초점거리·조리개·거리 값을 확인하세요.".into());
        }
        let database = xml::parse(&format!("<lensdatabase>{}</lensdatabase>", self.xml))?;
        let rows = database
            .profiles
            .iter()
            .filter(|p| {
                p.calibration == self.calibration
                    && p.crop == self.crop
                    && p.aspect == self.aspect
                    && p.center == self.center
            })
            .collect::<Vec<_>>();
        let model = self
            .vignetting_selected
            .and_then(|i| self.vignetting.get(i))
            .map(|v| v.model)
            .or_else(|| {
                rows.iter()
                    .find_map(|p| p.vignetting.first().map(|v| v.model))
            })
            .ok_or("주변부 밝기 측정값이 없습니다.")?;
        let mut entries = Vec::new();
        for p in &rows {
            for v in p.vignetting.iter().filter(|v| v.model == model) {
                if !entries
                    .iter()
                    .any(|(f, existing): &(f64, &Vignetting)| *f == p.focal && *existing == v)
                {
                    entries.push((p.focal, v));
                }
            }
        }
        if entries.is_empty() {
            return Err("호환 주변부 밝기 교정값이 없습니다.".into());
        }
        let span = focal_span(self, &rows);
        let distances = entries
            .iter()
            .map(|(f, v)| {
                let df = if span > 0. {
                    (focal - f) / span
                } else {
                    focal - f
                };
                df.hypot(4. / aperture - 4. / v.aperture)
                    .hypot(0.1 / distance - 0.1 / v.distance)
            })
            .collect::<Vec<_>>();
        let nearest = distances.iter().copied().fold(f64::INFINITY, f64::min);
        if nearest > 1. {
            return Err("주변부 밝기 측정값에서 너무 멉니다. 기존 설정은 유지합니다.".into());
        }
        let calculated = if let Some(i) = distances.iter().position(|v| *v < 1e-4) {
            entries[i].1.clone()
        } else {
            let mut coefficients = [0.; 3];
            let mut total = 0.;
            for ((f, v), d) in entries.iter().zip(&distances) {
                let weight = (nearest / d).powf(3.5);
                total += weight;
                for (i, c) in coefficients.iter_mut().enumerate() {
                    let scale = if model == VignetteModel::Acm {
                        f.powi(-2 * (i as i32 + 1))
                    } else {
                        1.
                    };
                    *c += weight * v.coefficients[i] * scale;
                }
            }
            for (i, c) in coefficients.iter_mut().enumerate() {
                let scale = if model == VignetteModel::Acm {
                    focal.powi(-2 * (i as i32 + 1))
                } else {
                    1.
                };
                *c /= total * scale;
            }
            Vignetting {
                model,
                aperture,
                distance,
                coefficients,
            }
        };
        let mut result = match self.at_focal(focal) {
            Ok(p) => p,
            Err(error) => {
                if self.distortion_enabled()
                    || self.tca.as_ref().is_some_and(|t| t.effect_enabled())
                {
                    return Err(error);
                }

                let mut p = self.clone();
                p.focal = focal;
                p.real_focal = focal;
                p.enabled = false;
                p.coefficients = [0.; 5];
                p.tca = None;
                p.interpolated = None;
                p.vignetting = Arc::new(Vec::new());
                p.vignetting_selected = None;
                p.vignetting_enabled = false;
                p.vignetting_query = None;
                p
            }
        };
        if !calculated.valid_for(&result) {
            return Err("보간된 주변부 밝기 분모가 잘못되었습니다. 기존 설정은 유지합니다.".into());
        }
        let mut table = result.vignetting.as_ref().clone();
        let selected = table
            .iter()
            .position(|v| v == &calculated)
            .unwrap_or_else(|| {
                let i = table.len();
                table.push(calculated);
                i
            });
        result.vignetting = Arc::new(table);
        result.vignetting_selected = Some(selected);
        result.vignetting_enabled = self.vignetting_enabled;
        result.vignetting_query = Some([focal, aperture, distance]);
        if !result.valid() {
            return Err("보간 결과가 허용 범위를 넘습니다. 기존 설정은 유지합니다.".into());
        }
        Ok(result)
    }
}
