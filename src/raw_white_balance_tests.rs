use super::*;
#[test]
fn raw_white_balance_four_channel_solver_dark_invalid_degenerate_ranges_and_orientations() {
    let matrix = [[1., 0., 0., 0.2], [0., 1., 0., 0.1], [0., 0., 1., 0.3]];
    let average = [0.2, 0.4, 0.1, 0.3];
    let result = gains(average, matrix, 0.7, false).unwrap();
    let rgb = std::array::from_fn::<_, 3, _>(|i| {
        (0..4)
            .map(|c| {
                f64::from(matrix[i][c])
                    * average[c]
                    * f64::from(if c == 3 { 0.7 } else { result[c] })
            })
            .sum::<f64>()
    });
    assert!((rgb[0] - rgb[1]).abs() < 1e-7 && (rgb[1] - rgb[2]).abs() < 1e-7);
    assert!(gains([0.; 4], matrix, 1., false).is_err());
    assert!(gains([1., 0., 1., 0.], matrix, 1., false).is_err());
    assert!(gains([1.; 4], [[0.; 4]; 3], 1., false).is_err());
    assert!(gains([1e-40, 1., 1., 0.], matrix, 1., false).is_err());
    assert_eq!(gains([0.2, 0., 0., 0.], matrix, 1., true).unwrap(), [1.; 3]);
    let original = Rgba32FImage::from_fn(3, 2, |x, y| image::Rgba([(y * 3 + x) as f32; 4]));
    for o in 1..=8 {
        let mut oriented = DynamicImage::ImageRgba32F(original.clone());
        oriented.apply_orientation(image::metadata::Orientation::from_exif(o).unwrap());
        let oriented = oriented.into_rgba32f();
        for (x, y, p) in oriented.enumerate_pixels() {
            let [nx, ny] = native([x, y], [3, 2], o);
            assert_eq!(p, original.get_pixel(nx, ny));
        }
    }
}

#[test]
fn raw_white_balance_sensor_patch_gray_linear_dng_orientations_crops_source_and_independent_samples()
 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw");
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/raw-white-balance");
    std::fs::create_dir_all(&out).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    let mut cases = Vec::new();
    for case in manifest.as_array().unwrap() {
        let filename = case["file"].as_str().unwrap();
        let doc = crate::files::open(&root.join(filename)).unwrap().document;
        let raw = doc.layers[0].raw_source.clone().unwrap();
        let bytes = raw.bytes.clone();
        let original = doc.layers[0].precision.clone().unwrap();
        let cfa = case["kind"] == "cfa";
        let points: &[[f32; 2]] = if cfa {
            &[[0.5, 0.5]]
        } else {
            &[[0., 0.], [0.5, 0.5], [1., 1.], [0.25, 0.75], [0.9, 0.1]]
        };
        let radii: &[u32] = if cfa { &[0, 1] } else { &[0, 1, 4] };
        for focus in points {
            for radius in radii {
                let sample = raw
                    .white_balance_at(*focus, *radius)
                    .unwrap_or_else(|e| panic!("{filename}/{focus:?}/{radius}: {e}"));
                let gains = sample.gains;
                assert!(sample.gains_fourth.is_none());
                assert_eq!(gains[1], 1.);
                let settings = Settings {
                    white_balance: WhiteBalance::Custom,
                    gains,
                    ..Default::default()
                };
                let result = raw.develop(&settings).unwrap();
                assert!(result.as_raw().iter().all(|v| v.is_finite()));
                let center: [u32; 2] = std::array::from_fn(|i| {
                    (focus[i] * [result.width() - 1, result.height() - 1][i] as f32).round() as u32
                });
                let min = center.map(|v| v.saturating_sub(*radius));
                let max = std::array::from_fn::<_, 2, _>(|i| {
                    center[i]
                        .saturating_add(*radius)
                        .min([result.width() - 1, result.height() - 1][i])
                });
                let mut average = [0f64; 3];
                let mut count = 0.;
                for y in min[1]..=max[1] {
                    for x in min[0]..=max[0] {
                        let p = result.get_pixel(x, y);
                        for c in 0..3 {
                            let v = f64::from(p[c]);
                            average[c] += if v <= 0.04045 {
                                v / 12.92
                            } else {
                                ((v + 0.055) / 1.055).powf(2.4)
                            };
                        }
                        count += 1.;
                    }
                }
                average = average.map(|v| v / count);
                assert!(
                    (average[0] - average[1]).abs() < 2e-5
                        && (average[1] - average[2]).abs() < 2e-5,
                    "{filename}/{focus:?}/{radius}: {average:?}"
                );
                let file = format!("sample-{}.rgba32.le", cases.len());
                std::fs::write(
                    out.join(&file),
                    result
                        .as_raw()
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                cases.push(serde_json::json!({"input":filename,"focus":focus,"radius":radius,"gains":gains,"size":[result.width(),result.height()],"output":file}));
            }
        }
        assert!(Arc::ptr_eq(&raw.bytes, &bytes));
        assert!(Arc::ptr_eq(
            doc.layers[0].precision.as_ref().unwrap(),
            &original
        ));
        assert_eq!(raw.record.settings, Settings::default());
        for focus in [[f32::NAN, 0.], [1.1, 0.], [-0.1, 0.]] {
            assert!(raw.white_balance_at(focus, 1).is_err());
        }
        assert!(raw.white_balance_at([0.5; 2], 65).is_err());
    }
    std::fs::write(
        out.join("formula.json"),
        serde_json::to_vec(&serde_json::json!({"cases":cases})).unwrap(),
    )
    .unwrap();
}

