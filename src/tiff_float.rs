use crate::codecs::Decoded;
use image::{DynamicImage, Rgba, Rgba32FImage};
use std::{
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
};
use tiff::{
    decoder::{Decoder, DecodingResult, Limits},
    tags::Tag,
};

// The generic TIFF adapter lacks float16/64, gray alpha/Gray32F and leaves
// associated RGB samples premultiplied. Other TIFFs use the common path.
pub fn read(path: &Path) -> Result<Option<Decoded>, String> {
    let mut reader = BufReader::new(File::open(path).map_err(|e| e.to_string())?);
    let signature = reader.fill_buf().map_err(|e| e.to_string())?;
    if ![b"II*\0", b"MM\0*", b"II+\0", b"MM\0+"]
        .iter()
        .any(|magic| signature.starts_with(*magic))
    {
        return Ok(None);
    }
    let mut limits = Limits::default();
    limits.ifd_value_size = crate::source_metadata::MAX_BYTES;
    limits.decoding_buffer_size = crate::document::MAX_PIXELS as usize * 32;
    let mut decoder = Decoder::new(reader)
        .map_err(|e| e.to_string())?
        .with_limits(limits);
    if decoder
        .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
        .map_err(|e| e.to_string())?
        == Some(3)
    {
        return crate::tiff_palette::read(&mut decoder).map(Some);
    }
    let color = decoder.colortype().map_err(|e| e.to_string())?;
    let (gray, channels, bits) = match color {
        tiff::ColorType::Gray(bits @ (2 | 4)) => (true, 1, bits),
        tiff::ColorType::Gray(bits @ (16 | 32 | 64)) => (true, 1, bits),
        tiff::ColorType::Multiband {
            bit_depth: bits @ (8 | 16 | 32 | 64),
            num_samples: 2,
        } => (true, 2, bits),
        tiff::ColorType::RGBA(bits @ (8 | 16 | 32 | 64)) => (false, 4, bits),
        tiff::ColorType::RGB(bits @ (16 | 32 | 64)) => (false, 3, bits),
        _ => return Ok(None),
    };
    let formats = decoder
        .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
        .map_err(|e| e.to_string())?;
    let floating = formats
        .as_deref()
        .is_some_and(|v| !v.is_empty() && v.iter().all(|v| *v == 3));
    let unsigned = formats
        .as_deref()
        .is_none_or(|v| !v.is_empty() && v.iter().all(|v| *v == 1));
    let formats_ok = if bits == 64 || floating {
        floating && matches!(bits, 16 | 32 | 64)
    } else {
        unsigned
    };
    if !formats_ok {
        return Ok(None);
    }
    if (channels == 1 && bits == 16 || channels == 3 && bits != 32) && !floating {
        return Ok(None);
    }
    let white_native = gray
        && bits >= 8
        && decoder
            .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
            .map_err(|e| e.to_string())?
            == Some(0);
    let associated = if matches!(channels, 2 | 4) {
        match decoder
            .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
            .map_err(|e| e.to_string())?
            .as_deref()
        {
            Some([1]) => true,
            Some([2]) => false,
            _ if !gray => return Ok(None),
            _ => return Err("TIFF의 추가 채널이 알파로 정의되지 않았습니다.".into()),
        }
    } else {
        false
    };
    let separated_float_rgb = !gray
        && floating
        && bits == 32
        && decoder
            .find_tag_unsigned::<u16>(Tag::PlanarConfiguration)
            .map_err(|e| e.to_string())?
            == Some(2);
    if !gray
        && !associated
        && !(floating && matches!(bits, 16 | 64) || unsigned && bits == 32 || separated_float_rgb)
    {
        return Ok(None);
    }
    let (width, height) = decoder.dimensions().map_err(|e| e.to_string())?;
    crate::document::validate_size(width, height)?;
    let mut notes = vec![];
    let source_icc = asset(&mut decoder, Tag::IccProfile, "ICC", &mut notes);
    let xmp = asset(&mut decoder, Tag::Unknown(700), "XMP", &mut notes);
    let source_metadata = crate::source_metadata::Metadata::from_bytes(None, xmp, &mut notes);
    let orientation = decoder
        .find_tag_unsigned::<u16>(Tag::Orientation)
        .map_err(|e| e.to_string())?
        .and_then(|v| u8::try_from(v).ok())
        .and_then(image::metadata::Orientation::from_exif)
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    if decoder.more_images() {
        notes.push(
            "현재 TIFF의 첫 이미지 한 장으로 엽니다. 여러 페이지 편집은 아직 지원하지 않습니다."
                .into(),
        );
    }
    let mut decoded = DecodingResult::F32(vec![]);
    let layout = if white_native {
        decoder.read_image_to_buffer_uninverted(&mut decoded)
    } else {
        decoder.read_image_to_buffer(&mut decoded)
    }
    .map_err(|e| e.to_string())?;
    if bits < 8 {
        let DecodingResult::U8(data) = decoded else {
            return Err("회색조 TIFF의 패킹 형식이 잘못되었습니다.".into());
        };
        let stride = layout
            .row_stride
            .ok_or("회색조 TIFF의 패킹 행 간격이 없습니다")?
            .get();
        if data.len() < layout.complete_len
            || stride < (width as usize * bits as usize).div_ceil(8)
            || layout.planes != 1
        {
            return Err("회색조 TIFF의 패킹 데이터가 잘렸습니다.".into());
        }
        let order = decoder
            .find_tag_unsigned::<u16>(Tag::FillOrder)
            .map_err(|e| e.to_string())?
            .unwrap_or(1);
        if !matches!(order, 1 | 2) {
            return Err("회색조 TIFF의 비트 저장 순서가 잘못되었습니다.".into());
        }
        let maximum = (1u8 << bits) - 1;
        let packed = image::GrayImage::from_fn(width, height, |x, y| {
            let bit = x as usize * bits as usize;
            let value = data[y as usize * stride + bit / 8];
            let value = if order == 2 {
                value.reverse_bits()
            } else {
                value
            };
            image::Luma([((value >> (8 - bits - bit as u8 % 8)) & maximum) * (255 / maximum)])
        });
        let mut pixels = DynamicImage::ImageLuma8(packed);
        if let Some(profile) = source_icc.as_deref() {
            pixels = crate::color_management::apply(pixels, profile, &mut notes);
        }
        pixels.apply_orientation(orientation);
        notes.push(format!(
            "{bits}비트 회색조 TIFF의 패킹을 풀어 8비트 작업 버퍼에 유지합니다."
        ));
        return Ok(Some(Decoded {
            pixels,
            notes,
            source_icc,
            source_metadata,
        }));
    }
    let (values, sample_bytes) = match &decoded {
        DecodingResult::U8(v) if bits == 8 => (v.len(), 1),
        DecodingResult::U16(v) if bits == 16 => (v.len(), 2),
        DecodingResult::U32(v) if bits == 32 => (v.len(), 4),
        DecodingResult::F16(v) if bits == 16 && v.iter().all(|v| v.is_finite()) => (v.len(), 2),
        DecodingResult::F32(v) if bits == 32 && v.iter().all(|v| v.is_finite()) => (v.len(), 4),
        DecodingResult::F64(v) if bits == 64 && v.iter().all(|v| v.is_finite()) => (v.len(), 8),
        _ => return Err("TIFF의 샘플 형식 또는 부동소수점 값이 잘못되었습니다.".into()),
    };
    if values * sample_bytes < layout.complete_len {
        return Err("TIFF의 픽셀 크기 또는 부동소수점 값이 잘못되었습니다.".into());
    }
    let row_stride = layout
        .row_stride
        .ok_or("회색조 TIFF의 행 배치가 잘못되었습니다")?
        .get()
        / sample_bytes;
    if layout.planes != 1 && layout.planes != channels {
        return Err("회색조 TIFF의 평면 수가 색상 채널과 일치하지 않습니다.".into());
    }
    // plane_stride also exists for a single interleaved plane. The plane count,
    // rather than an optional stride, distinguishes separated sample storage.
    let separate = layout.planes > 1;
    let component_stride = if separate {
        layout
            .plane_stride
            .ok_or("회색조 TIFF의 평면 간격이 없습니다")?
            .get()
            / sample_bytes
    } else {
        1
    };
    let mut pixels = Rgba32FImage::new(width, height);
    for (x, y, target) in pixels.enumerate_pixels_mut() {
        let index = y as usize * row_stride + x as usize * if separate { 1 } else { channels };
        let alpha = if matches!(channels, 1 | 3) {
            1.
        } else {
            sample(&decoded, index + component_stride * (channels - 1))
                .ok_or("TIFF의 알파 평면이 잘렸습니다")?
        };
        if !(0. ..=1.).contains(&alpha) {
            return Err("TIFF의 알파 값이 0–1 범위를 벗어났습니다.".into());
        }
        *target = Rgba([0., 0., 0., alpha as f32]);
        for channel in 0..3 {
            let native = sample(
                &decoded,
                index + if gray { 0 } else { channel * component_stride },
            )
            .ok_or("TIFF의 색상 평면이 잘렸습니다")?;
            let value = if associated {
                if alpha == 0. {
                    0.
                } else if white_native {
                    ((alpha - native) / alpha) as f32
                } else {
                    (native / alpha) as f32
                }
            } else if white_native {
                (1. - native) as f32
            } else {
                native as f32
            };
            if !value.is_finite() {
                return Err("TIFF의 연관 알파 복원 값이 float32 범위를 초과합니다.".into());
            }
            target[channel] = value;
        }
    }
    drop(decoded);
    let pixels = if let Some(profile) = source_icc.as_deref() {
        let precise_integer_rgb =
            !gray && unsigned && bits == 32 && profile != crate::color_management::srgb_profile();
        if !gray && !precise_integer_rgb {
            crate::color_management::apply(DynamicImage::ImageRgba32F(pixels), profile, &mut notes)
                .into_rgba32f()
        } else if gray && profile == crate::color_management::gray_profile() {
            notes.push(
                "내장 회색조 ICC가 작업 sRGB 곡선과 일치해 원래 float 비트를 유지했습니다.".into(),
            );
            pixels
        } else {
            let label = if gray { "회색조" } else { "RGB" };
            match crate::icc_float::convert(&pixels, profile, gray) {
                Ok(result) => {
                    notes.push(format!(
                        "내장 {label} ICC를 적용해 작업 sRGB로 변환했습니다."
                    ));
                    result
                }
                Err(error) => {
                    notes.push(format!("내장 {label} ICC를 적용하지 못했습니다: {error}. 원래 픽셀과 원본 ICC를 유지합니다."));
                    pixels
                }
            }
        }
    } else {
        pixels
    };
    let mut dynamic = DynamicImage::ImageRgba32F(pixels);
    dynamic.apply_orientation(orientation);
    notes.push(if separated_float_rgb && !associated {
        "32비트 RGB TIFF의 분리 평면을 모두 읽어 작업 색과 알파를 유지합니다.".into()
    } else if bits == 32 && unsigned {
        "32비트 정수 TIFF를 최대값 4294967295로 정규화해 32비트 float 작업 버퍼로 변환합니다. 일부 정수 단계는 반올림됩니다.".into()
    } else if bits == 64 {
        "64비트 부동소수점 TIFF를 32비트 작업 버퍼로 변환합니다. 정밀도와 매우 작은 값은 반올림되며 32비트 표현 범위를 넘는 색상은 오류로 처리합니다.".into()
    } else if floating && bits == 16 {
        "16비트 부동소수점 TIFF의 음수·HDR·알파를 32비트 작업 버퍼에 유지합니다.".into()
    } else if gray && bits == 32 {
        "32비트 회색조 TIFF의 음수·HDR 값을 RGB 작업 버퍼에 유지합니다.".into()
    } else if gray {
        format!("{bits}비트 회색조 TIFF의 알파와 색을 32비트 작업 버퍼에 유지합니다.")
    } else {
        format!(
            "{bits}비트 RGB TIFF의 연관 알파를 32비트 작업 버퍼로 분리해 분수·HDR 값을 유지합니다."
        )
    });
    Ok(Some(Decoded {
        pixels: dynamic,
        notes,
        source_icc,
        source_metadata,
    }))
}

fn sample(data: &DecodingResult, index: usize) -> Option<f64> {
    match data {
        DecodingResult::U8(v) => v.get(index).map(|v| f64::from(*v) / 255.),
        DecodingResult::U16(v) => v.get(index).map(|v| f64::from(*v) / 65535.),
        DecodingResult::U32(v) => v.get(index).map(|v| f64::from(*v) / 4294967295.),
        DecodingResult::F16(v) => v.get(index).map(|v| v.to_f64()),
        DecodingResult::F32(v) => v.get(index).map(|v| f64::from(*v)),
        DecodingResult::F64(v) => v.get(index).copied(),
        _ => None,
    }
}

pub(super) fn asset(
    decoder: &mut Decoder<BufReader<File>>,
    tag: Tag,
    label: &str,
    notes: &mut Vec<String>,
) -> Option<Vec<u8>> {
    match decoder.find_tag_unsigned_vec::<u8>(tag) {
        Ok(bytes) => bytes,
        Err(error) => {
            notes.push(format!(
                "원본 TIFF {label}를 읽지 못했습니다: {error}. 픽셀 읽기를 계속합니다."
            ));
            None
        }
    }
}

#[cfg(test)]
#[path = "tiff_float_tests.rs"]
mod tests;
