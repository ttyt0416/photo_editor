use crate::{document::Document, files::ExportOptions};
use ravif::{Encoder, MatrixCoefficients, PixelRange};
use std::{io::Write, path::Path};
pub const COLOR_LABELS: &[&str] = &["YCbCr (BT.601)", "YCbCr (BT.709)", "RGB"];
fn planes(p: [f32; 4], mode: usize, maximum: f32) -> [u16; 3] {
    let [r, g, b] = std::array::from_fn::<_, 3, _>(|i| p[i].clamp(0., 1.));
    let rgb = if mode == 2 {
        [g, b, r]
    } else {
        let (kr, kb) = if mode == 1 {
            (0.2126, 0.0722)
        } else {
            (0.299, 0.114)
        };
        let y = kr * r + (1. - kr - kb) * g + kb * b;
        [
            y,
            (b - y) / (2. * (1. - kb)) + 0.5,
            (r - y) / (2. * (1. - kr)) + 0.5,
        ]
    };
    rgb.map(|v| (v.clamp(0., 1.) * maximum).round() as u16)
}
pub fn encode(doc: &Document, options: &ExportOptions) -> Result<Vec<u8>, String> {
    if !(1..=100).contains(&options.avif_quality)
        || !(1..=100).contains(&options.avif_alpha_quality)
        || !(1..=10).contains(&options.avif_speed)
        || !matches!(options.avif_depth, 8 | 10)
        || options.avif_color >= COLOR_LABELS.len()
        || options.avif_threads > 256
    {
        return Err("AVIF 품질·속도·정밀도 설정을 확인하세요.".into());
    }
    let encoder = Encoder::new()
        .with_quality(f32::from(options.avif_quality))
        .with_alpha_quality(f32::from(options.avif_alpha_quality))
        .with_speed(options.avif_speed)
        .with_num_threads(if options.avif_threads == 0 {
            None
        } else {
            Some(usize::from(options.avif_threads))
        });
    let pixels = crate::precision::composite(doc);
    if pixels.as_raw().iter().any(|v| !v.is_finite()) {
        return Err("AVIF 출력에 비정상 픽셀 값이 있습니다.".into());
    }
    let alpha = pixels.pixels().any(|p| p[3] < 1.);
    let matrix = match options.avif_color {
        1 => MatrixCoefficients::BT709,
        2 => MatrixCoefficients::Identity,
        _ => MatrixCoefficients::BT601,
    };
    let w = doc.width as usize;
    let h = doc.height as usize;
    let result = if options.avif_depth == 10 {
        let color = pixels
            .pixels()
            .map(|p| planes(p.0, options.avif_color, 1023.));
        let coverage = alpha.then(|| {
            pixels
                .pixels()
                .map(|p| (p[3].clamp(0., 1.) * 1023.).round() as u16)
        });
        encoder.encode_raw_planes_10_bit(w, h, color, coverage, PixelRange::Full, matrix)
    } else {
        let color = pixels
            .pixels()
            .map(|p| planes(p.0, options.avif_color, 255.).map(|v| v as u8));
        let coverage = alpha.then(|| {
            pixels
                .pixels()
                .map(|p| (p[3].clamp(0., 1.) * 255.).round() as u8)
        });
        encoder.encode_raw_planes_8_bit(w, h, color, coverage, PixelRange::Full, matrix)
    }
    .map_err(|e| format!("AVIF 인코딩 실패: {e}"))?;
    Ok(result.avif_file)
}
pub fn save(doc: &Document, path: &Path, options: &ExportOptions) -> Result<(), String> {
    // Encode before opening the temporary destination; invalid options and codec errors keep files intact.
    let bytes = encode(doc, options)?;
    crate::files::atomic_write(path, |file| {
        file.write_all(&bytes).map_err(|e| e.to_string())
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn avif_quality_alpha_speed_depth_matrix_and_precise_source_roundtrip() {
        let source = image::Rgba32FImage::from_fn(16, 8, |x, y| {
            image::Rgba([
                0.101 + x as f32 / 31.,
                0.211 + y as f32 / 37.,
                0.321 + x as f32 / 47.,
                if x == 0 {
                    0.
                } else if x == 1 {
                    0.5
                } else {
                    1.
                },
            ])
        });
        let doc = crate::precision::document(
            "AVIF precise".into(),
            image::DynamicImage::ImageRgba32F(source),
        )
        .unwrap();
        let original = doc.layers[0].precision.clone().unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/avif-output");
        std::fs::create_dir_all(&dir).unwrap();
        let expected = crate::precision::composite(&doc);
        crate::precision::preview(&expected)
            .save(dir.join("source.png"))
            .unwrap();
        for depth in [8, 10] {
            for mode in 0..3 {
                for quality in [40, 90, 100] {
                    let options = ExportOptions {
                        avif_depth: depth,
                        avif_color: mode,
                        avif_quality: quality,
                        avif_alpha_quality: 100,
                        avif_speed: 10,
                        ..Default::default()
                    };
                    let path = dir.join(format!("d{depth}-c{mode}-q{quality}.avif"));
                    save(&doc, &path, &options).unwrap();
                    let bytes = std::fs::read(&path).unwrap();
                    let parsed = avif_parse::read_avif(&mut bytes.as_slice()).unwrap();
                    assert_eq!(parsed.primary_item_metadata().unwrap().bit_depth, depth);
                    assert!(parsed.alpha_item.is_some());
                    let restored = crate::files::open(&path).unwrap().document;
                    assert_eq!(
                        restored.layers[0].bit_depth,
                        if depth == 8 { 8 } else { 16 }
                    );
                    let actual = crate::precision::composite(&restored);
                    let maximum = if depth == 8 { 255. } else { 1023. };
                    let mut error = 0.;
                    let mut count = 0.;
                    for (a, b) in actual.pixels().zip(expected.pixels()) {
                        assert!((a[3] - b[3]).abs() <= 1. / maximum + 1e-6);
                        if b[3] > 0. {
                            for c in 0..3 {
                                error += (a[c] - b[c]).abs();
                                count += 1.;
                            }
                        }
                    }
                    if quality >= 90 {
                        assert!(
                            error / count < 0.025,
                            "depth {depth} c{mode} q{quality} error {}",
                            error / count
                        );
                    }
                    if depth == 10 && mode == 2 && quality == 100 {
                        assert!(
                            actual
                                .pixels()
                                .filter(|p| p[3] > 0.)
                                .flat_map(|p| p.0[..3].to_vec())
                                .filter(|v| (*v * 255. - (*v * 255.).round()).abs() > 0.08)
                                .count()
                                > 10
                        );
                        for (a, b) in actual.pixels().zip(expected.pixels()) {
                            for c in 0..4 {
                                assert!(
                                    (a[c] - b[c]).abs() <= 2. / 1023. + 1e-6,
                                    "10-bit RGB precision: {a:?} {b:?}"
                                );
                            }
                        }
                    }
                }
            }
        }
        for (speed, alpha) in [(1, 40), (4, 80), (10, 100)] {
            let options = ExportOptions {
                avif_speed: speed,
                avif_alpha_quality: alpha,
                avif_color: 2,
                ..Default::default()
            };
            save(&doc, &dir.join(format!("s{speed}-a{alpha}.avif")), &options).unwrap();
        }
        assert!(std::sync::Arc::ptr_eq(
            &original,
            doc.layers[0].precision.as_ref().unwrap()
        ));
        assert_eq!(crate::precision::composite(&doc), expected);
    }
    #[test]
    fn avif_invalid_options_leave_existing_files_and_source_unchanged() {
        let doc = Document::new(8, 8, true).unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/avif-invalid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("existing.avif");
        std::fs::write(&path, b"previous").unwrap();
        for options in [
            ExportOptions {
                avif_quality: 0,
                ..Default::default()
            },
            ExportOptions {
                avif_quality: 101,
                ..Default::default()
            },
            ExportOptions {
                avif_alpha_quality: 0,
                ..Default::default()
            },
            ExportOptions {
                avif_speed: 0,
                ..Default::default()
            },
            ExportOptions {
                avif_speed: 11,
                ..Default::default()
            },
            ExportOptions {
                avif_depth: 12,
                ..Default::default()
            },
            ExportOptions {
                avif_color: 3,
                ..Default::default()
            },
            ExportOptions {
                avif_threads: 257,
                ..Default::default()
            },
        ] {
            assert!(save(&doc, &path, &options).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"previous");
        }
    }
}
