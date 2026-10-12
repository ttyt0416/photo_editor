use super::*;
use crate::files::{ExportOptions, export, load_project, open, save_project};
use image::ImageEncoder;
use std::sync::Arc;
fn source_exif(orientation: u16, little: bool) -> Vec<u8> {
    let word = |v: u16| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let long = |v: u32| {
        if little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    };
    let mut data = if little {
        b"II".to_vec()
    } else {
        b"MM".to_vec()
    };
    data.extend(word(42));
    data.extend(long(8));
    data.extend(word(5));
    for (tag, kind, value) in [
        (0x100, 3, 7),
        (0x101, 4, 5),
        (0x10f, 2, 104),
        (0x112, 3, u32::from(orientation)),
        (0x8769, 4, 74),
    ] {
        data.extend(word(tag));
        data.extend(word(kind));
        data.extend(long(if tag == 0x10f { 9 } else { 1 }));
        if kind == 3 {
            data.extend(word(value as u16));
            data.extend([0, 0]);
        } else {
            data.extend(long(value));
        }
    }
    data.extend(long(114));
    assert_eq!(data.len(), 74);
    data.extend(word(2));
    for (tag, kind, value) in [(0xa002, 4, 7), (0xa003, 3, 5)] {
        data.extend(word(tag));
        data.extend(word(kind));
        data.extend(long(1));
        if kind == 3 {
            data.extend(word(value as u16));
            data.extend([0, 0]);
        } else {
            data.extend(long(value));
        }
    }
    data.extend(long(0));
    assert_eq!(data.len(), 104);
    data.extend(b"CameraQA\0");
    data.push(0);
    assert_eq!(data.len(), 114);
    // A real tiny JPEG thumbnail is detached in the exported copy.
    let mut thumbnail = vec![];
    jpeg_encoder::Encoder::new(&mut thumbnail, 80)
        .encode(&[10, 20, 30], 1, 1, jpeg_encoder::ColorType::Rgb)
        .unwrap();
    data.extend(word(2));
    for (tag, value) in [(0x201, 144), (0x202, thumbnail.len() as u32)] {
        data.extend(word(tag));
        data.extend(word(4));
        data.extend(long(1));
        data.extend(long(value));
    }
    data.extend(long(0));
    assert_eq!(data.len(), 144);
    data.extend(thumbnail);
    data
}
#[test]
fn exif_export_orientation_size_thumbnail_options_pixels_and_originals() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/exif-output");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    let pixels = image::RgbaImage::from_fn(7, 5, |x, y| {
        image::Rgba([
            (x * 31) as u8,
            (y * 47) as u8,
            (x * 19 + y * 23) as u8,
            if x == 0 { 128 } else { 255 },
        ])
    });
    for little in [false, true] {
        for orientation in 1..=8 {
            let original_exif = source_exif(orientation, little);
            let input_name = format!("source-o{orientation}-le{little}.png");
            let input = root.join(&input_name);
            let mut encoder =
                image::codecs::png::PngEncoder::new(std::fs::File::create(&input).unwrap());
            encoder.set_exif_metadata(original_exif.clone()).unwrap();
            encoder
                .write_image(pixels.as_raw(), 7, 5, image::ExtendedColorType::Rgba8)
                .unwrap();
            let bytes = std::fs::read(&input).unwrap();
            let mut editor = crate::document::Editor::new(open(&input).unwrap().document, false);
            let original = editor.document.source_metadata.exif.clone().unwrap();
            editor.begin("resize");
            crate::editing::resize(&mut editor.document, 9, 11, true).unwrap();
            editor.finish();
            editor.undo();
            editor.redo();
            assert!(Arc::ptr_eq(
                &original,
                editor.document.source_metadata.exif.as_ref().unwrap()
            ));
            let expected = editor.document.composite();
            let golden = format!("source-o{orientation}-le{little}-edited.png");
            expected.save(root.join(&golden)).unwrap();
            let source = editor.document.layers[0].pixels.clone();
            for (label, format, lossless) in [
                ("png", image::ImageFormat::Png, true),
                ("jpg", image::ImageFormat::Jpeg, true),
                ("lossless.webp", image::ImageFormat::WebP, true),
                ("lossy.webp", image::ImageFormat::WebP, false),
            ] {
                for icc in [false, true] {
                    let mut baseline = None;
                    for include in [false, true] {
                        let options = ExportOptions {
                            include_exif: include,
                            include_icc: icc,
                            webp_lossless: lossless,
                            webp_quality: 90,
                            ..Default::default()
                        };
                        let name = format!("o{orientation}-le{little}-i{icc}-e{include}.{label}");
                        let path = root.join(&name);
                        export(&editor.document, &path, format, &options).unwrap();
                        let opened = open(&path).unwrap();
                        assert_eq!((opened.document.width, opened.document.height), (9, 11));
                        let decoded = opened.document.composite();
                        assert_eq!(opened.document.source_metadata.exif.is_some(), include);
                        if include {
                            assert_eq!(
                                image::metadata::Orientation::from_exif_chunk(
                                    opened.document.source_metadata.exif.as_ref().unwrap()
                                ),
                                Some(image::metadata::Orientation::NoTransforms)
                            );
                        }
                        if format != image::ImageFormat::Jpeg && lossless {
                            assert_eq!(decoded, expected);
                        }
                        if let Some(before) = &baseline {
                            assert_eq!(&decoded, before);
                        } else {
                            baseline = Some(decoded.clone());
                        }
                        let raw = format!("{name}.rgba8");
                        std::fs::write(root.join(&raw), decoded.as_raw()).unwrap();
                        let project = root.join(format!("{name}.rphoto"));
                        save_project(&opened.document, &project).unwrap();
                        assert_eq!(load_project(&project).unwrap().composite(), decoded);
                        assert_eq!(
                            editor
                                .document
                                .source_metadata
                                .exif
                                .as_deref()
                                .unwrap()
                                .as_slice(),
                            original_exif
                        );
                        assert!(Arc::ptr_eq(&source, &editor.document.layers[0].pixels));
                        assert_eq!(std::fs::read(&input).unwrap(), bytes);
                        cases.push(serde_json::json!({"file":name,"raw":raw,"input":input_name,"golden":golden,"orientation":orientation,"little":little,"icc":icc,"include":include,"lossless":lossless,"format":label}));
                    }
                }
            }
            let name = format!("wide-le{little}-o{orientation}.exif");
            std::fs::write(
                root.join(&name),
                normalize(&original_exif, 70000, 80000).unwrap(),
            )
            .unwrap();
        }
    }
    let high_image = image::ImageBuffer::from_fn(9, 11, |x, y| {
        image::Rgba([
            (x * 1351 + y * 9289) as u16,
            (x * 941 + y * 2251) as u16,
            (x * 317 + y * 3911) as u16,
            32768u16,
        ])
    });
    let mut high = crate::precision::document(
        "16-bit EXIF".into(),
        image::DynamicImage::ImageRgba16(high_image.clone()),
    )
    .unwrap();
    high.source_metadata.exif = Some(Arc::new(source_exif(6, true)));
    let precision = high.layers[0].precision.clone().unwrap();
    high_image.save(root.join("native16-golden.png")).unwrap();
    for include in [false, true] {
        let name = format!("native16-e{include}.png");
        let path = root.join(&name);
        export(
            &high,
            &path,
            image::ImageFormat::Png,
            &ExportOptions {
                include_exif: include,
                ..Default::default()
            },
        )
        .unwrap();
        let opened = open(&path).unwrap();
        assert_eq!(opened.document.layers[0].bit_depth, 16);
        let decoded = crate::precision::composite(&opened.document);
        assert_eq!(decoded, crate::precision::composite(&high));
        let raw = format!("{name}.rgba32f.le");
        std::fs::write(
            root.join(&raw),
            decoded
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        save_project(&opened.document, &root.join(format!("{name}.rphoto"))).unwrap();
        assert!(Arc::ptr_eq(
            &precision,
            high.layers[0].precision.as_ref().unwrap()
        ));
        cases.push(serde_json::json!({"file":name,"raw":raw,"input":"source-o6-letrue.png","golden":"native16-golden.png","orientation":6,"little":true,"icc":true,"include":include,"lossless":true,"format":"png","depth":16}));
    }
    assert_eq!(cases.len(), 258);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn exif_invalid_offsets_types_cycles_and_jpeg_capacity_preserve_destination() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/exif-output/errors");
    std::fs::create_dir_all(&root).unwrap();
    let base = source_exif(6, true);
    let mut invalid = vec![vec![], vec![0; 8], base[..20].to_vec()];
    let mut cycle = base.clone();
    cycle[66..70].copy_from_slice(&8u32.to_le_bytes());
    invalid.push(cycle);
    let mut far = base.clone();
    far[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
    invalid.push(far);
    let mut kind = base.clone();
    kind[48..50].copy_from_slice(&1u16.to_le_bytes());
    invalid.push(kind);
    let mut doc = crate::document::Document::new(3, 2, true).unwrap();
    for (index, bytes) in invalid.into_iter().enumerate() {
        doc.source_metadata.exif = Some(Arc::new(bytes));
        for (label, format) in [
            ("png", image::ImageFormat::Png),
            ("jpg", image::ImageFormat::Jpeg),
            ("webp", image::ImageFormat::WebP),
        ] {
            let path = root.join(format!("invalid{index}.{label}"));
            std::fs::write(&path, b"original").unwrap();
            assert!(export(&doc, &path, format, &ExportOptions::default()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"original");
            export(
                &doc,
                &path,
                format,
                &ExportOptions {
                    include_exif: false,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(open(&path).unwrap().document.source_metadata.exif.is_none());
        }
    }
    let mut big = base;
    big.resize(65529, 0);
    doc.source_metadata.exif = Some(Arc::new(big));
    let path = root.join("too-large.jpg");
    std::fs::write(&path, b"original").unwrap();
    assert!(
        export(
            &doc,
            &path,
            image::ImageFormat::Jpeg,
            &ExportOptions::default()
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    assert!(
        std::fs::read_dir(root).unwrap().all(|p| !p
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp"))
    );
}
