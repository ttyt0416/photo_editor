use super::*;
use crate::files::{ExportOptions, export, load_project, open, save_project};
use image::ImageEncoder;

#[test]
fn tiff_camera_exif_gps_offsets_history_project_and_png_export() {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixtures = base.join("tests/fixtures/tiff-exif");
    let root = base.join("qa-output/tiff-exif");
    std::fs::create_dir_all(&root).unwrap();
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(cases.len(), 48);
    for case in cases {
        let name = case["file"].as_str().unwrap();
        let path = fixtures.join(name);
        let original = std::fs::read(&path).unwrap();
        let imported = open(&path).unwrap();
        let source = imported
            .document
            .source_metadata
            .exif
            .clone()
            .expect("TIFF camera EXIF must be captured");
        let orientation = case["orientation"].as_u64().unwrap() as u8;
        assert_eq!(
            image::metadata::Orientation::from_exif_chunk(&source),
            image::metadata::Orientation::from_exif(orientation)
        );
        let mut editor = crate::document::Editor::new(imported.document, false);
        let pixels = crate::precision::composite(&editor.document);
        editor.edit("rename", |d| d.layers[0].name = "EXIF retained".into());
        editor.undo();
        editor.redo();
        assert!(std::sync::Arc::ptr_eq(
            &source,
            editor.document.source_metadata.exif.as_ref().unwrap()
        ));
        let project = root.join(format!("{name}.rphoto"));
        save_project(&editor.document, &project).unwrap();
        let restored = load_project(&project).unwrap();
        assert_eq!(
            restored.source_metadata.exif.as_deref().unwrap().as_slice(),
            source.as_slice()
        );
        assert_eq!(crate::precision::composite(&restored), pixels);
        for include in [true, false] {
            let output = root.join(format!("{name}-exif{include}.png"));
            export(
                &restored,
                &output,
                image::ImageFormat::Png,
                &ExportOptions {
                    include_exif: include,
                    ..Default::default()
                },
            )
            .unwrap();
            let exported = open(&output).unwrap();
            assert_eq!(exported.document.source_metadata.exif.is_some(), include);
            if include {
                assert_eq!(
                    image::metadata::Orientation::from_exif_chunk(
                        exported.document.source_metadata.exif.as_deref().unwrap()
                    ),
                    Some(image::metadata::Orientation::NoTransforms)
                );
            }
        }
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}

fn exif(orientation: u16, little: bool) -> Vec<u8> {
    let mut data = if little {
        b"II".to_vec()
    } else {
        b"MM".to_vec()
    };
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
    data.extend(word(42));
    data.extend(long(8));
    data.extend(word(2));
    data.extend(word(0x10f));
    data.extend(word(2));
    data.extend(long(9));
    data.extend(long(38));
    data.extend(word(0x112));
    data.extend(word(3));
    data.extend(long(1));
    data.extend(word(orientation));
    data.extend([0, 0]);
    data.extend(long(0));
    data.extend(b"CameraQA\0");
    data
}
fn png_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut body = kind.to_vec();
    body.extend(data);
    let mut crc = 0xffffffffu32;
    for &byte in &body {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ if crc & 1 == 1 { 0xedb88320 } else { 0 };
        }
    }
    let mut out = (data.len() as u32).to_be_bytes().to_vec();
    out.extend(body);
    out.extend((!crc).to_be_bytes());
    out
}
fn riff_chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut result = kind.to_vec();
    result.extend((data.len() as u32).to_le_bytes());
    result.extend(data);
    if data.len() % 2 == 1 {
        result.push(0);
    }
    result
}
#[test]
fn original_exif_xmp_orientation_history_project_and_tiff_large_packet_preserved() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/source-metadata");
    std::fs::create_dir_all(&root).unwrap();
    let image = image::RgbaImage::from_fn(7, 5, |x, y| {
        image::Rgba([
            (x * 31) as u8,
            (y * 47) as u8,
            (x * 19 + y * 23) as u8,
            if x == 0 { 128 } else { 255 },
        ])
    });
    let mut cases = vec![];
    let xmp="<?xpacket begin='\u{feff}'?><x:xmpmeta xmlns:x='adobe:ns:meta/'><rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#'><rdf:Description xmlns:qa='urn:photo-editor:qa' qa:creator='촬영 테스트' qa:rating='4'/></rdf:RDF></x:xmpmeta><?xpacket end='w'?>".as_bytes();
    for extension in ["png", "jpg", "webp"] {
        for little in [false, true] {
            for orientation in 1..=8 {
                let raw_exif = exif(orientation, little);
                let name = format!("o{orientation}-le{little}.{extension}");
                let path = root.join(&name);
                let profile = crate::color_management::srgb_profile();
                match extension {
                    "png" => {
                        let mut encoded = vec![];
                        let mut encoder = image::codecs::png::PngEncoder::new(&mut encoded);
                        encoder.set_icc_profile(profile.to_vec()).unwrap();
                        encoder.set_exif_metadata(raw_exif.clone()).unwrap();
                        encoder
                            .write_image(image.as_raw(), 7, 5, image::ExtendedColorType::Rgba8)
                            .unwrap();
                        let mut text = b"XML:com.adobe.xmp\0".to_vec();
                        text.extend([u8::from(little), 0, 0, 0]);
                        if little {
                            let mut zipped = flate2::write::ZlibEncoder::new(
                                Vec::new(),
                                flate2::Compression::default(),
                            );
                            zipped.write_all(xmp).unwrap();
                            text.extend(zipped.finish().unwrap());
                        } else {
                            text.extend(xmp);
                        }
                        let chunk = png_chunk(b"iTXt", &text);
                        encoded.splice(33..33, chunk);
                        std::fs::write(&path, encoded).unwrap();
                    }
                    "jpg" => {
                        let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
                        let mut encoder =
                            jpeg_encoder::Encoder::new(std::fs::File::create(&path).unwrap(), 95);
                        encoder.add_icc_profile(profile).unwrap();
                        let mut payload = b"Exif\0\0".to_vec();
                        payload.extend(&raw_exif);
                        encoder.add_app_segment(1, payload).unwrap();
                        let mut payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
                        payload.extend(xmp);
                        encoder.add_app_segment(1, payload).unwrap();
                        encoder
                            .encode(rgb.as_raw(), 7, 5, jpeg_encoder::ColorType::Rgb)
                            .unwrap();
                    }
                    "webp" => {
                        let mut encoded = vec![];
                        image::codecs::webp::WebPEncoder::new_lossless(&mut encoded)
                            .write_image(image.as_raw(), 7, 5, image::ExtendedColorType::Rgba8)
                            .unwrap();
                        assert_eq!(&encoded[12..16], b"VP8L");
                        let mut extension = [0u8; 10];
                        extension[0] = 0x3c;
                        extension[4..7].copy_from_slice(&6u32.to_le_bytes()[..3]);
                        extension[7..10].copy_from_slice(&4u32.to_le_bytes()[..3]);
                        let mut body = b"WEBP".to_vec();
                        body.extend(riff_chunk(b"VP8X", &extension));
                        body.extend(riff_chunk(b"ICCP", profile));
                        body.extend(&encoded[12..]);
                        body.extend(riff_chunk(b"EXIF", &raw_exif));
                        body.extend(riff_chunk(b"XMP ", xmp));
                        let mut result = b"RIFF".to_vec();
                        result.extend((body.len() as u32).to_le_bytes());
                        result.extend(body);
                        std::fs::write(&path, result).unwrap();
                    }
                    _ => unreachable!(),
                }
                let original = std::fs::read(&path).unwrap();
                let opened = open(&path).unwrap();
                assert_eq!(
                    opened
                        .document
                        .source_metadata
                        .exif
                        .as_deref()
                        .unwrap()
                        .as_slice(),
                    raw_exif
                );
                assert_eq!(
                    opened
                        .document
                        .source_metadata
                        .xmp
                        .as_deref()
                        .unwrap()
                        .as_slice(),
                    xmp
                );
                assert_eq!(
                    opened.document.source_icc.as_deref().unwrap().as_slice(),
                    profile
                );
                let metadata = opened.document.source_metadata.clone();
                let mut editor = crate::document::Editor::new(opened.document, false);
                let decoded = editor.document.composite();
                if extension != "jpg" {
                    let mut expected = image::DynamicImage::ImageRgba8(image.clone());
                    expected.apply_orientation(
                        image::metadata::Orientation::from_exif(orientation as u8).unwrap(),
                    );
                    assert_eq!(decoded, expected.to_rgba8());
                }
                editor.edit("rename", |doc| doc.layers[0].name = "metadata".into());
                editor.undo();
                editor.redo();
                assert!(Arc::ptr_eq(
                    metadata.exif.as_ref().unwrap(),
                    editor.document.source_metadata.exif.as_ref().unwrap()
                ));
                assert!(Arc::ptr_eq(
                    metadata.xmp.as_ref().unwrap(),
                    editor.document.source_metadata.xmp.as_ref().unwrap()
                ));
                let project = format!("{name}.rphoto");
                save_project(&editor.document, &root.join(&project)).unwrap();
                let restored = load_project(&root.join(&project)).unwrap();
                assert_eq!(
                    restored.source_metadata.exif.as_deref().unwrap().as_slice(),
                    raw_exif
                );
                assert_eq!(
                    restored.source_metadata.xmp.as_deref().unwrap().as_slice(),
                    xmp
                );
                assert_eq!(restored.composite(), decoded);
                assert_eq!(
                    open(&root.join(&project)).unwrap().document.composite(),
                    decoded
                );
                let raw = format!("{name}.rgba8");
                std::fs::write(root.join(&raw), decoded.as_raw()).unwrap();
                assert_eq!(std::fs::read(&path).unwrap(), original);
                cases.push(serde_json::json!({"file":name,"project":project,"raw":raw,"width":decoded.width(),"height":decoded.height(),"orientation":orientation,"little":little}));
            }
        }
    }
    // Native bounded TIFF tag reading also keeps a packet larger than tiny pixel buffers.
    let path = root.join("large-xmp.tiff");
    let mut writer =
        tiff::encoder::TiffEncoder::new(std::fs::File::create(&path).unwrap()).unwrap();
    let mut frame = writer
        .new_image::<tiff::encoder::colortype::RGB8>(2, 2)
        .unwrap();
    let packet = format!(
        "<x:xmpmeta xmlns:x='adobe:ns:meta/'><!--{}--></x:xmpmeta>",
        "촬영".repeat(2000)
    );
    frame
        .encoder()
        .write_tag(tiff::tags::Tag::Unknown(700), packet.as_bytes())
        .unwrap();
    frame
        .write_data(&[10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120])
        .unwrap();
    let opened = open(&path).unwrap();
    assert_eq!(
        opened
            .document
            .source_metadata
            .xmp
            .as_deref()
            .unwrap()
            .as_slice(),
        packet.as_bytes()
    );
    assert!(opened.document.source_metadata.exif.is_none());
    let project = "large-xmp.tiff.rphoto";
    save_project(&opened.document, &root.join(project)).unwrap();
    assert_eq!(
        load_project(&root.join(project)).unwrap().composite(),
        opened.document.composite()
    );
    cases.push(serde_json::json!({"file":"large-xmp.tiff","project":project}));
    assert_eq!(cases.len(), 49);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
    // No source metadata: existing default exports and project versions are unchanged.
    let doc = crate::document::Document::from_image("plain".into(), image);
    let path = root.join("plain.png");
    export(
        &doc,
        &path,
        image::ImageFormat::Png,
        &ExportOptions {
            include_icc: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        open(&path)
            .unwrap()
            .document
            .source_metadata
            .record()
            .is_empty()
    );
}

