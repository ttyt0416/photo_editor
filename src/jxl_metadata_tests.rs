use super::*;
use crate::files::{load_project, save_project};
use std::sync::Arc;

fn root() -> std::path::PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jxl-metadata");
    std::fs::create_dir_all(&root).unwrap();
    root
}
fn boxed(kind: &[u8; 4], data: &[u8], mode: u8) -> Vec<u8> {
    let mut bytes = match mode {
        0 => ((8 + data.len()) as u32).to_be_bytes().to_vec(),
        1 => 1u32.to_be_bytes().to_vec(),
        _ => 0u32.to_be_bytes().to_vec(),
    };
    bytes.extend(kind);
    if mode == 1 {
        bytes.extend(((16 + data.len()) as u64).to_be_bytes());
    }
    bytes.extend(data);
    bytes
}
fn container() -> Vec<u8> {
    [SIGNATURE, &boxed(b"ftyp", b"jxl \0\0\0\0jxl ", 0)].concat()
}
fn raw(name: &str) -> Vec<u8> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/fixtures/jxl/{name}/input.jxl"));
    let bytes = std::fs::read(path).unwrap();
    if bytes.starts_with(b"\xff\x0a") {
        return bytes;
    }
    let mut at = 0;
    let mut raw = vec![];
    while at < bytes.len() {
        let (kind, body, next) = crate::jpeg2000_icc::next_box(&bytes, at).unwrap();
        if &kind == b"jxlc" {
            raw.extend(body);
        } else if &kind == b"jxlp" {
            raw.extend(&body[4..]);
        }
        at = next;
    }
    assert!(raw.starts_with(b"\xff\x0a"));
    raw
}
fn source_exif(little: bool) -> Vec<u8> {
    let mut bytes = if little { b"II\x2a\0" } else { b"MM\0\x2a" }.to_vec();
    let word = |n: u16| {
        if little {
            n.to_le_bytes()
        } else {
            n.to_be_bytes()
        }
    };
    let long = |n: u32| {
        if little {
            n.to_le_bytes()
        } else {
            n.to_be_bytes()
        }
    };
    bytes.extend(long(8));
    bytes.extend(word(2));
    bytes.extend(word(0x10f));
    bytes.extend(word(2));
    bytes.extend(long(10));
    bytes.extend(long(38));
    bytes.extend(word(0x112));
    bytes.extend(word(3));
    bytes.extend(long(1));
    bytes.extend(word(8));
    bytes.extend([0, 0]);
    bytes.extend(long(0));
    bytes.extend(b"QA Camera\0");
    bytes
}
fn source_xmp() -> Vec<u8> {
    format!("<?xpacket begin='\u{feff}'?><r xmlns:t='http://ns.adobe.com/tiff/1.0/' t:Orientation='8' t:ImageWidth='7' t:ImageLength='5'><creator>촬영자</creator><!--{}--></r><?xpacket end='w'?>", "opaque".repeat(12000)).into_bytes()
}
#[test]
fn jxl_original_exif_xmp_before_after_codestream_precision_orientation_and_project() {
    let root = root();
    let mut cases = vec![];
    let xmp = source_xmp();
    std::fs::write(root.join("source.xmp"), &xmp).unwrap();
    for name in [
        "alpha_nonpremultiplied",
        "alpha_premultiplied",
        "alpha_triangles",
        "grayscale_jpeg",
        "lossless_pfm",
        "sunset_logo",
    ] {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/jxl/{name}/input.jxl"));
        let expected = crate::jxl_input::read(&base).unwrap().pixels;
        let codestream = raw(name);
        for before in [false, true] {
            let exif = source_exif(before);
            let prefix = if before { &b"Exif\0\0"[..] } else { &b""[..] };
            let payload = [
                (prefix.len() as u32).to_be_bytes().as_slice(),
                prefix,
                &exif,
            ]
            .concat();
            let mut bytes = container();
            if !before {
                bytes.extend(boxed(b"jxlc", &codestream, 0));
            }
            bytes.extend(boxed(b"Exif", &payload, u8::from(before)));
            bytes.extend(boxed(b"xml ", &xmp, if before { 1 } else { 2 }));
            if before {
                bytes.extend(boxed(b"jxlc", &codestream, 0));
            }
            let file = format!("{name}-{before}.jxl");
            let path = root.join(&file);
            std::fs::write(&path, &bytes).unwrap();
            let decoded = crate::jxl_input::read(&path).unwrap();
            assert_eq!(decoded.pixels, expected);
            let decoded_raw = format!("{file}.decoded.rgba32f.le");
            std::fs::write(
                root.join(&decoded_raw),
                decoded
                    .pixels
                    .to_rgba32f()
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            assert_eq!(
                decoded.source_metadata.exif.as_ref().unwrap().as_slice(),
                exif
            );
            assert_eq!(
                decoded.source_metadata.xmp.as_ref().unwrap().as_slice(),
                xmp
            );
            let opened = crate::files::open(&path).unwrap();
            let doc = opened.document;
            let metadata = doc.source_metadata.xmp.clone().unwrap();
            let pixels = crate::precision::composite(&doc);
            let raw = format!("{file}.rgba32f.le");
            std::fs::write(
                root.join(&raw),
                pixels
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            let mut editor = crate::document::Editor::new(doc, false);
            editor.edit("레이어 이름", |d| d.layers[0].name = "편집됨".into());
            editor.undo();
            editor.redo();
            assert!(Arc::ptr_eq(
                &metadata,
                editor.document.source_metadata.xmp.as_ref().unwrap()
            ));
            let project = root.join(format!("{file}.rphoto"));
            save_project(&editor.document, &project).unwrap();
            let restored = load_project(&project).unwrap();
            assert_eq!(crate::precision::composite(&restored), pixels);
            assert_eq!(
                restored.source_metadata.exif.as_ref().unwrap().as_slice(),
                exif
            );
            assert_eq!(
                restored.source_metadata.xmp.as_ref().unwrap().as_slice(),
                xmp
            );
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            let exif_file = format!("{file}.exif");
            std::fs::write(root.join(&exif_file), &exif).unwrap();
            cases.push(serde_json::json!({"file":file,"raw":raw,"decoded":decoded_raw,"name":name,"before":before,"exif":exif_file,"w":pixels.width(),"h":pixels.height(),"depth":restored.layers[0].bit_depth}));
        }
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

#[test]
fn jxl_partial_codestream_unknown_boxes_and_bounded_invalid_metadata_keep_pixels() {
    let root = root().join("errors");
    std::fs::create_dir_all(&root).unwrap();
    let raw = raw("grayscale_jpeg");
    let base =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jxl/grayscale_jpeg/input.jxl");
    let expected = crate::jxl_input::read(&base).unwrap().pixels;
    let mut split = container();
    split.extend(boxed(b"xml ", &source_xmp(), 0));
    split.extend(boxed(
        b"jxlp",
        &[0u32.to_be_bytes().as_slice(), &raw[..raw.len() / 2]].concat(),
        0,
    ));
    split.extend(boxed(b"abcd", &vec![0; 100000], 1));
    split.extend(boxed(
        b"jxlp",
        &[
            0x80000001u32.to_be_bytes().as_slice(),
            &raw[raw.len() / 2..],
        ]
        .concat(),
        0,
    ));
    let path = root.join("split.jxl");
    std::fs::write(&path, &split).unwrap();
    let decoded = crate::jxl_input::read(&path).unwrap();
    assert_eq!(decoded.pixels, expected);
    assert_eq!(
        decoded.source_metadata.xmp.as_ref().unwrap().as_slice(),
        source_xmp()
    );
    for (name, metadata) in [
        (
            "duplicate",
            [boxed(b"xml ", b"one", 0), boxed(b"xml ", b"two", 0)].concat(),
        ),
        (
            "bad-offset",
            boxed(
                b"Exif",
                &[u32::MAX.to_be_bytes().as_slice(), &source_exif(true)].concat(),
                0,
            ),
        ),
        ("bad-header", boxed(b"Exif", b"\0\0\0\0wrong tiff", 0)),
        (
            "oversized",
            boxed(b"xml ", &vec![0; crate::source_metadata::MAX_BYTES + 1], 0),
        ),
        (
            "compressed",
            boxed(b"brob", b"xml deliberately not valid Brotli", 0),
        ),
    ] {
        // Before the image, invalid auxiliary data must never enter jxl-oxide.
        let bytes = [container(), metadata, boxed(b"jxlc", &raw, 0)].concat();
        let path = root.join(format!("{name}.jxl"));
        std::fs::write(&path, &bytes).unwrap();
        let decoded = crate::jxl_input::read(&path).unwrap();
        assert_eq!(decoded.pixels, expected);
        assert!(decoded.source_metadata.exif.is_none() && decoded.source_metadata.xmp.is_none());
        assert!(decoded.notes.iter().any(|n| n.contains("보관")));
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
    for suffix in [
        b"\0\0\0\x03xml ".as_slice(),
        b"\0\0\0\x01xml \xff\xff\xff\xff\xff\xff\xff\xff",
        b"\0\0\0\x20xml truncated",
    ] {
        let bytes = [container(), boxed(b"jxlc", &raw, 0), suffix.to_vec()].concat();
        let path = root.join("truncated-tail.jxl");
        std::fs::write(&path, &bytes).unwrap();
        let decoded = crate::jxl_input::read(&path).unwrap();
        assert_eq!(decoded.pixels, expected);
        assert!(decoded.notes.iter().any(|n| n.contains("검색")));
    }
    // The streaming pixel adapter returns only decoding boxes and avoids all
    // auxiliary allocations, irrespective of the caller's small read buffer.
    let path = root.join("split.jxl");
    let (mut reader, _) = open(&path, &mut vec![]).unwrap();
    let mut filtered: Vec<u8> = vec![];
    let mut small = [0; 3];
    loop {
        let n = reader.read(&mut small).unwrap();
        if n == 0 {
            break;
        }
        filtered.extend(&small[..n]);
    }
    assert!(!filtered.windows(4).any(|w| w == b"xml " || w == b"abcd"));
    assert!(filtered.windows(4).any(|w| w == b"jxlp"));
}

#[test]
fn jxl_brotli_exif_xmp_windows_quality_placement_projects_and_normalized_png() {
    let root = root().join("compressed");
    std::fs::create_dir_all(&root).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jxl-metadata");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap()).unwrap();
    let raw = raw("grayscale_jpeg");
    let original =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jxl/grayscale_jpeg/input.jxl");
    let baseline = crate::files::open(&original).unwrap().document;
    let expected = crate::precision::composite(&baseline);
    let mut cases = vec![];
    for case in manifest["cases"].as_array().unwrap() {
        let exif_br = std::fs::read(fixtures.join(case["exif"].as_str().unwrap())).unwrap();
        let xmp_br = std::fs::read(fixtures.join(case["xmp"].as_str().unwrap())).unwrap();
        let expected_exif = source_exif(case["little"].as_bool().unwrap());
        assert_eq!(
            decompress(&xmp_br, source_xmp().len()).unwrap(),
            source_xmp()
        );
        assert!(decompress(&xmp_br, source_xmp().len() - 1).is_err());
        for before in [false, true] {
            let mut bytes = container();
            if !before {
                bytes.extend(boxed(b"jxlc", &raw, 0));
            }
            bytes.extend(boxed(
                b"brob",
                &[b"Exif".as_slice(), &exif_br].concat(),
                u8::from(before),
            ));
            bytes.extend(boxed(
                b"brob",
                &[b"xml ".as_slice(), &xmp_br].concat(),
                if before { 1 } else { 2 },
            ));
            if before {
                bytes.extend(boxed(b"jxlc", &raw, 0));
            }
            let name = format!("q{}-w{}-b{before}.jxl", case["quality"], case["window"]);
            let path = root.join(&name);
            std::fs::write(&path, &bytes).unwrap();
            let opened = crate::files::open(&path).unwrap();
            assert!(
                !opened
                    .notes
                    .iter()
                    .any(|n| n.contains("압축 메타데이터 보관을 생략"))
            );
            let doc = opened.document;
            assert_eq!(crate::precision::composite(&doc), expected);
            assert_eq!(
                doc.source_metadata.exif.as_ref().unwrap().as_slice(),
                expected_exif
            );
            assert_eq!(
                doc.source_metadata.xmp.as_ref().unwrap().as_slice(),
                source_xmp()
            );
            let source = doc.source_metadata.xmp.clone().unwrap();
            save_project(&doc, &root.join(format!("{name}.rphoto"))).unwrap();
            let project = load_project(&root.join(format!("{name}.rphoto"))).unwrap();
            assert_eq!(crate::precision::composite(&project), expected);
            assert_eq!(project.source_metadata.exif, doc.source_metadata.exif);
            assert_eq!(project.source_metadata.xmp, doc.source_metadata.xmp);
            let mut compressed = None;
            for include in [false, true] {
                let png = root.join(format!("{name}-{include}.png"));
                crate::files::export(
                    &doc,
                    &png,
                    image::ImageFormat::Png,
                    &crate::files::ExportOptions {
                        include_exif: include,
                        include_xmp: include,
                        ..Default::default()
                    },
                )
                .unwrap();
                let restored = crate::files::open(&png).unwrap().document;
                assert_eq!(restored.source_metadata.exif.is_some(), include);
                assert_eq!(restored.source_metadata.xmp.is_some(), include);
                let pixels = crate::precision::composite(&restored);
                if let Some(baseline) = &compressed {
                    assert_eq!(&pixels, baseline);
                } else {
                    compressed = Some(pixels);
                }
            }
            assert!(Arc::ptr_eq(
                &source,
                doc.source_metadata.xmp.as_ref().unwrap()
            ));
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            cases.push(serde_json::json!({"file":name,"quality":case["quality"],"window":case["window"],"before":before,"little":case["little"],"exif_fixture":case["exif"],"xmp_fixture":case["xmp"]}));
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
fn jxl_brotli_truncation_trailing_expansion_large_window_duplicate_and_unknown_keep_pixels() {
    let root = root().join("compressed-errors");
    std::fs::create_dir_all(&root).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jxl-metadata");
    let encoded = std::fs::read(fixtures.join("q5-w22-xmp.br")).unwrap();
    let mut trailing = encoded.clone();
    trailing.push(0);
    let bomb = std::fs::read(fixtures.join("over-budget.br")).unwrap();
    let raw = raw("grayscale_jpeg");
    let expected = crate::jxl_input::read(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jxl/grayscale_jpeg/input.jxl"),
    )
    .unwrap()
    .pixels;
    for (index, encoded) in [
        encoded[..encoded.len() - 1].to_vec(),
        trailing,
        bomb,
        vec![0x11, 30],
        vec![255; 4],
    ]
    .into_iter()
    .enumerate()
    {
        assert!(decompress(&encoded, crate::source_metadata::MAX_BYTES).is_err());
        let bytes = [
            container(),
            boxed(b"brob", &[b"xml ".as_slice(), &encoded].concat(), 0),
            boxed(b"jxlc", &raw, 0),
        ]
        .concat();
        let path = root.join(format!("invalid-{index}.jxl"));
        std::fs::write(&path, &bytes).unwrap();
        let decoded = crate::jxl_input::read(&path).unwrap();
        assert_eq!(decoded.pixels, expected);
        assert!(decoded.source_metadata.xmp.is_none());
        assert!(
            decoded
                .notes
                .iter()
                .any(|n| n.contains("압축 메타데이터 보관을 생략"))
        );
    }
    let compressed = boxed(b"brob", &[b"xml ".as_slice(), &encoded].concat(), 0);
    let plain = boxed(b"xml ", &source_xmp(), 0);
    for (index, boxes) in [[plain.clone(), compressed.clone()], [compressed, plain]]
        .into_iter()
        .enumerate()
    {
        let bytes = [container(), boxes.concat(), boxed(b"jxlc", &raw, 0)].concat();
        let path = root.join(format!("duplicate-{index}.jxl"));
        std::fs::write(&path, &bytes).unwrap();
        let decoded = crate::jxl_input::read(&path).unwrap();
        assert_eq!(decoded.pixels, expected);
        assert!(decoded.source_metadata.xmp.is_none());
        assert!(decoded.notes.iter().any(|n| n.contains("여러 XMP")));
    }
    let bytes = [
        container(),
        boxed(b"brob", b"jumbnot valid Brotli", 0),
        boxed(b"jxlc", &raw, 0),
    ]
    .concat();
    let path = root.join("unknown.jxl");
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(crate::jxl_input::read(&path).unwrap().pixels, expected);
}
