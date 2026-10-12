use super::*;
use crate::files::{ExportOptions, load_project, open, save_project};
use std::sync::Arc;

fn output() -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jpeg2000-xmp");
    std::fs::create_dir_all(&root).unwrap();
    root
}
fn source() -> Vec<u8> {
    format!("<?xpacket begin='\u{feff}'?><r xmlns:t='http://ns.adobe.com/tiff/1.0/' xmlns:e='http://ns.adobe.com/exif/1.0/' xmlns:q='urn:qa' t:Orientation='6' t:ImageWidth='7' t:ImageLength='5' e:PixelXDimension='7' e:PixelYDimension='5'><q:creator>촬영자</q:creator><!--{}--></r><?xpacket end='w'?>", "opaque".repeat(12000)).into_bytes()
}
fn box_bytes(uuid: &[u8; 16], data: &[u8], mode: u8) -> Vec<u8> {
    let mut bytes = match mode {
        0 => ((24 + data.len()) as u32).to_be_bytes().to_vec(),
        1 => 1u32.to_be_bytes().to_vec(),
        _ => 0u32.to_be_bytes().to_vec(),
    };
    bytes.extend(b"uuid");
    if mode == 1 {
        bytes.extend(((32 + data.len()) as u64).to_be_bytes());
    }
    bytes.extend(uuid);
    bytes.extend(data);
    bytes
}
fn codestream(bytes: &[u8]) -> &[u8] {
    let mut at = 0;
    while at < bytes.len() {
        let (kind, body, next) = crate::jpeg2000_icc::next_box(bytes, at).unwrap();
        if &kind == b"jp2c" {
            return body;
        }
        at = next;
    }
    panic!("no codestream")
}
#[test]
fn jp2_xmp_source_uuid_lengths_unknown_boxes_history_and_project_preserved() {
    let root = output().join("inputs");
    std::fs::create_dir_all(&root).unwrap();
    let base = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/independent.jp2"),
    )
    .unwrap();
    let before = open(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/independent.jp2"),
    )
    .unwrap()
    .document;
    let original = source();
    std::fs::write(root.join("source.xmp"), &original).unwrap();
    for mode in 0..3 {
        let mut bytes = base.clone();
        bytes.extend(box_bytes(&[0; 16], b"unrelated uuid", 0));
        bytes.extend(box_bytes(&UUID, &original, mode));
        let path = root.join(format!("length-{mode}.jp2"));
        std::fs::write(&path, &bytes).unwrap();
        let opened = open(&path).unwrap();
        assert!(
            opened
                .notes
                .iter()
                .any(|n| n.contains("원본 촬영 메타데이터"))
        );
        let doc = opened.document;
        assert_eq!(
            doc.source_metadata.xmp.as_ref().unwrap().as_slice(),
            original
        );
        assert_eq!(
            crate::precision::composite(&doc),
            crate::precision::composite(&before)
        );
        let metadata = doc.source_metadata.xmp.clone().unwrap();
        let original_name = doc.layers[0].name.clone();
        let mut editor = crate::document::Editor::new(doc, false);
        editor.edit("사진 이름", |d| d.layers[0].name = "편집됨".into());
        assert!(Arc::ptr_eq(
            &metadata,
            editor.document.source_metadata.xmp.as_ref().unwrap()
        ));
        editor.undo();
        assert_eq!(editor.document.layers[0].name, original_name);
        assert!(Arc::ptr_eq(
            &metadata,
            editor.document.source_metadata.xmp.as_ref().unwrap()
        ));
        editor.redo();
        assert_eq!(editor.document.layers[0].name, "편집됨");
        let project = root.join(format!("length-{mode}.rphoto"));
        save_project(&editor.document, &project).unwrap();
        let restored = load_project(&project).unwrap();
        assert_eq!(
            restored.source_metadata.xmp.as_ref().unwrap().as_slice(),
            original
        );
        assert_eq!(
            crate::precision::composite(&restored),
            crate::precision::composite(&before)
        );
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    for data in [vec![255, 0], b"<not-xml".to_vec()] {
        let bytes = box_bytes(&UUID, &data, 0);
        assert_eq!(packet(&bytes).unwrap().unwrap(), data);
    }
    for bytes in [
        [box_bytes(&UUID, b"a", 0), box_bytes(&UUID, b"b", 0)].concat(),
        b"\0\0\0\x0auuidxx".to_vec(),
        b"\0\0\0\x20uuidtruncated".to_vec(),
        b"\0\0\0\x01uuid\xff\xff\xff\xff\xff\xff\xff\xff".to_vec(),
        box_bytes(&UUID, &vec![0; crate::source_metadata::MAX_BYTES + 1], 0),
    ] {
        let mut notes = vec![];
        assert!(read(&bytes, &mut notes).xmp.is_none());
        assert_eq!(notes.len(), 1);
        assert!(notes[0].contains("원본 JP2 XMP를 읽지 못했습니다"));
    }
    let mut duplicate = base;
    duplicate.extend(box_bytes(&UUID, b"a", 0));
    duplicate.extend(box_bytes(&UUID, b"b", 0));
    let path = root.join("duplicate.jp2");
    std::fs::write(&path, &duplicate).unwrap();
    let opened = open(&path).unwrap();
    assert!(opened.document.source_metadata.xmp.is_none());
    assert!(opened.notes.iter().any(|n| n.contains("여러 XMP")));
    assert_eq!(
        crate::precision::composite(&opened.document),
        crate::precision::composite(&before)
    );
}

