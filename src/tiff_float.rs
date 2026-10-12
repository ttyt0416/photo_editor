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
// this missing case here; other TIFF variants use the existing common path.
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
    limits.decoding_buffer_size = crate::document::MAX_PIXELS as usize * 4;
    let mut decoder = Decoder::new(reader)
        .map_err(|e| e.to_string())?
        .with_limits(limits);
    if decoder.colortype().map_err(|e| e.to_string())? != tiff::ColorType::Gray(32)
        || decoder
            .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
            .map_err(|e| e.to_string())?
            .as_deref()
            != Some(&[3])
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
    let DecodingResult::F32(values) = decoder.read_image().map_err(|e| e.to_string())? else {
        return Err("회색조 TIFF의 float32 픽셀 형식이 일치하지 않습니다.".into());
    };
    if values.len() != width as usize * height as usize || values.iter().any(|v| !v.is_finite()) {
        return Err("회색조 TIFF의 픽셀 크기 또는 부동소수점 값이 잘못되었습니다.".into());
    }
    let pixels = Rgba32FImage::from_fn(width, height, |x, y| {
        let value = values[(y * width + x) as usize];
        Rgba([value, value, value, 1.])
    });
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
