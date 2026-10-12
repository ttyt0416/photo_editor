use super::*;
fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw-calibration")
}
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/raw-calibration");
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn raw_dng_camera_calibration_signatures_single_dual_analog_xy_modes_sources_and_projects() {
    let inputs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture().join("manifest.json")).unwrap()).unwrap();
    let mut cases = vec![];
    for item in inputs.as_array().unwrap() {
        let name = item["file"].as_str().unwrap();
        let doc = crate::files::open(&fixture().join(name))
            .unwrap_or_else(|e| panic!("{name}: {e}"))
            .document;
        let source = doc.layers[0].raw_source.as_ref().unwrap();
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
            if item["variant"] == "mismatch" || item["variant"] == "identity" {
                let original = crate::files::open(
                    &Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("tests/fixtures")
                        .join(item["source"].as_str().unwrap()),
                )
                .unwrap()
                .document;
                assert_eq!(
                    pixels,
                    original.layers[0]
                        .raw_source
                        .as_ref()
                        .unwrap()
                        .develop(&settings)
                        .unwrap(),
                    "{name}/{mode:?}"
                );
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
    }
    std::fs::write(
        output().join("cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn raw_dng_camera_calibration_signature_matrix_bounds_denominator_and_independent_blending() {
    let name = "raw-analog-raw-dual-linear-4c-32-ill21-17-analog-cc-match.dng";
    let bytes = std::fs::read(fixture().join(name)).unwrap();
    let (_, raw, _) = develop_sensor(&Arc::new(bytes.clone()), name, false).unwrap();
    let cal = dng_calibration::Calibration::read(&bytes, 4).unwrap();
    assert!(dng_calibration::Calibration::read(&bytes, 3).is_err());
    let native = color_profile::matrix_for_kelvin(&raw.color_matrix, 4000.)
        .unwrap()
        .unwrap();
    let corrected = cal.at_kelvin(native, 4000.).unwrap();
    assert!(
        corrected
            .as_flattened()
            .iter()
            .zip(native.as_flattened())
            .any(|(a, b)| (a - b).abs() > 1e-4)
    );
    // Interpolating the two composed endpoints is not equivalent to the DNG
    // rule that interpolates CM and CC separately before multiplication.
    let first = rawler::imgop::xyz::Illuminant::D65;
    let second = rawler::imgop::xyz::Illuminant::A;
    let endpoint = |light| {
        let mut m = [[0.; 3]; 4];
        for (row, input) in m
            .iter_mut()
            .zip(raw.color_matrix[&light].as_chunks::<3>().0)
        {
            row.copy_from_slice(input);
        }
        cal.at_light(m, light).unwrap()
    };
    let w = color_profile::weight(first, second, 4000.).unwrap();
    let a = endpoint(first);
    let b = endpoint(second);
    let combined: [[f32; 3]; 4] = std::array::from_fn(|i| {
        std::array::from_fn(|j| (w * f64::from(a[i][j]) + (1. - w) * f64::from(b[i][j])) as f32)
    });
    assert!(
        corrected
            .as_flattened()
            .iter()
            .zip(combined.as_flattened())
            .any(|(a, b)| (a - b).abs() > 1e-5)
    );
    let ((_, _, cc), (_, count, sig)) = (
        dng_balance::entry(&bytes, 50723).unwrap().unwrap(),
        dng_balance::entry(&bytes, 50931).unwrap().unwrap(),
    );
    let mut bad = bytes.clone();
    bad[cc + 4..cc + 8].copy_from_slice(&0u32.to_le_bytes());
    assert!(dng_calibration::Calibration::read(&bad, 4).is_err());
    let mut invalid = bytes.clone();
    invalid[sig] = 0xff;
    assert!(dng_calibration::Calibration::read(&invalid, 4).is_err());
    let mut unterminated = bytes.clone();
    unterminated[sig + count - 1] = b'x';
    assert!(dng_calibration::Calibration::read(&unterminated, 4).is_err());
    let mut embedded = bytes.clone();
    embedded[sig + 3] = 0;
    assert!(dng_calibration::Calibration::read(&embedded, 4).is_err());
    assert!(dng_calibration::Calibration::read(&bytes[..cc + 2], 4).is_err());
    let mismatch = std::fs::read(
        fixture().join("raw-analog-raw-dual-linear-4c-32-ill21-17-analog-cc-mismatch.dng"),
    )
    .unwrap();
    let (_, _, cc) = dng_balance::entry(&mismatch, 50723).unwrap().unwrap();
    let mut ignored = mismatch.clone();
    ignored[cc + 4..cc + 8].copy_from_slice(&0u32.to_le_bytes());
    let inactive = dng_calibration::Calibration::read(&ignored, 4).unwrap();
    assert_eq!(inactive.at_kelvin(native, 4000.).unwrap(), native);
}
