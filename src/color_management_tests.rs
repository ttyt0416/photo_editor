use super::*;
use image::{ImageBuffer, ImageEncoder};
use std::path::Path;
fn root() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/icc-input");
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn rgba() -> image::RgbaImage {
    ImageBuffer::from_fn(17, 9, |x, y| {
        let i = x + y * 17;
        image::Rgba([
            (i * 37 % 256) as u8,
            (i * 71 % 256) as u8,
            (i * 113 % 256) as u8,
            if i.is_multiple_of(7) {
                0
            } else if i % 7 == 1 {
                255
            } else {
                (i * 19 % 256) as u8
            },
        ])
    })
}
fn pixel16(x: u32, y: u32) -> image::Rgba<u16> {
    let i = x + y * 17;
    image::Rgba([
        (i * 7193 % 65536) as u16,
        (i * 11317 % 65536) as u16,
        (i * 17293 % 65536) as u16,
        if i.is_multiple_of(7) {
            0
        } else if i % 7 == 1 {
            65535
        } else {
            (i * 8879 % 65536) as u16
        },
    ])
}
fn rgba16() -> ImageBuffer<image::Rgba<u16>, Vec<u16>> {
    ImageBuffer::from_fn(17, 9, pixel16)
}
fn write_float(path: &Path, pixels: &image::Rgba32FImage) {
    std::fs::write(
        path,
        pixels
            .as_raw()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
#[test]
fn icc_rgb_gray_eight_sixteen_float_alpha_projects_and_independent_outputs() {
    let root = root();
    let mut cases = vec![];
    for (name, profile) in [
        ("srgb", ColorProfile::new_srgb()),
        ("p3", ColorProfile::new_display_p3()),
        ("adobe", ColorProfile::new_adobe_rgb()),
        ("gray1", ColorProfile::new_gray_with_gamma(1.)),
        ("gray18", ColorProfile::new_gray_with_gamma(1.8)),
        ("gray22", ColorProfile::new_gray_with_gamma(2.2)),
    ] {
        let icc = profile.encode().unwrap();
        std::fs::write(root.join(format!("{name}.icc")), &icc).unwrap();
        let gray = profile.color_space == DataColorSpace::Gray;
        for mode in 0..if gray { 4 } else { 6 } {
            let b = rgba();
            let image = if gray {
                let l8 = ImageBuffer::from_fn(17, 9, |x, y| image::Luma([b.get_pixel(x, y)[0]]));
                let la8 = ImageBuffer::from_fn(17, 9, |x, y| {
                    image::LumaA([b.get_pixel(x, y)[0], b.get_pixel(x, y)[3]])
                });
                match mode {
                    0 => DynamicImage::ImageLuma8(l8),
                    1 => DynamicImage::ImageLumaA8(la8),
                    2 => DynamicImage::ImageLuma16(ImageBuffer::from_fn(17, 9, |x, y| {
                        image::Luma([pixel16(x, y)[0]])
                    })),
                    _ => DynamicImage::ImageLumaA16(ImageBuffer::from_fn(17, 9, |x, y| {
                        image::LumaA([pixel16(x, y)[0], pixel16(x, y)[3]])
                    })),
                }
            } else {
                let original = DynamicImage::ImageRgba8(b);
                match mode {
                    0 => DynamicImage::ImageRgb8(original.to_rgb8()),
                    1 => original,
                    2 => DynamicImage::ImageRgb16(DynamicImage::ImageRgba16(rgba16()).to_rgb16()),
                    3 => DynamicImage::ImageRgba16(rgba16()),
                    4 => DynamicImage::ImageRgb32F(original.to_rgb32f()),
                    _ => DynamicImage::ImageRgba32F(original.to_rgba32f()),
                }
            };
            let before = image.to_rgba32f();
            let alpha_integer = image.to_rgba16();
            let mut notes = vec![];
            let output = apply(image, &icc, &mut notes);
            assert_eq!(notes.len(), 1);
            assert!(notes[0].contains("적용해"), "{name}/{mode}: {notes:?}");
            let actual = output.to_rgba32f();
            if mode >= 4 {
                for (s, d) in before.pixels().zip(actual.pixels()) {
                    assert_eq!(s[3].to_bits(), d[3].to_bits());
                }
            } else {
                for (s, d) in alpha_integer.pixels().zip(output.to_rgba16().pixels()) {
                    assert_eq!(s[3], d[3]);
                }
            }
            let id = format!("{name}-{mode}");
            write_float(&root.join(format!("{id}.src.f32")), &before);
            write_float(&root.join(format!("{id}.dst.f32")), &actual);
            let doc = crate::precision::document(id.clone(), output).unwrap();
            let depth = doc.layers[0].bit_depth;
            assert_eq!(
                depth,
                if mode >= 4 {
                    32
                } else if mode >= 2 {
                    16
                } else {
                    8
                }
            );
            let expected = crate::precision::composite(&doc);
            let project = root.join(format!("{id}.rphoto"));
            crate::files::save_project(&doc, &project).unwrap();
            assert_eq!(
                crate::precision::composite(&crate::files::load_project(&project).unwrap()),
                expected
            );
            cases.push(serde_json::json!({"id":id,"profile":name,"gray":gray,"mode":mode,"depth":depth,"size":[17,9]}));
        }
    }
    assert_eq!(cases.len(), 30);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn icc_bad_profile_channels_hdr_nonfinite_keep_original_pixels_and_note() {
    let rgb = DynamicImage::ImageRgba8(rgba());
    let gray = ColorProfile::new_gray_with_gamma(2.2).encode().unwrap();
    for profile in [
        vec![],
        vec![0; 127],
        vec![0; 256],
        gray,
        vec![0; 16 * 1024 * 1024 + 1],
    ] {
        let mut notes = vec![];
        let output = apply(rgb.clone(), &profile, &mut notes);
        assert_eq!(output, rgb);
        assert!(notes[0].contains("못했습니다"));
    }
    let icc = ColorProfile::new_display_p3().encode().unwrap();
    for v in [-1., 2., f32::MAX, f32::INFINITY, f32::NAN] {
        let source = image::Rgba32FImage::from_pixel(2, 2, image::Rgba([v, 0.25, 0.5, 0.123]));
        let before = source
            .as_raw()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>();
        let mut notes = vec![];
        let output = apply(DynamicImage::ImageRgba32F(source), &icc, &mut notes).to_rgba32f();
        assert_eq!(
            output
                .as_raw()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            before
        );
        assert!(notes[0].contains("못했습니다"));
    }
}
#[test]
fn icc_png_jpeg_decoder_conversion_and_original_preservation() {
    let root = root().join("files");
    std::fs::create_dir_all(&root).unwrap();
    let profile = ColorProfile::new_display_p3().encode().unwrap();
    let source = DynamicImage::ImageRgba8(rgba());
    for depth in [8, 16] {
        let path = root.join(format!("p3-{depth}.png"));
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = image::codecs::png::PngEncoder::new(file);
        encoder.set_icc_profile(profile.clone()).unwrap();
        let input = if depth == 8 {
            source.clone()
        } else {
            DynamicImage::ImageRgba16(source.to_rgba16())
        };
        input.clone().write_with_encoder(encoder).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let expected =
            crate::precision::document("reference".into(), apply(input, &profile, &mut vec![]))
                .unwrap();
        let opened = crate::files::open(&path).unwrap();
        assert!(
            opened.notes.iter().any(|n| n.contains("적용해")),
            "{:?}",
            opened.notes
        );
        assert_eq!(
            crate::precision::composite(&opened.document),
            crate::precision::composite(&expected)
        );
        assert_eq!(opened.document.layers[0].bit_depth, depth);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    let rgb = source.to_rgb8();
    let path = root.join("p3.jpg");
    let mut encoded = vec![];
    let mut encoder = jpeg_encoder::Encoder::new(&mut encoded, 95);
    encoder.add_icc_profile(&profile).unwrap();
    encoder
        .encode(rgb.as_raw(), 17, 9, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    std::fs::write(&path, &encoded).unwrap();
    let (pixels, notes) = crate::jpeg_output::read_bytes(&encoded).unwrap();
    assert!(notes[0].contains("적용해"));
    let opened = crate::files::open(&path).unwrap();
    assert!(opened.notes.starts_with(&notes));
    assert!(opened.notes.iter().any(|n| n.contains("프로젝트에 보존")));
    assert_eq!(
        opened.document.source_icc.as_deref().unwrap().as_slice(),
        profile
    );
    let expected = crate::precision::document("jpeg".into(), pixels).unwrap();
    assert_eq!(
        crate::precision::composite(&opened.document),
        crate::precision::composite(&expected)
    );
    assert_eq!(std::fs::read(&path).unwrap(), encoded);
}

#[test]
fn icc_tiff_gray_and_rgb_import_svg_display_and_original_bytes() {
    use base64::Engine;
    let root = root().join("files");
    std::fs::create_dir_all(&root).unwrap();
    let p3 = ColorProfile::new_display_p3().encode().unwrap();
    let gray = ColorProfile::new_gray_with_gamma(1.8).encode().unwrap();
    for is_gray in [false, true] {
        let path = root.join(format!("icc-gray{is_gray}.tiff"));
        let source = if is_gray {
            DynamicImage::ImageLuma16(ImageBuffer::from_fn(17, 9, |x, y| {
                image::Luma([pixel16(x, y)[0]])
            }))
        } else {
            DynamicImage::ImageRgba16(rgba16())
        };
        let profile = if is_gray { &gray } else { &p3 };
        let file = std::fs::File::create(&path).unwrap();
        let mut encoder = tiff::encoder::TiffEncoder::new(file).unwrap();
        if is_gray {
            let mut image = encoder
                .new_image::<tiff::encoder::colortype::Gray16>(17, 9)
                .unwrap();
            image
                .encoder()
                .write_tag(tiff::tags::Tag::IccProfile, profile.as_slice())
                .unwrap();
            image.write_data(source.to_luma16().as_raw()).unwrap();
        } else {
            let mut image = encoder
                .new_image::<tiff::encoder::colortype::RGBA16>(17, 9)
                .unwrap();
            image
                .encoder()
                .write_tag(tiff::tags::Tag::IccProfile, profile.as_slice())
                .unwrap();
            image.write_data(source.to_rgba16().as_raw()).unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let mut direct = tiff::decoder::Decoder::new(std::io::BufReader::new(
            std::fs::File::open(&path).unwrap(),
        ))
        .unwrap();
        assert_eq!(
            direct.get_tag_u8_vec(tiff::tags::Tag::IccProfile).unwrap(),
            *profile
        );
        let opened = crate::files::open(&path).unwrap();
        assert!(
            opened.notes.iter().any(|n| n.contains("적용해")),
            "{:?}",
            opened.notes
        );
        let expected =
            crate::precision::document("ICC".into(), apply(source, profile, &mut vec![])).unwrap();
        assert_eq!(
            crate::precision::composite(&opened.document),
            crate::precision::composite(&expected)
        );
        assert_eq!(opened.document.layers[0].bit_depth, 16);
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    let source = DynamicImage::ImageRgba8(rgba());
    let mut png = vec![];
    let mut encoder = image::codecs::png::PngEncoder::new(&mut png);
    encoder.set_icc_profile(p3.clone()).unwrap();
    source.clone().write_with_encoder(encoder).unwrap();
    let encoded = base64::engine::general_purpose::STANDARD.encode(&png);
    let svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"17\" height=\"9\"><image width=\"17\" height=\"9\" href=\"data:image/png;base64,{encoded}\"/></svg>"
    );
    let original =
        crate::svg_assets::image_file(&svg, 1, crate::svg_assets::ImageFile::Original).unwrap();
    assert_eq!(original.bytes, png);
    let display =
        crate::svg_assets::image_file(&svg, 1, crate::svg_assets::ImageFile::Display).unwrap();
    let actual = image::load_from_memory(&display.bytes).unwrap().to_rgba8();
    let expected = apply(source, &p3, &mut vec![]).to_rgba8();
    assert_eq!(actual, expected);
    assert_eq!(
        crate::svg_assets::image_file(&svg, 1, crate::svg_assets::ImageFile::Original)
            .unwrap()
            .bytes,
        png
    );
}

#[test]
fn icc_output_png_jpeg_tiff_profile_choice_exact_native_pixels_hdr_and_source() {
    use image::ImageDecoder;
    let root = root().join("output");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    std::fs::write(root.join("working-srgb.icc"), srgb_profile()).unwrap();
    for depth in [8, 16, 32] {
        let source = match depth {
            8 => DynamicImage::ImageRgba8(rgba()),
            16 => DynamicImage::ImageRgba16(rgba16()),
            _ => DynamicImage::ImageRgba32F(ImageBuffer::from_fn(17, 9, |x, y| {
                let i = x + y * 17;
                image::Rgba([
                    if i % 11 == 0 {
                        -2.
                    } else if i % 11 == 1 {
                        f32::MAX
                    } else {
                        i as f32 / 61.
                    },
                    0.12345,
                    0.54321,
                    if i.is_multiple_of(7) {
                        0.
                    } else if i % 7 == 1 {
                        1.
                    } else {
                        0.25
                    },
                ])
            })),
        };
        let doc = crate::precision::document(format!("depth{depth}"), source).unwrap();
        let raw_pixels = doc.layers[0].pixels.clone();
        let precision = doc.layers[0].precision.clone();
        let expected = crate::precision::composite(&doc);
        for (format, extension) in [
            (image::ImageFormat::Png, "png"),
            (image::ImageFormat::Jpeg, "jpg"),
            (image::ImageFormat::Tiff, "tiff"),
        ] {
            let mut untagged = None;
            for include in [false, true] {
                let options = crate::files::ExportOptions {
                    include_icc: include,
                    tiff_depth: if depth == 32 { 3 } else { 0 },
                    ..Default::default()
                };
                let name = format!("d{depth}-{include}.{extension}");
                let path = root.join(&name);
                crate::files::export(&doc, &path, format, &options).unwrap();
                let bytes = std::fs::read(&path).unwrap();
                let profile = if format == image::ImageFormat::Tiff {
                    tiff_profile(&path).unwrap()
                } else {
                    image::ImageReader::open(&path)
                        .unwrap()
                        .with_guessed_format()
                        .unwrap()
                        .into_decoder()
                        .unwrap()
                        .icc_profile()
                        .unwrap()
                };
                assert_eq!(profile.as_deref(), include.then_some(srgb_profile()));
                let opened = crate::files::open(&path).unwrap();
                let decoded = crate::precision::composite(&opened.document);
                if let Some(reference) = &untagged {
                    assert_eq!(&decoded, reference, "{name}");
                } else {
                    untagged = Some(decoded.clone());
                }
                if format == image::ImageFormat::Tiff {
                    assert_eq!(decoded, expected);
                }
                if include {
                    assert!(opened.notes.iter().any(|n| n.contains("적용해")));
                }
                assert!(std::sync::Arc::ptr_eq(&raw_pixels, &doc.layers[0].pixels));
                if let Some(original) = &precision {
                    assert!(std::sync::Arc::ptr_eq(
                        original,
                        doc.layers[0].precision.as_ref().unwrap()
                    ));
                }
                assert_eq!(std::fs::read(&path).unwrap(), bytes);
                let project = root.join(format!("{name}.rphoto"));
                crate::files::save_project(&opened.document, &project).unwrap();
                assert_eq!(
                    crate::precision::composite(&crate::files::load_project(&project).unwrap()),
                    decoded
                );
                cases.push(serde_json::json!({"file":name,"depth":depth,"include":include}));
            }
        }
    }
    assert_eq!(cases.len(), 18);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn icc_exact_working_profile_identity_preserves_float_hdr_bits_rejects_nonfinite() {
    let source = DynamicImage::ImageRgba32F(ImageBuffer::from_fn(5, 3, |x, y| {
        image::Rgba([
            if x == 0 {
                -0.0
            } else if x == 1 {
                -f32::MAX
            } else {
                f32::MAX
            },
            y as f32 + 0.125,
            0.03125,
            0.12345,
        ])
    }));
    let before = source
        .to_rgba32f()
        .as_raw()
        .iter()
        .map(|v| v.to_bits())
        .collect::<Vec<_>>();
    let mut notes = vec![];
    let output = apply(source, srgb_profile(), &mut notes).to_rgba32f();
    assert_eq!(
        output
            .as_raw()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        before
    );
    assert!(notes[0].contains("적용해"));
    for value in [f32::NAN, f32::INFINITY] {
        let source = DynamicImage::ImageRgba32F(ImageBuffer::from_pixel(
            1,
            1,
            image::Rgba([value, 0., 0., 1.]),
        ));
        let mut notes = vec![];
        let output = apply(source, srgb_profile(), &mut notes).to_rgba32f();
        assert_eq!(output.get_pixel(0, 0)[0].to_bits(), value.to_bits());
        assert!(notes[0].contains("못했습니다"));
    }
}
