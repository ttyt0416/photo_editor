use super::*;
fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw-analog")
}
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/raw-analog");
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn raw_dng_analog_color_sensor_modes_identity_four_channels_source_and_projects() {
    let inputs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture().join("manifest.json")).unwrap()).unwrap();
    let mut cases = vec![];
    for item in inputs.as_array().unwrap() {
        let name = item["file"].as_str().unwrap();
        let doc = crate::files::open(&fixture().join(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .document;
        let source = doc.layers[0].raw_source.as_ref().unwrap();
        let bytes = source.bytes.clone();
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
            if item["unit"] == true {
                let original = crate::files::open(
                    &Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures")
                        .join(item["source"].as_str().unwrap()),
                )
                .unwrap()
                .document;
                let expected = original.layers[0]
                    .raw_source
                    .as_ref()
                    .unwrap()
                    .develop(&settings)
                    .unwrap();
                assert_eq!(pixels, expected);
            }
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
        assert!(Arc::ptr_eq(&source.bytes, &bytes));
        let path = output().join(format!("{name}.rphoto"));
        crate::files::save_project(&doc, &path).unwrap();
        let restored = crate::files::load_project(&path).unwrap();
        assert_eq!(
            *restored.layers[0].raw_source.as_ref().unwrap().bytes,
            *bytes
        );
        assert_eq!(
            restored.layers[0].precision.as_ref().unwrap().as_raw(),
            doc.layers[0].precision.as_ref().unwrap().as_raw()
        );
    }
    std::fs::write(
        output().join("cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn raw_dng_analog_tag_bounds_non_dng_zero_denominator_channels_and_overflow() {
    let bytes = std::fs::read(fixture().join("raw-dual-linear-4c-32-ill21-17-analog.dng")).unwrap();
    assert_eq!(
        dng_balance::analog(&bytes, 4).unwrap(),
        Some([1.5, 0.8, 1.25, 7. / 6.])
    );
    assert!(dng_balance::analog(&bytes, 3).is_err());
    assert!(dng_balance::analog(b"no dng", 4).unwrap().is_none());
    let mut offset = 0;
    let mut directory = 0;
    for i in 0..u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize {
        let p = 10 + i * 12;
        if u16::from_le_bytes(bytes[p..p + 2].try_into().unwrap()) == 50727 {
            directory = p;
            offset = u32::from_le_bytes(bytes[p + 8..p + 12].try_into().unwrap()) as usize;
        }
    }
    let mut bad = bytes.clone();
    bad[offset + 4..offset + 8].copy_from_slice(&0u32.to_le_bytes());
    assert!(dng_balance::analog(&bad, 4).is_err());
    bad = bytes.clone();
    bad[offset..offset + 4].copy_from_slice(&0u32.to_le_bytes());
    assert!(dng_balance::analog(&bad, 4).is_err());
    bad = bytes.clone();
    bad[directory + 2..directory + 4].copy_from_slice(&3u16.to_le_bytes());
    assert!(dng_balance::analog(&bad, 4).is_err());
    assert!(dng_balance::analog(&bytes[..offset + 2], 4).is_err());
    let (_, mut raw, _) = develop_sensor(&Arc::new(bytes.clone()), "analog.dng", false).unwrap();
    raw.color_matrix.values_mut().for_each(|m| m.fill(f32::MAX));
    assert!(dng_balance::apply_analog(&mut raw, &bytes, 4).is_err());
    assert!(
        dng_balance::analog(
            &std::fs::read(fixture().join("raw-color-linear-4c-16-ill23-analog.dng")).unwrap(),
            4
        )
        .is_ok()
    );
}
