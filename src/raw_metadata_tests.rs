use super::*;
use std::{path::Path, sync::Arc};
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/raw-metadata");
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn raw_shooting_exif_ifd_endian_lens_rationals_iso_project_pixels_original_and_failed_save() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(root.join("tests/fixtures/raw-metadata/manifest.json")).unwrap(),
    )
    .unwrap();
    let mut records = Vec::new();
    for case in manifest.as_array().unwrap() {
        let filename = case["file"].as_str().unwrap();
        let doc = crate::files::open(&root.join("tests/fixtures/raw-metadata").join(filename))
            .unwrap()
            .document;
        let source = doc.layers[0].raw_source.clone().unwrap();
        let shooting = &source.record.info.shooting;
        assert!(shooting.valid());
        let actual = serde_json::to_value(shooting).unwrap();
        for (key, expected) in case["expected"].as_object().unwrap() {
            assert_eq!(&actual[key], expected, "{filename} {key}");
        }
        assert_eq!(
            shooting.warning.is_some(),
            case["invalid"].as_bool().unwrap()
        );
        let baseline = crate::files::open(
            &root
                .join("tests/fixtures/raw")
                .join(case["base"].as_str().unwrap()),
        )
        .unwrap()
        .document;
        let pixels = doc.layers[0].precision.as_ref().unwrap();
        assert!(
            pixels
                .as_raw()
                .iter()
                .zip(baseline.layers[0].precision.as_ref().unwrap().as_raw())
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        let path = output().join(format!("{filename}.rphoto"));
        crate::files::save_project(&doc, &path).unwrap();
        let mut restored = crate::files::load_project(&path).unwrap();
        assert_eq!(
            restored.layers[0]
                .raw_source
                .as_ref()
                .unwrap()
                .record
                .info
                .shooting,
            *shooting
        );
        assert_eq!(
            restored.layers[0]
                .raw_source
                .as_ref()
                .unwrap()
                .bytes
                .as_ref(),
            source.bytes.as_ref()
        );
        assert!(
            restored.layers[0]
                .precision
                .as_ref()
                .unwrap()
                .as_raw()
                .iter()
                .zip(pixels.as_raw())
                .all(|(a, b)| a.to_bits() == b.to_bits())
        );
        let changed = crate::raw_input::Settings {
            exposure: 1.,
            ..Default::default()
        };
        let developed = source.develop(&changed).unwrap();
        assert!(developed.as_raw().iter().all(|v| v.is_finite()));
        assert_eq!(source.record.info.shooting, *shooting);
        let before = std::fs::read(&path).unwrap();
        let mut invalid = (*restored.layers[0].raw_source.as_ref().unwrap().as_ref()).clone();
        invalid.record.info.shooting.shutter = Some([1, 0]);
        restored.layers[0].raw_source = Some(Arc::new(invalid));
        assert!(crate::files::save_project(&restored, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        records.push(serde_json::json!({"file":filename,"shooting":shooting}));
    }
    std::fs::write(
        output().join("records.json"),
        serde_json::to_vec_pretty(&records).unwrap(),
    )
    .unwrap();
}
#[test]
fn raw_shooting_legacy_missing_values_unknown_fields_and_invalid_numbers_do_not_invent_metadata() {
    let exif = rawler::exif::Exif {
        iso_speed: Some(0),
        lens_model: Some("   ".into()),
        focal_length: Some(rawler::formats::tiff::Rational { n: 0, d: 1 }),
        ..Default::default()
    };
    let info = Shooting::from_exif(exif);
    assert!(info.lens_model.is_none() && info.iso.is_none() && info.focal_length.is_none());
    assert!(info.warning.is_some());
    assert!(info.valid());
    let mut bad = Shooting {
        shutter: Some([1, 0]),
        ..Default::default()
    };
    assert!(!bad.valid());
    bad.shutter = None;
    bad.lens_make = Some("x".repeat(4097));
    assert!(!bad.valid());
    assert!(serde_json::from_str::<Shooting>(r#"{"unexpected":1}"#).is_err());
    assert!(serde_json::from_str::<Shooting>("{}").unwrap().is_empty());
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc = crate::files::open(&root.join("tests/fixtures/raw/linear-32-o1.dng"))
        .unwrap()
        .document;
    let record = &doc.layers[0].raw_source.as_ref().unwrap().record;
    let value = serde_json::to_value(record).unwrap();
    assert!(value["info"].get("shooting").is_none());
    let old: crate::raw_input::Record = serde_json::from_value(value).unwrap();
    assert!(old.info.shooting.is_empty());
    assert!(old.valid());
}
#[test]
fn raw_shooting_information_labels_in_actual_raw_dialog_preserve_original_and_cancel() {
    use eframe::egui;
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let doc = crate::files::open(&root.join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let source = doc.layers[0].raw_source.clone().unwrap();
    let pixels = doc.layers[0].precision.clone().unwrap();
    let ctx = egui::Context::default();
    let mut panel =
        crate::raw_panel::Panel::new(1, doc.active, 0, source.clone(), pixels.clone(), &ctx);
    let mut action = None;
    for i in 0..3 {
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::Vec2::new(880., 620.),
                )),
                time: Some(i as f64 * 0.1),
                ..Default::default()
            },
            |_| action = panel.ui(&ctx),
        );
        let labels = out
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    Some(t.galley.job.text.as_str())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let lens = labels.contains(&"렌즈: Test Optics Known Prime 35mm");
        let shooting = labels.contains(&"35.0 mm · f/2.8 · 1/125 s · ISO 400");
        out.textures_delta.clear();
        if i == 2 {
            assert!(lens && shooting);
        }
        assert!(action.is_none());
    }
    let mut out = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::Vec2::new(880., 620.),
            )),
            time: Some(0.4),
            events: vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            ..Default::default()
        },
        |_| action = panel.ui(&ctx),
    );
    out.textures_delta.clear();
    assert!(matches!(action, Some(crate::raw_panel::Action::Cancel)));
    assert!(Arc::ptr_eq(&panel.source, &source));
    assert!(Arc::ptr_eq(
        doc.layers[0].precision.as_ref().unwrap(),
        &pixels
    ));
    assert!(!panel.ready());
}
