use crate::{document::Document, files::ExportOptions};
use jpeg_encoder::{ChromaSubsamplingMethod, ColorType, Encoder, PixelDensity, SamplingFactor};
use std::path::Path;
pub const SAMPLING_LABELS: &[&str] = &[
    "자동", "4:4:4", "4:2:2", "4:2:0", "4:4:0", "4:1:1", "4:1:0", "4:4:1", "4:2:1",
];
pub const FACTORS: &[(u8, u8)] = &[
    (1, 1),
    (2, 1),
    (2, 2),
    (1, 2),
    (4, 1),
    (4, 2),
    (1, 4),
    (2, 4),
];
pub fn save(doc: &Document, path: &Path, options: &ExportOptions) -> Result<(), String> {
    if !(1..=100).contains(&options.quality)
        || options.jpeg_sampling >= SAMPLING_LABELS.len()
        || !(2..=64).contains(&options.jpeg_scans)
        || options.jpeg_resolution == 0
    {
        return Err("JPEG 품질·샘플링·스캔·해상도 설정을 확인하세요.".into());
    }
    let w = u16::try_from(doc.width).map_err(|_| "JPEG의 가로는 65535px 이하여야 합니다.")?;
    let h = u16::try_from(doc.height).map_err(|_| "JPEG의 세로는 65535px 이하여야 합니다.")?;
    let pixels = doc.composite();
    let rgb = image::RgbImage::from_fn(doc.width, doc.height, |x, y| {
        let p = pixels.get_pixel(x, y);
        let alpha = u16::from(p[3]);
        image::Rgb(std::array::from_fn(|c| {
            ((u16::from(p[c]) * alpha + u16::from(options.background[c]) * (255 - alpha) + 127)
                / 255) as u8
        }))
    });
    crate::files::atomic_write(path, |file| {
        let mut encoder = Encoder::new(file, options.quality);
        if options.include_icc {
            encoder
                .add_icc_profile(crate::color_management::srgb_profile())
                .map_err(|e| e.to_string())?;
        }
        if options.jpeg_sampling > 0 {
            let (x, y) = FACTORS[options.jpeg_sampling - 1];
            encoder.set_sampling_factor(SamplingFactor::from_factors(x, y).unwrap());
        }
        encoder.set_chroma_subsampling_method(if options.jpeg_average {
            ChromaSubsamplingMethod::Average
        } else {
            ChromaSubsamplingMethod::Nearest
        });
        encoder.set_optimized_huffman_tables(options.jpeg_optimize);
        if options.jpeg_progressive {
            encoder.set_progressive_scans(options.jpeg_scans);
        }
        encoder.set_density(PixelDensity::dpi(options.jpeg_resolution));
        if options.jpeg_restart > 0 {
            encoder.set_restart_interval(options.jpeg_restart);
        }
        encoder
            .encode(rgb.as_raw(), w, h, ColorType::Rgb)
            .map_err(|e| e.to_string())
    })
}

