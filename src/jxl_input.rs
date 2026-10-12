use image::{DynamicImage, ImageBuffer, Rgba32FImage};
use jxl_oxide::{InitializeResult, JxlImage, image::BitDepth};
use std::{io::Read, path::Path};
fn error(e: impl std::fmt::Display) -> String {
    format!("JPEG XL 파일을 열 수 없습니다: {e}")
}
pub fn read(path: &Path) -> Result<crate::codecs::Decoded, String> {
    read_impl(path, true)
}
#[cfg(test)]
pub(crate) fn read_native(path: &Path) -> Result<crate::codecs::Decoded, String> {
    read_impl(path, false)
}
fn read_impl(path: &Path, working: bool) -> Result<crate::codecs::Decoded, String> {
    let mut metadata_notes = vec![];
    let (mut file, source_metadata) =
        crate::jxl_metadata::open(path, &mut metadata_notes).map_err(error)?;
    let mut uninit = JxlImage::builder()
        .alloc_tracker(jxl_grid::AllocTracker::with_limit(
            (crate::document::MAX_PIXELS * 16) as usize,
        ))
        .build_uninit();
    let mut buffer = [0_u8; 4096];
    let mut valid = 0;
    let mut image = loop {
        let count = file.read(&mut buffer[valid..]).map_err(error)?;
        if count == 0 {
            return Err("JPEG XL 머리글이 잘렸습니다.".into());
        }
        valid += count;
        let used = uninit.feed_bytes(&buffer[..valid]).map_err(error)?;
        buffer.copy_within(used..valid, 0);
        valid -= used;
        match uninit.try_init().map_err(error)? {
            InitializeResult::NeedMoreData(next) => uninit = next,
            InitializeResult::Initialized(image) => break image,
        }
    };
    crate::document::validate_size(image.width(), image.height())?;
    let source_icc = image.original_icc().and_then(|profile| {
        if profile.len() <= crate::source_metadata::MAX_BYTES {
            Some(profile.to_vec())
        } else {
            metadata_notes.push("JPEG XL 원본 ICC가 보관 예산을 초과했습니다.".into());
            None
        }
    });
    let metadata = &image.image_header().metadata;
    let alpha = metadata.ec_info.iter().find(|e| e.is_alpha());
    let associated = alpha.and_then(|a| a.alpha_associated()).unwrap_or(false);
    let depth = std::iter::once(metadata.bit_depth)
        .chain(alpha.map(|a| a.bit_depth))
        .collect::<Vec<_>>();
    let mut notes=vec!["JPEG XL는 대표 이미지 한 장으로 엽니다. 애니메이션·추가 채널·레이어 편집은 아직 지원하지 않습니다.".into()];
    notes.extend(metadata_notes);
    if image.pixel_format().has_black() {
        image.request_color_encoding(jxl_oxide::EnumColourEncoding::srgb(
            jxl_oxide::RenderingIntent::Relative,
        ));
        notes.push(
            "JPEG XL의 CMYK는 RGB 표시로 변환합니다. 원본 CMYK 채널 편집은 아직 지원하지 않습니다."
                .into(),
        );
    }
    while image.num_loaded_keyframes() == 0 {
        if valid == 0 {
            valid = file.read(&mut buffer).map_err(error)?;
            if valid == 0 {
                return Err("JPEG XL 첫 이미지 데이터가 잘렸습니다.".into());
            }
        }
        let used = image.feed_bytes(&buffer[..valid]).map_err(error)?;
        if used == 0 && valid == buffer.len() {
            return Err("JPEG XL 입력을 처리할 수 없습니다.".into());
        }
        buffer.copy_within(used..valid, 0);
        valid -= used;
        if image.num_loaded_keyframes() == 0 && valid > 0 {
            let count = file.read(&mut buffer[valid..]).map_err(error)?;
            if count == 0 {
                return Err("JPEG XL 첫 이미지 데이터가 잘렸습니다.".into());
            }
            valid += count;
        }
    }
    let rendered = image.render_frame(0).map_err(error)?;
    let mut stream = rendered.stream();
    let (w, h, c) = (stream.width(), stream.height(), stream.channels() as usize);
    crate::document::validate_size(w, h)?;
    let gray = image.pixel_format().is_grayscale();
    let has_alpha = image.pixel_format().has_alpha();
    if c != if gray {
        if has_alpha { 2 } else { 1 }
    } else if has_alpha {
        4
    } else {
        3
    } {
        return Err("JPEG XL 색상 채널을 RGB로 변환하지 못했습니다.".into());
    }
    // A row of float samples keeps negative and over-range color values intact, including grayscale.
    let mut pixels = Rgba32FImage::new(w, h);
    let mut row = vec![0_f32; w as usize * c];
    for y in 0..h {
        if stream.write_to_buffer(&mut row) != row.len() {
            return Err("JPEG XL 픽셀 데이터가 잘렸습니다.".into());
        }
        for (x, sample) in row.chunks_exact(c).enumerate() {
            let rgb = if gray {
                [sample[0]; 3]
            } else {
                [sample[0], sample[1], sample[2]]
            };
            let a = if has_alpha { sample[c - 1] } else { 1. };
            let rgb = if associated && a > 0. {
                rgb.map(|v| v / a)
            } else {
                rgb
            };
            if rgb.iter().chain([a].iter()).any(|v| !v.is_finite()) {
                return Err("JPEG XL에 유효하지 않은 부동소수점 값이 있습니다.".into());
            }
            pixels.put_pixel(x as u32, y, image::Rgba([rgb[0], rgb[1], rgb[2], a]));
        }
    }
    if associated {
        notes.push("미리 곱해진 JPEG XL 알파를 편집용 독립 색상/알파로 변환했습니다.".into());
    }
    let pixels = if working {
        crate::jxl_color::apply(pixels, &image.rendered_icc(), gray, &mut notes)
    } else {
        pixels
    };
    let float = depth
        .iter()
        .any(|d| matches!(d, BitDepth::FloatSample { .. }) || d.bits_per_sample() > 16)
        || pixels
            .pixels()
            .any(|p| p.0.iter().any(|v| !(0. ..=1.).contains(v)));
    let bits = depth.iter().map(|d| d.bits_per_sample()).max().unwrap_or(8);
    let pixels = if float {
        if depth
            .iter()
            .all(|d| matches!(d, BitDepth::IntegerSample { .. }) && d.bits_per_sample() <= 16)
        {
            notes.push("범위 밖 JPEG XL 색 값을 자르지 않도록 32비트 작업 버퍼에 유지합니다. 화면 미리보기는 SDR로 표시합니다.".into());
        }
        DynamicImage::ImageRgba32F(pixels)
    } else if bits > 8 {
        DynamicImage::ImageRgba16(ImageBuffer::from_fn(w, h, |x, y| {
            image::Rgba(
                pixels
                    .get_pixel(x, y)
                    .0
                    .map(|v| (v * 65535.).round() as u16),
            )
        }))
    } else {
        DynamicImage::ImageRgba8(ImageBuffer::from_fn(w, h, |x, y| {
            image::Rgba(pixels.get_pixel(x, y).0.map(|v| (v * 255.).round() as u8))
        }))
    };
    Ok(crate::codecs::Decoded {
        pixels,
        notes,
        source_icc,
        source_metadata,
    })
}

