use super::*;
use crate::files::{ExportOptions, export, load_project, open, save_project};
use image::ImageEncoder;
use std::sync::Arc;

#[test]
fn source_icc_import_history_project_and_working_export_preserve_original_bytes() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/source-icc");
    std::fs::create_dir_all(&root).unwrap();
    let source = crate::precision::document(
        "Source ICC".into(),
        image::DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(7, 5, |x, y| {
            image::Rgba([
                (x * 7193 + y * 5311) as u16,
                (x * 271 + y * 9311) as u16,
                (x * 971 + y * 3711) as u16,
                if x == 0 { 32768 } else { 65535 },
            ])
        })),
    )
    .unwrap();
    let mut cases = vec![];
    for (extension, format, gray) in [
        ("png", Some(image::ImageFormat::Png), false),
        ("jpg", Some(image::ImageFormat::Jpeg), false),
        ("tiff", Some(image::ImageFormat::Tiff), false),
        ("webp", Some(image::ImageFormat::WebP), false),
        ("jp2", None, false),
        ("gray.jp2", None, true),
    ] {
        let name = format!("native.{extension}");
        let path = root.join(&name);
        let options = ExportOptions {
            jp2_icc: true,
            jp2_grayscale: gray,
            jp2_depth: 16,
            ..Default::default()
        };
        if let Some(format) = format {
            export(&source, &path, format, &options).unwrap();
        } else {
            crate::jpeg2000_output::save(&source, &path, false, &options).unwrap();
        }
        let original_bytes = std::fs::read(&path).unwrap();
        let opened = open(&path).unwrap();
        let profile = opened.document.source_icc.clone().unwrap();
        let expected = if gray { gray_profile() } else { srgb_profile() };
        assert_eq!(profile.as_slice(), expected);
        assert!(opened.notes.iter().any(|n| n.contains("프로젝트에 보존")));
        let mut editor = crate::document::Editor::new(opened.document, false);
        let before = crate::precision::composite(&editor.document);
        editor.edit("rename", |doc| doc.layers[0].name = "renamed".into());
        editor.undo();
        editor.redo();
        assert!(Arc::ptr_eq(
            &profile,
            editor.document.source_icc.as_ref().unwrap()
        ));
        assert_eq!(before, crate::precision::composite(&editor.document));
        let project_name = format!("{name}.rphoto");
        let project = root.join(&project_name);
        save_project(&editor.document, &project).unwrap();
        let restored = load_project(&project).unwrap();
        assert_eq!(restored.source_icc.as_deref(), Some(profile.as_ref()));
        assert_eq!(restored.document_id, editor.document.document_id);
        assert_eq!(crate::precision::composite(&restored), before);
        assert_eq!(
            crate::precision::composite(&open(&project).unwrap().document),
            before
        );
        assert_eq!(std::fs::read(&path).unwrap(), original_bytes);
        cases.push(serde_json::json!({"source":name,"project":project_name,"profile":if gray{"gray"}else{"rgb"}}));
    }
    for (label, profile) in [
        (
            "p3",
            crate::icc_fixture::encode(&moxcms::ColorProfile::new_display_p3()),
        ),
        ("invalid", vec![19u8; 128]),
    ] {
        let path = root.join(format!("{label}.png"));
        let mut encoder =
            image::codecs::png::PngEncoder::new(std::fs::File::create(&path).unwrap());
        encoder.set_icc_profile(profile.clone()).unwrap();
        let pixels = source.composite();
        encoder
            .write_image(pixels.as_raw(), 7, 5, image::ExtendedColorType::Rgba8)
            .unwrap();
        let opened = open(&path).unwrap();
        assert_eq!(
            opened.document.source_icc.as_deref().unwrap().as_slice(),
            profile
        );
        assert!(
            opened
                .notes
                .iter()
                .any(|n| n.contains(if label == "invalid" {
                    "적용하지 못"
                } else {
                    "sRGB로 변환"
                }))
        );
        let project_name = format!("{label}.rphoto");
        save_project(&opened.document, &root.join(&project_name)).unwrap();
        let restored = load_project(&root.join(&project_name)).unwrap();
        assert_eq!(restored.source_icc.as_deref().unwrap().as_slice(), profile);
        assert_eq!(
            crate::precision::composite(&restored),
            crate::precision::composite(&opened.document)
        );
        let exported = root.join(format!("{label}-working.png"));
        export(
            &restored,
            &exported,
            image::ImageFormat::Png,
            &ExportOptions::default(),
        )
        .unwrap();
        let mut decoder = image::codecs::png::PngDecoder::new(std::io::BufReader::new(
            std::fs::File::open(exported).unwrap(),
        ))
        .unwrap();
        use image::ImageDecoder;
        assert_eq!(
            decoder.icc_profile().unwrap().as_deref(),
            Some(srgb_profile())
        );
        cases.push(serde_json::json!({"source":format!("{label}.png"),"project":project_name,"profile":label}));
    }
    assert_eq!(cases.len(), 8);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

