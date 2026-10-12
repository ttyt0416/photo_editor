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
