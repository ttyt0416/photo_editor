use image::{DynamicImage, ImageBuffer};
use moxcms::{ColorProfile, DataColorSpace, Layout, RenderingIntent, TransformOptions};

pub fn srgb_profile() -> &'static [u8] {
    include_bytes!("../data/color/srgb.icc")
}

pub fn gray_profile() -> &'static [u8] {
    include_bytes!("../data/color/srgb-gray.icc")
}

/// Convert the embedded profile at the decoding boundary. Failed/unsupported
/// conversions keep the original pixels, with an explicit import note.
pub fn apply(image: DynamicImage, profile: &[u8], notes: &mut Vec<String>) -> DynamicImage {
    match convert(&image, profile) {
        Ok(converted) => {
            notes.push("내장 ICC 색상 프로파일을 적용해 작업 sRGB로 변환했습니다.".into());
            converted
        }
        Err(error) => {
            notes.push(format!(
                "내장 ICC 색상 프로파일을 적용하지 못했습니다: {error}. 원래 픽셀을 유지했습니다."
            ));
            image
        }
    }
}

pub fn tiff_profile(path: &std::path::Path) -> Result<Option<Vec<u8>>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut limits = tiff::decoder::Limits::default();
    limits.ifd_value_size = 16 * 1024 * 1024;
    limits.decoding_buffer_size = 16 * 1024 * 1024;
    let mut decoder = tiff::decoder::Decoder::new(std::io::BufReader::new(file))
        .map_err(|e| e.to_string())?
        .with_limits(limits);
    // Read the byte vector directly. The generic image decoder restricts tag
    // Value allocations to the image buffer size, dropping ICC on tiny images.
    decoder
        .find_tag_unsigned_vec::<u8>(tiff::tags::Tag::IccProfile)
        .map_err(|e| e.to_string())
}

