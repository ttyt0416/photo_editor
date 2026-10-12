use super::*;
fn fixture() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw-dual")
}
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/raw-dual");
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn raw_dual_temperature_profile_outputs_tint_endpoints_slots_and_native_preservation() {
    let inputs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture().join("manifest.json")).unwrap()).unwrap();
    let mut cases = vec![];
    for item in inputs.as_array().unwrap() {
        let name = item["file"].as_str().unwrap();
        let mut doc = crate::files::open(&fixture().join(name)).unwrap().document;
        let source = doc.layers[0].raw_source.clone().unwrap();
        let bytes = source.bytes.clone();
        for (index, (k, t)) in [
            (2000., 0.),
            (2856., -50.),
            (4000., 75.),
            (6500., 0.),
            (10000., 50.),
            (50000., 0.),
        ]
        .into_iter()
        .enumerate()
        {
            let settings = Settings {
                white_balance: WhiteBalance::Temperature,
                temperature: k,
                tint: t,
                exposure: 1.,
                ..Default::default()
            };
            let pixels = source
                .develop(&settings)
                .unwrap_or_else(|e| panic!("{name}/{k}: {e}"));
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
            cases.push(serde_json::json!({"input":name,"settings":settings,"output":file}));
            if index == 2 {
                doc.layers[0].pixels = Arc::new(crate::precision::preview(&pixels));
                doc.layers[0].precision = Some(Arc::new(pixels.clone()));
                let mut changed = (*source).clone();
                changed.record.settings = settings.clone();
                doc.layers[0].raw_source = Some(Arc::new(changed));
            }
        }
        assert!(Arc::ptr_eq(&source.bytes, &bytes));
        assert_eq!(*bytes, std::fs::read(fixture().join(name)).unwrap());
        assert_eq!(source.record.settings, Settings::default());
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
        assert_eq!(
            restored.layers[0]
                .raw_source
                .as_ref()
                .unwrap()
                .record
                .settings,
            doc.layers[0].raw_source.as_ref().unwrap().record.settings
        );
    }
    std::fs::write(
        output().join("cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn raw_dual_profile_interpolation_cct_neutral_and_invalid_matrix_guards() {
    let bytes = Arc::new(std::fs::read(fixture().join("linear-4c-32-ill17-21.dng")).unwrap());
    let (_, mut raw, _) = develop_sensor(&bytes, "dual.dng", false).unwrap();
    let m1 = raw.color_matrix[&Illuminant::A].clone();
    let m2 = raw.color_matrix[&Illuminant::D65].clone();
    assert_eq!(
        color_profile::temperature_matrix(&raw, 1000.)
            .unwrap()
            .unwrap()
            .as_flattened(),
        m1
    );
    assert_eq!(
        color_profile::temperature_matrix(&raw, 50000.)
            .unwrap()
            .unwrap()
            .as_flattened(),
        m2
    );
    let a = temperature::correlated_temperature(color_matrix::white_point(Illuminant::A).unwrap());
    let b =
        temperature::correlated_temperature(color_matrix::white_point(Illuminant::D65).unwrap());
    let middle = (2. / (1. / a + 1. / b)) as f32;
    let matrix = color_profile::temperature_matrix(&raw, middle)
        .unwrap()
        .unwrap();
    for (v, (x, y)) in matrix.as_flattened().iter().zip(m1.iter().zip(&m2)) {
        assert!((*v - (*x + *y) * 0.5).abs() < 2e-7);
    }
    let mut ccts = vec![];
    for k in [1000., 2000., 2856., 6500., 15000., 50000.] {
        let xyz = temperature::white_point(k, 0.).unwrap();
        let got = temperature::correlated_temperature(xyz);
        assert!((got - f64::from(k)).abs() < 0.01);
        ccts.push(serde_json::json!({"white":xyz,"temperature":got}));
    }
    for light in [
        Illuminant::A,
        Illuminant::D50,
        Illuminant::D65,
        Illuminant::D75,
    ] {
        let xyz = color_matrix::white_point(light).unwrap();
        ccts.push(
            serde_json::json!({"white":xyz,"temperature":temperature::correlated_temperature(xyz)}),
        );
    }
    std::fs::write(
        output().join("ccts.json"),
        serde_json::to_vec_pretty(&ccts).unwrap(),
    )
    .unwrap();
    for (k, t) in [(2000., 0.), (4000., 75.), (10000., -50.)] {
        let native = color_profile::temperature_matrix(&raw, k).unwrap().unwrap();
        let white = temperature::white_point(k, t).unwrap();
        let prepared = temperature::prepare(&raw, 4, k, t).unwrap();
        let neutral = native.map(|row| {
            row.iter()
                .zip(white)
                .map(|(a, b)| f64::from(*a) * b)
                .sum::<f64>()
        });
        for row in prepared.matrix {
            let gray = row
                .iter()
                .zip(neutral)
                .zip(prepared.gains)
                .map(|((a, b), g)| f64::from(*a) * b * f64::from(g))
                .sum::<f64>();
            assert!((gray - neutral[1]).abs() < 2e-6);
        }
    }
    assert!(color_profile::temperature_matrix(&raw, f32::NAN).is_err());
    raw.color_matrix.insert(Illuminant::D50, m1.clone());
    assert!(color_profile::temperature_matrix(&raw, 4000.).is_err());
    raw.color_matrix.remove(&Illuminant::D50);
    raw.color_matrix.insert(Illuminant::A, vec![1.; 9]);
    assert!(color_profile::temperature_matrix(&raw, 4000.).is_err());
    assert!(temperature::prepare(&raw, 4, 4000., 0.).is_err());
    raw.color_matrix.insert(Illuminant::A, m1.clone());
    raw.color_matrix.insert(Illuminant::D65, vec![f32::NAN; 12]);
    assert!(color_profile::temperature_matrix(&raw, 4000.).is_err());
    raw.color_matrix.insert(Illuminant::D65, m2.clone());
    raw.color_matrix.remove(&Illuminant::A);
    raw.color_matrix.insert(Illuminant::Daylight, m1);
    assert!(color_profile::temperature_matrix(&raw, 4000.).is_err());
    raw.color_matrix.clear();
    assert!(
        color_profile::temperature_matrix(&raw, 4000.)
            .unwrap()
            .is_none()
    );
    assert!(temperature::prepare(&raw, 4, 4000., 0.).is_err());
    raw.color_matrix.insert(Illuminant::D65, vec![1.; 12]);
    assert!(temperature::prepare(&raw, 4, 4000., 0.).is_err());
}

#[test]
fn raw_dual_camera_profile_neutral_iteration_invalid_and_single_profile_compatibility() {
    let bytes = Arc::new(std::fs::read(fixture().join("linear-4c-32-ill17-21.dng")).unwrap());
    let (_, mut raw, _) = develop_sensor(&bytes, "dual.dng", false).unwrap();
    for k in [2000., 4000., 6500., 10000.] {
        let native = color_profile::temperature_matrix(&raw, k).unwrap().unwrap();
        let white = temperature::white_point(k, 0.).unwrap();
        let neutral = native.map(|row| {
            row.iter()
                .zip(white)
                .map(|(a, b)| f64::from(*a) * b)
                .sum::<f64>()
        });
        let gains = neutral.map(|v| (neutral[1] / v) as f32);
        let (_, got) = color_profile::balance_profile(&raw, 4, gains)
            .unwrap()
            .unwrap();
        for (a, b) in got.into_iter().zip(white) {
            assert!((a - b).abs() < 2e-6, "{k}: {got:?} {white:?}");
        }
    }
    assert!(color_profile::balance_profile(&raw, 4, [0., 1., 1., 1.]).is_err());
    assert!(color_profile::balance_profile(&raw, 3, [1.; 4]).is_err());
    let a = raw.color_matrix.remove(&Illuminant::A).unwrap();
    assert!(
        color_profile::balance_profile(&raw, 4, [1.; 4])
            .unwrap()
            .is_none()
    );
    raw.color_matrix.insert(Illuminant::A, a.clone());
    raw.color_matrix.insert(Illuminant::D50, a);
    assert!(color_profile::balance_profile(&raw, 4, [1.; 4]).is_err());
    raw.color_matrix.clear();
    raw.color_matrix.insert(Illuminant::A, vec![1.; 12]);
    raw.color_matrix.insert(Illuminant::D65, vec![1.; 12]);
    assert!(color_profile::balance_profile(&raw, 4, [1.; 4]).is_err());
    let low = [[10., 0., 0.], [0., 1., 0.], [0., 0., 0.1], [0., 0., 0.]];
    let high = [[0.1, 0., 0.], [0., 1., 0.], [0., 0., 10.], [0., 0., 0.]];
    raw.color_matrix
        .insert(Illuminant::A, low.as_flattened().to_vec());
    raw.color_matrix
        .insert(Illuminant::D65, high.as_flattened().to_vec());
    assert!(
        color_profile::balance_profile(&raw, 4, [1.; 4])
            .unwrap_err()
            .contains("수렴")
    );
    let matrix = [[-1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]];
    raw.color_matrix
        .insert(Illuminant::A, matrix.as_flattened().to_vec());
    raw.color_matrix
        .insert(Illuminant::D65, matrix.as_flattened().to_vec());
    assert!(
        color_profile::balance_profile(&raw, 4, [1.; 4])
            .unwrap_err()
            .contains("광원 색상")
    );
}
#[test]
fn raw_dual_camera_outputs_exposure_sources_native_versions_and_default_open() {
    let inputs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture().join("manifest.json")).unwrap()).unwrap();
    let mut cases = vec![];
    for item in inputs.as_array().unwrap() {
        let name = item["file"].as_str().unwrap();
        let doc = crate::files::open(&fixture().join(name)).unwrap().document;
        let source = doc.layers[0].raw_source.as_ref().unwrap();
        for (index, ev) in [-2., 0., 4.].into_iter().enumerate() {
            let settings = Settings {
                exposure: ev,
                ..Default::default()
            };
            let pixels = source
                .develop(&settings)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert!(pixels.as_raw().iter().all(|v| v.is_finite()));
            if ev == 0. {
                assert_eq!(
                    pixels.as_raw(),
                    doc.layers[0].precision.as_ref().unwrap().as_raw()
                );
            }
            let filename = format!("{name}-camera-{index}.rgba32.le");
            std::fs::write(
                output().join(&filename),
                pixels
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            cases.push(serde_json::json!({"input":name,"settings":settings,"output":filename}));
        }
        let path = output().join(format!("{name}-camera.rphoto"));
        crate::files::save_project(&doc, &path).unwrap();
        let restored = crate::files::load_project(&path).unwrap();
        assert_eq!(
            restored.layers[0].precision.as_ref().unwrap().as_raw(),
            doc.layers[0].precision.as_ref().unwrap().as_raw()
        );
        assert_eq!(
            *restored.layers[0].raw_source.as_ref().unwrap().bytes,
            *source.bytes
        );
    }
    std::fs::write(
        output().join("camera-cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

#[test]
fn raw_dual_custom_gain_outputs_exposure_native_and_picker_neutral_patches() {
    let inputs: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture().join("manifest.json")).unwrap()).unwrap();
    let mut cases = vec![];
    let mut picks = vec![];
    for item in inputs.as_array().unwrap() {
        let name = item["file"].as_str().unwrap();
        let doc = crate::files::open(&fixture().join(name)).unwrap().document;
        let source = doc.layers[0].raw_source.as_ref().unwrap();
        let bytes = source.bytes.clone();
        for (index, (g, g4)) in [
            ([1.5, 1., 2.], None),
            ([0.9, 1.2, 1.1], Some(1.25)),
            ([2., 1., 4.], Some(3.)),
        ]
        .into_iter()
        .enumerate()
        {
            let settings = Settings {
                white_balance: WhiteBalance::Custom,
                gains: g,
                gains_fourth: if item["channels"] == 4 { g4 } else { None },
                exposure: 1.,
                ..Default::default()
            };
            let pixels = source
                .develop(&settings)
                .unwrap_or_else(|e| panic!("{name}/{index}: {e}"));
            let file = format!("{name}-custom-{index}.rgba32.le");
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
        let points: &[[f32; 2]] = if item["cfa"] == true {
            &[[0.5, 0.5]]
        } else {
            &[[0.5, 0.5], [0.25, 0.75], [0.75, 0.25]]
        };
        for (index, focus) in points.iter().enumerate() {
            for radius in [0, 1] {
                let sample = source
                    .white_balance_at(*focus, radius)
                    .unwrap_or_else(|e| panic!("{name}/{focus:?}/{radius}: {e}"));
                let g = sample.gains;
                let settings = Settings {
                    white_balance: WhiteBalance::Custom,
                    gains: g,
                    gains_fourth: sample.gains_fourth,
                    ..Default::default()
                };
                let pixels = source.develop(&settings).unwrap();
                let center = std::array::from_fn::<_, 2, _>(|i| {
                    (focus[i] * [pixels.width() - 1, pixels.height() - 1][i] as f32).round() as u32
                });
                let mut sum = [0.; 3];
                let mut count = 0.;
                for y in center[1].saturating_sub(radius)
                    ..=center[1].saturating_add(radius).min(pixels.height() - 1)
                {
                    for x in center[0].saturating_sub(radius)
                        ..=center[0].saturating_add(radius).min(pixels.width() - 1)
                    {
                        let p = pixels.get_pixel(x, y);
                        for i in 0..3 {
                            let v = f64::from(p[i]);
                            sum[i] += if v <= 0.04045 {
                                v / 12.92
                            } else {
                                ((v + 0.055) / 1.055).powf(2.4)
                            };
                        }
                        count += 1.;
                    }
                }
                let rgb = sum.map(|v| v / count);
                assert!(
                    (rgb[0] - rgb[1]).abs() < 3e-6 && (rgb[1] - rgb[2]).abs() < 3e-6,
                    "{name}: {g:?} {rgb:?}"
                );
                let file = format!("{name}-pick-{index}-{radius}.rgba32.le");
                std::fs::write(
                    output().join(&file),
                    pixels
                        .as_raw()
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                picks.push(serde_json::json!({"input":name,"settings":settings,"output":file,"focus":focus,"radius":radius}));
            }
        }
        assert!(Arc::ptr_eq(&source.bytes, &bytes));
        assert_eq!(source.record.settings, Settings::default());
    }
    std::fs::write(
        output().join("custom-cases.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
    std::fs::write(
        output().join("pick-cases.json"),
        serde_json::to_vec_pretty(&picks).unwrap(),
    )
    .unwrap();
}
