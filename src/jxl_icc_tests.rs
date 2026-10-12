use crate::files::{load_project, save_project};
use std::{path::Path, sync::Arc};

fn boxed(kind: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    [
        ((payload.len() + 8) as u32).to_be_bytes().as_slice(),
        kind,
        payload,
    ]
    .concat()
}
#[test]
fn jxl_original_rgb_gray_icc_exact_history_project_metadata_and_native_pixels() {
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = base.join("qa-output/jxl-icc");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for name in ["bench_oriented_brg", "grayscale"] {
        let source = base.join(format!("tests/fixtures/jxl-icc/{name}"));
        let bytes = std::fs::read(source.join("input.jxl")).unwrap();
        let profile = std::fs::read(source.join("original.icc")).unwrap();
        let (reader, _) =
            crate::jxl_metadata::open(&source.join("input.jxl"), &mut vec![]).unwrap();
        let image = jxl_oxide::JxlImage::builder().read(reader).unwrap();
        std::fs::write(
            root.join(format!("{name}.rendered.icc")),
            image.rendered_icc(),
        )
        .unwrap();
        let mut codestream = vec![];
        if bytes.starts_with(b"\xff\x0a") {
            codestream = bytes.clone();
        } else {
            let mut at = 0;
            while at < bytes.len() {
                let (kind, body, next) = crate::jpeg2000_icc::next_box(&bytes, at).unwrap();
                if &kind == b"jxlc" {
                    codestream.extend(body);
                } else if &kind == b"jxlp" {
                    codestream.extend(&body[4..]);
                }
                at = next;
            }
        }
        for metadata in [false, true] {
            let path = root.join(format!("{name}-{metadata}.jxl"));
            let mut input = bytes.clone();
            let exif = b"II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x08\0\0\0\0\0\0\0";
            let xmp = b"<r xmlns:t='http://ns.adobe.com/tiff/1.0/' t:Orientation='8' t:ImageWidth='7' t:ImageLength='5'><creator>ICC fixture</creator></r>";
            if metadata {
                input = boxed(b"JXL ", b"\x0d\x0a\x87\x0a");
                input.extend(boxed(b"ftyp", b"jxl \0\0\0\0jxl "));
                input.extend(boxed(
                    b"Exif",
                    &[0u32.to_be_bytes().as_slice(), exif].concat(),
                ));
                input.extend(boxed(b"jxlc", &codestream));
                input.extend(boxed(b"xml ", xmp));
            }
            std::fs::write(&path, &input).unwrap();
            let decoded = crate::jxl_input::read_native(&path).unwrap();
            assert_eq!(decoded.source_icc.as_ref().unwrap(), &profile);
            let pixels = decoded.pixels.to_rgba32f();
            let raw = format!("{name}-{metadata}.rgba32f.le");
            std::fs::write(
                root.join(&raw),
                pixels
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            let opened = crate::files::open(&path).unwrap();
            assert!(opened.notes.iter().any(|n| n.contains("원본 ICC")));
            let doc = opened.document;
            assert_eq!(doc.source_icc.as_ref().unwrap().as_slice(), profile);
            if metadata {
                assert_eq!(doc.source_metadata.exif.as_ref().unwrap().as_slice(), exif);
                assert_eq!(doc.source_metadata.xmp.as_ref().unwrap().as_slice(), xmp);
            }
            let source_profile = doc.source_icc.clone().unwrap();
            let source_pixels = doc.layers[0].pixels.clone();
            let expected = crate::precision::composite(&doc);
            let mut editor = crate::document::Editor::new(doc, false);
            editor.edit("레이어 이름", |d| {
                d.layers[0].name = "ICC preserved".into()
            });
            editor.undo();
            editor.redo();
            assert!(Arc::ptr_eq(
                &source_profile,
                editor.document.source_icc.as_ref().unwrap()
            ));
            assert!(Arc::ptr_eq(
                &source_pixels,
                &editor.document.layers[0].pixels
            ));
            let project_path = root.join(format!("{name}-{metadata}.rphoto"));
            save_project(&editor.document, &project_path).unwrap();
            let restored = load_project(&project_path).unwrap();
            assert_eq!(restored.source_icc.as_ref().unwrap().as_slice(), profile);
            assert_eq!(crate::precision::composite(&restored), expected);
            assert_eq!(
                restored.source_metadata.exif,
                editor.document.source_metadata.exif
            );
            assert_eq!(
                restored.source_metadata.xmp,
                editor.document.source_metadata.xmp
            );
            assert_eq!(std::fs::read(path).unwrap(), input);
            cases.push(serde_json::json!({"name":name,"metadata":metadata,"raw":raw,"w":pixels.width(),"h":pixels.height(),"depth":restored.layers[0].bit_depth}));
        }
    }
    let path = [
        "alpha_triangles",
        "lossless_pfm",
        "alpha_nonpremultiplied",
        "sunset_logo",
    ]
    .into_iter()
    .map(|name| base.join(format!("tests/fixtures/jxl/{name}/input.jxl")))
    .find(|path| {
        let (reader, _) = crate::jxl_metadata::open(path, &mut vec![]).unwrap();
        let image = jxl_oxide::JxlImage::builder().read(reader).unwrap();
        !image.image_header().metadata.colour_encoding.want_icc()
    })
    .expect("implicit color encoding fixture");
    let decoded = crate::jxl_input::read(&path).unwrap();
    assert!(decoded.source_icc.is_none());
    let doc = crate::files::open(&path).unwrap().document;
    assert!(doc.source_icc.is_none());
    save_project(&doc, &root.join("implicit.rphoto")).unwrap();
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