pub(crate) fn convert(image: &DynamicImage, bytes: &[u8]) -> Result<DynamicImage, String> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("색상 프로파일이 메모리 예산을 초과합니다".into());
    }
    let mut profile = ColorProfile::new_from_slice(bytes).map_err(|e| e.to_string())?;
    // Source ICC curves remain authoritative. The known sRGB destination may
    // use its exact transfer function, including the linear shadow segment.
    profile.cicp = None;
    let destination = ColorProfile::new_srgb();
    let options = TransformOptions {
        rendering_intent: RenderingIntent::RelativeColorimetric,
        prefer_fixed_point: false,
        allow_use_cicp_transfer: true,
        allow_extended_range_rgb_xyz: true,
        ..Default::default()
    };
    let gray = matches!(
        image,
        DynamicImage::ImageLuma8(_)
            | DynamicImage::ImageLumaA8(_)
            | DynamicImage::ImageLuma16(_)
            | DynamicImage::ImageLumaA16(_)
    );
    if profile.color_space
        != if gray {
            DataColorSpace::Gray
        } else {
            DataColorSpace::Rgb
        }
    {
        return Err("프로파일 색 공간과 복원된 이미지 채널이 일치하지 않습니다".into());
    }
    let (w, h) = (image.width(), image.height());
    crate::document::validate_size(w, h)?;
    if bytes == srgb_profile() || bytes == gray_profile() {
        if let DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_) = image
            && image.to_rgba32f().as_raw().iter().any(|v| !v.is_finite())
        {
            return Err("색상 값이 유한하지 않습니다".into());
        }
        // An image already tagged with our exact working profile needs no
        // roundtrip through matrix/TRC tables, including native HDR floats.
        return Ok(match image {
            DynamicImage::ImageLuma8(_)
            | DynamicImage::ImageLumaA8(_)
            | DynamicImage::ImageRgb8(_)
            | DynamicImage::ImageRgba8(_) => DynamicImage::ImageRgba8(image.to_rgba8()),
            DynamicImage::ImageLuma16(_)
            | DynamicImage::ImageLumaA16(_)
            | DynamicImage::ImageRgb16(_)
            | DynamicImage::ImageRgba16(_) => DynamicImage::ImageRgba16(image.to_rgba16()),
            _ => image.clone(),
        });
    }
    macro_rules! integer {
        ($buffer:expr, $layout:expr, $channels:expr, $maximum:expr, $variant:ident) => {{
            let transform = profile.create_transform_f32($layout, &destination, Layout::Rgba, options).map_err(|e| e.to_string())?;
            let mut output = vec![0; w as usize * h as usize * 4];
            let mut normalized = vec![0f32; w as usize * $channels];
            let mut converted = vec![0f32; w as usize * 4];
            for (src, dst) in $buffer.as_raw().chunks_exact(w as usize * $channels).zip(output.chunks_exact_mut(w as usize * 4)) {
                for (value, normalized) in src.iter().zip(normalized.iter_mut()) { *normalized = *value as f32 / $maximum as f32; }
                transform.transform(&normalized, &mut converted).map_err(|e| e.to_string())?;
                if converted.iter().any(|v| !v.is_finite()) { return Err("색상 변환 결과가 유한하지 않습니다".into()); }
                for (value, dst) in converted.iter().zip(dst.iter_mut()) {
                    *dst = (value.clamp(0., 1.) * $maximum as f32).round() as _;
                }
                // Keep alpha bit-for-bit, including hidden RGB under zero alpha.
                for (src, dst) in src.chunks_exact($channels).zip(dst.chunks_exact_mut(4)) {
                    dst[3] = if $channels == 2 || $channels == 4 { src[$channels - 1] } else { $maximum };
                }
            }
            Ok(DynamicImage::$variant(ImageBuffer::from_raw(w, h, output).ok_or("ICC 변환 픽셀 버퍼가 잘못되었습니다")?))
        }};
    }
    match image {
        DynamicImage::ImageRgb8(b) => {
            integer!(b, Layout::Rgb, 3, 255, ImageRgba8)
        }
        DynamicImage::ImageRgba8(b) => {
            integer!(b, Layout::Rgba, 4, 255, ImageRgba8)
        }
        DynamicImage::ImageLuma8(b) => {
            integer!(b, Layout::Gray, 1, 255, ImageRgba8)
        }
        DynamicImage::ImageLumaA8(b) => integer!(b, Layout::GrayAlpha, 2, 255, ImageRgba8),
        DynamicImage::ImageRgb16(b) => integer!(b, Layout::Rgb, 3, 65535, ImageRgba16),
        DynamicImage::ImageRgba16(b) => integer!(b, Layout::Rgba, 4, 65535, ImageRgba16),
        DynamicImage::ImageLuma16(b) => integer!(b, Layout::Gray, 1, 65535, ImageRgba16),
        DynamicImage::ImageLumaA16(b) => integer!(b, Layout::GrayAlpha, 2, 65535, ImageRgba16),
        DynamicImage::ImageRgb32F(_) | DynamicImage::ImageRgba32F(_) => {
            let input = image.to_rgba32f();
            if input.pixels().any(|p| {
                p.0.iter().any(|v| !v.is_finite())
                    || p.0[..3].iter().any(|v| !(0. ..=1.).contains(v))
            }) {
                return Err(
                    "범위 밖 HDR·음수 부동소수점의 ICC 변환은 아직 제공하지 않습니다".into(),
                );
            }
            let transform = profile
                .create_transform_f32(Layout::Rgba, &destination, Layout::Rgba, options)
                .map_err(|e| e.to_string())?;
            let mut output = input.clone();
            for (src, dst) in input
                .as_raw()
                .chunks_exact(w as usize * 4)
                .zip(output.as_mut().chunks_exact_mut(w as usize * 4))
            {
                transform.transform(src, dst).map_err(|e| e.to_string())?;
                for (src, dst) in src
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .zip(dst.as_chunks_mut::<4>().0.iter_mut())
                {
                    dst[3] = src[3];
                }
            }
            if output.as_raw().iter().any(|v| !v.is_finite()) {
                return Err("색상 변환 결과가 유한하지 않습니다".into());
            }
            Ok(DynamicImage::ImageRgba32F(output))
        }
        _ => Err("이미지 채널 형식을 아직 변환할 수 없습니다".into()),
    }
}

#[cfg(test)]
#[path = "color_management_tests.rs"]
mod tests;
