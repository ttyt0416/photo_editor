use super::*;
fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw-color")
}
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/raw-color");
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn raw_single_illuminant_three_four_channel_linear_cfa_balance_sensor_precision_and_sources() {
    let cases: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture().join("manifest.json")).unwrap()).unwrap();
    let mut results = vec![];
    for case in cases.as_array().unwrap() {
        let name = case["file"].as_str().unwrap();
        let doc = crate::files::open(&fixture().join(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .document;
        let source = doc.layers[0].raw_source.as_ref().unwrap();
        let bytes = source.bytes.clone();
        let fourth = case["channels"] == 4;
        assert_eq!(source.record.info.camera_wb_fourth.is_some(), fourth);
        if fourth {
            assert_eq!(
                source.record.info.camera_wb_fourth.unwrap(),
                case["wb"][3].as_f64().unwrap() as f32
            );
        }
        for (index, mode) in [
            WhiteBalance::Camera,
            WhiteBalance::Neutral,
            WhiteBalance::Custom,
            WhiteBalance::Temperature,
        ]
        .into_iter()
        .enumerate()
        {
            let settings = Settings {
                white_balance: mode,
                gains: [0.5, 2., 3.],
                gains_fourth: fourth.then_some(1.25),
                temperature: 4000.,
                tint: 30.,
                exposure: 1.,
            };
            let pixels = source
                .develop(&settings)
                .unwrap_or_else(|e| panic!("{name}/{mode:?}: {e}"));
            assert!(pixels.as_raw().iter().all(|v| v.is_finite()));
            assert!(pixels.pixels().all(|p| p[3] == 1.));
            let file = format!("{name}-{index}.rgba32.le");
            std::fs::write(
                output().join(&file),
                pixels
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            results.push(serde_json::json!({"input":name,"settings":settings,"output":file}));
        }
        assert!(Arc::ptr_eq(&source.bytes, &bytes));
        assert_eq!(source.record.settings, Settings::default());
        assert_eq!(*bytes, std::fs::read(fixture().join(name)).unwrap());
        let path = output().join(format!("{name}.rphoto"));
        crate::files::save_project(&doc, &path).unwrap();
        let restored = crate::files::load_project(&path).unwrap();
        assert_eq!(
            *restored.layers[0].raw_source.as_ref().unwrap().bytes,
            *bytes
        );
        assert_eq!(
            restored.layers[0]
                .raw_source
                .as_ref()
                .unwrap()
                .record
                .info
                .camera_wb_fourth,
            source.record.info.camera_wb_fourth
        );
        assert_eq!(
            restored.layers[0].precision.as_ref().unwrap().as_raw(),
            doc.layers[0].precision.as_ref().unwrap().as_raw()
        );
    }
    std::fs::write(
        output().join("cases.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
}
#[test]
fn raw_four_channel_camera_tag_validation_gain_settings_and_project_versions() {
    let bytes = std::fs::read(fixture().join("linear-4c-32-ill21.dng")).unwrap();
    assert_eq!(
        dng_balance::four_channel(&bytes).unwrap(),
        Some([2., 1., 4., 2.])
    );
    let short = std::fs::read(fixture().join("linear-4c-16-ill17.dng")).unwrap();
    assert_eq!(
        dng_balance::four_channel(&short).unwrap(),
        Some([1., 0.5, 1., 0.5])
    );
    let mut invalid = bytes.clone();
    let count = u16::from_le_bytes(invalid[8..10].try_into().unwrap()) as usize;
    for i in 0..count {
        let p = 10 + i * 12;
        if u16::from_le_bytes(invalid[p..p + 2].try_into().unwrap()) == 50728 {
            let at = u32::from_le_bytes(invalid[p + 8..p + 12].try_into().unwrap()) as usize;
            invalid[at + 28..at + 32].copy_from_slice(&0u32.to_le_bytes());
        }
    }
    assert!(
        dng_balance::four_channel(&invalid)
            .unwrap_err()
            .contains("분모")
    );
    let source = crate::files::open(&fixture().join("linear-4c-32-ill21.dng"))
        .unwrap()
        .document
        .layers[0]
        .raw_source
        .clone()
        .unwrap();
    assert!(
        Source {
            record: source.record.clone(),
            bytes: Arc::new(invalid)
        }
        .develop(&Settings::default())
        .is_err()
    );
    let settings = Settings {
        gains_fourth: Some(1.25),
        ..Default::default()
    };
    assert!(settings.valid());
    assert!(
        !Settings {
            gains_fourth: Some(f32::NAN),
            ..settings.clone()
        }
        .valid()
    );
    assert!(
        !Settings {
            gains_fourth: Some(33.),
            ..settings.clone()
        }
        .valid()
    );
    let record = source.record.clone();
    assert!(record.needs_v11());
    assert!(dng_balance::four_channel(b"not-a-dng").unwrap().is_none());
}

#[test]
fn raw_four_channel_v11_project_validation_legacy_enrichment_and_source_preservation() {
    use std::io::{Read, Write};
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
    let doc = crate::files::open(&fixture().join("linear-4c-32-ill17.dng"))
        .unwrap()
        .document;
    let path = output().join("validation.rphoto");
    crate::files::save_project(&doc, &path).unwrap();
    let previous = std::fs::read(&path).unwrap();
    let mut archive = ZipArchive::new(std::io::Cursor::new(&previous)).unwrap();
    for mode in 0..4 {
        let edited = output().join(format!("version-{mode}.rphoto"));
        let mut writer = ZipWriter::new(std::fs::File::create(&edited).unwrap());
        for i in 0..archive.len() {
            let mut asset = archive.by_index(i).unwrap();
            let name = asset.name().to_owned();
            let mut data = vec![];
            asset.read_to_end(&mut data).unwrap();
            if name == "manifest.json" {
                let mut json: serde_json::Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(json["version"], 11);
                match mode {
                    0 => json["version"] = 10.into(),
                    1 => {
                        json["version"] = 9.into();
                        let info = json["layers"][0]["raw_source"]["info"]
                            .as_object_mut()
                            .unwrap();
                        info.remove("camera_wb_fourth");
                        info.remove("camera_colors");
                    }
                    2 => json["layers"][0]["raw_source"]["settings"]["gains_fourth"] = 33.into(),
                    _ => {
                        json["layers"][0]["raw_source"]["info"]["camera_colors"] =
                            serde_json::json!([0, 1, 2, 255])
                    }
                }
                data = serde_json::to_vec(&json).unwrap();
            }
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&data).unwrap();
        }
        writer.finish().unwrap();
        if mode == 1 {
            let restored = crate::files::load_project(&edited).unwrap();
            let raw = restored.layers[0].raw_source.as_ref().unwrap();
            assert_eq!(raw.record.info.camera_wb_fourth, Some(2.));
            assert!(raw.record.needs_v11());
            assert_eq!(raw.bytes, doc.layers[0].raw_source.as_ref().unwrap().bytes);
            assert_eq!(
                restored.layers[0].precision.as_ref().unwrap().as_raw(),
                doc.layers[0].precision.as_ref().unwrap().as_raw()
            );
            crate::files::save_project(&restored, &output().join("legacy-upgraded.rphoto"))
                .unwrap();
        } else {
            assert!(crate::files::load_project(&edited).is_err(), "mode {mode}");
        }
    }
    let mut invalid = doc.clone();
    Arc::make_mut(invalid.layers[0].raw_source.as_mut().unwrap())
        .record
        .settings
        .gains_fourth = Some(f32::INFINITY);
    assert!(crate::files::save_project(&invalid, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), previous);
}
#[test]
fn raw_camera_matrix_adaptation_neutral_direction_and_singular_guard() {
    let native = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.2, 0.4, 0.3]];
    let adapted = color_matrix::adapt_input(native, Illuminant::A).unwrap();
    let d65 = [0.95047, 1., 1.08883];
    let a = [1.0985, 1., 0.35585];
    for (i, row) in adapted.iter().enumerate() {
        let actual = row
            .iter()
            .zip(d65)
            .map(|(x, y)| f64::from(*x) * y)
            .sum::<f64>();
        let expected = native[i]
            .iter()
            .zip(a)
            .map(|(x, y)| f64::from(*x) * y)
            .sum::<f64>();
        assert!((actual - expected).abs() < 2e-7);
    }
    assert!(color_matrix::inverse([[1., 1., 1.]; 3]).is_err());
    assert!(color_matrix::inverse([[f64::NAN; 3]; 3]).is_err());
    assert!(color_matrix::adapt_input(native, Illuminant::Unknown).is_err());
    let bytes = std::fs::read(fixture().join("cfa-4c-16-ill213451.dng")).unwrap();
    assert_eq!(
        dng_balance::plane_colors(&bytes).unwrap(),
        Some([3, 4, 5, 1])
    );
    let mut invalid = bytes.clone();
    let n = u16::from_le_bytes(invalid[8..10].try_into().unwrap()) as usize;
    for i in 0..n {
        let at = 10 + i * 12;
        if u16::from_le_bytes(invalid[at..at + 2].try_into().unwrap()) == 50710 {
            invalid[at + 8..at + 12].copy_from_slice(&[0, 1, 1, 3]);
        }
    }
    assert!(dng_balance::plane_colors(&invalid).is_err());
}

#[test]
fn raw_four_channel_picker_all_illuminants_planes_fourth_gain_and_project() {
    let inputs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture().join("manifest.json")).unwrap()).unwrap();
    let mut cases = vec![];
    for item in inputs
        .as_array()
        .unwrap()
        .iter()
        .filter(|v| v["channels"] == 4)
    {
        let name = item["file"].as_str().unwrap();
        let mut doc = crate::files::open(&fixture().join(name)).unwrap().document;
        let source = doc.layers[0].raw_source.clone().unwrap();
        let sample = source.white_balance_at([0.5, 0.5], 1).unwrap();
        assert!(sample.gains_fourth.is_some());
        assert_eq!(sample.gains[1], 1.);
        let settings = Settings {
            white_balance: WhiteBalance::Custom,
            gains: sample.gains,
            gains_fourth: sample.gains_fourth,
            ..Default::default()
        };
        let pixels = source.develop(&settings).unwrap();
        let file = format!("{name}-picked.rgba32.le");
        std::fs::write(
            output().join(&file),
            pixels
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let mut changed = (*source).clone();
        changed.record.settings = settings.clone();
        doc.layers[0].raw_source = Some(Arc::new(changed));
        doc.layers[0].pixels = Arc::new(crate::precision::preview(&pixels));
        doc.layers[0].precision = Some(Arc::new(pixels));
        let native = format!("{name}-picked.rphoto");
        crate::files::save_project(&doc, &output().join(&native)).unwrap();
        let restored = crate::files::load_project(&output().join(&native)).unwrap();
        assert_eq!(
            restored.layers[0]
                .raw_source
                .as_ref()
                .unwrap()
                .record
                .settings,
            settings
        );
        assert_eq!(
            *restored.layers[0].raw_source.as_ref().unwrap().bytes,
            *source.bytes
        );
        assert_eq!(
            restored.layers[0].precision.as_ref().unwrap().as_raw(),
            doc.layers[0].precision.as_ref().unwrap().as_raw()
        );
        cases.push(serde_json::json!({"input":name,"settings":settings,"output":file,"project":native,"focus":[0.5,0.5],"radius":1}));
    }
    std::fs::write(
        output().join("pick-cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
