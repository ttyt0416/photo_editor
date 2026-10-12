// Bounded classic DNG tags omitted by Rawler 0.8.0 (Adobe DNG 1.7.1).
pub(super) struct Reader<'a> {
    pub bytes: &'a [u8],
    pub little: bool,
}
impl Reader<'_> {
    pub fn u16(&self, at: usize) -> Result<u16, String> {
        let v: [u8; 2] = self
            .bytes
            .get(at..at + 2)
            .ok_or("RAW DNG 태그가 잘렸습니다.")?
            .try_into()
            .unwrap();
        Ok(if self.little {
            u16::from_le_bytes(v)
        } else {
            u16::from_be_bytes(v)
        })
    }
    pub fn u32(&self, at: usize) -> Result<u32, String> {
        let v: [u8; 4] = self
            .bytes
            .get(at..at + 4)
            .ok_or("RAW DNG 태그가 잘렸습니다.")?
            .try_into()
            .unwrap();
        Ok(if self.little {
            u32::from_le_bytes(v)
        } else {
            u32::from_be_bytes(v)
        })
    }
}
pub(super) fn entry(bytes: &[u8], wanted: u16) -> Result<Option<(u16, usize, usize)>, String> {
    if bytes.len() < 8 {
        return Ok(None);
    }
    let little = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Ok(None),
    };
    let r = Reader { bytes, little };
    if r.u16(2)? != 42 {
        return Ok(None);
    }
    let mut pending = vec![r.u32(4)? as usize];
    let mut seen = std::collections::HashSet::new();
    let mut dng = false;
    let mut found = None;
    while let Some(offset) = pending.pop() {
        if offset == 0 || !seen.insert(offset) {
            continue;
        }
        if seen.len() > 1024 {
            return Err("RAW DNG 디렉터리가 너무 많습니다.".into());
        }
        let count = r.u16(offset)? as usize;
        let end = offset + 2 + count * 12;
        if end + 4 > bytes.len() {
            return Err("RAW DNG 디렉터리가 잘렸습니다.".into());
        }
        for i in 0..count {
            let p = offset + 2 + i * 12;
            let tag = r.u16(p)?;
            let typ = r.u16(p + 2)?;
            let n = r.u32(p + 4)? as usize;
            if tag == 50706 && typ == 1 && n == 4 {
                dng = true;
            }
            if tag == wanted && found.is_none() {
                let size = match typ {
                    1 | 2 => 1,
                    3 => 2,
                    4 => 4,
                    5 | 10 => 8,
                    _ => 1,
                };
                let start = if n.saturating_mul(size) <= 4 {
                    p + 8
                } else {
                    r.u32(p + 8)? as usize
                };
                found = Some((typ, n, start));
            }
            if tag == 330 && [4, 13].contains(&typ) {
                if n > 1024 {
                    return Err("RAW DNG 하위 디렉터리가 너무 많습니다.".into());
                }
                let start = if n == 1 {
                    p + 8
                } else {
                    r.u32(p + 8)? as usize
                };
                for j in 0..n {
                    pending.push(r.u32(start + j * 4)? as usize);
                }
            }
        }
        pending.push(r.u32(end)? as usize);
    }
    Ok(if dng { found } else { None })
}
pub(super) fn four_channel(bytes: &[u8]) -> Result<Option<[f32; 4]>, String> {
    let Some((typ, count, start)) = entry(bytes, 50728)? else {
        return Ok(None);
    };
    if count != 4 || ![3, 5].contains(&typ) {
        return Err("4채널 DNG의 AsShotNeutral 개수 또는 유형이 잘못되었습니다.".into());
    }
    let r = Reader {
        bytes,
        little: &bytes[..2] == b"II",
    };
    let mut gains = [0.; 4];
    for (i, gain) in gains.iter_mut().enumerate() {
        let neutral = if typ == 3 {
            f64::from(r.u16(start + i * 2)?)
        } else {
            let at = start + i * 8;
            let denominator = r.u32(at + 4)?;
            if denominator == 0 {
                return Err("4채널 DNG의 AsShotNeutral 분모가 0입니다.".into());
            }
            f64::from(r.u32(at)?) / f64::from(denominator)
        };
        let value = 1. / neutral;
        if !value.is_finite() || value > f64::from(f32::MAX) || value < f64::from(f32::MIN_POSITIVE)
        {
            return Err("4채널 DNG의 AsShotNeutral 값이 잘못되었습니다.".into());
        }
        *gain = value as f32;
    }
    Ok(Some(gains))
}
pub(super) fn plane_colors(bytes: &[u8]) -> Result<Option<[u8; 4]>, String> {
    let Some((typ, count, start)) = entry(bytes, 50710)? else {
        return Ok(None);
    };
    if typ != 1 || count != 4 {
        return Err("4색 DNG의 CFAPlaneColor 개수 또는 유형이 잘못되었습니다.".into());
    }
    let values: [u8; 4] = bytes
        .get(start..start + 4)
        .ok_or("4색 DNG의 CFAPlaneColor가 잘렸습니다.")?
        .try_into()
        .unwrap();
    if values.iter().any(|v| *v > 5)
        || values
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != 4
    {
        return Err(
            "이 4색 DNG 센서의 색 목록은 아직 처리할 수 없습니다. 원본은 유지합니다.".into(),
        );
    }
    Ok(Some(values))
}

