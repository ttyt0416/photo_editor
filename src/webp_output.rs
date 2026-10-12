use image::ImageEncoder;
use std::io::Write;

pub fn write(
    image: &image::RgbaImage,
    file: &mut std::fs::File,
    options: &crate::files::ExportOptions,
    exif: Option<&[u8]>,
) -> Result<(), String> {
    if options.webp_lossless {
        let mut encoder = image::codecs::webp::WebPEncoder::new_lossless(file);
        if options.include_icc {
            encoder
                .set_icc_profile(crate::color_management::srgb_profile().to_vec())
                .map_err(|e| e.to_string())?;
        }
        if let Some(exif) = exif {
            encoder
                .set_exif_metadata(exif.to_vec())
                .map_err(|e| e.to_string())?;
        }
        return encoder
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                image::ExtendedColorType::Rgba8,
            )
            .map_err(|e| e.to_string());
    }
    let mut config = webp::WebPConfig::new().map_err(|_| "WebP 인코더 초기화에 실패했습니다.")?;
    config.quality = f32::from(options.webp_quality);
    config.method = i32::from(options.webp_method);
    config.alpha_quality = 100;
    config.exact = 1;
    config.use_sharp_yuv = 1;
    let encoded = webp::Encoder::from_rgba(image.as_raw(), image.width(), image.height())
        .encode_advanced(&config)
        .map_err(|e| format!("WebP 인코딩 실패: {e:?}"))?;
    if options.include_icc || exif.is_some() {
        file.write_all(&with_metadata(
            &encoded,
            image.width(),
            image.height(),
            options.include_icc,
            exif,
        )?)
        .map_err(|e| e.to_string())
    } else {
        file.write_all(&encoded).map_err(|e| e.to_string())
    }
}

// Only the fresh, single-frame libwebp encoder output enters this wrapper.
// Keep its ALPH/VP8 payload intact; ICCP follows VP8X per the WebP specification.
fn with_metadata(
    encoded: &[u8],
    width: u32,
    height: u32,
    icc: bool,
    exif: Option<&[u8]>,
) -> Result<Vec<u8>, String> {
    let invalid = || "WebP 인코더의 출력 구조가 잘못되었습니다.".to_owned();
    if encoded.len() < 20
        || &encoded[..4] != b"RIFF"
        || &encoded[8..12] != b"WEBP"
        || u32::from_le_bytes(encoded[4..8].try_into().unwrap()) as u64 + 8 != encoded.len() as u64
        || width == 0
        || height == 0
        || width > 0x1000000
        || height > 0x1000000
    {
        return Err(invalid());
    }
    let mut extension = [0u8; 10];
    let start = if &encoded[12..16] == b"VP8X" {
        if encoded.len() < 30 || encoded[16..20] != 10u32.to_le_bytes() {
            return Err(invalid());
        }
        extension.copy_from_slice(&encoded[20..30]);
        30
    } else if &encoded[12..16] == b"VP8 " {
        extension[4..7].copy_from_slice(&(width - 1).to_le_bytes()[..3]);
        extension[7..10].copy_from_slice(&(height - 1).to_le_bytes()[..3]);
        12
    } else {
        return Err(invalid());
    };
    if icc {
        extension[0] |= 0x20;
    }
    if exif.is_some() {
        extension[0] |= 0x08;
    }
    let profile = crate::color_management::srgb_profile();
    let length =
        30 + if icc {
            8 + profile.len() + profile.len() % 2
        } else {
            0
        } + exif.map_or(0, |bytes| 8 + bytes.len() + bytes.len() % 2)
            + encoded.len()
            - start;
    let riff_length = u32::try_from(length - 8).map_err(|_| invalid())?;
    let mut result = Vec::with_capacity(length);
    result.extend_from_slice(b"RIFF");
    result.extend_from_slice(&riff_length.to_le_bytes());
    result.extend_from_slice(b"WEBPVP8X");
    result.extend_from_slice(&10u32.to_le_bytes());
    result.extend_from_slice(&extension);
    fn chunk(result: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
        result.extend_from_slice(kind);
        result.extend_from_slice(&(data.len() as u32).to_le_bytes());
        result.extend_from_slice(data);
        if data.len() % 2 == 1 {
            result.push(0);
        }
    }
    if icc {
        chunk(&mut result, b"ICCP", profile);
    }
    result.extend_from_slice(&encoded[start..]);
    if let Some(exif) = exif {
        chunk(&mut result, b"EXIF", exif);
    }
    Ok(result)
}

#[cfg(test)]
#[path = "webp_output_tests.rs"]
mod tests;