#[cfg(test)]
#[path = "jxl_icc_tests.rs"]
mod icc_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    fn boxed(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut result = ((data.len() + 8) as u32).to_be_bytes().to_vec();
        result.extend(kind);
        result.extend(data);
        result
    }
    #[test]
    fn container_exif_orientation_does_not_rotate_already_oriented_jxl_and_truncated_inputs_are_safe()
     {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let original_path = root.join("tests/fixtures/jxl/lossless_pfm/input.jxl");
        let bytes = std::fs::read(&original_path).unwrap();
        let codestream = if bytes.starts_with(b"\xff\x0a") {
            bytes
        } else {
            let mut offset = 0;
            let mut raw = Vec::new();
            while offset + 8 <= bytes.len() {
                let len =
                    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
                let len = if len == 0 { bytes.len() - offset } else { len };
                assert!(len >= 8 && offset + len <= bytes.len());
                if &bytes[offset + 4..offset + 8] == b"jxlc" {
                    raw.extend(&bytes[offset + 8..offset + len]);
                }
                offset += len;
            }
            assert!(raw.starts_with(b"\xff\x0a"));
            raw
        };
        let expected = read(&original_path).unwrap().pixels;
        let out = root.join("qa-output/jxl");
        std::fs::create_dir_all(&out).unwrap();
        let path = out.join("exif-orientation.jxl");
        let mut container = boxed(b"JXL ", b"\x0d\x0a\x87\x0a");
        container.extend(boxed(b"ftyp", b"jxl \0\0\0\0jxl "));
        // EXIF orientation 8 belongs to the source metadata. JPEG XL's own orientation is authoritative.
        let mut exif = vec![0; 4];
        exif.extend(b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x08\0\0\0\0\0\0\0");
        container.extend(boxed(b"Exif", &exif));
        container.extend(boxed(b"jxlc", &codestream));
        std::fs::write(&path, &container).unwrap();
        use image::ImageDecoder;
        let mut legacy =
            jxl_oxide::integration::JxlDecoder::new(File::open(&path).unwrap()).unwrap();
        assert_eq!(
            legacy.orientation().unwrap(),
            image::metadata::Orientation::Rotate270
        );
        assert_eq!(read(&path).unwrap().pixels, expected);
        assert_eq!(std::fs::read(&path).unwrap(), container);
        let invalid = out.join("truncated.jxl");
        for bytes in [
            &b""[..],
            &codestream[..1],
            &codestream[..5],
            &codestream[..codestream.len() / 2],
            &b"wrong format"[..],
        ] {
            std::fs::write(&invalid, bytes).unwrap();
            assert!(read(&invalid).is_err());
            assert!(crate::files::open(&invalid).is_err());
            assert_eq!(std::fs::read(&invalid).unwrap(), bytes);
        }
    }
}
