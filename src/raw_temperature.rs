// The unmodified CIE observer CSV is CC BY-SA 4.0; attribution: data/cie/README.md.
use super::{RawImage, color_matrix};
use std::sync::OnceLock;

fn observer() -> &'static [[f64; 4]] {
    static TABLE: OnceLock<Vec<[f64; 4]>> = OnceLock::new();
    TABLE.get_or_init(|| {
        include_str!("../data/cie/CIE_xyz_1931_2deg.csv")
            .lines()
            .map(|line| {
                let row: Vec<f64> = line
                    .split(',')
                    .map(|v| v.parse().expect("checked CIE table"))
                    .collect();
                row.try_into().expect("four CIE columns")
            })
            .collect()
    })
}
fn planck_xyz(kelvin: f64) -> [f64; 3] {
    // Exact SI h, c and k (NIST SP 961); a common scale cancels in Y normalization.
    const C2: f64 = 6.626_070_15e-34 * 299_792_458. / 1.380_649e-23;
    let mut xyz = [0.; 3];
    for &[nm, x, y, z] in observer() {
        let lambda = nm * 1e-9;
        let power = 1. / (lambda.powi(5) * (C2 / (lambda * kelvin)).exp_m1());
        for (sum, cmf) in xyz.iter_mut().zip([x, y, z]) {
            *sum += power * cmf;
        }
    }
    xyz.map(|v| v / xyz[1])
}
fn uv(xyz: [f64; 3]) -> [f64; 2] {
    let denominator = xyz[0] + 15. * xyz[1] + 3. * xyz[2];
    [4. * xyz[0] / denominator, 6. * xyz[1] / denominator]
}
pub(super) fn white_point(kelvin: f32, tint: f32) -> Result<[f64; 3], String> {
    if !kelvin.is_finite()
        || !(1000. ..=50000.).contains(&kelvin)
        || !tint.is_finite()
        || !(-200. ..=200.).contains(&tint)
    {
        return Err("RAW 색온도·틴트 값이 잘못되었습니다.".into());
    }
    let temperature = f64::from(kelvin);
    let [mut u, mut v] = uv(planck_xyz(temperature));
    if tint != 0. {
        let step = temperature * 1e-4;
        let lower = uv(planck_xyz(temperature - step));
        let upper = uv(planck_xyz(temperature + step));
        let tangent = [upper[0] - lower[0], upper[1] - lower[1]];
        // The positive normal points toward green illuminants: correction is magenta.
        let mut normal = [tangent[1], -tangent[0]];
        if normal[1] < 0. {
            normal = normal.map(|n| -n);
        }
        let length = normal[0].hypot(normal[1]);
        if !length.is_finite() || length <= 0. {
            return Err("RAW 색온도 궤적을 계산할 수 없습니다.".into());
        }
        let offset = f64::from(tint) * 1e-4 / length;
        u += normal[0] * offset;
        v += normal[1] * offset;
    }
    // CIE 1960 UCS -> xy -> XYZ, with Y = 1.
    let denominator = 2. * u - 8. * v + 4.;
    let x = 3. * u / denominator;
    let y = 2. * v / denominator;
    let result = [x / y, 1., (1. - x - y) / y];
    if result.iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("이 색온도·틴트의 광원 색상을 계산할 수 없습니다.".into());
    }
    Ok(result)
}
pub(super) fn sensor_gains(
    matrix: &[[f32; 3]; 4],
    channels: usize,
    white: [f64; 3],
) -> Result<[f32; 4], String> {
    if ![3, 4].contains(&channels) {
        return Err("RAW 센서 색 채널 수가 잘못되었습니다.".into());
    }
    let neutral = matrix.map(|row| {
        row.into_iter()
            .zip(white)
            .map(|(a, b)| f64::from(a) * b)
            .sum::<f64>()
    });
    if neutral[..channels]
        .iter()
        .any(|v| !v.is_finite() || *v <= 0.)
    {
        return Err("카메라 색 행렬에서 이 광원의 화이트 밸런스를 계산할 수 없습니다. 촬영값 또는 사용자 지정 배율을 사용하세요.".into());
    }
    let mut gains = [1.; 4];
    for i in 0..channels {
        let g = neutral[1] / neutral[i];
        if !g.is_finite() || g > f64::from(f32::MAX) || g < f64::from(f32::MIN_POSITIVE) {
            return Err("RAW 색온도 배율이 부동소수점 범위를 넘습니다.".into());
        }
        gains[i] = g as f32;
    }
    Ok(gains)
}
pub(super) struct Prepared {
    pub gains: [f32; 4],
    pub matrix: [[f32; 4]; 3],
}
#[cfg(test)]
pub(super) fn prepare(
    raw: &RawImage,
    channels: usize,
    kelvin: f32,
    tint: f32,
) -> Result<Prepared, String> {
    prepare_calibrated(raw, channels, kelvin, tint, &Default::default())
}
pub(super) fn prepare_calibrated(
    raw: &RawImage,
    channels: usize,
    kelvin: f32,
    tint: f32,
    cal: &super::dng_calibration::Calibration,
) -> Result<Prepared, String> {
    if raw.is_monochrome() {
        return Ok(Prepared {
            gains: [1.; 4],
            matrix: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        });
    }
    if raw
        .color_matrix
        .values()
        .any(|flat| flat.len() != channels * 3)
    {
        return Err("RAW 광원 행렬과 센서 채널 수가 서로 다릅니다.".into());
    }
    let white = white_point(kelvin, tint)?;
    let native=super::color_profile::matrix_calibrated(&raw.color_matrix,f64::from(kelvin),cal)?.ok_or("이 RAW에는 색온도 계산에 필요한 카메라 색 행렬이 없습니다. 촬영값 또는 사용자 지정 배율을 사용하세요.")?;
    let gains = sensor_gains(&native, channels, white)?;
    let matrix = super::calibration_matrix(color_matrix::adapt_white(native, white)?)?;
    Ok(Prepared { gains, matrix })
}
#[cfg(test)]
pub(super) fn gains(
    raw: &RawImage,
    channels: usize,
    kelvin: f32,
    tint: f32,
) -> Result<[f32; 4], String> {
    prepare(raw, channels, kelvin, tint).map(|p| p.gains)
}
pub(super) fn correlated_temperature(xyz: [f64; 3]) -> f64 {
    let target = uv(xyz);
    let distance = |k: f64| {
        let point = uv(planck_xyz(k.exp()));
        (point[0] - target[0]).powi(2) + (point[1] - target[1]).powi(2)
    };
    let (mut low, mut high) = (1000f64.ln(), 50000f64.ln());
    let ratio = (5f64.sqrt() - 1.) / 2.;
    let mut a = high - ratio * (high - low);
    let mut b = low + ratio * (high - low);
    let (mut da, mut db) = (distance(a), distance(b));
    for _ in 0..64 {
        if da < db {
            high = b;
            b = a;
            db = da;
            a = high - ratio * (high - low);
            da = distance(a);
        } else {
            low = a;
            a = b;
            da = db;
            b = low + ratio * (high - low);
            db = distance(b);
        }
    }
    ((low + high) / 2.).exp()
}

#[cfg(test)]
#[path = "raw_temperature_tests.rs"]
mod tests;
