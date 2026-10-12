use crate::codecs::Decoded;
use image::{DynamicImage, ImageBuffer, Rgba};
use std::{fs::File, io::BufReader};
use tiff::{
    decoder::{Decoder, DecodingResult},
    tags::Tag,
};

pub fn read(decoder: &mut Decoder<BufReader<File>>) -> Result<Decoded, String> {
    let samples = decoder
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)
        .map_err(|e| e.to_string())?
        .unwrap_or(1);
    if samples != 1 {
        return Err("추가 채널이 있는 팔레트 TIFF는 아직 지원하지 않습니다.".into());
    }
    let depths = decoder
        .find_tag_unsigned_vec::<u8>(Tag::BitsPerSample)
        .map_err(|e| e.to_string())?
        .ok_or("팔레트 TIFF의 비트 정밀도가 없습니다")?;
    let [bits @ (1 | 2 | 4 | 8 | 16)] = depths.as_slice() else {
        return Err("팔레트 TIFF의 색 번호 정밀도가 지원되지 않습니다.".into());
    };
    let bits = *bits;
    if !decoder
        .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
        .map_err(|e| e.to_string())?
        .as_deref()
        .is_none_or(|v| v == [1])
    {
        return Err("팔레트 TIFF의 색 번호가 부호 없는 정수가 아닙니다.".into());
    }
    let order = decoder
        .find_tag_unsigned::<u16>(Tag::FillOrder)
        .map_err(|e| e.to_string())?
        .unwrap_or(1);
    if order != 1 && !(order == 2 && bits == 1) {
        return Err("팔레트 TIFF의 이 비트 저장 순서는 아직 지원하지 않습니다.".into());
    }
    let colors = decoder
        .find_tag_unsigned_vec::<u16>(Tag::ColorMap)
        .map_err(|e| e.to_string())?
        .ok_or("팔레트 TIFF의 ColorMap이 없습니다")?;
    let count = 1usize << bits;
    if colors.len() != 3 * count {
        return Err("팔레트 TIFF의 ColorMap 크기가 비트 정밀도와 일치하지 않습니다.".into());
    }
    let (width, height) = decoder.dimensions().map_err(|e| e.to_string())?;
    crate::document::validate_size(width, height)?;
    let mut notes = vec![];
    let source_icc = crate::tiff_float::asset(decoder, Tag::IccProfile, "ICC", &mut notes);
    let xmp = crate::tiff_float::asset(decoder, Tag::Unknown(700), "XMP", &mut notes);
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
    if decoder
        .find_tag(Tag::Unknown(301))
        .map_err(|e| e.to_string())?
        .is_some()
    {
        notes.push("팔레트 TIFF의 별도 TransferFunction은 아직 적용하지 않습니다. RGB 팔레트 값과 원본 파일을 유지합니다.".into());
    }
    let mut indices = DecodingResult::U8(vec![]);
    let layout = decoder
        .read_palette_indices_to_buffer(&mut indices)
        .map_err(|e| e.to_string())?;
    let (size, row) = match &indices {
        DecodingResult::U8(v) if bits <= 8 => (v.len(), layout.row_stride.map(|v| v.get())),
        DecodingResult::U16(v) if bits == 16 => {
            (v.len() * 2, layout.row_stride.map(|v| v.get() / 2))
        }
        _ => return Err("팔레트 TIFF의 색 번호 형식이 잘못되었습니다.".into()),
    };
    let row = row.ok_or("팔레트 TIFF의 행 간격이 없습니다")?;
    if size < layout.complete_len
        || layout.planes != 1
        || row
            < if bits == 16 {
                width as usize
            } else {
                (width as usize * bits as usize).div_ceil(8)
            }
    {
        return Err("팔레트 TIFF의 색 번호 데이터가 잘렸습니다.".into());
    }
    let mut pixels = ImageBuffer::<Rgba<u16>, Vec<u16>>::new(width, height);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let index = match &indices {
            DecodingResult::U8(data) => {
                let bit = x as usize * bits as usize;
                let value = data[y as usize * row + bit / 8];
                let value = if order == 2 {
                    value.reverse_bits()
                } else {
                    value
                };
                usize::from((value >> (8 - bits - bit as u8 % 8)) & ((1u16 << bits) - 1) as u8)
            }
            DecodingResult::U16(data) => usize::from(data[y as usize * row + x as usize]),
            _ => unreachable!(),
        };
        *pixel = Rgba([
            colors[index],
            colors[count + index],
            colors[2 * count + index],
            65535,
        ]);
    }
    let mut pixels = DynamicImage::ImageRgba16(pixels);
    if let Some(profile) = source_icc.as_deref()
        && profile != crate::color_management::srgb_profile()
    {
        pixels = match crate::icc_float::convert(&pixels.to_rgba32f(), profile, false) {
            Ok(converted) => {
                notes.push(
                    "팔레트 RGB ICC를 정밀 변환한 뒤 16비트 작업 값으로 반올림했습니다.".into(),
                );
                DynamicImage::ImageRgba16(DynamicImage::ImageRgba32F(converted).into_rgba16())
            }
            Err(_) => crate::color_management::apply(pixels, profile, &mut notes),
        };
    }
    pixels.apply_orientation(orientation);
    notes.push(format!(
        "{bits}비트 색 번호의 TIFF 팔레트를 16비트 RGB 작업 버퍼에 유지합니다."
    ));
    Ok(Decoded {
        pixels,
        notes,
        source_icc,
        source_metadata,
    })
}

#[cfg(test)]
#[path = "tiff_palette_tests.rs"]
mod tests;
