use super::*;
use crate::{
    files,
    raw_input::{Settings, WhiteBalance},
};
use std::{path::Path, sync::Arc};
fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw")
}
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/raw-temperature");
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn raw_temperature_spectral_white_sensor_gains_invalid_and_four_channels() {
    assert_eq!(observer().len(), 471);
    // Standard incandescent A is approximately the 2856 K Planck white.
    let a = white_point(2856., 0.).unwrap();
    let sum = a.iter().sum::<f64>();
    assert!((a[0] / sum - 0.44757).abs() < 0.00005);
    assert!((a[1] / sum - 0.40745).abs() < 0.00005);
    let neutral = white_point(6500., 0.).unwrap();
    let plus = uv(white_point(6500., 100.).unwrap());
    let minus = uv(white_point(6500., -100.).unwrap());
    let center = uv(neutral);
    assert!(((plus[0] - center[0]).hypot(plus[1] - center[1]) - 0.01).abs() < 1e-12);
    assert!((minus[0] + plus[0] - 2. * center[0]).abs() < 1e-12);
    assert!(plus[1] > center[1]);
    // Synthetic four-channel camera: every balanced channel has the same green level.
    let matrix = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.2, 0.3, 0.4]];
    let gain = sensor_gains(&matrix, 4, neutral).unwrap();
    for i in 0..4 {
        let sensor = matrix[i]
            .iter()
            .zip(neutral)
            .map(|(a, b)| f64::from(*a) * b)
            .sum::<f64>();
        assert!((sensor * f64::from(gain[i]) - 1.).abs() < 1e-6);
    }
    let mut invalid = matrix;
    invalid[3] = [0.; 3];
    assert!(sensor_gains(&invalid, 4, neutral).is_err());
    invalid[0] = [f32::NAN, 0., 0.];
    assert!(sensor_gains(&invalid, 3, neutral).is_err());
    for (k, t) in [
        (f32::NAN, 0.),
        (999., 0.),
        (50001., 0.),
        (6500., 201.),
        (6500., f32::INFINITY),
    ] {
        assert!(white_point(k, t).is_err());
    }
    let points:Vec<_>=[1000.,2856.,6500.,15000.,50000.].into_iter().flat_map(|k|[-200.,0.,200.].map(move |t|serde_json::json!({"temperature":k,"tint":t,"white":white_point(k,t).ok()}))).collect();
    std::fs::write(
        output().join("white-points.json"),
        serde_json::to_vec_pretty(&points).unwrap(),
    )
    .unwrap();
}
#[test]
fn raw_temperature_twenty_dng_sensor_outputs_precision_source_and_project_versions() {
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture().join("manifest.json")).unwrap()).unwrap();
    let mut cases = vec![];
    for case in manifest.as_array().unwrap() {
        let name = case["file"].as_str().unwrap();
        let mut doc = files::open(&fixture().join(name)).unwrap().document;
        let raw = doc.layers[0].raw_source.clone().unwrap();
        let bytes = raw.bytes.clone();
        for (index, (k, t, ev)) in [
            (2000., 0., 0.),
            (2856., -50., -1.),
            (6500., 0., 0.),
            (6500., 100., 2.),
            (15000., 50., 1.),
            (50000., -100., 0.),
        ]
        .into_iter()
        .enumerate()
        {
            let settings = Settings {
                white_balance: WhiteBalance::Temperature,
                temperature: k,
                tint: t,
                exposure: ev,
                ..Default::default()
            };
            let pixels = raw
                .develop(&settings)
                .unwrap_or_else(|e| panic!("{name}/{k}/{t}: {e}"));
            assert!(pixels.as_raw().iter().all(|v| v.is_finite()));
            let filename = format!("{name}-{index}.rgba32.le");
            std::fs::write(
                output().join(&filename),
                pixels
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            cases.push(serde_json::json!({"input":name,"temperature":k,"tint":t,"exposure":ev,"output":filename}));
            if index == 3 {
                let mut source = (*raw).clone();
                source.record.settings = settings.clone();
                doc.layers[0].raw_source = Some(Arc::new(source));
                doc.layers[0].pixels = Arc::new(crate::precision::preview(&pixels));
                doc.layers[0].precision = Some(Arc::new(pixels));
                let path = output().join(format!("{name}.rphoto"));
                files::save_project(&doc, &path).unwrap();
                let restored = files::load_project(&path).unwrap();
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
                    *bytes
                );
                assert_eq!(
                    restored.layers[0].precision.as_ref().unwrap().as_raw(),
                    doc.layers[0].precision.as_ref().unwrap().as_raw()
                );
            }
        }
        assert!(Arc::ptr_eq(&raw.bytes, &bytes));
        assert_eq!(raw.record.settings, Settings::default());
    }
    std::fs::write(
        output().join("cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
    let old = r#"{"white_balance":"Camera","gains":[1,1,1],"exposure":0}"#;
    let settings: Settings = serde_json::from_str(old).unwrap();
    assert_eq!(settings, Settings::default());
    assert!(!settings.needs_v10());
    let old_json = serde_json::to_value(&settings).unwrap();
    assert!(old_json.get("temperature").is_none());
    assert!(old_json.get("tint").is_none());
    assert!(
        Settings {
            white_balance: WhiteBalance::Temperature,
            ..settings.clone()
        }
        .needs_v10()
    );
    assert!(
        Settings {
            temperature: 3000.,
            ..settings.clone()
        }
        .needs_v10()
    );
    for bad in [
        Settings {
            temperature: 0.,
            ..settings.clone()
        },
        Settings {
            tint: f32::NAN,
            ..settings.clone()
        },
    ] {
        assert!(!bad.valid());
    }
}
#[test]
fn raw_temperature_missing_matrix_is_explicit_and_monochrome_unchanged() {
    let open = files::open(&fixture().join("linear-32-o1.dng")).unwrap();
    let source = open.document.layers[0].raw_source.as_ref().unwrap();
    let (_, mut raw, _) =
        crate::raw_input::develop_sensor(&source.bytes, &source.record.filename, false).unwrap();
    raw.color_matrix.clear();
    assert!(gains(&raw, 3, 6500., 0.).unwrap_err().contains("색 행렬"));
    let mono = files::open(&fixture().join("mono-32-o6.dng")).unwrap();
    let source = mono.document.layers[0].raw_source.as_ref().unwrap();
    let camera = source.develop(&Settings::default()).unwrap();
    let temp = source
        .develop(&Settings {
            white_balance: WhiteBalance::Temperature,
            temperature: 1000.,
            tint: 200.,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(camera.as_raw(), temp.as_raw());
}

#[test]
fn raw_temperature_version_downgrade_and_bad_settings_rejected_save_failure_atomic() {
    use std::io::{Read, Write};
    use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};
    let mut doc = files::open(&fixture().join("linear-32-o1.dng"))
        .unwrap()
        .document;
    Arc::make_mut(doc.layers[0].raw_source.as_mut().unwrap())
        .record
        .settings = Settings {
        white_balance: WhiteBalance::Temperature,
        temperature: 4000.,
        tint: 25.,
        ..Default::default()
    };
    let path = output().join("version-validation.rphoto");
    files::save_project(&doc, &path).unwrap();
    let previous = std::fs::read(&path).unwrap();
    let mut archive = ZipArchive::new(std::io::Cursor::new(&previous)).unwrap();
    for mode in 0..5 {
        let bad = output().join(format!("invalid-{mode}.rphoto"));
        let mut writer = ZipWriter::new(std::fs::File::create(&bad).unwrap());
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let name = entry.name().to_owned();
            let mut bytes = vec![];
            entry.read_to_end(&mut bytes).unwrap();
            if name == "manifest.json" {
                let mut json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(json["version"], 10);
                match mode {
                    0 => json["version"] = 9.into(),
                    1 => json["layers"][0]["raw_source"]["settings"]["temperature"] = 999.into(),
                    2 => json["layers"][0]["raw_source"]["settings"]["tint"] = 201.into(),
                    3 => json["version"] = u32::MAX.into(),
                    _ => {
                        json["version"] = 9.into();
                        json["layers"][0]["raw_source"]["settings"]["white_balance"] =
                            "Custom".into();
                    }
                }
                bytes = serde_json::to_vec(&json).unwrap();
            }
            writer
                .start_file(name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap();
        assert!(files::load_project(&bad).is_err());
    }
    Arc::make_mut(doc.layers[0].raw_source.as_mut().unwrap())
        .record
        .settings
        .tint = f32::INFINITY;
    assert!(files::save_project(&doc, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), previous);
}
