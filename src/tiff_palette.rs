use crate::codecs::Decoded;
use image::{DynamicImage, ImageBuffer, Rgba, Rgba32FImage};
use std::{fs::File, io::BufReader};
use tiff::{
    decoder::{Decoder, DecodingResult},
    tags::Tag,
};

pub fn read(
    decoder: &mut Decoder<BufReader<File>>,
    path: &std::path::Path,
) -> Result<Decoded, String> {
    let samples = decoder
        .find_tag_unsigned::<u16>(Tag::SamplesPerPixel)
        .map_err(|e| e.to_string())?
        .unwrap_or(1);
    if !matches!(samples, 1 | 2) {
        return Err("추가 채널이 있는 팔레트 TIFF는 아직 지원하지 않습니다.".into());
    }
    let associated = if samples == 2 {
        match decoder
            .find_tag_unsigned_vec::<u16>(Tag::ExtraSamples)
            .map_err(|e| e.to_string())?
            .as_deref()
        {
            Some([1]) => true,
            Some([2]) => false,
            _ => return Err("팔레트 TIFF의 추가 채널이 알파로 정의되지 않았습니다.".into()),
        }
    } else {
        false
    };
    let depths = decoder
        .find_tag_unsigned_vec::<u8>(Tag::BitsPerSample)
        .map_err(|e| e.to_string())?
        .ok_or("팔레트 TIFF의 비트 정밀도가 없습니다")?;
    let Some(&bits) = depths.first() else {
        return Err("팔레트 TIFF의 색 번호 정밀도가 지원되지 않습니다.".into());
    };
    if !matches!(bits, 1 | 2 | 4 | 8 | 16)
        || !matches!(depths.len(), 1) && depths.len() != usize::from(samples)
        || depths.iter().any(|v| *v != bits)
    {
        return Err("팔레트 TIFF의 색 번호/알파 정밀도가 일치하지 않습니다.".into());
    }
    if !decoder
        .find_tag_unsigned_vec::<u16>(Tag::SampleFormat)
        .map_err(|e| e.to_string())?
        .as_deref()
        .is_none_or(|v| !v.is_empty() && v.iter().all(|v| *v == 1))
    {
        return Err("팔레트 TIFF의 색 번호가 부호 없는 정수가 아닙니다.".into());
    }
    let order = decoder
        .find_tag_unsigned::<u16>(Tag::FillOrder)
        .map_err(|e| e.to_string())?
        .unwrap_or(1);
    if order != 1 && !(order == 2 && bits == 1 && samples == 1) {
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
    let exif = crate::tiff_exif::read(path, &mut notes);
    let mut source_metadata = crate::source_metadata::Metadata::from_bytes(exif, xmp, &mut notes);
    crate::tiff_iptc::remember(path, &mut source_metadata, &mut notes);
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
    let size = match &indices {
        DecodingResult::U8(v) if bits <= 8 => v.len(),
        DecodingResult::U16(v) if bits == 16 => v.len() * 2,
        _ => return Err("팔레트 TIFF의 색 번호 형식이 잘못되었습니다.".into()),
    };
    let row = layout
        .row_stride
        .ok_or("팔레트 TIFF의 행 간격이 없습니다")?
        .get();
    let separate = layout.planes > 1;
    let components = if separate { 1 } else { usize::from(samples) };
    if size < layout.complete_len
        || layout.planes != 1 && layout.planes != usize::from(samples)
        || row < (width as usize * bits as usize * components).div_ceil(8)
    {
        return Err("팔레트 TIFF의 색 번호 데이터가 잘렸습니다.".into());
    }
    let component_bits = if separate {
        layout
            .plane_stride
            .ok_or("팔레트 TIFF의 평면 간격이 없습니다")?
            .get()
            * 8
    } else {
        usize::from(bits)
    };
    let value = |bit: usize| -> usize {
        match &indices {
            DecodingResult::U8(data) => {
                let value = data[bit / 8];
                let value = if order == 2 {
                    value.reverse_bits()
                } else {
                    value
                };
                usize::from((value >> (8 - bits - bit as u8 % 8)) & ((1u16 << bits) - 1) as u8)
            }
            DecodingResult::U16(data) => usize::from(data[bit / 16]),
            _ => unreachable!(),
        }
    };
    let mut pixels = if samples == 1 {
        DynamicImage::ImageRgba16(ImageBuffer::<Rgba<u16>, Vec<u16>>::new(width, height))
    } else {
        DynamicImage::ImageRgba32F(Rgba32FImage::new(width, height))
    };
    for y in 0..height {
        for x in 0..width {
            let bit = y as usize * row * 8 + x as usize * usize::from(bits) * components;
            let index = value(bit);
            let rgb = [
                colors[index],
                colors[count + index],
                colors[2 * count + index],
            ];
            match &mut pixels {
                DynamicImage::ImageRgba16(buffer) => {
                    *buffer.get_pixel_mut(x, y) = Rgba([rgb[0], rgb[1], rgb[2], 65535])
                }
                DynamicImage::ImageRgba32F(buffer) => {
                    let alpha = value(bit + component_bits) as f64 / (count - 1) as f64;
                    let rgb = rgb.map(|v| {
                        let color = f64::from(v) / 65535.;
                        if associated {
                            if alpha == 0. { 0. } else { color / alpha }
                        } else {
                            color
                        }
                    });
                    *buffer.get_pixel_mut(x, y) =
                        Rgba([rgb[0] as f32, rgb[1] as f32, rgb[2] as f32, alpha as f32]);
                }
                _ => unreachable!(),
            }
        }
    }
    if let Some(profile) = source_icc.as_deref()
        && profile != crate::color_management::srgb_profile()
    {
        pixels = match crate::icc_float::convert(&pixels.to_rgba32f(), profile, false) {
            Ok(converted) => {
                if samples == 2 {
                    notes.push(
                        "팔레트 RGB ICC를 정밀 변환해 float 작업 색과 알파를 유지합니다.".into(),
                    );
                    DynamicImage::ImageRgba32F(converted)
                } else {
                    notes.push(
                        "팔레트 RGB ICC를 정밀 변환한 뒤 16비트 작업 값으로 반올림했습니다.".into(),
                    );
                    DynamicImage::ImageRgba16(DynamicImage::ImageRgba32F(converted).into_rgba16())
                }
            }
            Err(_) => crate::color_management::apply(pixels, profile, &mut notes),
        };
    }
    pixels.apply_orientation(orientation);
    notes.push(if samples == 2 {
        format!("{bits}비트 색 번호/알파의 TIFF 팔레트를 32비트 float 작업 버퍼에 유지합니다. 연관 알파는 RGB 색상표 값에서 분리합니다.")
    } else { format!("{bits}비트 색 번호의 TIFF 팔레트를 16비트 RGB 작업 버퍼에 유지합니다.") });
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
