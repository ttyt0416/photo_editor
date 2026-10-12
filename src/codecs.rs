use image::{DynamicImage, ImageBuffer};
use std::path::Path;

pub struct Decoded {
    pub pixels: DynamicImage,
    pub notes: Vec<String>,
    pub source_icc: Option<Vec<u8>>,
    pub source_metadata: crate::source_metadata::Metadata,
}

pub fn decode(path: &Path, extension: &str) -> Result<Option<Decoded>, String> {
    let mut notes = Vec::new();
    let mut source_icc = None;
    let mut source_metadata = crate::source_metadata::Metadata::default();
    let bytes = || -> Result<Vec<u8>, String> {
        if std::fs::metadata(path).map_err(|e| e.to_string())?.len()
            > crate::document::MAX_PIXELS * 16
        {
            return Err("파일이 현재 메모리 예산을 초과합니다.".into());
        }
        std::fs::read(path).map_err(|e| e.to_string())
    };
    let image = match extension {
        "tif" | "tiff" => return crate::tiff_float::read(path),
        "pcx" => DynamicImage::ImageRgba8(crate::pcx::read(&bytes()?)?),
        "avif" => {
            notes.push("AVIF는 대표 이미지 한 장으로 엽니다. 프레임·다중 이미지 편집은 아직 지원하지 않습니다.".into());
            let decoded = avif_decode::Decoder::from_avif(&bytes()?)
                .map_err(|e| e.to_string())?
                .to_image()
                .map_err(|e| e.to_string())?;
            macro_rules! image {
                ($img:expr,$channels:expr,$variant:ident) => {{
                    let (buffer, w, h) = $img.into_contiguous_buf();
                    crate::document::validate_size(w as u32, h as u32)?;
                    let data = buffer.into_iter().flat_map($channels).collect();
                    DynamicImage::$variant(
                        ImageBuffer::from_raw(w as u32, h as u32, data)
                            .ok_or("AVIF 픽셀 버퍼가 잘못되었습니다.")?,
                    )
                }};
            }
            match decoded {
                avif_decode::Image::Rgb8(img) => image!(img, |p| [p.r, p.g, p.b], ImageRgb8),
                avif_decode::Image::Rgba8(img) => image!(img, |p| [p.r, p.g, p.b, p.a], ImageRgba8),
                avif_decode::Image::Rgb16(img) => image!(img, |p| [p.r, p.g, p.b], ImageRgb16),
                avif_decode::Image::Rgba16(img) => {
                    image!(img, |p| [p.r, p.g, p.b, p.a], ImageRgba16)
                }
                avif_decode::Image::Gray8(img) => image!(img, |p| [p.value()], ImageLuma8),
                avif_decode::Image::Gray16(img) => image!(img, |p| [p.value()], ImageLuma16),
            }
        }
        "heic" | "heif" => {
            notes.push("HEIC/HEIF는 대표 이미지 한 장으로 엽니다. 다중 이미지와 보조 채널은 아직 편집할 수 없습니다.".into());
            let decoded = heif_oxide::decode_bytes(&bytes()?).map_err(|e| e.to_string())?;
            if decoded.color.icc_present {
                notes.push("내장 ICC 색상 프로파일은 현재 적용되지 않습니다.".into());
            }
            if decoded
                .color
                .nclx
                .is_some_and(|c| matches!(c.transfer, 16 | 18))
            {
                notes.push("이 HEIC의 HDR 전달 곡선(PQ/HLG)은 현재 톤 매핑되지 않습니다. 원본 파일을 함께 보관하세요.".into());
            }
            let (w, h) = (decoded.width, decoded.height);
            crate::document::validate_size(w, h)?;
            match decoded.pixels {
                heif_oxide::Pixels::Rgb8(p) => DynamicImage::ImageRgb8(
                    ImageBuffer::from_raw(w, h, p).ok_or("HEIC 픽셀 버퍼가 잘못되었습니다.")?,
                ),
                heif_oxide::Pixels::Rgba8(p) => DynamicImage::ImageRgba8(
                    ImageBuffer::from_raw(w, h, p).ok_or("HEIC 픽셀 버퍼가 잘못되었습니다.")?,
                ),
                heif_oxide::Pixels::Rgb16(p) => DynamicImage::ImageRgb16(
                    ImageBuffer::from_raw(w, h, p).ok_or("HEIC 픽셀 버퍼가 잘못되었습니다.")?,
                ),
                heif_oxide::Pixels::Rgba16(p) => DynamicImage::ImageRgba16(
                    ImageBuffer::from_raw(w, h, p).ok_or("HEIC 픽셀 버퍼가 잘못되었습니다.")?,
                ),
            }
        }
        "jxl" => return crate::jxl_input::read(path).map(Some),
        "jp2" | "j2k" | "jpc" | "jpf" | "jpx" => {
            let encoded = bytes()?;
            source_metadata = crate::jpeg2000_xmp::read(&encoded, &mut notes);
            let decoded = jpeg2k::Image::from_bytes(&encoded).map_err(|e| e.to_string())?;
            if decoded.has_icc_profile()
                && matches!(crate::jpeg2000_icc::profile(&encoded), Ok(None))
            {
                notes.push("JPEG2000의 이 색상 프로파일 변환은 아직 제공하지 않습니다.".into());
            }
            source_icc = crate::jpeg2000_icc::profile(&encoded)
                .ok()
                .flatten()
                .map(|p| p.to_vec());
            let pixels = crate::jpeg2000_input::pixels(&decoded, &encoded, &mut notes)?;
            crate::jpeg2000_icc::apply(pixels, decoded.components(), &encoded, &mut notes)
        }
        _ => return Ok(None),
    };
    Ok(Some(Decoded {
        pixels: image,
        notes,
        source_icc,
        source_metadata,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn heic_and_independently_encoded_jpeg2000_decode_pixels_and_alpha() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let heic = decode(&dir.join("flat-red.heic"), "heic")
            .unwrap()
            .unwrap()
            .pixels;
        assert_eq!((heic.width(), heic.height()), (64, 64));
        let red = heic.to_rgba8().get_pixel(32, 32).0;
        assert!(red[0] > 200 && red[1] < 50 && red[2] < 50);
        assert_eq!(red[3], 255);
        let jp2 = decode(&dir.join("independent.jp2"), "jp2")
            .unwrap()
            .unwrap()
            .pixels;
        assert_eq!((jp2.width(), jp2.height()), (7, 5));
        assert_eq!(jp2.to_rgba8().get_pixel(2, 3).0, [60, 120, 120, 100]);
        assert_eq!(jp2.to_rgba8().get_pixel(6, 4).0, [180, 160, 120, 255]);
        let invalid = dir.join("independent.jp2");
        for extension in ["avif", "heic", "jxl"] {
            assert!(decode(&invalid, extension).is_err());
        }
    }
    #[test]
    fn official_jxl_reference_pixels_alpha_orientation_and_precision_are_retained() {
        use std::io::Read;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jxl");
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
        let output_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jxl");
        std::fs::create_dir_all(&output_root).unwrap();
        let mut measurements = serde_json::Map::new();
        for name in [
            "alpha_triangles",
            "alpha_nonpremultiplied",
            "alpha_premultiplied",
            "sunset_logo",
            "lossless_pfm",
            "grayscale_jpeg",
        ] {
            let input = root.join(name).join("input.jxl");
            let original = std::fs::read(&input).unwrap();
            let decoded = crate::jxl_input::read_native(&input).unwrap();
            assert!(decoded.notes.iter().any(|n| n.contains("대표 이미지")));
            let shape = manifest["cases"][name]["shape"].as_array().unwrap();
            let h = shape[1].as_u64().unwrap() as u32;
            let w = shape[2].as_u64().unwrap() as u32;
            let channels = shape[3].as_u64().unwrap() as usize;
            assert_eq!(
                (decoded.pixels.width(), decoded.pixels.height()),
                (w, h),
                "{name}"
            );
            let depth = crate::precision::bit_depth(&decoded.pixels);
            let compressed =
                std::fs::read(root.join(name).join("reference_image.npy.zlib")).unwrap();
            let mut reference = vec![];
            flate2::read::ZlibDecoder::new(&compressed[..])
                .read_to_end(&mut reference)
                .unwrap();
            let offset = manifest["cases"][name]["npy_data_offset"].as_u64().unwrap() as usize;
            let data = &reference[offset..];
            assert_eq!(data.len(), (w as usize) * (h as usize) * channels * 4);
            let metadata: serde_json::Value =
                serde_json::from_slice(&std::fs::read(root.join(name).join("test.json")).unwrap())
                    .unwrap();
            let peak = metadata["frames"][0]["peak_error"].as_f64().unwrap() as f32;
            let quant = if depth == 8 {
                0.5 / 255.
            } else if depth == 16 {
                0.5 / 65535.
            } else {
                0.
            };
            let rgba = decoded.pixels.to_rgba32f();
            let mut maximum = 0.0_f32;
            let mut maxima = vec![0.0_f32; channels];
            let mut sums = vec![0.0_f64; channels];
            let mut worst = String::new();
            for (index, p) in rgba.pixels().enumerate() {
                for channel in 0..channels {
                    let mut actual = p[if channels == 1 { 0 } else { channel }];
                    let start = (index * channels + channel) * 4;
                    let expected = f32::from_le_bytes(data[start..start + 4].try_into().unwrap());
                    // The official associated-alpha reference stores premultiplied RGB.
                    if name == "alpha_premultiplied" && channel < 3 {
                        actual *= p[3];
                    }
                    let error = (actual - expected).abs();
                    maxima[channel] = maxima[channel].max(error);
                    sums[channel] += f64::from(error).powi(2);
                    if (actual - expected).abs() > maximum {
                        maximum = (actual - expected).abs();
                        worst = format!(
                            "index={index} c={channel} actual={:?} expected={expected}",
                            p.0
                        );
                    }
                }
                if channels != 4 {
                    assert_eq!(p[3], 1.);
                }
            }
            // Upstream jxl-oxide tests use separate VarDCT RGB and Modular alpha bounds.
            // The corpus descriptor's common alpha bound is stricter than its VarDCT RGB test.
            let rms_limit = metadata["frames"][0]["rms_error"].as_f64().unwrap();
            for channel in 0..channels {
                let (peak_limit, rms_limit) = if name == "alpha_premultiplied" && channel < 3 {
                    (0.004, 0.0001)
                } else {
                    (peak + quant, rms_limit + f64::from(quant))
                };
                let rms = (sums[channel] / f64::from(w * h)).sqrt();
                assert!(
                    maxima[channel] <= peak_limit + 1e-6 && rms <= rms_limit + 1e-7,
                    "{name} c={channel}: max {} rms {rms} peak limit {peak_limit} rms limit {rms_limit} {worst}",
                    maxima[channel]
                );
            }
            println!("{name}: depth={depth} peaks={maxima:?}");
            let mut compressed = flate2::write::ZlibEncoder::new(
                std::fs::File::create(output_root.join(format!("{name}.rgba32.le.zlib"))).unwrap(),
                flate2::Compression::default(),
            );
            use std::io::Write;
            let raw: Vec<u8> = rgba.as_raw().iter().flat_map(|v| v.to_le_bytes()).collect();
            compressed.write_all(&raw).unwrap();
            compressed.finish().unwrap();
            measurements.insert(name.into(),serde_json::json!({"width":w,"height":h,"reference_channels":channels,"depth":depth,"peak":maxima,"rms":sums.iter().map(|sum|(sum/f64::from(w*h)).sqrt()).collect::<Vec<_>>() }));
            let opened = crate::files::open(&input).unwrap();
            let layer = opened.document.active_layer().unwrap();
            let working = crate::jxl_input::read(&input).unwrap().pixels;
            assert_eq!(layer.bit_depth, crate::precision::bit_depth(&working));
            if layer.bit_depth > 8 {
                assert_eq!(layer.precision.as_deref().unwrap(), &working.to_rgba32f());
            }
            if name == "lossless_pfm" {
                let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jxl");
                std::fs::create_dir_all(&dir).unwrap();
                crate::files::save_project(&opened.document, &dir.join("float32.rphoto")).unwrap();
                let restored = crate::files::load_project(&dir.join("float32.rphoto")).unwrap();
                assert_eq!(restored.active_layer().unwrap().precision, layer.precision);
            }
            assert_eq!(std::fs::read(input).unwrap(), original);
        }
        std::fs::write(
            output_root.join("measurements.json"),
            serde_json::to_vec_pretty(&measurements).unwrap(),
        )
        .unwrap();
    }
}