type LegacyInfo = ([f32; 4], Option<[u8; 4]>);
pub(super) fn legacy_info(bytes: &[u8]) -> Result<Option<LegacyInfo>, String> {
    if !entry(bytes, 50721)?.is_some_and(|(_, count, _)| count == 12)
        && !entry(bytes, 50722)?.is_some_and(|(_, count, _)| count == 12)
    {
        return Ok(None);
    }
    if !entry(bytes, 50728)?.is_some_and(|(_, count, _)| count == 4) {
        return Ok(None);
    }
    let Some(wb) = four_channel(bytes)? else {
        return Ok(None);
    };
    Ok(Some((wb, plane_colors(bytes)?)))
}

pub(super) fn analog(bytes: &[u8], channels: usize) -> Result<Option<[f32; 4]>, String> {
    let Some((typ, count, start)) = entry(bytes, 50727)? else {
        return Ok(None);
    };
    if typ != 5 || count != channels || ![1, 3, 4].contains(&channels) {
        return Err("DNG AnalogBalance 개수 또는 유형이 잘못되었습니다.".into());
    }
    let reader = Reader {
        bytes,
        little: &bytes[..2] == b"II",
    };
    let mut gains = [1.; 4];
    for (i, gain) in gains.iter_mut().take(channels).enumerate() {
        let numerator = reader.u32(start + i * 8)?;
        let denominator = reader.u32(start + i * 8 + 4)?;
        if denominator == 0 || numerator == 0 {
            return Err("DNG AnalogBalance는 양의 유한 배율이어야 합니다.".into());
        }
        let value = f64::from(numerator) / f64::from(denominator);
        if !value.is_finite() || value > f64::from(f32::MAX) || value < f64::from(f32::MIN_POSITIVE)
        {
            return Err("DNG AnalogBalance 배율이 범위를 넘습니다.".into());
        }
        *gain = value as f32;
    }
    Ok(Some(gains))
}
pub(super) fn apply_analog(
    raw: &mut super::RawImage,
    bytes: &[u8],
    channels: usize,
) -> Result<(), String> {
    let Some(gains) = analog(bytes, channels)? else {
        return Ok(());
    };
    if channels == 1 {
        if gains[0] != 1. {
            return Err("흑백 DNG의 별도 AnalogBalance 현상은 아직 구현되지 않았습니다.".into());
        }
        return Ok(());
    }
    if gains[..channels].iter().all(|v| *v == 1.) {
        return Ok(());
    }
    if raw.color_matrix.is_empty() {
        return Err("DNG AnalogBalance를 변환할 카메라 색 행렬이 없습니다.".into());
    }
    for flat in raw.color_matrix.values_mut() {
        if flat.len() != channels * 3 {
            return Err("DNG AnalogBalance와 카메라 행렬 채널 수가 서로 다릅니다.".into());
        }
        for (row, gain) in flat.as_chunks_mut::<3>().0.iter_mut().zip(gains) {
            for value in row {
                let adjusted = f64::from(*value) * f64::from(gain);
                if !adjusted.is_finite() || adjusted.abs() > f64::from(f32::MAX) {
                    return Err("DNG AnalogBalance 색 행렬이 부동소수점 범위를 넘습니다.".into());
                }
                *value = adjusted as f32;
            }
        }
    }
    Ok(())
}
