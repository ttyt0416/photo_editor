use image::{DynamicImage, ImageBuffer};

fn scale(value: i32, precision: u32, signed: bool, maximum: i64) -> i64 {
    let old_maximum = (1i64 << precision) - 1;
    let value = i64::from(value) + if signed { 1i64 << (precision - 1) } else { 0 };
    value.clamp(0, old_maximum) * maximum / old_maximum
}

fn sycc_rgb(y: i32, cb: i32, cr: i32, precision: u32) -> [i32; 3] {
    let center = (1i32 << (precision - 1)) as f64;
    let (cb, cr) = (f64::from(cb) - center, f64::from(cr) - center);
    // Full-range sYCC integer display convention, with chroma centered at
    // half range; retain the original component depth before normalization.
    [1.402 * cr, -0.344 * cb - 0.714 * cr, 1.772 * cb].map(|correction| {
        (i64::from(y) + correction.trunc() as i64).clamp(0, (1i64 << precision) - 1) as i32
    })
}

fn eycc_rgb(y: i32, cb: i32, cr: i32, precision: u32, signed: [bool; 2]) -> [i32; 3] {
    let center = (1i32 << (precision - 1)) as f32;
    let y = y as f32;
    let cb = cb as f32 - if signed[0] { 0. } else { center };
    let cr = cr as f32 - if signed[1] { 0. } else { center };
    // Matrix coefficients, f32 evaluation and integer rounding convention:
    // OpenJPEG src/bin/common/color.c, color_esycc_to_rgb. Keep the decoded
    // component depth until conversion; signed chroma is already centered.
    let matrix = [
        [1., -0.0000368, 1.40199],
        [1.0003, -0.344125, -0.7141128],
        [0.999823, 1.77204, -0.000008],
    ];
    matrix.map(|row| {
        ((row[0] * y + row[1] * cb + row[2] * cr + 0.5) as i32).clamp(0, (1i32 << precision) - 1)
    })
}

fn component_sampling(
    bytes: &[u8],
    components: &[jpeg2k::ImageComponent],
) -> Result<Vec<[u32; 2]>, String> {
    let fail = || "이 JPEG2000의 샘플링·시작 좌표를 아직 처리할 수 없습니다.".to_string();
    let mut stream = bytes;
    if !bytes.starts_with(&[0xff, 0x4f]) {
        let mut at = 0usize;
        let mut found = None;
        while at < bytes.len() {
            let header = bytes
                .get(at..at.checked_add(8).ok_or_else(fail)?)
                .ok_or_else(fail)?;
            let n = u32::from_be_bytes(header[..4].try_into().unwrap());
            let (length, header_size) = match n {
                0 => (bytes.len() - at, 8),
                1 => {
                    let length = u64::from_be_bytes(
                        bytes
                            .get(at + 8..at + 16)
                            .ok_or_else(fail)?
                            .try_into()
                            .unwrap(),
                    );
                    (usize::try_from(length).map_err(|_| fail())?, 16)
                }
                _ => (n as usize, 8),
            };
            if length < header_size {
                return Err(fail());
            }
            let end = at
                .checked_add(length)
                .filter(|end| *end <= bytes.len())
                .ok_or_else(fail)?;
            if &header[4..8] == b"jp2c" {
                found = Some(&bytes[at + header_size..end]);
                break;
            }
            at = end;
        }
        stream = found.ok_or_else(fail)?;
    }
    if stream.get(..4) != Some(&[0xff, 0x4f, 0xff, 0x51]) {
        return Err(fail());
    }
    let u32_at = |at| -> Result<u32, String> {
        Ok(u32::from_be_bytes(
            stream.get(at..at + 4).ok_or_else(fail)?.try_into().unwrap(),
        ))
    };
    let (width, height, x0, y0) = (u32_at(8)?, u32_at(12)?, u32_at(16)?, u32_at(20)?);
    let count =
        u16::from_be_bytes(stream.get(40..42).ok_or_else(fail)?.try_into().unwrap()) as usize;
    let length =
        u16::from_be_bytes(stream.get(4..6).ok_or_else(fail)?.try_into().unwrap()) as usize;
    if count == 0
        || count != components.len()
        || length != 38 + count * 3
        || stream.len() < 4 + length
        || width.checked_sub(x0) != Some(components[0].width())
        || height.checked_sub(y0) != Some(components[0].height())
    {
        return Err(fail());
    }
    let sampling = (0..count)
        .map(|c| [u32::from(stream[43 + c * 3]), u32::from(stream[44 + c * 3])])
        .collect::<Vec<_>>();
    if sampling[0] != [1, 1] || sampling.iter().any(|s| s.contains(&0)) {
        return Err(fail());
    }
    for (c, component) in components.iter().enumerate() {
        let [dx, dy] = sampling[c];
        if (component.width(), component.height())
            != (
                width.div_ceil(dx) - x0.div_ceil(dx),
                height.div_ceil(dy) - y0.div_ceil(dy),
            )
        {
            return Err(fail());
        }
    }
    Ok(sampling)
}

