use super::*;
use crate::files::{ExportOptions, export, load_project, open, save_project};
use image::{ImageBuffer, ImageFormat};

#[test]
fn tiff_white_gray_alpha_uint_float_invert_unassociate_icc_planar_projects() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = base.join("tests/fixtures/tiff-white-alpha");
    let root = base.join("qa-output/tiff-white-alpha");
    std::fs::create_dir_all(&root).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 384);
    for (index, case) in cases.into_iter().enumerate() {
        let name = case["file"].as_str().unwrap();
        let path = fixtures.join(name);
        let before = std::fs::read(&path).unwrap();
        if index == 0 {
            assert!(image::open(&path).is_err());
        }
        let imported = open(&path).unwrap();
        let document = imported.document;
        assert_eq!(document.layers[0].bit_depth, 32);
        let pixels = document.layers[0].precision.as_ref().unwrap();
        if case["associated"].as_bool().unwrap() || case["bits"].as_u64() == Some(32) {
            assert!(pixels.pixels().any(|p| p[0] < 0.));
        }
        std::fs::write(
            root.join(format!("{name}.f32")),
            pixels
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let composite = crate::precision::composite(&document);
        std::fs::write(
            root.join(format!("{name}.comp.f32")),
            composite
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        save_project(&document, &root.join(format!("{name}.rphoto"))).unwrap();
        let restored = load_project(&root.join(format!("{name}.rphoto"))).unwrap();
        assert_eq!(
            restored.layers[0]
                .precision
                .as_ref()
                .unwrap()
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
        assert_eq!(restored.source_icc, document.source_icc);
        assert_eq!(restored.source_metadata.xmp, document.source_metadata.xmp);
        for (ext, format) in [("tiff", ImageFormat::Tiff), ("exr", ImageFormat::OpenExr)] {
            let output = root.join(format!("{name}-working.{ext}"));
            export(&restored, &output, format, &Default::default()).unwrap();
            assert_eq!(
                crate::precision::composite(&open(&output).unwrap().document),
                composite
            );
        }
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[test]
fn tiff_raw_white_read_restores_interpretation_on_success_and_limit_error() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let path = base.join("tiff-white-alpha/d32-none-plan0-assoc0-big0-end0-zip0.tiff");
    let mut decoder = Decoder::new(BufReader::new(File::open(path).unwrap())).unwrap();
    let mut decoded = DecodingResult::F32(vec![]);
    decoder
        .read_image_to_buffer_uninverted(&mut decoded)
        .unwrap();
    assert_eq!(
        decoder
            .find_tag_unsigned::<u16>(Tag::PhotometricInterpretation)
            .unwrap(),
        Some(0)
    );
    assert!(
        decoder.read_image_to_buffer(&mut decoded).is_err(),
        "ordinary white multiband behavior must be restored"
    );
    let path = base.join("tiff-gray-float/none-big0-endFalse-white1-zip0.tiff");
    let mut limits = Limits::default();
    limits.decoding_buffer_size = 1;
    let mut decoder = Decoder::new(BufReader::new(File::open(path).unwrap()))
        .unwrap()
        .with_limits(limits);
    assert!(
        decoder
            .read_image_to_buffer_uninverted(&mut decoded)
            .is_err()
    );
    decoder = decoder.with_limits(Limits::default());
    decoder.read_image_to_buffer(&mut decoded).unwrap();
    let DecodingResult::F32(values) = decoded else {
        panic!("expected float gray")
    };
    assert_eq!(
        values[0], -2.,
        "white inversion must be restored after errors"
    );
}

#[test]
fn tiff_gray_integer_alpha_associated_straight_planar_icc_hidden_hdr_projects() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = base.join("tests/fixtures/tiff-gray-integer-alpha");
    let root = base.join("qa-output/tiff-gray-integer-alpha");
    std::fs::create_dir_all(&root).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 256);
    for (index, case) in cases.into_iter().enumerate() {
        let name = case["file"].as_str().unwrap();
        let path = fixtures.join(name);
        let before = std::fs::read(&path).unwrap();
        if index == 0 {
            assert!(
                image::open(&path).is_err(),
                "original generic gray-alpha adapter must reject this supported source"
            );
        }
        let imported = open(&path).unwrap();
        assert!(
            imported
                .notes
                .iter()
                .any(|n| n.contains("회색조 TIFF의 알파와 색을 32비트"))
        );
        let document = imported.document;
        assert_eq!(document.layers[0].bit_depth, 32);
        let pixels = document.layers[0].precision.as_ref().unwrap();
        if case["associated"].as_bool().unwrap() {
            assert!(pixels.pixels().any(|p| p[0] > 1.));
        }
        std::fs::write(
            root.join(format!("{name}.f32")),
            pixels
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let composite = crate::precision::composite(&document);
        std::fs::write(
            root.join(format!("{name}.comp.f32")),
            composite
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        save_project(&document, &root.join(format!("{name}.rphoto"))).unwrap();
        let restored = load_project(&root.join(format!("{name}.rphoto"))).unwrap();
        assert_eq!(
            restored.layers[0]
                .precision
                .as_ref()
                .unwrap()
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
        assert_eq!(restored.source_icc, document.source_icc);
        assert_eq!(restored.source_metadata.xmp, document.source_metadata.xmp);
        for (ext, format) in [("tiff", ImageFormat::Tiff), ("exr", ImageFormat::OpenExr)] {
            let output = root.join(format!("{name}-working.{ext}"));
            export(&restored, &output, format, &Default::default()).unwrap();
            assert_eq!(
                crate::precision::composite(&open(&output).unwrap().document),
                composite
            );
        }
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[test]
fn tiff_integer_rgb_associated_float_ratios_hdr_icc_planar_project_auto_tiff() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = base.join("tests/fixtures/tiff-integer-associated");
    let root = base.join("qa-output/tiff-integer-associated");
    std::fs::create_dir_all(&root).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 96);
    for (index, case) in cases.into_iter().enumerate() {
        let name = case["file"].as_str().unwrap();
        let path = fixtures.join(name);
        let before = std::fs::read(&path).unwrap();
        let imported = open(&path).unwrap();
        assert!(
            imported
                .notes
                .iter()
                .any(|n| n.contains("32비트 작업 버퍼로 분리"))
        );
        let document = imported.document;
        assert_eq!(document.layers[0].bit_depth, 32);
        let pixels = document.layers[0].precision.as_ref().unwrap();
        assert!(pixels.pixels().any(|p| p.0[..3].iter().any(|v| *v > 1.)));
        if index == 0 {
            let generic = image::open(&path).unwrap().to_rgba32f();
            assert!(
                generic
                    .pixels()
                    .zip(pixels.pixels())
                    .any(|(g, p)| p[3] > 0. && p[3] < 1. && g[0] != p[0]),
                "generic integer adapter must reproduce associated pixels"
            );
        }
        std::fs::write(
            root.join(format!("{name}.f32")),
            pixels
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let composite = crate::precision::composite(&document);
        std::fs::write(
            root.join(format!("{name}.comp.f32")),
            composite
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        save_project(&document, &root.join(format!("{name}.rphoto"))).unwrap();
        let restored = load_project(&root.join(format!("{name}.rphoto"))).unwrap();
        assert_eq!(
            restored.layers[0]
                .precision
                .as_ref()
                .unwrap()
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
        assert_eq!(restored.source_icc, document.source_icc);
        assert_eq!(restored.source_metadata.xmp, document.source_metadata.xmp);
        for (extension, format) in [("tiff", ImageFormat::Tiff), ("exr", ImageFormat::OpenExr)] {
            let output = root.join(format!("{name}-working.{extension}"));
            export(&restored, &output, format, &Default::default()).unwrap();
            assert_eq!(open(&output).unwrap().document.layers[0].bit_depth, 32);
            assert_eq!(
                crate::precision::composite(&open(&output).unwrap().document),
                composite
            );
        }
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[test]
fn tiff_rgb_float_associated_hdr_planar_icc_projects_independent_outputs() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = base.join("tests/fixtures/tiff-rgb-associated");
    let root = base.join("qa-output/tiff-rgb-associated");
    std::fs::create_dir_all(&root).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 48);
    for (index, case) in cases.into_iter().enumerate() {
        let name = case["file"].as_str().unwrap();
        let path = fixtures.join(name);
        let before = std::fs::read(&path).unwrap();
        let imported = open(&path).unwrap();
        assert!(
            imported
                .notes
                .iter()
                .any(|n| n.contains("RGB TIFF의 연관 알파")),
            "{name}: {:?}",
            imported.notes
        );
        let document = imported.document;
        let native = document.layers[0].precision.as_ref().unwrap();
        if index == 0 {
            let generic = image::open(&path).unwrap().to_rgba32f();
            assert!(
                generic
                    .pixels()
                    .zip(native.pixels())
                    .any(|(g, n)| n[3] > 0. && n[3] < 1. && g[0] != n[0]),
                "original generic decoder must reproduce still-associated RGB"
            );
        }
        std::fs::write(
            root.join(format!("{name}.f32")),
            native
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let composite = crate::precision::composite(&document);
        std::fs::write(
            root.join(format!("{name}.comp.f32")),
            composite
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let project = root.join(format!("{name}.rphoto"));
        save_project(&document, &project).unwrap();
        let restored = load_project(&project).unwrap();
        assert_eq!(restored.source_icc, document.source_icc);
        assert_eq!(restored.source_metadata.xmp, document.source_metadata.xmp);
        assert_eq!(
            restored.layers[0]
                .precision
                .as_ref()
                .unwrap()
                .as_raw()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            native
                .as_raw()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
        for (ext, format) in [("tiff", ImageFormat::Tiff), ("exr", ImageFormat::OpenExr)] {
            let output = root.join(format!("{name}-working.{ext}"));
            export(
                &restored,
                &output,
                format,
                &ExportOptions {
                    tiff_depth: 3,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                crate::precision::composite(&open(&output).unwrap().document),
                composite
            );
        }
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[test]
fn tiff_rgb_float_associated_invalid_alpha_or_unassociation_overflow_rejected() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/tiff-rgb-associated/errors");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("bad.tiff");
    for (color, alpha) in [
        (1., -0.1),
        (1., 1.1),
        (f32::NAN, 0.5),
        (1., f32::INFINITY),
        (f32::MAX, f32::MIN_POSITIVE),
    ] {
        let mut encoder = tiff::encoder::TiffEncoder::new(File::create(&path).unwrap()).unwrap();
        let mut image = encoder
            .new_image::<tiff::encoder::colortype::RGBA32Float>(1, 1)
            .unwrap();
        image
            .encoder()
            .write_tag(Tag::ExtraSamples, &[1u16][..])
            .unwrap();
        image.write_data(&[color, 1., 2., alpha]).unwrap();
        assert!(open(&path).is_err());
    }
}

#[test]
fn tiff_gray_alpha_float_planar_associated_original_bits_projects_outputs() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = base.join("tests/fixtures/tiff-gray-alpha");
    let root = base.join("qa-output/tiff-gray-alpha");
    std::fs::create_dir_all(&root).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 96);
    for case in cases {
        let name = case["file"].as_str().unwrap();
        let path = fixtures.join(name);
        let before = std::fs::read(&path).unwrap();
        let imported = open(&path).unwrap();
        let document = imported.document;
        assert_eq!(document.layers[0].bit_depth, 32);
        let original = document.layers[0].precision.as_ref().unwrap();
        assert!(original.pixels().any(|p| p[0] < 0. && p[3] > 0.));
        assert!(original.pixels().any(|p| p[0] > 1. && p[3] > 0.));
        std::fs::write(
            root.join(format!("{name}.f32")),
            original
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let composite = crate::precision::composite(&document);
        std::fs::write(
            root.join(format!("{name}.comp.f32")),
            composite
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        save_project(&document, &root.join(format!("{name}.rphoto"))).unwrap();
        let restored = load_project(&root.join(format!("{name}.rphoto"))).unwrap();
        assert_eq!(restored.layers[0].precision, document.layers[0].precision);
        assert_eq!(restored.source_icc, document.source_icc);
        assert_eq!(restored.source_metadata.xmp, document.source_metadata.xmp);
        for (ext, format) in [("tiff", ImageFormat::Tiff), ("exr", ImageFormat::OpenExr)] {
            let output = root.join(format!("{name}-working.{ext}"));
            export(
                &restored,
                &output,
                format,
                &ExportOptions {
                    tiff_depth: 3,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                crate::precision::composite(&open(&output).unwrap().document),
                composite
            );
        }
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[test]
fn tiff_gray_alpha_invalid_alpha_missing_definition_and_overflow_rejected() {
    struct GrayAlpha;
    impl tiff::encoder::colortype::ColorType for GrayAlpha {
        type Inner = f32;
        const TIFF_VALUE: tiff::tags::PhotometricInterpretation =
            tiff::tags::PhotometricInterpretation::BlackIsZero;
        const BITS_PER_SAMPLE: &'static [u16] = &[32, 32];
        const SAMPLE_FORMAT: &'static [tiff::tags::SampleFormat] =
            &[tiff::tags::SampleFormat::IEEEFP; 2];
        fn horizontal_predict(_: &[f32], _: &mut Vec<f32>) {
            unreachable!()
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/tiff-gray-alpha/errors");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("bad.tiff");
    for (extra, gray, alpha) in [
        (2, 1., -0.125),
        (2, 1., 1.25),
        (2, 1., f32::NAN),
        (2, 1., f32::INFINITY),
        (1, f32::MAX, f32::MIN_POSITIVE),
        (0, 1., 0.5),
    ] {
        let mut encoder = tiff::encoder::TiffEncoder::new(File::create(&path).unwrap()).unwrap();
        let mut image = encoder.new_image::<GrayAlpha>(1, 1).unwrap();
        if extra != 0 {
            image
                .encoder()
                .write_tag(Tag::ExtraSamples, &[extra as u16][..])
                .unwrap();
        }
        image.write_data(&[gray, alpha]).unwrap();
        assert!(open(&path).is_err(), "{extra}/{gray}/{alpha}");
    }
}

#[test]
fn tiff_gray_float_independent_endian_bigtiff_compressed_hdr_icc_orientation_projects() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = base.join("tests/fixtures/tiff-gray-float");
    let root = base.join("qa-output/tiff-gray-float");
    std::fs::create_dir_all(&root).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 80);
    for case in cases {
        let name = case["file"].as_str().unwrap();
        let input = fixtures.join(name);
        let original = std::fs::read(&input).unwrap();
        let opened = open(&input).unwrap();
        assert!(
            opened
                .notes
                .iter()
                .any(|n| n.contains("32비트 회색조 TIFF")),
            "{name}: {:?}",
            opened.notes
        );
        assert_eq!(
            opened.notes.iter().any(|n| n.contains("여러 페이지")),
            case["multipage"].as_bool().unwrap()
        );
        let profile = case["profile"].as_str().unwrap();
        if profile == "invalid" {
            assert!(opened.notes.iter().any(|n| n.contains("적용하지 못")));
        }
        if profile == "linear" || profile == "gamma18" {
            assert!(opened.notes.iter().any(|n| n.contains("작업 sRGB로 변환")));
        }
        if profile == "working" {
            assert!(opened.notes.iter().any(|n| n.contains("원래 float 비트")));
        }
        assert_eq!(opened.document.layers[0].bit_depth, 32);
        let source = opened.document.source_icc.clone();
        if profile == "none" {
            assert!(source.is_none());
        } else {
            assert_eq!(
                source.as_deref().unwrap().as_slice(),
                std::fs::read(fixtures.join(format!("{profile}.icc"))).unwrap()
            );
        }
        let pixels = crate::precision::composite(&opened.document);
        assert!(pixels.pixels().any(|p| p[0] < 0.));
        assert!(pixels.pixels().any(|p| p[0] > 1.));
        let mut editor = crate::document::Editor::new(opened.document, false);
        editor.edit("name", |d| d.layers[0].name = "gray HDR".into());
        editor.undo();
        editor.redo();
        assert_eq!(editor.document.source_icc, source);
        let project = root.join(format!("{name}.rphoto"));
        save_project(&editor.document, &project).unwrap();
        let restored = load_project(&project).unwrap();
        assert_eq!(crate::precision::composite(&restored), pixels);
        assert_eq!(
            restored.source_metadata.xmp,
            editor.document.source_metadata.xmp
        );
        assert_eq!(restored.source_icc, source);
        std::fs::write(
            root.join(format!("{name}.f32")),
            pixels
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        for (extension, format) in [("tiff", ImageFormat::Tiff), ("exr", ImageFormat::OpenExr)] {
            let output = root.join(format!("{name}-working.{extension}"));
            export(
                &restored,
                &output,
                format,
                &ExportOptions {
                    tiff_depth: 3,
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(
                crate::precision::composite(&open(&output).unwrap().document),
                pixels
            );
        }
        assert_eq!(std::fs::read(input).unwrap(), original);
    }
}

#[test]
fn tiff_gray_float_nonfinite_truncated_and_regular_tiff_fallback() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/tiff-gray-float/errors");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("bad.tiff");
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut encoder = tiff::encoder::TiffEncoder::new(File::create(&path).unwrap()).unwrap();
        encoder
            .new_image::<tiff::encoder::colortype::Gray32Float>(2, 1)
            .unwrap()
            .write_data(&[0.125, value])
            .unwrap();
        assert!(open(&path).err().unwrap().contains("부동소수점"));
    }
    let bytes = std::fs::read(&path).unwrap();
    std::fs::write(&path, &bytes[..bytes.len() / 2]).unwrap();
    assert!(open(&path).is_err());
    let input = DynamicImage::ImageRgba16(ImageBuffer::from_pixel(
        2,
        1,
        Rgba([1, 12345, 65535, 32768]),
    ));
    input.save_with_format(&path, ImageFormat::Tiff).unwrap();
    assert!(read(&path).unwrap().is_none());
    assert_eq!(open(&path).unwrap().document.layers[0].bit_depth, 16);
    input.save_with_format(&path, ImageFormat::Png).unwrap();
    assert!(read(&path).unwrap().is_none());
    assert_eq!(open(&path).unwrap().document.layers[0].bit_depth, 16);
}