#[test]
fn source_icc_project_versions_missing_oversize_corrupt_assets_and_atomic_save() {
    use std::io::{Read, Write};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/source-icc/errors");
    std::fs::create_dir_all(&root).unwrap();
    let mut doc = crate::document::Document::new(2, 2, true).unwrap();
    doc.source_icc = Some(Arc::new(srgb_profile().to_vec()));
    let valid = root.join("valid.rphoto");
    save_project(&doc, &valid).unwrap();
    let baseline = std::fs::read(&valid).unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&baseline)).unwrap();
    let mut entries = vec![];
    for i in 0..zip.len() {
        let mut asset = zip.by_index(i).unwrap();
        let name = asset.name().to_owned();
        let mut bytes = vec![];
        asset.read_to_end(&mut bytes).unwrap();
        entries.push((name, bytes));
    }
    for label in ["old-version", "missing", "oversize", "unsupported-version"] {
        let path = root.join(format!("{label}.rphoto"));
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, original) in &entries {
            if label == "missing" && name == "metadata/source.icc" {
                continue;
            }
            let mut bytes = original.clone();
            if name == "manifest.json" && matches!(label, "old-version" | "unsupported-version") {
                let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                manifest["version"] =
                    serde_json::json!(if label == "old-version" { 11 } else { 15 });
                bytes = serde_json::to_vec(&manifest).unwrap();
            }
            if label == "oversize" && name == "metadata/source.icc" {
                bytes = vec![0; MAX_PROFILE_BYTES + 1];
            }
            writer
                .start_file(
                    name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap();
        assert!(load_project(&path).is_err(), "{label}");
    }
    let mut corrupt = baseline.clone();
    let at = corrupt
        .windows(srgb_profile().len())
        .position(|v| v == srgb_profile())
        .unwrap();
    corrupt[at + 20] ^= 1;
    let damaged = root.join("corrupt.rphoto");
    std::fs::write(&damaged, corrupt).unwrap();
    assert!(load_project(&damaged).is_err());
    doc.source_icc = Some(Arc::new(vec![0; MAX_PROFILE_BYTES + 1]));
    assert!(save_project(&doc, &valid).is_err());
    assert_eq!(std::fs::read(&valid).unwrap(), baseline);
    let mut notes = vec![];
    doc.source_icc = None;
    remember(&mut doc, Some(vec![0; MAX_PROFILE_BYTES + 1]), &mut notes);
    assert!(doc.source_icc.is_none());
    assert!(notes[0].contains("예산"));
    let legacy = root.join("without-profile.rphoto");
    save_project(&doc, &legacy).unwrap();
    assert!(load_project(&legacy).unwrap().source_icc.is_none());
    let mut zip = zip::ZipArchive::new(std::fs::File::open(legacy).unwrap()).unwrap();
    let mut manifest = String::new();
    zip.by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut manifest)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest).unwrap();
    assert_eq!(manifest["version"], 8);
    assert!(manifest.get("source_icc").is_none());
}
