use super::*;
fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw-white")
}
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/raw-white");
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn raw_dng_white_xy_analog_single_dual_sensor_modes_camera_metadata_and_projects() {
    let inputs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture().join("manifest.json")).unwrap()).unwrap();
    let mut cases = vec![];
    for item in inputs.as_array().unwrap() {
        let name = item["file"].as_str().unwrap();
        let doc = crate::files::open(&fixture().join(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .document;
        let source = doc.layers[0].raw_source.as_ref().unwrap();
        let shot = dng_white::info(&source.bytes).unwrap().unwrap();
        assert_eq!(source.record.info.camera_wb, shot.gains[..3]);
        assert_eq!(
            source.record.info.camera_wb_fourth,
            if shot.channels == 4 {
                Some(shot.gains[3])
            } else {
                None
            }
        );
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
                gains: [1.5, 1., 2.],
                gains_fourth: if item["channels"] == 4 {
                    Some(1.25)
                } else {
                    None
                },
                temperature: 4000.,
                tint: 30.,
                exposure: 1.5,
            };
            let pixels = source
                .develop(&settings)
                .unwrap_or_else(|e| panic!("{name}/{mode:?}: {e}"));
            assert!(pixels.as_raw().iter().all(|v| v.is_finite()));
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
            cases.push(serde_json::json!({"input":name,"settings":settings,"output":file}));
        }
        let sample = source.white_balance_at([0.5, 0.5], 1).unwrap();
        let settings = Settings {
            white_balance: WhiteBalance::Custom,
            gains: sample.gains,
            gains_fourth: sample.gains_fourth,
            ..Default::default()
        };
        let pixels = source.develop(&settings).unwrap();
        let file = format!("{name}-pick.rgba32.le");
        std::fs::write(
            output().join(&file),
            pixels
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        cases.push(serde_json::json!({"input":name,"settings":settings,"output":file,"focus":[0.5,0.5],"radius":1}));
        let path = output().join(format!("{name}.rphoto"));
        crate::files::save_project(&doc, &path).unwrap();
        let restored = crate::files::load_project(&path).unwrap();
        assert_eq!(
            *restored.layers[0].raw_source.as_ref().unwrap().bytes,
            *source.bytes
        );
        assert_eq!(
            restored.layers[0].precision.as_ref().unwrap().as_raw(),
            doc.layers[0].precision.as_ref().unwrap().as_raw()
        );
        assert_eq!(
            restored.layers[0]
                .raw_source
                .as_ref()
                .unwrap()
                .record
                .info
                .camera_wb,
            source.record.info.camera_wb
        );
    }
    std::fs::write(
        output().join("cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn raw_dng_white_xy_tag_bounds_physical_coordinates_mutual_exclusion_and_legacy_metadata() {
    let bytes =
        std::fs::read(fixture().join("raw-dual-linear-4c-32-ill21-17-analog-xyD65.dng")).unwrap();
    let white = dng_white::white(&bytes).unwrap().unwrap();
    assert!((white[0] - 0.31271 / 0.32902).abs() < 1e-12);
    assert!(dng_white::white(b"not dng").unwrap().is_none());
    let mut offset = 0;
    let mut directory = 0;
    for i in 0..u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize {
        let p = 10 + i * 12;
        if u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap()) == 50729 {
            directory = p;
            offset = u32::from_le_bytes(bytes[p + 8..p + 12].try_into().unwrap()) as usize;
        }
    }
    for (relative, value) in [(0, 0u32), (4, 0), (0, 100000)] {
        let mut bad = bytes.clone();
        bad[offset + relative..offset + relative + 4].copy_from_slice(&value.to_le_bytes());
        assert!(dng_white::white(&bad).is_err());
    }
    let mut bad = bytes.clone();
    bad[directory + 2..directory + 4].copy_from_slice(&3u16.to_le_bytes());
    assert!(dng_white::white(&bad).is_err());
    assert!(dng_white::white(&bytes[..offset + 8]).is_err());
    let mut conflict = bytes.clone();
    for i in 0..u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize {
        let p = 10 + i * 12;
        if u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap()) == 50727 {
            conflict[p..p + 2].copy_from_slice(&50728u16.to_le_bytes());
        }
    }
    assert!(dng_white::white(&conflict).unwrap_err().contains("동시에"));
    let mut source =
        (*crate::files::open(&fixture().join("raw-dual-linear-4c-32-ill21-17-analog-xyD65.dng"))
            .unwrap()
            .document
            .layers[0]
            .raw_source
            .clone()
            .unwrap())
        .clone();
    let original = source.record.info.clone();
    source.record.info.camera_wb = [1.; 3];
    source.record.info.camera_wb_fourth = None;
    source.refresh_shot_white();
    assert_eq!(source.record.info.camera_wb, original.camera_wb);
    assert_eq!(
        source.record.info.camera_wb_fourth,
        original.camera_wb_fourth
    );
    let mut broken = source.clone();
    broken.bytes = Arc::new(bad);
    let before = broken.record.info.clone();
    broken.refresh_shot_white();
    assert_eq!(broken.record.info.camera_wb, before.camera_wb);
}

#[test]
fn raw_dng_white_xy_legacy_project_metadata_refresh_preserves_saved_pixels() {
    for name in [
        "raw-dual-linear-3c-32-ill17-21-analog-xyD65.dng",
        "raw-dual-linear-4c-32-ill21-17-analog-xyD65.dng",
    ] {
        let mut doc = crate::files::open(&fixture().join(name)).unwrap().document;
        let expected = doc.layers[0]
            .raw_source
            .as_ref()
            .unwrap()
            .record
            .info
            .clone();
        let mut source = (**doc.layers[0].raw_source.as_ref().unwrap()).clone();
        source.record.info.camera_wb = [1.; 3];
        source.record.info.camera_wb_fourth = None;
        source.record.info.camera_colors = None;
        let bytes = source.bytes.clone();
        doc.layers[0].raw_source = Some(Arc::new(source));
        let before = doc.layers[0].precision.as_ref().unwrap().as_raw().clone();
        let path = output().join(format!("{name}-legacy.rphoto"));
        crate::files::save_project(&doc, &path).unwrap();
        let restored = crate::files::load_project(&path).unwrap();
        assert_eq!(
            restored.layers[0].precision.as_ref().unwrap().as_raw(),
            &before
        );
        let raw = restored.layers[0].raw_source.as_ref().unwrap();
        assert_eq!(raw.record.info.camera_wb, expected.camera_wb);
        assert_eq!(raw.record.info.camera_wb_fourth, expected.camera_wb_fourth);
        assert_eq!(*raw.bytes, *bytes);
        assert_eq!(raw.record.settings, Settings::default());
        crate::files::save_project(&restored, &output().join(format!("{name}-upgraded.rphoto")))
            .unwrap();
    }
}
