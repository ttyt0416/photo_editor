use super::{color_profile, dng_balance as tags, temperature};
use rawler::imgop::xyz::Illuminant;
use std::collections::HashMap;
pub(super) struct ShotInfo {
    pub gains: [f32; 4],
    pub channels: usize,
    pub colors: Option<[u8; 4]>,
}
pub(super) fn white(bytes: &[u8]) -> Result<Option<[f64; 3]>, String> {
    let Some((typ, count, start)) = tags::entry(bytes, 50729)? else {
        return Ok(None);
    };
    if typ != 5 || count != 2 {
        return Err("DNG AsShotWhiteXY 개수 또는 유형이 잘못되었습니다.".into());
    }
    if tags::entry(bytes, 50728)?.is_some() {
        return Err("DNG에는 AsShotWhiteXY와 AsShotNeutral을 동시에 지정할 수 없습니다.".into());
    }
    let reader = tags::Reader {
        bytes,
        little: &bytes[..2] == b"II",
    };
    let mut xy = [0.; 2];
    for (i, v) in xy.iter_mut().enumerate() {
        let denominator = reader.u32(start + i * 8 + 4)?;
        if denominator == 0 {
            return Err("DNG AsShotWhiteXY 분모가0입니다.".into());
        }
        *v = f64::from(reader.u32(start + i * 8)?) / f64::from(denominator);
    }
    let [x, y] = xy;
    if x <= 0. || y <= 0. || x + y >= 1. {
        return Err("DNG AsShotWhiteXY가 유효한 광원 좌표가 아닙니다.".into());
    }
    Ok(Some([x / y, 1., (1. - x - y) / y]))
}
pub(super) fn info(bytes: &[u8]) -> Result<Option<ShotInfo>, String> {
    let Some(white) = white(bytes)? else {
        return Ok(None);
    };
    let reader = tags::Reader {
        bytes,
        little: &bytes[..2] == b"II",
    };
    let mut matrices = HashMap::new();
    let mut channels = 0;
    for slot in 0..2 {
        let Some((typ, count, start)) = tags::entry(bytes, 50721 + slot)? else {
            continue;
        };
        if typ != 10 || ![9, 12].contains(&count) {
            return Err("DNG 촬영 광원 색 행렬이 잘못되었습니다.".into());
        }
        if channels != 0 && channels != count / 3 {
            return Err("DNG 광원 행렬의 채널 수가 서로 다릅니다.".into());
        }
        channels = count / 3;
        let Some((kind, n, light_at)) = tags::entry(bytes, 50778 + slot)? else {
            return Err("DNG 촬영 광원의 기준 광원 정보가 없습니다.".into());
        };
        if kind != 3 || n != 1 {
            return Err("DNG 촬영 기준 광원 유형이 잘못되었습니다.".into());
        }
        let light = Illuminant::try_from(reader.u16(light_at)?)?;
        let mut flat = Vec::with_capacity(count);
        for i in 0..count {
            let num = reader.u32(start + i * 8)? as i32;
            let den = reader.u32(start + i * 8 + 4)? as i32;
            if den == 0 {
                return Err("DNG 촬영 광원 행렬 분모가0입니다.".into());
            }
            flat.push((f64::from(num) / f64::from(den)) as f32);
        }
        if matrices.insert(light, flat).is_some() {
            return Err("DNG에 같은 광원의 행렬이 중복되었습니다.".into());
        }
    }
    if ![3, 4].contains(&channels) {
        return Err("AsShotWhiteXY를 변환할 DNG 카메라 색 행렬이 없습니다.".into());
    }
    if let Some(analog) = tags::analog(bytes, channels)? {
        for flat in matrices.values_mut() {
            for (row, gain) in flat.as_chunks_mut::<3>().0.iter_mut().zip(analog) {
                for v in row {
                    *v = (f64::from(*v) * f64::from(gain)) as f32;
                }
            }
        }
    }
    let cal = super::dng_calibration::Calibration::read(bytes, channels)?;
    let native = color_profile::matrix_calibrated(
        &matrices,
        temperature::correlated_temperature(white),
        &cal,
    )?
    .ok_or("AsShotWhiteXY의 기준 광원은 아직 처리할 수 없습니다.")?;
    let gains = temperature::sensor_gains(&native, channels, white)?;
    let cfa = tags::entry(bytes, 262)?.is_some_and(|(typ, count, at)| {
        typ == 3 && count == 1 && reader.u16(at).is_ok_and(|v| v == 32803)
    });
    let colors = if channels == 4 && cfa {
        tags::plane_colors(bytes)?
    } else {
        None
    };
    Ok(Some(ShotInfo {
        gains,
        channels,
        colors,
    }))
}