fn sycc_sampling(bytes: &[u8], components: &[jpeg2k::ImageComponent]) -> Result<[u32; 2], String> {
    let sampling = component_sampling(bytes, components)?;
    if !(3..=4).contains(&sampling.len())
        || sampling[1] != sampling[2]
        || ![[2, 1], [2, 2]].contains(&sampling[1])
        || (sampling.len() == 4 && sampling[3] != [1, 1])
    {
        return Err("이 JPEG2000 sYCC의 샘플링·시작 좌표를 아직 처리할 수 없습니다.".into());
    }
    Ok(sampling[1])
}

pub fn pixels(
    image: &jpeg2k::Image,
    bytes: &[u8],
    notes: &mut Vec<String>,
) -> Result<DynamicImage, String> {
    let components = image.components();
    let first = components
        .first()
        .ok_or("JPEG2000에 이미지 채널이 없습니다.")?;
    let (w, h) = (first.width(), first.height());
    crate::document::validate_size(w, h)?;
    let cmyk = matches!(image.color_space(), jpeg2k::ColorSpace::CMYK);
    if !(1..=4).contains(&components.len()) && !(cmyk && components.len() == 5) {
        return Err("이 JPEG2000의 다중 채널 구조는 아직 처리할 수 없습니다.".into());
    }
    if components
        .iter()
        .any(|c| !(1..=16).contains(&c.precision()))
    {
        return Err("이 JPEG2000의 채널 정밀도는 아직 처리할 수 없습니다.".into());
    }
    let sycc = matches!(image.color_space(), jpeg2k::ColorSpace::SYCC);
    let eycc = matches!(image.color_space(), jpeg2k::ColorSpace::EYCC);
    let mut sampling = [1, 1];
    if components.iter().any(|c| (c.width(), c.height()) != (w, h)) {
        if !sycc || components.len() < 3 {
            return Err("이 JPEG2000의 채널별 크기·샘플링은 아직 처리할 수 없습니다.".into());
        }
        sampling = sycc_sampling(bytes, components)?;
    }
    let unknown = matches!(
        image.color_space(),
        jpeg2k::ColorSpace::Unknown | jpeg2k::ColorSpace::Unspecified
    );
    if !unknown
        && !sycc
        && !eycc
        && !cmyk
        && !matches!(
            image.color_space(),
            jpeg2k::ColorSpace::SRGB | jpeg2k::ColorSpace::Gray
        )
    {
        return Err("이 JPEG2000의 색상 공간은 아직 처리할 수 없습니다.".into());
    }
    if cmyk {
        if !(4..=5).contains(&components.len())
            || components[..4]
                .iter()
                .any(|c| c.is_signed() || c.is_alpha())
        {
            return Err(
                "이 JPEG2000 CMYK의 색 채널 수·부호·투명도 정의를 처리할 수 없습니다.".into(),
            );
        }
        if components.len() == 5 && !components[4].is_alpha() {
            return Err("이 JPEG2000 CMYK의 다섯 번째 채널이 투명도로 정의되지 않았습니다.".into());
        }
        notes.push("JPEG2000의 CMYK를 색 프로파일 없이 작업 RGB로 변환했습니다. 정확한 인쇄 색 변환과 CMYK 편집·저장은 남은 기능입니다.".into());
    }
    if sycc {
        if components.len() < 3
            || components[..3]
                .iter()
                .any(|c| c.is_signed() || c.precision() != first.precision())
        {
            return Err(
                "이 JPEG2000 sYCC의 색 채널 정밀도·부호 구조는 아직 처리할 수 없습니다.".into(),
            );
        }
        notes.push("JPEG2000의 sYCC 색상을 작업 RGB로 변환했습니다.".into());
    }
    if eycc {
        if !(3..=4).contains(&components.len())
            || first.is_signed()
            || components[..3]
                .iter()
                .any(|c| c.precision() != first.precision() || c.is_alpha())
        {
            return Err("이 JPEG2000 eYCC의 색 채널 수·정밀도·밝기 부호·투명도 정의를 아직 처리할 수 없습니다.".into());
        }
        if components.len() == 4 && !components[3].is_alpha() {
            return Err("이 JPEG2000 eYCC의 네 번째 채널이 투명도로 정의되지 않았습니다.".into());
        }
        if component_sampling(bytes, components)?
            .iter()
            .any(|s| *s != [1, 1])
        {
            return Err("이 JPEG2000 eYCC의 채널별 샘플링은 아직 처리할 수 없습니다.".into());
        }
        notes.push("JPEG2000의 eYCC 색상을 작업 RGB로 변환했습니다.".into());
    }
    if components.len() == 2 && !components[1].is_alpha() {
        if !unknown {
            return Err("이 JPEG2000의 두 번째 채널이 투명도로 정의되지 않았습니다.".into());
        }
        notes.push("색상 정보가 없는2채널 JPEG2000 코드스트림을 밝기·투명도로 읽었습니다.".into());
    }
    let leading = [
        image.x_offset() % sampling[0],
        image.y_offset() % sampling[1],
    ];
    if sycc && leading != [0, 0] {
        notes.push("JPEG2000의 시작 경계에 없는 크로마 샘플은0으로 읽었습니다.".into());
    }
    let depth = components.iter().map(|c| c.precision()).max().unwrap();
    let maximum = if depth > 8 { 65535 } else { 255 };
    // An odd origin can leave a one-pixel image with an empty chroma
    // component. Do not call the dependency's slice accessor on its null data.
    let planes = components
        .iter()
        .map(|c| {
            if c.width() == 0 || c.height() == 0 {
                &[][..]
            } else {
                c.data()
            }
        })
        .collect::<Vec<_>>();
    // Expand in the integer domain before converting to the document's float
    // buffer. This matches the RGBA precision assets used by project saving.
    let values = (0..w as usize * h as usize).flat_map(|index| {
        let components = &components;
        let planes = &planes;
        let rgb = sycc.then(|| {
            let x = (index % w as usize).checked_sub(leading[0] as usize);
            let y = (index / w as usize).checked_sub(leading[1] as usize);
            let (cb, cr) = if let (Some(x), Some(y)) = (x, y) {
                let chroma = y / sampling[1] as usize * components[1].width() as usize
                    + x / sampling[0] as usize;
                (planes[1][chroma], planes[2][chroma])
            } else {
                (0, 0)
            };
            sycc_rgb(planes[0][index], cb, cr, first.precision())
        });
        let rgb = rgb.or_else(|| {
            eycc.then(|| {
                eycc_rgb(
                    planes[0][index],
                    planes[1][index],
                    planes[2][index],
                    first.precision(),
                    [components[1].is_signed(), components[2].is_signed()],
                )
            })
        });
        let cmyk_rgb = cmyk.then(|| {
            let complements: [i64; 4] = std::array::from_fn(|c| {
                let limit = (1i64 << components[c].precision()) - 1;
                limit - i64::from(planes[c][index]).clamp(0, limit)
            });
            let black_limit = (1i64 << components[3].precision()) - 1;
            std::array::from_fn::<_, 3, _>(|c| {
                let color_limit = (1i64 << components[c].precision()) - 1;
                complements[c] * complements[3] * maximum / (color_limit * black_limit)
            })
        });
        (0..4).map(move |c| {
            if c < 3
                && let Some(rgb) = cmyk_rgb
            {
                return rgb[c];
            }

            let channel = if c == 3 && cmyk {
                (components.len() == 5).then_some(4)
            } else if c == 3 {
                match components.len() {
                    2 => Some(1),
                    4 => Some(3),
                    _ => None,
                }
            } else if components.len() < 3 {
                Some(0)
            } else {
                Some(c)
            };
            channel
                .map(|channel| {
                    let component = &components[channel];
                    if c < 3
                        && let Some(rgb) = rgb
                    {
                        return scale(rgb[c], first.precision(), false, maximum);
                    }
                    scale(
                        planes[channel][index],
                        component.precision(),
                        component.is_signed(),
                        maximum,
                    )
                })
                .unwrap_or(maximum)
        })
    });
    Ok(if depth > 8 {
        DynamicImage::ImageRgba16(
            ImageBuffer::from_raw(w, h, values.map(|v| v as u16).collect())
                .ok_or("JPEG2000 픽셀 버퍼가 잘못되었습니다.")?,
        )
    } else {
        DynamicImage::ImageRgba8(
            ImageBuffer::from_raw(w, h, values.map(|v| v as u8).collect())
                .ok_or("JPEG2000 픽셀 버퍼가 잘못되었습니다.")?,
        )
    })
}
#[cfg(test)]
#[path = "jpeg2000_input_tests.rs"]
mod tests;
