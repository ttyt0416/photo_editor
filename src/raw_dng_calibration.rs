use super::{color_profile, dng_balance as tags};
use rawler::imgop::xyz::Illuminant;
type Matrix = [[f64; 4]; 4];
#[derive(Default)]
pub(super) struct Calibration {
    profiles: Vec<(Illuminant, Matrix)>,
    analog: [f64; 4],
    channels: usize,
}
fn signature(bytes: &[u8], tag: u16) -> Result<&str, String> {
    let Some((typ, count, start)) = tags::entry(bytes, tag)? else {
        return Ok("");
    };
    if ![1, 2].contains(&typ) || count > 4096 {
        return Err("DNG 카메라 보정 서명의 유형 또는 길이가 잘못되었습니다.".into());
    }
    let data = bytes
        .get(start..start + count)
        .ok_or("DNG 카메라 보정 서명이 잘렸습니다.")?;
    let text = data
        .strip_suffix(&[0])
        .ok_or("DNG 카메라 보정 서명의 끝이 잘못되었습니다.")?;
    if text.contains(&0) {
        return Err("DNG 카메라 보정 서명에 잘못된 문자가 있습니다.".into());
    }
    std::str::from_utf8(text).map_err(|_| "DNG 카메라 보정 서명이 UTF-8이 아닙니다.".into())
}
impl Calibration {
    pub fn read(bytes: &[u8], channels: usize) -> Result<Self, String> {
        if tags::entry(bytes, 50723)?.is_none() && tags::entry(bytes, 50724)?.is_none() {
            return Ok(Self::default());
        }
        if signature(bytes, 50931)? != signature(bytes, 50932)? {
            return Ok(Self::default());
        }
        if ![3, 4].contains(&channels) {
            return Err("이 센서의 DNG CameraCalibration은 아직 처리할 수 없습니다.".into());
        }
        let reader = tags::Reader {
            bytes,
            little: &bytes[..2] == b"II",
        };
        let mut profiles = vec![];
        for slot in 0..2 {
            if tags::entry(bytes, 50721 + slot)?.is_none() {
                if tags::entry(bytes, 50723 + slot)?.is_some() {
                    return Err("DNG 카메라 보정에 대응하는 색 행렬이 없습니다.".into());
                }
                continue;
            }
            let Some((kind, n, at)) = tags::entry(bytes, 50778 + slot)? else {
                return Err("DNG 카메라 보정의 기준 광원이 없습니다.".into());
            };
            if kind != 3 || n != 1 {
                return Err("DNG 카메라 보정 광원 유형이 잘못되었습니다.".into());
            }
            let light = Illuminant::try_from(reader.u16(at)?)?;
            super::color_matrix::white_point(light)?;
            let mut matrix = std::array::from_fn::<_, 4, _>(|i| {
                std::array::from_fn::<_, 4, _>(|j| f64::from(i == j))
            });
            if let Some((typ, count, start)) = tags::entry(bytes, 50723 + slot)? {
                if typ != 10 || count != channels * channels {
                    return Err(
                        "DNG CameraCalibration 행렬의 유형 또는 채널 수가 잘못되었습니다.".into(),
                    );
                }
                for (i, row) in matrix.iter_mut().take(channels).enumerate() {
                    for (j, value) in row.iter_mut().take(channels).enumerate() {
                        let at = start + (i * channels + j) * 8;
                        let numerator = reader.u32(at)? as i32;
                        let denominator = reader.u32(at + 4)? as i32;
                        if denominator == 0 {
                            return Err("DNG CameraCalibration 행렬 분모가0입니다.".into());
                        }
                        *value = f64::from(numerator) / f64::from(denominator);
                    }
                }
            }
            if profiles.iter().any(|(previous, _)| *previous == light) {
                return Err("DNG 카메라 보정 광원이 중복되었습니다.".into());
            }
            profiles.push((light, matrix));
        }
        let analog = tags::analog(bytes, channels)?
            .unwrap_or([1.; 4])
            .map(f64::from);
        Ok(Self {
            profiles,
            analog,
            channels,
        })
    }
    fn apply(&self, native: [[f32; 3]; 4], matrix: Matrix) -> Result<[[f32; 3]; 4], String> {
        if self.profiles.is_empty() {
            return Ok(native);
        }
        // Native matrices already include AB. AB*CC*AB^-1 preserves the DNG
        // order AB*CC*CM while CM and CC are interpolated independently.
        let mut result = [[0.; 3]; 4];
        for (i, row) in result.iter_mut().take(self.channels).enumerate() {
            for (axis, value) in row.iter_mut().enumerate() {
                let sum = (0..self.channels)
                    .map(|j| {
                        matrix[i][j]
                            * (self.analog[i] / self.analog[j])
                            * f64::from(native[j][axis])
                    })
                    .sum::<f64>();
                if !sum.is_finite() || sum.abs() > f64::from(f32::MAX) {
                    return Err("DNG 카메라 보정 행렬이 부동소수점 범위를 넘습니다.".into());
                }
                *value = sum as f32;
            }
        }
        Ok(result)
    }
    pub fn at_kelvin(&self, native: [[f32; 3]; 4], kelvin: f64) -> Result<[[f32; 3]; 4], String> {
        let Some(&(light, m1)) = self.profiles.first() else {
            return Ok(native);
        };
        let matrix = if let Some(&(other, m2)) = self.profiles.get(1) {
            let w = color_profile::weight(light, other, kelvin)?;
            std::array::from_fn(|i| std::array::from_fn(|j| w * m1[i][j] + (1. - w) * m2[i][j]))
        } else {
            m1
        };
        self.apply(native, matrix)
    }
    pub fn at_light(
        &self,
        native: [[f32; 3]; 4],
        light: Illuminant,
    ) -> Result<[[f32; 3]; 4], String> {
        if self.profiles.is_empty() {
            return Ok(native);
        }
        let matrix = self
            .profiles
            .iter()
            .find(|(l, _)| *l == light)
            .ok_or("DNG 카메라 보정과 색 행렬의 광원이 서로 다릅니다.")?
            .1;
        self.apply(native, matrix)
    }
}
