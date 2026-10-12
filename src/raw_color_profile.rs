use super::{RawImage, color_matrix, temperature};
use rawler::imgop::xyz::Illuminant;
use std::sync::OnceLock;
const LIGHTS: [Illuminant; 9] = [
    Illuminant::D65,
    Illuminant::A,
    Illuminant::B,
    Illuminant::C,
    Illuminant::D50,
    Illuminant::D55,
    Illuminant::D75,
    Illuminant::Daylight,
    Illuminant::Flash,
];
fn temperatures() -> &'static [f64; 9] {
    static VALUES: OnceLock<[f64; 9]> = OnceLock::new();
    VALUES.get_or_init(|| {
        LIGHTS.map(|light| {
            temperature::correlated_temperature(
                color_matrix::white_point(light).expect("supported illuminant"),
            )
        })
    })
}
#[cfg(test)]
pub(super) fn temperature_matrix(
    raw: &RawImage,
    kelvin: f32,
) -> Result<Option<[[f32; 3]; 4]>, String> {
    matrix_for_kelvin(&raw.color_matrix, f64::from(kelvin))
}
pub(super) fn matrix_for_kelvin(
    matrices: &std::collections::HashMap<Illuminant, Vec<f32>>,
    kelvin: f64,
) -> Result<Option<[[f32; 3]; 4]>, String> {
    if !kelvin.is_finite() || !(1000. ..=50000.).contains(&kelvin) {
        return Err("RAW 색온도 값이 잘못되었습니다.".into());
    }
    let mut profiles = vec![];
    for (index, light) in LIGHTS.iter().enumerate() {
        if let Some(flat) = matrices.get(light) {
            if ![9, 12].contains(&flat.len()) || flat.iter().any(|v| !v.is_finite()) {
                return Err("RAW 광원 프로파일 행렬이 잘못되었습니다.".into());
            }
            profiles.push((temperatures()[index], flat));
        }
    }
    if profiles.len() > 2 {
        return Err("세 개 이상의 RAW 광원 행렬 보간은 아직 구현되지 않았습니다.".into());
    }
    profiles.sort_by(|a, b| a.0.total_cmp(&b.0));
    let Some(&(first, m1)) = profiles.first() else {
        return Ok(None);
    };
    let flat = if let Some(&(second, m2)) = profiles.get(1) {
        if m1.len() != m2.len() {
            return Err("RAW 광원 행렬의 센서 채널 수가 서로 다릅니다.".into());
        }
        if (first - second).abs() < 0.01 {
            return Err("서로 구분할 수 없는 RAW 광원 행렬입니다.".into());
        }
        let weight = ((1. / kelvin - 1. / second) / (1. / first - 1. / second)).clamp(0., 1.);
        m1.iter()
            .zip(m2)
            .map(|(a, b)| (weight * f64::from(*a) + (1. - weight) * f64::from(*b)) as f32)
            .collect::<Vec<_>>()
    } else {
        m1.clone()
    };
    let mut matrix = [[0.; 3]; 4];
    for (i, row) in flat.as_chunks::<3>().0.iter().enumerate() {
        matrix[i].copy_from_slice(row);
    }
    Ok(Some(matrix))
}

pub(super) fn weight(first: Illuminant, second: Illuminant, kelvin: f64) -> Result<f64, String> {
    let at = |light| {
        LIGHTS
            .iter()
            .position(|v| *v == light)
            .map(|i| temperatures()[i])
            .ok_or("이 DNG 카메라 보정 광원은 아직 처리할 수 없습니다.")
    };
    let a = at(first)?;
    let b = at(second)?;
    if (a - b).abs() < 0.01 {
        return Err("DNG 카메라 보정 광원을 구분할 수 없습니다.".into());
    }
    Ok(((1. / kelvin - 1. / b) / (1. / a - 1. / b)).clamp(0., 1.))
}
pub(super) fn matrix_calibrated(
    matrices: &std::collections::HashMap<Illuminant, Vec<f32>>,
    kelvin: f64,
    cal: &super::dng_calibration::Calibration,
) -> Result<Option<[[f32; 3]; 4]>, String> {
    matrix_for_kelvin(matrices, kelvin)?
        .map(|native| cal.at_kelvin(native, kelvin))
        .transpose()
}
type MatrixWhite = ([[f32; 3]; 4], [f64; 3]);
#[cfg(test)]
pub(super) fn balance_profile(
    raw: &RawImage,
    channels: usize,
    gains: [f32; 4],
) -> Result<Option<MatrixWhite>, String> {
    balance_profile_calibrated(raw, channels, gains, &Default::default())
}
pub(super) fn balance_profile_calibrated(
    raw: &RawImage,
    channels: usize,
    gains: [f32; 4],
    cal: &super::dng_calibration::Calibration,
) -> Result<Option<MatrixWhite>, String> {
    let count = LIGHTS
        .iter()
        .filter(|light| raw.color_matrix.contains_key(light))
        .count();
    if raw.is_monochrome() || count < 2 {
        return Ok(None);
    }
    if ![3, 4].contains(&channels) || raw.color_matrix.values().any(|m| m.len() != channels * 3) {
        return Err("RAW 광원 행렬과 센서 채널 수가 서로 다릅니다.".into());
    }
    if gains[..channels].iter().any(|v| !v.is_finite() || *v <= 0.) {
        return Err("화이트 밸런스 값이 잘못되었습니다.".into());
    }
    let neutral = std::array::from_fn::<_, 4, _>(|i| {
        if i < channels {
            1. / f64::from(gains[i])
        } else {
            0.
        }
    });
    let mut white = color_matrix::white_point(Illuminant::D65)?;
    // DNG CameraNeutral -> xy uses iterative profile selection. Bounded failure
    // preserves the original instead of silently choosing an unrelated profile.
    for _ in 0..32 {
        let k = temperature::correlated_temperature(white);
        let native =
            matrix_calibrated(&raw.color_matrix, k, cal)?.ok_or("RAW 광원 행렬이 없습니다.")?;
        let inverse = color_matrix::pseudo_inverse(native.map(|row| row.map(f64::from)))?;
        let xyz = inverse.map(|row| row.iter().zip(neutral).map(|(a, b)| a * b).sum::<f64>());
        if xyz.iter().any(|v| !v.is_finite() || *v <= 0.) {
            return Err("화이트 밸런스에서 유효한 RAW 광원 색상을 계산할 수 없습니다.".into());
        }
        let next = xyz.map(|v| v / xyz[1]);
        let sum = white.iter().sum::<f64>();
        let next_sum = next.iter().sum::<f64>();
        let distance = (0..2)
            .map(|i| (white[i] / sum - next[i] / next_sum).abs())
            .fold(0., f64::max);
        if distance < 1e-7 {
            return Ok(Some((native, next)));
        }
        white = next;
    }
    Err("화이트 밸런스의 RAW 광원 계산이 수렴하지 않았습니다. 색온도·틴트 모드를 사용하거나 배율을 조절하세요.".into())
}
