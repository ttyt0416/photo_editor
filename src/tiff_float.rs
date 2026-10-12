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

// image's generic TIFF adapter has no Gray32F DynamicImage variant. Decode only
// gray float cases here; other TIFF variants use the existing common path.
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
    limits.decoding_buffer_size = crate::document::MAX_PIXELS as usize * 8;
    let mut decoder = Decoder::new(reader)
        .map_err(|e| e.to_string())?
        .with_limits(limits);
    let color = decoder.colortype().map_err(|e| e.to_string())?;
    let channels = match color {
        tiff::ColorType::Gray(32) => 1,
        tiff::ColorType::Multiband {
            bit_depth: 32,
            num_samples: 2,
        } => 2,
        _ => return Ok(None),
    };
    let formats = decoder
        .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
        .map_err(|e| e.to_string())?;
    if !formats
        .as_deref()
        .is_some_and(|values| !values.is_empty() && values.iter().all(|v| *v == 3))
    {
        return Ok(None);
    }
    let associated = if channels == 2 {
        if decoder
            .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
            .map_err(|e| e.to_string())?
            != Some(1)
        {
            return Err("흰색 기준 회색조 float TIFF의 알파 변환은 아직 제공하지 않습니다.".into());
        }
        match decoder
            .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
            .map_err(|e| e.to_string())?
            .as_deref()
        {
            Some([1]) => true,
            Some([2]) => false,
            _ => return Err("회색조 TIFF의 두 번째 채널이 알파로 정의되지 않았습니다.".into()),
        }
    } else {
        false
    };
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
    let layout = decoder
        .read_image_to_buffer(&mut decoded)
        .map_err(|e| e.to_string())?;
    let DecodingResult::F32(values) = decoded else {
        return Err("회색조 TIFF의 float32 픽셀 형식이 일치하지 않습니다.".into());
    };
    if values.len() * 4 < layout.complete_len || values.iter().any(|v| !v.is_finite()) {
        return Err("회색조 TIFF의 픽셀 크기 또는 부동소수점 값이 잘못되었습니다.".into());
    }
    let row_stride = layout
        .row_stride
        .ok_or("회색조 TIFF의 행 배치가 잘못되었습니다")?
        .get()
        / 4;
    if layout.planes != 1 && layout.planes != channels {
        return Err("회색조 TIFF의 평면 수가 색상 채널과 일치하지 않습니다.".into());
    }
    // plane_stride also exists for a single interleaved plane. The plane count,
    // rather than an optional stride, distinguishes separated sample storage.
    let separate = layout.planes > 1;
    let alpha_offset = if separate {
        layout
            .plane_stride
            .ok_or("회색조 TIFF의 평면 간격이 없습니다")?
            .get()
            / 4
    } else {
        1
    };
    let mut pixels = Rgba32FImage::new(width, height);
    for (x, y, target) in pixels.enumerate_pixels_mut() {
        let index = y as usize * row_stride + x as usize * if separate { 1 } else { channels };
        let alpha = if channels == 1 {
            1.
        } else {
            *values
                .get(index + alpha_offset)
                .ok_or("회색조 TIFF의 알파 평면이 잘렸습니다")?
        };
        if !(0. ..=1.).contains(&alpha) {
            return Err("회색조 TIFF의 알파 값이 0–1 범위를 벗어났습니다.".into());
        }
        let value = if associated {
            if alpha == 0. {
                0.
            } else {
                (f64::from(
                    *values
                        .get(index)
                        .ok_or("회색조 TIFF의 색상 평면이 잘렸습니다")?,
                ) / f64::from(alpha)) as f32
            }
        } else {
            *values
                .get(index)
                .ok_or("회색조 TIFF의 색상 평면이 잘렸습니다")?
        };
        if !value.is_finite() {
            return Err("회색조 TIFF의 연관 알파 복원 값이 float32 범위를 초과합니다.".into());
        }
        *target = Rgba([value, value, value, alpha]);
    }
    drop(values);
    let pixels = if let Some(profile) = source_icc.as_deref() {
        if profile == crate::color_management::gray_profile() {
            notes.push(
                "내장 회색조 ICC가 작업 sRGB 곡선과 일치해 원래 float 비트를 유지했습니다.".into(),
            );
            pixels
        } else {
            match crate::icc_float::convert(&pixels, profile, true) {
                Ok(result) => {
                    notes.push("내장 회색조 ICC를 적용해 작업 sRGB로 변환했습니다.".into());
                    result
                }
                Err(error) => {
                    notes.push(format!("내장 회색조 ICC를 적용하지 못했습니다: {error}. 원래 픽셀과 원본 ICC를 유지합니다."));
                    pixels
                }
            }
        }
    } else {
        pixels
    };
    let mut dynamic = DynamicImage::ImageRgba32F(pixels);
    dynamic.apply_orientation(orientation);
    notes.push("32비트 회색조 TIFF의 음수·HDR 값을 RGB 작업 버퍼에 유지합니다.".into());
    Ok(Some(Decoded {
        pixels: dynamic,
        notes,
        source_icc,
        source_metadata,
    }))
}

fn asset(
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