// The image wrapper fixes the maximum at 100 scans; our 64 scans per
// component output needs 192. Keep a finite bound and the document pixel budget.
pub fn read(path: &Path) -> Result<crate::codecs::Decoded, String> {
    if std::fs::metadata(path).map_err(|e| e.to_string())?.len() > crate::document::MAX_PIXELS * 16
    {
        return Err("JPEG 파일이 메모리 예산을 초과합니다.".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    decode_bytes(&bytes)
}
pub(crate) fn read_bytes(bytes: &[u8]) -> Result<(image::DynamicImage, Vec<String>), String> {
    decode_bytes(bytes).map(|decoded| (decoded.pixels, decoded.notes))
}
fn decode_bytes(bytes: &[u8]) -> Result<crate::codecs::Decoded, String> {
    use zune_core::{bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions};
    if bytes.len() as u64 > crate::document::MAX_PIXELS * 16 {
        return Err("JPEG 파일이 메모리 예산을 초과합니다.".into());
    }
    let options = DecoderOptions::default()
        .set_strict_mode(false)
        .set_max_width(65535)
        .set_max_height(65535)
        .jpeg_set_max_scans(256);
    let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.decode_headers().map_err(|e| e.to_string())?;
    let (w, h) = decoder
        .dimensions()
        .ok_or("JPEG 크기를 읽을 수 없습니다.")?;
    crate::document::validate_size(w as u32, h as u32)?;
    let profile = decoder.icc_profile();
    let orientation = decoder
        .exif()
        .and_then(|v| image::metadata::Orientation::from_exif_chunk(v))
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let gray = decoder.input_colorspace() == Some(ColorSpace::Luma);
    decoder.set_options(decoder.options().jpeg_set_out_colorspace(if gray {
        ColorSpace::Luma
    } else {
        ColorSpace::RGB
    }));
    let pixels = decoder.decode().map_err(|e| e.to_string())?;
    let mut image = if gray {
        image::DynamicImage::ImageLuma8(
            image::GrayImage::from_raw(w as u32, h as u32, pixels)
                .ok_or("JPEG 픽셀 버퍼가 잘못되었습니다.")?,
        )
    } else {
        image::DynamicImage::ImageRgb8(
            image::RgbImage::from_raw(w as u32, h as u32, pixels)
                .ok_or("JPEG 픽셀 버퍼가 잘못되었습니다.")?,
        )
    };
    image.apply_orientation(orientation);
    let mut notes = vec![];
    if let Some(profile) = &profile {
        image = crate::color_management::apply(image, profile, &mut notes);
    }
    Ok(crate::codecs::Decoded {
        pixels: image,
        notes,
        source_icc: profile,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn segments(bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
        assert_eq!(&bytes[..2], &[255, 216]);
        let mut i = 2;
        let mut out = vec![];
        while i < bytes.len() {
            if bytes[i] != 255 {
                i += 1;
                continue;
            }
            while i < bytes.len() && bytes[i] == 255 {
                i += 1;
            }
            if i == bytes.len() {
                break;
            }
            let code = bytes[i];
            i += 1;
            if code == 0 || matches!(code, 208..=217) {
                continue;
            }
            let length = u16::from_be_bytes(bytes[i..i + 2].try_into().unwrap()) as usize;
            assert!(length >= 2 && i + length <= bytes.len());
            out.push((code, bytes[i + 2..i + length].to_vec()));
            i += length;
        }
        out
    }
    #[test]
    fn all_sampling_progressive_huffman_matte_restart_resolution_and_precision_sources_roundtrip() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jpeg-output");
        std::fs::create_dir_all(&dir).unwrap();
        let pixels = image::RgbaImage::from_fn(37, 29, |x, y| {
            image::Rgba([
                (30 + x * 4) as u8,
                (40 + y * 5) as u8,
                (50 + x + y) as u8,
                if x == 0 {
                    0
                } else if x == 1 {
                    128
                } else {
                    255
                },
            ])
        });
        let doc = Document::from_image("JPEG".into(), pixels);
        let source = doc.layers[0].pixels.clone();
        let expected = image::RgbImage::from_fn(37, 29, |x, y| {
            let p = doc.layers[0].pixels.get_pixel(x, y);
            let alpha = u16::from(p[3]);
            image::Rgb(std::array::from_fn(|c| {
                ((u16::from(p[c]) * alpha + [20, 30, 40][c] * (255 - alpha) + 127) / 255) as u8
            }))
        });
        expected.save(dir.join("matte.png")).unwrap();
        for sampling in 1..SAMPLING_LABELS.len() {
            for progressive in [false, true] {
                for optimize in [false, true] {
                    let options = ExportOptions {
                        quality: 95,
                        background: [20, 30, 40],
                        jpeg_sampling: sampling,
                        jpeg_progressive: progressive,
                        jpeg_optimize: optimize,
                        jpeg_resolution: 300,
                        jpeg_restart: 3,
                        ..Default::default()
                    };
                    let path = dir.join(format!(
                        "s{sampling}-p{}-o{}.jpg",
                        u8::from(progressive),
                        u8::from(optimize)
                    ));
                    crate::files::export(&doc, &path, image::ImageFormat::Jpeg, &options).unwrap();
                    let bytes = std::fs::read(&path).unwrap();
                    let tags = segments(&bytes);
                    let frame = tags
                        .iter()
                        .find(|v| v.0 == if progressive { 194 } else { 192 })
                        .unwrap();
                    assert_eq!(frame.1[0], 8);
                    assert_eq!(
                        (
                            u16::from_be_bytes(frame.1[1..3].try_into().unwrap()),
                            u16::from_be_bytes(frame.1[3..5].try_into().unwrap())
                        ),
                        (29, 37)
                    );
                    let (h, v) = FACTORS[sampling - 1];
                    assert_eq!(frame.1[7], h * 16 + v);
                    let jfif = tags.iter().find(|v| v.0 == 224).unwrap();
                    assert_eq!(&jfif.1[..5], b"JFIF\0");
                    assert_eq!(jfif.1[7], 1);
                    assert_eq!(&jfif.1[8..12], &[1, 44, 1, 44]);
                    assert_eq!(tags.iter().find(|v| v.0 == 221).unwrap().1, vec![0, 3]);
                    if progressive {
                        assert!(tags.iter().filter(|v| v.0 == 218).count() > 3);
                    }
                    let restored = crate::files::open(&path).unwrap().document.composite();
                    assert_eq!(restored.dimensions(), expected.dimensions());
                    let error = restored
                        .pixels()
                        .zip(expected.pixels())
                        .flat_map(|(a, b)| (0..3).map(move |c| f64::from(a[c].abs_diff(b[c]))))
                        .sum::<f64>()
                        / (37. * 29. * 3.);
                    assert!(
                        error < 12.,
                        "s{sampling} progressive{progressive} optimize{optimize} error{error}"
                    );
                }
            }
        }
        for scans in [2, 8, 64] {
            let options = ExportOptions {
                quality: 100,
                jpeg_progressive: true,
                jpeg_scans: scans,
                jpeg_optimize: true,
                jpeg_restart: 1,
                jpeg_sampling: 3,
                ..Default::default()
            };
            let path = dir.join(format!("scans{scans}.jpg"));
            save(&doc, &path, &options).unwrap();
            assert_eq!(
                crate::files::open(&path)
                    .unwrap()
                    .document
                    .composite()
                    .dimensions(),
                (37, 29)
            );
        }
        for quality in [50, 100] {
            let options = ExportOptions {
                quality,
                jpeg_sampling: 0,
                ..Default::default()
            };
            let path = dir.join(format!("auto{quality}.jpg"));
            save(&doc, &path, &options).unwrap();
            let tags = segments(&std::fs::read(&path).unwrap());
            assert_eq!(
                tags.iter().find(|t| t.0 == 192).unwrap().1[7],
                if quality < 90 { 34 } else { 17 }
            );
        }
        assert!(std::sync::Arc::ptr_eq(&source, &doc.layers[0].pixels));
        let hdr = crate::precision::document(
            "HDR JPEG".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
                16,
                8,
                image::Rgba([2.25, -0.125, 0.25, 0.5]),
            )),
        )
        .unwrap();
        let original = hdr.layers[0].precision.clone().unwrap();
        save(
            &hdr,
            &dir.join("hdr-matte.jpg"),
            &ExportOptions {
                quality: 100,
                background: [20, 30, 40],
                ..Default::default()
            },
        )
        .unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &original,
            hdr.layers[0].precision.as_ref().unwrap()
        ));
    }
    #[test]
    fn jpeg_reader_preserves_exif_grayscale_cmyk_profile_notice_and_guessed_format() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg-input");
        for name in (1..=8)
            .map(|v| format!("exif{v}"))
            .chain(["L".into(), "CMYK".into()])
        {
            let opened = crate::files::open(&root.join(format!("{name}.jpg"))).unwrap();
            let actual = opened.document.composite();
            let expected = image::open(root.join(format!("{name}.png")))
                .unwrap()
                .to_rgb8();
            assert_eq!(actual.dimensions(), expected.dimensions());
            let maximum = actual
                .pixels()
                .zip(expected.pixels())
                .flat_map(|(a, b)| (0..3).map(move |c| a[c].abs_diff(b[c])))
                .max()
                .unwrap();
            assert!(maximum <= 2, "{name}: difference {maximum}");
            assert_eq!(!opened.notes.is_empty(), name.starts_with("exif"));
        }
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jpeg-reader");
        std::fs::create_dir_all(&dir).unwrap();
        let guessed = dir.join("image.data");
        std::fs::copy(root.join("exif6.jpg"), &guessed).unwrap();
        assert_eq!(
            crate::files::open(&guessed)
                .unwrap()
                .document
                .composite()
                .dimensions(),
            (8, 12)
        );
        let mut bytes = std::fs::read(root.join("L.jpg")).unwrap();
        let sof = bytes.windows(2).position(|p| p == [255, 192]).unwrap();
        bytes[sof + 5..sof + 9].copy_from_slice(&[156, 64, 156, 64]);
        let oversized = dir.join("oversized.jpg");
        std::fs::write(&oversized, bytes).unwrap();
        assert!(crate::files::open(&oversized).is_err());
        std::fs::write(&oversized, b"broken JPEG").unwrap();
        assert!(read(&oversized).is_err());
    }

    #[test]
    fn invalid_jpeg_options_and_dimensions_keep_existing_output() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jpeg-invalid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("previous.jpg");
        std::fs::write(&path, b"previous").unwrap();
        let doc = Document::new(8, 8, true).unwrap();
        for options in [
            ExportOptions {
                jpeg_sampling: 9,
                ..Default::default()
            },
            ExportOptions {
                jpeg_scans: 1,
                ..Default::default()
            },
            ExportOptions {
                jpeg_scans: 65,
                ..Default::default()
            },
            ExportOptions {
                jpeg_resolution: 0,
                ..Default::default()
            },
        ] {
            assert!(save(&doc, &path, &options).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"previous");
        }
        let large = Document::new(65536, 1, true).unwrap();
        assert!(save(&large, &path, &ExportOptions::default()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"previous");
    }
}
