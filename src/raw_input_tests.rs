use super::*;
use crate::{document::Editor, files};
fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw")
}
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/raw");
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn dump(path: &Path, pixels: &Rgba32FImage) {
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
fn raw_sensor_dng_camera_wb_matrix_float_orientation_project_source_and_history() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root().join("manifest.json")).unwrap()).unwrap();
    for case in manifest.as_array().unwrap() {
        let name = case["file"].as_str().unwrap();
        let original = std::fs::read(root().join(name)).unwrap();
        let opened = files::open(&root().join(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(opened.project_path.is_none());
        assert!(opened.notes.iter().any(|s| s.contains("센서")));
        let doc = opened.document;
        let l = &doc.layers[0];
        assert_eq!(l.bit_depth, 32);
        let raw = l.raw_source.as_ref().unwrap();
        assert_eq!(*raw.bytes, original);
        assert_eq!(raw.record.info.bits, case["depth"].as_u64().unwrap() as u8);
        let (w, h) = if case["crop"].as_bool().unwrap() {
            (6, 4)
        } else {
            (12, 10)
        };
        let orientation = case["orientation"].as_u64().unwrap();
        let size = if orientation >= 5 { (h, w) } else { (w, h) };
        assert_eq!((doc.width, doc.height), size);
        let px = l.precision.as_ref().unwrap();
        assert!(px.pixels().all(|p| p[3] == 1.));
        dump(&output().join(format!("{name}.rgba32.le")), px);
        let project = output().join(format!("{name}.rphoto"));
        files::save_project(&doc, &project).unwrap();
        let restored = files::load_project(&project).unwrap();
        assert_eq!(restored.document_id, doc.document_id);
        assert_eq!(
            *restored.layers[0].raw_source.as_ref().unwrap().bytes,
            original
        );
        assert!(
            restored.layers[0]
                .precision
                .as_ref()
                .unwrap()
                .as_raw()
                .iter()
                .zip(px.as_raw())
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        let restored_raw = restored.layers[0].raw_source.as_ref().unwrap();
        assert_eq!(restored_raw.record.settings, Settings::default());
        let neutral = Settings {
            white_balance: WhiteBalance::Neutral,
            ..Default::default()
        };
        let result = restored_raw.develop(&neutral).unwrap();
        dump(&output().join(format!("{name}.neutral.rgba32.le")), &result);
        assert_eq!(result.dimensions(), px.dimensions());
        let source = restored_raw.clone();
        let mut editor = Editor::new(restored, false);
        let before = editor.document.layers[0].precision.clone().unwrap();
        editor.edit("RAW 현상", |d| {
            let l = d.active_layer_mut().unwrap();
            l.pixels = Arc::new(crate::precision::preview(&result));
            l.precision = Some(Arc::new(result));
            let mut changed = (*source).clone();
            changed.record.settings = neutral.clone();
            l.raw_source = Some(Arc::new(changed));
        });
        assert!(editor.can_undo());
        editor.undo();
        assert!(Arc::ptr_eq(
            editor.document.layers[0].precision.as_ref().unwrap(),
            &before
        ));
        assert_eq!(
            editor.document.layers[0]
                .raw_source
                .as_ref()
                .unwrap()
                .record
                .settings,
            Settings::default()
        );
        editor.redo();
        assert_eq!(
            editor.document.layers[0]
                .raw_source
                .as_ref()
                .unwrap()
                .record
                .settings,
            neutral
        );
        assert_eq!(std::fs::read(root().join(name)).unwrap(), original);
    }
}
#[test]
fn raw_exposure_custom_gains_preserve_hdr_original_and_reject_invalid_settings() {
    let opened = files::open(&root().join("linear-32-o1.dng")).unwrap();
    let raw = opened.document.layers[0].raw_source.as_ref().unwrap();
    let original = raw.bytes.clone();
    let settings = Settings {
        white_balance: WhiteBalance::Custom,
        gains: [0.5, 2., 3.],
        exposure: 2.,
        ..Default::default()
    };
    let pixels = raw.develop(&settings).unwrap();
    assert!(pixels.pixels().any(|p| p[1] > 1.));
    dump(&output().join("custom.rgba32.le"), &pixels);
    assert!(Arc::ptr_eq(&raw.bytes, &original));
    assert_eq!(raw.record.settings, Settings::default());
    for s in [
        Settings {
            exposure: f32::NAN,
            ..Default::default()
        },
        Settings {
            exposure: 17.,
            ..Default::default()
        },
        Settings {
            gains: [0., 1., 1.],
            ..Default::default()
        },
    ] {
        assert!(raw.develop(&s).is_err());
    }
    for prefix in [0, 7, 8, 16, 80, 180] {
        let source = Source {
            record: raw.record.clone(),
            bytes: Arc::new(original[..prefix].to_vec()),
        };
        assert!(source.develop(&Settings::default()).is_err());
    }
    let mut invalid = (*original).clone(); // Header width exceeding the current pixel budget.
    let n = u16::from_le_bytes(invalid[8..10].try_into().unwrap());
    for i in 0..n as usize {
        let p = 10 + i * 12;
        if u16::from_le_bytes(invalid[p..p + 2].try_into().unwrap()) == 256 {
            invalid[p + 8..p + 12].copy_from_slice(&u32::MAX.to_le_bytes());
        }
    }
    assert!(
        Source {
            record: raw.record.clone(),
            bytes: Arc::new(invalid)
        }
        .develop(&Settings::default())
        .is_err()
    );
}
#[test]
fn raw_project_version_assets_settings_and_failed_save_preserve_existing_file() {
    use std::io::{Read, Write};
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
    let doc = files::open(&root().join("linear-16-o1.dng"))
        .unwrap()
        .document;
    let project = output().join("validation.rphoto");
    files::save_project(&doc, &project).unwrap();
    let mut zip = ZipArchive::new(std::fs::File::open(&project).unwrap()).unwrap();
    let mut json = String::new();
    zip.by_name("manifest.json")
        .unwrap()
        .read_to_string(&mut json)
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(manifest["version"], 9);
    assert!(zip.by_name("layers/1-source.raw").is_ok());
    for mode in 0..4 {
        let bad = output().join(format!("invalid-{mode}.rphoto"));
        let mut out = ZipWriter::new(std::fs::File::create(&bad).unwrap());
        for i in 0..zip.len() {
            let mut asset = zip.by_index(i).unwrap();
            let name = asset.name().to_owned();
            if mode == 0 && name.ends_with("source.raw") {
                continue;
            }
            let mut data = Vec::new();
            asset.read_to_end(&mut data).unwrap();
            if name == "manifest.json" {
                let mut v = manifest.clone();
                match mode {
                    1 => v["version"] = 8.into(),
                    2 => v["layers"][0]["raw_source"]["settings"]["exposure"] = 99.into(),
                    3 => v["layers"][0]["raw_source"]["info"]["sensor"][0] = 0.into(),
                    _ => {}
                }
                data = serde_json::to_vec(&v).unwrap();
            }
            out.start_file(name, SimpleFileOptions::default()).unwrap();
            out.write_all(&data).unwrap();
        }
        out.finish().unwrap();
        assert!(files::load_project(&bad).is_err());
    }
    let bytes = std::fs::read(&project).unwrap();
    let mut invalid = doc.clone();
    let source = Arc::make_mut(invalid.layers[0].raw_source.as_mut().unwrap());
    source.record.settings.exposure = f32::INFINITY;
    assert!(files::save_project(&invalid, &project).is_err());
    assert_eq!(std::fs::read(&project).unwrap(), bytes);
    assert_eq!(
        EXTENSIONS.len(),
        rawler::decoders::supported_extensions().len()
    );
    for ext in rawler::decoders::supported_extensions() {
        assert!(crate::files::IMAGE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()));
    }
}