#[test]
fn jp2_xmp_output_precision_color_alpha_tiles_quality_layers_and_payload_unchanged() {
    let root = output();
    let original = source();
    std::fs::write(root.join("source.xmp"), &original).unwrap();
    let mut cases = vec![];
    for (w, h) in [(17, 9), (129, 129)] {
        for gray in [false, true] {
            for depth in [8, 16] {
                for alpha in [false, true] {
                    for lossless in [false, true] {
                        let pixels = image::Rgba32FImage::from_fn(w, h, |x, y| {
                            let i = x + y * w;
                            image::Rgba([
                                (i * 7193 % 65536) as f32 / 65535.,
                                (i * 11317 % 65536) as f32 / 65535.,
                                (i * 17293 % 65536) as f32 / 65535.,
                                if alpha && i.is_multiple_of(7) {
                                    0.5
                                } else {
                                    1.
                                },
                            ])
                        });
                        let image = if depth == 8 {
                            image::DynamicImage::ImageRgba8(crate::precision::preview(&pixels))
                        } else {
                            image::DynamicImage::ImageRgba16(crate::precision::image16(&pixels))
                        };
                        let mut doc = crate::precision::document("JP2 XMP".into(), image).unwrap();
                        doc.source_metadata.xmp = Some(Arc::new(original.clone()));
                        let metadata = doc.source_metadata.xmp.clone().unwrap();
                        let pixels = doc.layers[0].pixels.clone();
                        let high = doc.layers[0].precision.clone();
                        for icc in [false, true] {
                            let mut reference = None;
                            for include in [false, true] {
                                let name = format!(
                                    "{w}x{h}-g{gray}-d{depth}-a{alpha}-l{lossless}-i{icc}-x{include}.jp2"
                                );
                                let path = root.join(&name);
                                let options = ExportOptions {
                                    jp2_depth: depth,
                                    jp2_grayscale: gray,
                                    jp2_lossless: lossless,
                                    jp2_ratio: 2,
                                    jp2_layers: 4,
                                    jp2_progression: 2,
                                    jp2_tile: if w > 128 { 128 } else { 0 },
                                    jp2_resolution_enabled: true,
                                    jp2_resolution: 300,
                                    jp2_icc: icc,
                                    include_xmp: include,
                                    ..Default::default()
                                };
                                crate::jpeg2000_output::save(&doc, &path, false, &options).unwrap();
                                let bytes = std::fs::read(&path).unwrap();
                                let opened = open(&path).unwrap();
                                assert_eq!((opened.document.width, opened.document.height), (w, h));
                                assert_eq!(opened.document.source_metadata.xmp.is_some(), include);
                                if include {
                                    assert_eq!(
                                        packet(&bytes).unwrap().unwrap(),
                                        crate::xmp_output::prepare(&doc, true).unwrap().unwrap()
                                    );
                                }
                                let decoded = crate::precision::composite(&opened.document);
                                if let Some((encoded, restored)) = &reference {
                                    assert_eq!(codestream(&bytes), encoded);
                                    assert_eq!(&decoded, restored);
                                } else {
                                    reference =
                                        Some((codestream(&bytes).to_vec(), decoded.clone()));
                                }
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
                                let project = root.join(format!("{name}.rphoto"));
                                save_project(&opened.document, &project).unwrap();
                                let restored = load_project(&project).unwrap();
                                assert_eq!(crate::precision::composite(&restored), decoded);
                                assert_eq!(
                                    restored.source_metadata.xmp,
                                    opened.document.source_metadata.xmp
                                );
                                assert!(Arc::ptr_eq(
                                    &metadata,
                                    doc.source_metadata.xmp.as_ref().unwrap()
                                ));
                                assert_eq!(metadata.as_slice(), original);
                                assert!(Arc::ptr_eq(&pixels, &doc.layers[0].pixels));
                                if let Some(high) = &high {
                                    assert!(Arc::ptr_eq(
                                        high,
                                        doc.layers[0].precision.as_ref().unwrap()
                                    ));
                                }
                                cases.push(serde_json::json!({"file":name,"raw":raw,"w":w,"h":h,"gray":gray,"depth":depth,"alpha":alpha,"lossless":lossless,"icc":icc,"include":include}));
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases.len(), 128);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

#[test]
fn jp2_invalid_xmp_keeps_destination_and_raw_j2k_ignores_metadata() {
    let root = output().join("errors");
    std::fs::create_dir_all(&root).unwrap();
    let mut doc = crate::document::Document::new(7, 5, true).unwrap();
    doc.source_metadata.xmp = Some(Arc::new(b"<broken".to_vec()));
    let path = root.join("existing.jp2");
    std::fs::write(&path, b"original").unwrap();
    assert!(crate::jpeg2000_output::save(&doc, &path, false, &ExportOptions::default()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    crate::jpeg2000_output::save(
        &doc,
        &path,
        false,
        &ExportOptions {
            include_xmp: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(open(&path).unwrap().document.source_metadata.xmp.is_none());
    for include in [false, true] {
        crate::jpeg2000_output::save(
            &doc,
            &root.join(format!("raw-{include}.j2k")),
            true,
            &ExportOptions {
                include_xmp: include,
                ..Default::default()
            },
        )
        .unwrap();
    }
    assert_eq!(
        std::fs::read(root.join("raw-false.j2k")).unwrap(),
        std::fs::read(root.join("raw-true.j2k")).unwrap()
    );
    assert!(
        packet(&std::fs::read(root.join("raw-true.j2k")).unwrap())
            .unwrap()
            .is_none()
    );
    assert!(
        std::fs::read_dir(root).unwrap().all(|p| !p
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp"))
    );
}
