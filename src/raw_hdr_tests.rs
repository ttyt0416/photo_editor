use super::*;
#[test]
fn raw_hdr_finite_sensor_maximum_exposure_white_balance_and_signed_conversion() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let fixture = root.join("tests/fixtures/raw-hdr");
    let output = root.join("qa-output/raw-hdr");
    std::fs::create_dir_all(&output).unwrap();
    let cases: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture.join("manifest.json")).unwrap()).unwrap();
    let mut results = vec![];
    for case in cases.as_array().unwrap() {
        let name = case["file"].as_str().unwrap();
        let document = crate::files::open(&fixture.join(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .document;
        let source = document.layers[0].raw_source.as_ref().unwrap();
        let original = source.bytes.clone();
        for (mode_index, mode) in [
            WhiteBalance::Camera,
            WhiteBalance::Neutral,
            WhiteBalance::Custom,
            WhiteBalance::Temperature,
        ]
        .into_iter()
        .enumerate()
        {
            for (ev_index, exposure) in [-16., 0., 16.].into_iter().enumerate() {
                let settings = Settings {
                    white_balance: mode,
                    gains: [32., 0.01, 16.],
                    temperature: 2856.,
                    tint: 30.,
                    exposure,
                    ..Default::default()
                };
                let pixels = source
                    .develop(&settings)
                    .unwrap_or_else(|e| panic!("{name}/{mode:?}/{exposure}: {e}"));
                assert!(pixels.as_raw().iter().all(|v| v.is_finite()));
                assert!(pixels.pixels().all(|p| p[3] == 1.));
                let file = format!("{name}-{mode_index}-{ev_index}.rgba32.le");
                std::fs::write(
                    output.join(&file),
                    pixels
                        .as_raw()
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                results.push(serde_json::json!({"input":name,"settings":settings,"output":file}));
            }
        }
        assert!(Arc::ptr_eq(&source.bytes, &original));
        assert_eq!(source.record.settings, Settings::default());
        assert_eq!(*original, std::fs::read(fixture.join(name)).unwrap());
    }
    std::fs::write(
        output.join("cases.json"),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
}

#[test]
fn raw_hdr_wide_color_arithmetic_four_channels_cancellation_storage_and_invalid() {
    let max = f32::MAX;
    let identity = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]];
    for ev in [-16., 0., 16.] {
        let value =
            convert_pixel(&[max, max, max], &identity, &[32., 16., 8., 1.], ev, false).unwrap();
        assert!(value[..3].iter().all(|v| v.is_finite() && *v > 1.));
    }
    let matrix = [[1., 1., -1., 0.5], [-1., -1., 1., -0.5], [0., 0., 0., 1.]];
    let p = [max, max, max, max];
    let value = convert_pixel(&p, &matrix, &[1.; 4], 16., false).unwrap();
    assert!(value[0].is_finite() && value[0] > 1.);
    assert_eq!(value[1], -f32::MAX);
    assert!(value[2].is_finite() && value[2] > 1.);
    let cancel = [[max, max, -max, -max], [0., 1., 0., 0.], [0., 0., 1., 0.]];
    let result = convert_pixel(&[max; 4], &cancel, &[1.; 4], 16., false).unwrap();
    assert_eq!(result[0], 0.);
    let tiny = f32::from_bits(1);
    let result = convert_pixel(&[tiny; 3], &identity, &[32.; 4], 16., false).unwrap();
    assert!(result[..3].iter().all(|v| *v > tiny));
    for pixel in [vec![], vec![f32::NAN], vec![1.; 5]] {
        assert!(convert_pixel(&pixel, &identity, &[1.; 4], 0., false).is_err());
    }
    assert!(convert_pixel(&[1.; 3], &identity, &[1.; 4], 17., false).is_err());
    assert!(convert_pixel(&[1.; 3], &identity, &[f32::INFINITY; 4], 0., false).is_err());
}
#[test]
fn raw_hdr_project_native_psd_psb_exr_and_single_history_keep_precision_source() {
    use crate::{document::Editor, files, precision};
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = root.join("qa-output/raw-hdr");
    std::fs::create_dir_all(&dir).unwrap();
    let doc = files::open(&root.join("tests/fixtures/raw-hdr/linear-32-o1-hdr.dng"))
        .unwrap()
        .document;
    let before = doc.layers[0].precision.clone().unwrap();
    let source = doc.layers[0].raw_source.clone().unwrap();
    let bytes = source.bytes.clone();
    let mut editor = Editor::new(doc, false);
    let settings = Settings {
        white_balance: WhiteBalance::Custom,
        gains: [32., 0.01, 16.],
        exposure: 16.,
        ..Default::default()
    };
    let pixels = Arc::new(source.develop(&settings).unwrap());
    editor.edit("RAW 현상", |d| {
        let layer = d.active_layer_mut().unwrap();
        layer.pixels = Arc::new(precision::preview(&pixels));
        layer.precision = Some(pixels.clone());
        let mut changed = (*source).clone();
        changed.record.settings = settings.clone();
        layer.raw_source = Some(Arc::new(changed));
    });
    let native = dir.join("developed.rphoto");
    files::save_project(&editor.document, &native).unwrap();
    let restored = files::load_project(&native).unwrap();
    assert_eq!(
        restored.layers[0].precision.as_ref().unwrap().as_raw(),
        pixels.as_raw()
    );
    assert_eq!(
        *restored.layers[0].raw_source.as_ref().unwrap().bytes,
        *bytes
    );
    let rendered = precision::composite(&restored);
    for ext in ["psd", "psb", "exr"] {
        let path = dir.join(format!("developed.{ext}"));
        if ext == "exr" {
            files::export(
                &restored,
                &path,
                image::ImageFormat::OpenExr,
                &Default::default(),
            )
            .unwrap();
        } else {
            files::save_photoshop(&restored, &path).unwrap();
        }
        let actual = precision::composite(&files::open(&path).unwrap().document);
        assert_eq!(actual.as_raw(), rendered.as_raw());
    }
    editor.undo();
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &before
    ));
    assert!(Arc::ptr_eq(
        editor.document.layers[0].raw_source.as_ref().unwrap(),
        &source
    ));
    assert!(!editor.can_undo());
    editor.redo();
    assert_eq!(
        editor.document.layers[0]
            .precision
            .as_ref()
            .unwrap()
            .as_raw(),
        pixels.as_raw()
    );
    assert!(Arc::ptr_eq(
        &editor.document.layers[0].raw_source.as_ref().unwrap().bytes,
        &bytes
    ));
}
