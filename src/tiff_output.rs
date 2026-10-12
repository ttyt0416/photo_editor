use crate::{document::Document, files::ExportOptions};
use std::{
    io::{Seek, Write},
    path::Path,
};
use tiff::encoder::{
    Compression, DeflateLevel, Predictor, Rational, TiffEncoder, TiffKind, TiffValue,
    colortype::{ColorType, RGBA8, RGBA16, RGBA32Float},
};
use tiff::tags::{ExtraSamples, PhotometricInterpretation, ResolutionUnit, SampleFormat, Tag};

macro_rules! gray_alpha_integer {
    ($name:ident,$sample:ty,$bits:expr) => {
        struct $name;
        impl ColorType for $name {
            type Inner = $sample;
            const TIFF_VALUE: PhotometricInterpretation = PhotometricInterpretation::BlackIsZero;
            const BITS_PER_SAMPLE: &'static [u16] = &[$bits; 2];
            const SAMPLE_FORMAT: &'static [SampleFormat] = &[SampleFormat::Uint; 2];
            fn horizontal_predict(row: &[$sample], result: &mut Vec<$sample>) {
                result.extend_from_slice(&row[..2]);
                result.extend(
                    row[2..]
                        .iter()
                        .zip(row)
                        .map(|(value, previous)| value.wrapping_sub(*previous)),
                );
            }
        }
    };
}
gray_alpha_integer!(GrayAlpha8, u8, 8);
gray_alpha_integer!(GrayAlpha16, u16, 16);
struct GrayAlpha32;
impl ColorType for GrayAlpha32 {
    type Inner = f32;
    const TIFF_VALUE: PhotometricInterpretation = PhotometricInterpretation::BlackIsZero;
    const BITS_PER_SAMPLE: &'static [u16] = &[32; 2];
    const SAMPLE_FORMAT: &'static [SampleFormat] = &[SampleFormat::IEEEFP; 2];
    fn horizontal_predict(_: &[f32], _: &mut Vec<f32>) {
        unreachable!()
    }
}
pub const COMPRESSION_LABELS: &[&str] = &[
    "없음",
    "LZW",
    "Deflate 빠르게",
    "Deflate 표준",
    "Deflate 최대",
    "PackBits",
];
pub const DEPTH_LABELS: &[&str] = &["자동 (8/16/32비트)", "8비트", "16비트", "32비트 부동소수점"];
pub fn depth(doc: &Document, options: &ExportOptions) -> u8 {
    match options.tiff_depth {
        1 => 8,
        2 => 16,
        3 => 32,
        _ => {
            if doc.layers.iter().any(|l| l.bit_depth == 32) {
                32
            } else if doc.layers.iter().any(|l| l.bit_depth > 8) {
                16
            } else {
                8
            }
        }
    }
}
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
fn configure<W: Write + Seek, K: TiffKind>(
    encoder: TiffEncoder<W, K>,
    options: &ExportOptions,
) -> TiffEncoder<W, K> {
    let compression = match options.tiff_compression {
        1 => Compression::Lzw,
        2 => Compression::Deflate(DeflateLevel::Fast),
        3 => Compression::Deflate(DeflateLevel::Balanced),
        4 => Compression::Deflate(DeflateLevel::Best),
        5 => Compression::Packbits,
        _ => Compression::Uncompressed,
    };
    encoder
        .with_compression(compression)
        .with_predictor(if options.tiff_float_predictor {
            Predictor::FloatingPoint
        } else if options.tiff_predictor {
            Predictor::Horizontal
        } else {
            Predictor::None
        })
}
fn image<W: Write + Seek, K: TiffKind, C: ColorType>(
    encoder: &mut TiffEncoder<W, K>,
    size: [u32; 2],
    data: &[C::Inner],
    options: &ExportOptions,
    xmp: Option<&[u8]>,
) -> Result<(), String>
where
    [C::Inner]: TiffValue,
{
    let mut image = encoder.new_image::<C>(size[0], size[1]).map_err(err)?;
    // Each ColorType already includes alpha; declare it without adding a sample.
    image
        .encoder()
        .write_tag(
            Tag::ExtraSamples,
            &[ExtraSamples::UnassociatedAlpha.to_u16()][..],
        )
        .map_err(err)?;
    image
        .encoder()
        .write_tag(Tag::ResolutionUnit, ResolutionUnit::Inch.to_u16())
        .map_err(err)?;
    for tag in [Tag::XResolution, Tag::YResolution] {
        image
            .encoder()
            .write_tag(
                tag,
                Rational {
                    n: options.tiff_resolution,
                    d: 1,
                },
            )
            .map_err(err)?;
    }
    image
        .encoder()
        .write_tag(Tag::Software, "Rust Photo Editor")
        .map_err(err)?;
    if options.include_icc {
        image
            .encoder()
            .write_tag(
                Tag::IccProfile,
                if options.tiff_grayscale {
                    crate::color_management::gray_profile()
                } else {
                    crate::color_management::srgb_profile()
                },
            )
            .map_err(err)?;
    }
    if let Some(xmp) = xmp {
        image
            .encoder()
            .write_tag(Tag::Unknown(700), xmp)
            .map_err(err)?;
    }
    image.write_data(data).map_err(err)
}
fn encode<W: Write + Seek, K: TiffKind>(
    mut encoder: TiffEncoder<W, K>,
    doc: &Document,
    options: &ExportOptions,
) -> Result<(), String> {
    encoder = configure(encoder, options);
    let size = [doc.width, doc.height];
    let xmp = crate::xmp_output::prepare(doc, options.include_xmp)?;
    if options.tiff_grayscale {
        let pixels = crate::precision::composite(doc);
        if pixels.as_raw().iter().any(|v| !v.is_finite()) {
            return Err("TIFF 출력에 비정상 부동소수점 값이 있습니다.".into());
        }
        // Keep neutral float samples exactly. Colored inputs use the existing
        // working RGB luma policy; compute before limiting integer output.
        let values: Vec<f32> = pixels
            .pixels()
            .flat_map(|p| {
                let gray = if p[0] == p[1] && p[1] == p[2] {
                    p[0]
                } else {
                    (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]))
                        as f32
                };
                [gray, p[3]]
            })
            .collect();
        return match depth(doc, options) {
            8 => image::<_, _, GrayAlpha8>(
                &mut encoder,
                size,
                &values
                    .iter()
                    .map(|v| (f64::from(*v).clamp(0., 1.) * 255.).round() as u8)
                    .collect::<Vec<_>>(),
                options,
                xmp.as_deref(),
            ),
            16 => image::<_, _, GrayAlpha16>(
                &mut encoder,
                size,
                &values
                    .iter()
                    .map(|v| (f64::from(*v).clamp(0., 1.) * 65535.).round() as u16)
                    .collect::<Vec<_>>(),
                options,
                xmp.as_deref(),
            ),
            _ => image::<_, _, GrayAlpha32>(&mut encoder, size, &values, options, xmp.as_deref()),
        };
    }
    match depth(doc, options) {
        8 => image::<_, _, RGBA8>(
            &mut encoder,
            size,
            doc.composite().as_raw(),
            options,
            xmp.as_deref(),
        ),
        16 => image::<_, _, RGBA16>(
            &mut encoder,
            size,
            crate::precision::image16(&crate::precision::composite(doc)).as_raw(),
            options,
            xmp.as_deref(),
        ),
        _ => {
            let pixels = crate::precision::composite(doc);
            if pixels.as_raw().iter().any(|v| !v.is_finite()) {
                return Err("TIFF 출력에 비정상 부동소수점 값이 있습니다.".into());
            }
            image::<_, _, RGBA32Float>(&mut encoder, size, pixels.as_raw(), options, xmp.as_deref())
        }
    }
}
pub fn save(doc: &Document, path: &Path, options: &ExportOptions) -> Result<(), String> {
    if options.tiff_compression >= COMPRESSION_LABELS.len()
        || options.tiff_depth >= DEPTH_LABELS.len()
        || !(1..=9600).contains(&options.tiff_resolution)
    {
        return Err("TIFF 압축·정밀도·해상도 설정을 확인하세요.".into());
    }
    if (options.tiff_predictor || options.tiff_float_predictor)
        && !(1..=4).contains(&options.tiff_compression)
    {
        return Err("TIFF 예측은 LZW 또는 Deflate 압축에서 사용합니다.".into());
    }
    if options.tiff_float_predictor && (depth(doc, options) != 32 || options.tiff_predictor) {
        return Err("부동소수점 예측은32비트 TIFF에서 단독으로 사용합니다.".into());
    }
    if depth(doc, options) == 32 && options.tiff_predictor {
        return Err("32비트 TIFF에서는 수평 예측을 사용할 수 없습니다.".into());
    }
    crate::files::atomic_write(path, |file| {
        if options.tiff_big {
            encode(TiffEncoder::new_big(file).map_err(err)?, doc, options)
        } else {
            encode(TiffEncoder::new(file).map_err(err)?, doc, options)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::precision;
    #[test]
    fn grayscale_tiff_uint_float_alpha_hdr_all_compressions_profiles_projects() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/tiff-gray-output");
        std::fs::create_dir_all(&root).unwrap();
        let mut cases = vec![];
        for neutral in [false, true] {
            let pixels = image::Rgba32FImage::from_fn(17, 3, |x, y| {
                let value = if x == 3 {
                    f32::MAX
                } else if x == 4 {
                    -f32::MAX
                } else if x == 8 {
                    -0.0
                } else {
                    (x as f32 - 8.) / 4.
                };
                image::Rgba([
                    value,
                    if neutral { value } else { y as f32 / 2. },
                    if neutral { value } else { x as f32 / 8. },
                    if x == 0 {
                        0.
                    } else if x == 1 {
                        0.125
                    } else if x == 2 {
                        0.33333334
                    } else {
                        1.
                    },
                ])
            });
            let doc = precision::document(
                format!("gray{neutral}"),
                image::DynamicImage::ImageRgba32F(pixels),
            )
            .unwrap();
            let source = doc.layers[0].precision.clone();
            let composite = precision::composite(&doc);
            std::fs::write(
                root.join(format!("source{neutral}.f32")),
                composite
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            for bits in [8, 16, 32] {
                for compression in 0..COMPRESSION_LABELS.len() {
                    for big in [false, true] {
                        for predictor in [false, true] {
                            if predictor && !(1..=4).contains(&compression) {
                                continue;
                            }
                            for icc in [false, true] {
                                let name = format!(
                                    "n{neutral}-d{bits}-c{compression}-big{big}-p{predictor}-icc{icc}.tiff"
                                );
                                let options = ExportOptions {
                                    tiff_grayscale: true,
                                    tiff_depth: if bits == 8 {
                                        1
                                    } else if bits == 16 {
                                        2
                                    } else {
                                        3
                                    },
                                    tiff_compression: compression,
                                    tiff_big: big,
                                    tiff_float_predictor: predictor && bits == 32,
                                    tiff_predictor: predictor && bits != 32,
                                    include_icc: icc,
                                    ..Default::default()
                                };
                                crate::files::export(
                                    &doc,
                                    &root.join(&name),
                                    image::ImageFormat::Tiff,
                                    &options,
                                )
                                .unwrap();
                                let imported =
                                    crate::files::open(&root.join(&name)).unwrap().document;
                                let actual = precision::composite(&imported);
                                assert!(actual.pixels().all(|p| p[0].to_bits() == p[1].to_bits()
                                    && p[0].to_bits() == p[2].to_bits()));
                                assert_eq!(imported.source_icc.is_some(), icc);
                                if icc {
                                    assert_eq!(
                                        imported.source_icc.as_deref().unwrap().as_slice(),
                                        crate::color_management::gray_profile()
                                    );
                                }
                                if bits == 32 && neutral {
                                    assert_eq!(
                                        actual
                                            .as_raw()
                                            .iter()
                                            .map(|v| v.to_bits())
                                            .collect::<Vec<_>>(),
                                        composite
                                            .as_raw()
                                            .iter()
                                            .map(|v| v.to_bits())
                                            .collect::<Vec<_>>()
                                    );
                                }
                                let project = root.join(format!("{name}.rphoto"));
                                crate::files::save_project(&imported, &project).unwrap();
                                assert_eq!(
                                    precision::composite(
                                        &crate::files::load_project(&project).unwrap()
                                    ),
                                    actual
                                );
                                cases.push(serde_json::json!({"file":name,"neutral":neutral,"bits":bits,"compression":compression,"big":big,"predictor":predictor,"icc":icc}));
                            }
                        }
                    }
                }
            }
            assert_eq!(doc.layers[0].precision, source);
            assert_eq!(precision::composite(&doc), composite);
        }
        assert_eq!(cases.len(), 240);
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec_pretty(&cases).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn automatic_tiff_preserves_native_eight_sixteen_float_hdr_and_mixed_precision() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/tiff-auto");
        std::fs::create_dir_all(&root).unwrap();
        let mut cases = vec![];
        for bits in [8, 16, 32] {
            let source = image::Rgba32FImage::from_fn(17, 3, |x, y| {
                image::Rgba([
                    if bits == 32 {
                        (x as f32 - 8.) / 4.
                    } else {
                        x as f32 / 16.
                    },
                    if bits == 32 {
                        y as f32 + 2.5
                    } else {
                        y as f32 / 2.
                    },
                    x as f32 / 24.,
                    if x == 0 {
                        0.
                    } else if x == 1 {
                        0.125
                    } else {
                        1.
                    },
                ])
            });
            let input = match bits {
                8 => image::DynamicImage::ImageRgba8(precision::preview(&source)),
                16 => image::DynamicImage::ImageRgba16(precision::image16(&source)),
                _ => image::DynamicImage::ImageRgba32F(source),
            };
            let doc = precision::document(format!("native{bits}"), input).unwrap();
            let pixels = precision::composite(&doc);
            std::fs::write(
                root.join(format!("source{bits}.f32")),
                pixels
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            for compression in [0, 1, 3, 5] {
                for big in [false, true] {
                    let name = format!("auto{bits}-c{compression}-big{big}.tiff");
                    let options = ExportOptions {
                        tiff_compression: compression,
                        tiff_big: big,
                        tiff_float_predictor: bits == 32 && (1..=4).contains(&compression),
                        tiff_predictor: bits != 32 && (1..=4).contains(&compression),
                        ..Default::default()
                    };
                    // Auto must choose the precision needed before validating prediction.
                    crate::files::export(
                        &doc,
                        &root.join(&name),
                        image::ImageFormat::Tiff,
                        &options,
                    )
                    .unwrap();
                    let restored = crate::files::open(&root.join(&name)).unwrap().document;
                    assert_eq!(restored.layers[0].bit_depth, bits, "{name}");
                    let actual = precision::composite(&restored);
                    if bits == 32 {
                        assert_eq!(
                            actual
                                .as_raw()
                                .iter()
                                .map(|v| v.to_bits())
                                .collect::<Vec<_>>(),
                            pixels
                                .as_raw()
                                .iter()
                                .map(|v| v.to_bits())
                                .collect::<Vec<_>>()
                        );
                    } else {
                        assert!(
                            actual
                                .as_raw()
                                .iter()
                                .zip(pixels.as_raw())
                                .all(|(a, b)| (*a - *b).abs() <= 1. / ((1u32 << bits) - 1) as f32)
                        );
                    }
                    cases.push(serde_json::json!({"file":name,"bits":bits,"compression":compression,"big":big}));
                }
            }
        }
        let mut mixed = Document::new(3, 2, true).unwrap();
        mixed.layers[0].bit_depth = 32;
        mixed.layers[0].precision = Some(std::sync::Arc::new(image::Rgba32FImage::from_pixel(
            3,
            2,
            image::Rgba([2., -0.25, 0.5, 1.]),
        )));
        mixed.layers[0].visible = false;
        crate::files::export(
            &mixed,
            &root.join("hidden32.tiff"),
            image::ImageFormat::Tiff,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(
            crate::files::open(&root.join("hidden32.tiff"))
                .unwrap()
                .document
                .layers[0]
                .bit_depth,
            32
        );
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec_pretty(&cases).unwrap(),
        )
        .unwrap();
    }
    fn document() -> Document {
        precision::document(
            "TIFF HDR".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_fn(16, 8, |x, y| {
                image::Rgba([
                    2.25 + x as f32 / 19.,
                    -0.125 + y as f32 / 37.,
                    0.1234567 + y as f32 / 43.,
                    if x == 0 {
                        0.
                    } else if x == 1 {
                        0.5
                    } else {
                        1.
                    },
                ])
            })),
        )
        .unwrap()
    }
    #[test]
    fn six_compressions_classic_big_integer_float_predictor_alpha_hdr_and_resolution_roundtrip() {
        let doc = document();
        let expected = precision::composite(&doc);
        let original = doc.layers[0].precision.clone().unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/tiff-output");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("source32.le"),
            expected
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut count = 0;
        for bits in [8, 16, 32] {
            for compression in 0..COMPRESSION_LABELS.len() {
                for big in [false, true] {
                    for predictor in [false, true] {
                        if bits == 32 && predictor {
                            continue;
                        }
                        let options = ExportOptions {
                            tiff_depth: match bits {
                                8 => 1,
                                16 => 2,
                                _ => 3,
                            },
                            tiff_compression: compression,
                            tiff_big: big,
                            tiff_predictor: predictor,
                            tiff_resolution: 300,
                            ..Default::default()
                        };
                        if predictor && !(1..=4).contains(&compression) {
                            let invalid = dir.join("invalid-predictor.tiff");
                            std::fs::write(&invalid, b"previous").unwrap();
                            assert!(save(&doc, &invalid, &options).is_err());
                            assert_eq!(std::fs::read(&invalid).unwrap(), b"previous");
                            continue;
                        }
                        let path = dir.join(format!(
                            "d{bits}-c{compression}-p{}-b{}.tiff",
                            u8::from(predictor),
                            u8::from(big)
                        ));
                        crate::files::export(&doc, &path, image::ImageFormat::Tiff, &options)
                            .unwrap();
                        let bytes = std::fs::read(&path).unwrap();
                        assert_eq!(
                            u16::from_le_bytes(bytes[2..4].try_into().unwrap()),
                            if big { 43 } else { 42 }
                        );
                        let mut decoder =
                            tiff::decoder::Decoder::new(std::fs::File::open(&path).unwrap())
                                .unwrap();
                        assert_eq!(
                            decoder.get_tag_u16_vec(Tag::BitsPerSample).unwrap(),
                            vec![bits as u16; 4]
                        );
                        assert_eq!(
                            decoder.get_tag_u32(Tag::Compression).unwrap(),
                            [1, 5, 8, 8, 8, 32773][compression]
                        );
                        assert_eq!(
                            decoder.get_tag_u32(Tag::Predictor).unwrap(),
                            if predictor { 2 } else { 1 }
                        );
                        assert_eq!(decoder.get_tag_u16_vec(Tag::ExtraSamples).unwrap(), vec![2]);
                        assert_eq!(decoder.get_tag_u32(Tag::ResolutionUnit).unwrap(), 2);
                        assert_eq!(
                            decoder.get_tag_u32_vec(Tag::XResolution).unwrap(),
                            vec![300, 1]
                        );
                        assert_eq!(
                            decoder.get_tag_u32_vec(Tag::YResolution).unwrap(),
                            vec![300, 1]
                        );
                        let restored = crate::files::open(&path).unwrap().document;
                        assert_eq!(restored.layers[0].bit_depth, bits);
                        let actual = precision::composite(&restored);
                        for (a, b) in actual.pixels().zip(expected.pixels()) {
                            for c in 0..4 {
                                let expected = if bits == 32 { b[c] } else { b[c].clamp(0., 1.) };
                                assert!(
                                    (a[c] - expected).abs()
                                        <= if bits == 8 {
                                            1. / 255.
                                        } else if bits == 16 {
                                            1. / 65535.
                                        } else {
                                            0.000001
                                        },
                                    "{bits} compression {compression} b{big} pred{predictor}: {a:?} {b:?}"
                                );
                            }
                        }
                        count += 1;
                    }
                }
            }
        }
        assert_eq!(count, 52);
        assert!(std::sync::Arc::ptr_eq(
            &original,
            doc.layers[0].precision.as_ref().unwrap()
        ));
        assert_eq!(precision::composite(&doc), expected);
        let path = dir.join("automatic.tiff");
        crate::files::export(
            &doc,
            &path,
            image::ImageFormat::Tiff,
            &ExportOptions::default(),
        )
        .unwrap();
        assert_eq!(
            crate::files::open(&path).unwrap().document.layers[0].bit_depth,
            32
        );
    }
    #[test]
    fn invalid_tiff_options_preserve_existing_output_and_leave_no_temporary_files() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/tiff-invalid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("existing.tiff");
        std::fs::write(&path, b"previous").unwrap();
        let doc = document();
        for options in [
            ExportOptions {
                tiff_compression: 6,
                ..Default::default()
            },
            ExportOptions {
                tiff_depth: 4,
                ..Default::default()
            },
            ExportOptions {
                tiff_resolution: 0,
                ..Default::default()
            },
            ExportOptions {
                tiff_resolution: 9601,
                ..Default::default()
            },
            ExportOptions {
                tiff_float_predictor: true,
                tiff_compression: 1,
                tiff_depth: 2,
                ..Default::default()
            },
            ExportOptions {
                tiff_float_predictor: true,
                tiff_compression: 0,
                tiff_depth: 3,
                ..Default::default()
            },
            ExportOptions {
                tiff_float_predictor: true,
                tiff_compression: 5,
                tiff_depth: 3,
                ..Default::default()
            },
            ExportOptions {
                tiff_depth: 3,
                tiff_predictor: true,
                ..Default::default()
            },
        ] {
            assert!(crate::files::export(&doc, &path, image::ImageFormat::Tiff, &options).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"previous");
        }
        assert!(!std::fs::read_dir(dir).unwrap().any(|p| {
            p.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".photo-editor-")
        }));
    }
    #[test]
    fn floating_point_predictor_classic_big_compression_strips_keep_exact_hdr_negative_alpha_and_source()
     {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/tiff-predictor");
        std::fs::create_dir_all(&dir).unwrap();
        let mut manifest = serde_json::Map::new();
        for (w, h) in [(1, 2), (16, 8), (317, 257)] {
            let doc = precision::document(
                "TIFF float prediction".into(),
                image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_fn(w, h, |x, y| {
                    image::Rgba([
                        1.25 + x as f32 / 19.,
                        -0.125 + y as f32 / 37.,
                        -0.0,
                        if w == 1 {
                            0.75
                        } else if x == 0 {
                            0.
                        } else if x == 1 {
                            0.5
                        } else {
                            1.
                        },
                    ])
                })),
            )
            .unwrap();
            let source = doc.active_layer().unwrap().precision.clone().unwrap();
            let expected = precision::composite(&doc);
            let source_name = format!("source-{w}x{h}.le");
            std::fs::write(
                dir.join(&source_name),
                expected
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            for compression in 1..=4 {
                for big in [false, true] {
                    let options = ExportOptions {
                        tiff_depth: 3,
                        tiff_compression: compression,
                        tiff_float_predictor: true,
                        tiff_big: big,
                        ..Default::default()
                    };
                    let name = format!("{w}x{h}-c{compression}-b{}.tiff", u8::from(big));
                    if w == 317 {
                        let plain = ExportOptions {
                            tiff_float_predictor: false,
                            ..options.clone()
                        };
                        save(
                            &doc,
                            &dir.join(name.replace(".tiff", "-plain.tiff")),
                            &plain,
                        )
                        .unwrap();
                    }
                    let path = dir.join(&name);
                    save(&doc, &path, &options).unwrap();
                    let mut decoder =
                        tiff::decoder::Decoder::new(std::fs::File::open(&path).unwrap()).unwrap();
                    assert_eq!(decoder.get_tag_u32(Tag::Predictor).unwrap(), 3);
                    assert_eq!(decoder.get_tag_u16_vec(Tag::ExtraSamples).unwrap(), [2]);
                    let offsets = decoder.get_tag_u64_vec(Tag::StripOffsets).unwrap();
                    if w == 317 {
                        assert!(offsets.len() > 1);
                    }
                    let actual = match decoder.read_image().unwrap() {
                        tiff::decoder::DecodingResult::F32(data) => data,
                        _ => panic!("wrong TIFF precision"),
                    };
                    assert_eq!(
                        actual.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                        expected
                            .as_raw()
                            .iter()
                            .map(|v| v.to_bits())
                            .collect::<Vec<_>>()
                    );
                    let opened = crate::files::open(&path).unwrap();
                    assert_eq!(opened.document.active_layer().unwrap().bit_depth, 32);
                    assert_eq!(
                        opened
                            .document
                            .active_layer()
                            .unwrap()
                            .precision
                            .as_deref()
                            .unwrap(),
                        &expected
                    );
                    manifest.insert(name,serde_json::json!({"width":w,"height":h,"compression":compression,"big":big,"source":source_name,"strips":offsets.len()}));
                }
            }
            assert!(std::sync::Arc::ptr_eq(
                &source,
                doc.active_layer().unwrap().precision.as_ref().unwrap()
            ));
            assert_eq!(
                doc.active_layer().unwrap().precision.as_deref().unwrap(),
                source.as_ref()
            );
        }
        std::fs::write(
            dir.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn floating_predictor_generic_half_double_and_gray_strips_preserve_ieee_bits() {
        use std::io::Cursor;
        use tiff::tags::{PhotometricInterpretation, SampleFormat};
        struct GrayHalf;
        impl ColorType for GrayHalf {
            type Inner = u16;
            const TIFF_VALUE: PhotometricInterpretation = PhotometricInterpretation::BlackIsZero;
            const BITS_PER_SAMPLE: &'static [u16] = &[16];
            const SAMPLE_FORMAT: &'static [SampleFormat] = &[SampleFormat::IEEEFP];
            fn horizontal_predict(_: &[u16], _: &mut Vec<u16>) {
                unreachable!()
            }
        }
        let mut output = Cursor::new(Vec::<u8>::new());
        let half = [
            0x3c00_u16, 0xbc00, 0, 0x8000, 0x7bff, 0x0001, 0x3555, 0x7e34,
        ];
        {
            let mut encoder = TiffEncoder::new(&mut output)
                .unwrap()
                .with_compression(Compression::Lzw)
                .with_predictor(Predictor::FloatingPoint);
            let mut image = encoder.new_image::<GrayHalf>(2, 4).unwrap();
            image.rows_per_strip(2).unwrap();
            image.write_data(&half).unwrap();
        }
        let mut decoder = tiff::decoder::Decoder::new(Cursor::new(output.into_inner())).unwrap();
        match decoder.read_image().unwrap() {
            tiff::decoder::DecodingResult::F16(data) => {
                assert_eq!(data.iter().map(|v| v.to_bits()).collect::<Vec<_>>(), half)
            }
            _ => panic!("half precision changed"),
        }
        let mut output = Cursor::new(Vec::<u8>::new());
        let double = [
            -0.0_f64,
            -2.,
            1e200,
            0.1234567890123456,
            1e-200,
            f64::from_bits(0x7ff8000000001234),
            1.,
            2.,
            3.,
        ];
        {
            let mut encoder = TiffEncoder::new(&mut output)
                .unwrap()
                .with_compression(Compression::Deflate(DeflateLevel::Balanced))
                .with_predictor(Predictor::FloatingPoint);
            let mut image = encoder
                .new_image::<tiff::encoder::colortype::Gray64Float>(3, 3)
                .unwrap();
            image.rows_per_strip(2).unwrap();
            image.write_data(&double).unwrap();
        }
        let mut decoder = tiff::decoder::Decoder::new(Cursor::new(output.into_inner())).unwrap();
        match decoder.read_image().unwrap() {
            tiff::decoder::DecodingResult::F64(data) => assert_eq!(
                data.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                double.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
            ),
            _ => panic!("double precision changed"),
        }
    }
}