#[test]
fn metadata_asset_limits_crc_missing_version_and_atomic_destination_are_checked() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/source-metadata/errors");
    std::fs::create_dir_all(&root).unwrap();
    let mut doc = crate::document::Document::new(2, 2, true).unwrap();
    let mut notes = vec![];
    doc.source_metadata =
        Metadata::from_bytes(Some(exif(6, true)), Some(vec![0, 255, 13, 10]), &mut notes);
    let valid = root.join("valid.rphoto");
    save_project(&doc, &valid).unwrap();
    let original = std::fs::read(&valid).unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&original)).unwrap();
    let mut entries = vec![];
    for i in 0..archive.len() {
        let mut asset = archive.by_index(i).unwrap();
        let name = asset.name().to_owned();
        let mut bytes = vec![];
        asset.read_to_end(&mut bytes).unwrap();
        entries.push((name, bytes));
    }
    for kind in ["exif", "xmp"] {
        let name = format!("metadata/source.{kind}");
        for label in ["missing", "oversize", "old-version"] {
            let path = root.join(format!("{kind}-{label}.rphoto"));
            let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            for (entry, bytes) in &entries {
                if label == "missing" && entry == &name {
                    continue;
                }
                let mut data = bytes.clone();
                if label == "oversize" && entry == &name {
                    data = vec![0; MAX_BYTES + 1];
                }
                if label == "old-version" && entry == "manifest.json" {
                    let mut manifest: serde_json::Value = serde_json::from_slice(&data).unwrap();
                    manifest["version"] = serde_json::json!(12);
                    data = serde_json::to_vec(&manifest).unwrap();
                }
                writer
                    .start_file(
                        entry,
                        zip::write::SimpleFileOptions::default()
                            .compression_method(zip::CompressionMethod::Stored),
                    )
                    .unwrap();
                writer.write_all(&data).unwrap();
            }
            writer.finish().unwrap();
            assert!(load_project(&path).is_err(), "{kind}/{label}");
        }
        let asset = if kind == "exif" {
            doc.source_metadata.exif.as_ref().unwrap()
        } else {
            doc.source_metadata.xmp.as_ref().unwrap()
        };
        let mut broken = original.clone();
        let at = broken
            .windows(asset.len())
            .position(|p| p == asset.as_slice())
            .unwrap();
        broken[at] ^= 1;
        let path = root.join(format!("{kind}-corrupt.rphoto"));
        std::fs::write(&path, broken).unwrap();
        assert!(load_project(&path).is_err());
    }
    let loaded = load_project(&valid).unwrap();
    assert_eq!(
        loaded.source_metadata.xmp.as_deref().unwrap().as_slice(),
        [0, 255, 13, 10]
    );
    doc.source_metadata.exif = Some(Arc::new(vec![0; MAX_BYTES + 1]));
    assert!(save_project(&doc, &valid).is_err());
    assert_eq!(std::fs::read(&valid).unwrap(), original);
    let mut notes = vec![];
    let bounded = Metadata::from_bytes(
        Some(vec![0; MAX_BYTES + 1]),
        Some(vec![1, 2, 3]),
        &mut notes,
    );
    assert!(bounded.exif.is_none());
    assert_eq!(bounded.xmp.as_deref().unwrap().as_slice(), [1, 2, 3]);
    assert!(notes.iter().any(|n| n.contains("예산")));
    doc.source_metadata = Metadata::default();
    let legacy = root.join("without-metadata.rphoto");
    save_project(&doc, &legacy).unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(legacy).unwrap()).unwrap();
    let mut manifest = String::new();
    archive
        .by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["version"], 8);
    assert!(manifest.get("source_metadata").is_none());
}