#[test]
fn raw_apply_changed_size_linked_unlinked_mask_world_and_invalid_results_are_atomic() {
    let mut doc = crate::files::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw/linear-32-o1.dng"),
    )
    .unwrap()
    .document;
    let source = doc.layers[0].raw_source.clone().unwrap();
    let pixels = Arc::new(source.develop(&Settings::default()).unwrap());
    let layer = &mut doc.layers[0];
    let edited = Arc::new(Rgba32FImage::from_pixel(2, 2, image::Rgba([1.; 4])));
    layer.pixels = Arc::new(crate::precision::preview(&edited));
    layer.precision = Some(edited.clone());
    layer.mask = Some(Arc::new(image::GrayImage::from_pixel(
        2,
        2,
        image::Luma([128]),
    )));
    let baseline = layer.clone();
    for linked in [true, false] {
        let mut updated = baseline.clone();
        if !linked {
            crate::masks::set_world(
                &mut updated,
                [1., 0., 3., 0., 1., 5., 0., 0., 1.],
                false,
                crate::sampling::Sampling::Bilinear,
            )
            .unwrap();
        }
        let world = crate::masks::world(&updated);
        let mask = updated.mask.clone().unwrap();
        super::super::apply(&mut updated, pixels.clone(), Settings::default(), &source).unwrap();
        assert!(crate::masks::valid(&updated));
        assert_eq!(crate::masks::linked(&updated), linked);
        assert_eq!(crate::masks::world(&updated), world);
        assert!(Arc::ptr_eq(updated.mask.as_ref().unwrap(), &mask));
    }
    for fault in 0..3 {
        let mut candidate = baseline.clone();
        let mut settings = Settings::default();
        let mut invalid = pixels.clone();
        match fault {
            0 => {
                let mut bad = (*pixels).clone();
                bad.get_pixel_mut(0, 0)[0] = f32::NAN;
                invalid = Arc::new(bad);
            }
            1 => settings.gains[0] = 0.,
            _ => candidate.transform.warp = Some([1., 0., 0., 0., 1., 0., -0.2, 0., 1.]),
        }
        let transform = candidate.transform;
        let mask = candidate.mask.clone().unwrap();
        assert!(super::super::apply(&mut candidate, invalid, settings, &source).is_err());
        assert!(Arc::ptr_eq(candidate.precision.as_ref().unwrap(), &edited));
        assert!(Arc::ptr_eq(candidate.raw_source.as_ref().unwrap(), &source));
        assert!(Arc::ptr_eq(candidate.mask.as_ref().unwrap(), &mask));
        assert_eq!(candidate.transform, transform);
    }
}
