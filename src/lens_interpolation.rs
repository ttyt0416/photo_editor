use super::{Model, Profile, Tca, TcaModel, xml};
use std::sync::Arc;
// Cubic Hermite on the two surrounding measurements; outer neighbors supply tangents.
fn hermite(values: [Option<f64>; 4], t: f64) -> f64 {
    let a = values[1].unwrap();
    let b = values[2].unwrap();
    let left = values[0].map_or(b - a, |outer| (b - outer) * 0.5);
    let right = values[3].map_or(b - a, |outer| (outer - a) * 0.5);
    let t2 = t * t;
    let t3 = t2 * t;
    a * (2. * t3 - 3. * t2 + 1.)
        + left * (t3 - 2. * t2 + t)
        + b * (-2. * t3 + 3. * t2)
        + right * (t3 - t2)
}
type Neighborhood<'a> = ([Option<&'a Profile>; 4], f64, [f64; 2]);
fn neighborhood<'a>(rows: &[&'a Profile], focal: f64) -> Option<Neighborhood<'a>> {
    if rows.iter().any(|p| (p.focal - focal).abs() < 1e-6) {
        return None;
    }
    let right = rows.partition_point(|p| p.focal < focal);
    if right == 0 || right == rows.len() {
        return None;
    }
    let a = rows[right - 1].focal;
    let b = rows[right].focal;
    Some((
        [
            right.checked_sub(2).map(|i| rows[i]),
            Some(rows[right - 1]),
            Some(rows[right]),
            rows.get(right + 1).copied(),
        ],
        (focal - a) / (b - a),
        [a, b],
    ))
}
fn blend(
    rows: [Option<&Profile>; 4],
    t: f64,
    target: f64,
    power: i32,
    coefficient: impl Fn(&Profile) -> f64,
) -> f64 {
    let values = rows.map(|row| row.map(|p| coefficient(p) * p.focal.powi(power)));
    hermite(values, t) / target.powi(power)
}
fn distinct(
    rows: &mut Vec<&Profile>,
    equal: impl Fn(&Profile, &Profile) -> bool,
) -> Result<(), String> {
    rows.sort_by(|a, b| a.focal.total_cmp(&b.focal));
    for pair in rows.windows(2) {
        if (pair[0].focal - pair[1].focal).abs() < 1e-6 && !equal(pair[0], pair[1]) {
            return Err("같은 초점거리의 교정값이 여러 개입니다. 프로파일을 확인하세요.".into());
        }
    }
    rows.dedup_by(|a, b| (a.focal - b.focal).abs() < 1e-6);
    Ok(())
}
impl Profile {
    pub fn at_focal(&self, focal: f64) -> Result<Profile, String> {
        if !focal.is_finite() || !(0.1..=5000.).contains(&focal) || !self.valid() {
            return Err("초점거리 또는 현재 교정값을 확인하세요.".into());
        }
        if (focal - self.focal).abs() < 1e-6 {
            return Ok(self.clone());
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
        let mut result = self.clone();
        result.focal = focal;
        result.real_focal = focal;
        result.coefficients = [0.; 5];
        result.enabled = false;
        result.tca = None;
        result.interpolated = None;
        result.vignetting = Arc::new(Vec::new());
        result.vignetting_selected = None;
        result.vignetting_enabled = false;
        result.vignetting_query = None;
        let mut available = false;
        let mut distortion = rows
            .iter()
            .copied()
            .filter(|p| p.enabled && p.model == self.model)
            .collect::<Vec<_>>();
        distinct(&mut distortion, |a, b| {
            a.coefficients == b.coefficients && a.real_focal == b.real_focal
        })?;
        if let Some(exact) = distortion.iter().find(|p| (p.focal - focal).abs() < 1e-6) {
            result.coefficients = exact.coefficients;
            result.real_focal = exact.real_focal;
            result.enabled = self.enabled;
            available = true;
        } else if let Some((neighbors, t, range)) = neighborhood(&distortion, focal) {
            for i in 0..5 {
                let power = if self.model == Model::Acm {
                    if i < 3 { 1 - 2 * (i as i32 + 1) } else { 0 }
                } else {
                    1
                };
                result.coefficients[i] = blend(neighbors, t, focal, power, |p| p.coefficients[i]);
            }
            result.real_focal = hermite(neighbors.map(|p| p.map(|p| p.real_focal)), t);
            result.enabled = self.enabled;
            result.interpolated = Some(range);
            available = true;
        }
        let mut tcas = rows
            .iter()
            .copied()
            .filter(|p| p.tca.is_some())
            .collect::<Vec<_>>();
        distinct(&mut tcas, |a, b| a.tca == b.tca)?;
        let enabled = self.tca.as_ref().is_some_and(|t| t.enabled);
        if let Some(exact) = tcas.iter().find(|p| (p.focal - focal).abs() < 1e-6) {
            let mut tca = exact.tca.clone().unwrap();
            tca.enabled = enabled;
            result.tca = Some(tca);
            available = true;
        } else if let Some((neighbors, t, range)) = neighborhood(&tcas, focal) {
            let model = neighbors[1].unwrap().tca.as_ref().unwrap().model;
            if neighbors
                .iter()
                .flatten()
                .all(|p| p.tca.as_ref().unwrap().model == model)
            {
                let mut tca = Tca {
                    model,
                    red: [0.; 6],
                    blue: [0.; 6],
                    enabled,
                };
                for (blue, coefficients) in [(false, &mut tca.red), (true, &mut tca.blue)] {
                    for (i, v) in coefficients.iter_mut().enumerate() {
                        let power = match model {
                            TcaModel::Linear | TcaModel::Poly3 => {
                                if i == 0 {
                                    0
                                } else {
                                    1
                                }
                            }
                            TcaModel::Acm => {
                                if (1..=3).contains(&i) {
                                    1 - 2 * i as i32
                                } else {
                                    0
                                }
                            }
                        };
                        *v = blend(neighbors, t, focal, power, |p| {
                            let tca = p.tca.as_ref().unwrap();
                            if blue { tca.blue[i] } else { tca.red[i] }
                        });
                    }
                }
                result.tca = Some(tca);
                if result.interpolated.is_none() {
                    result.interpolated = Some(range);
                }
                available = true;
            }
        }
        if let Some(exact) = rows
            .iter()
            .find(|p| (p.focal - focal).abs() < 1e-6 && !p.vignetting.is_empty())
        {
            result.vignetting = exact.vignetting.clone();
            if let Some(old) = self
                .vignetting_selected
                .and_then(|i| self.vignetting.get(i))
            {
                result.vignetting_selected = result.vignetting.iter().position(|v| {
                    v.model == old.model && v.aperture == old.aperture && v.distance == old.distance
                });
            }
            result.vignetting_enabled =
                self.vignetting_enabled && result.vignetting_selected.is_some();
            available = true;
        }
        if !available {
            return Err(
                "측정된 초점거리 범위 안의 호환 교정값이 없습니다. 기존 설정은 유지합니다.".into(),
            );
        }
        if !result.valid() {
            return Err(
                "보간된 교정값이 접히거나 허용 범위를 넘습니다. 기존 설정은 유지합니다.".into(),
            );
        }
        Ok(result)
    }
}
