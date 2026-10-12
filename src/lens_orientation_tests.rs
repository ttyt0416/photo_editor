fn orientation_output() -> std::path::PathBuf {
    let dir = root().join("qa-output/lens-orientation");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
fn oriented(source: &Rgba32FImage, orientation: u8) -> Rgba32FImage {
    let mut pixels = image::DynamicImage::ImageRgba32F(source.clone());
    pixels.apply_orientation(image::metadata::Orientation::from_exif(orientation).unwrap());
    pixels.into_rgba32f()
}
#[test]
fn lens_orientation_eight_exif_transforms_preserve_off_axis_distortion_tca_vignetting_and_padding()
{
    use crate::sampling::Sampling;
    let db = load(&root().join("tests/fixtures/lens-profiles/synthetic.xml")).unwrap();
    let source = Rgba32FImage::from_fn(17, 11, |x, y| {
        Rgba([
            x as f32 * 0.17 - 0.4,
            y as f32 * 0.23 - 0.2,
            ((x * 11 + y * 7) % 31) as f32 * 0.11,
            [0., 1e-12, 0.4, 1.][((x + y) % 4) as usize],
        ])
    });
    let mut cases = Vec::new();
    let mut max_error = 0f32;
    for model in [Model::Poly3, Model::Poly5, Model::Ptlens, Model::Acm] {
        let mut base = db
            .profiles
            .iter()
            .find(|p| p.model == model)
            .unwrap()
            .clone();
        base.center = [0.17, -0.23];
        if model == Model::Acm {
            base.coefficients[3] = 0.004;
            base.coefficients[4] = -0.003;
        }
        for tca in [TcaModel::Linear, TcaModel::Poly3, TcaModel::Acm] {
            base.tca = Some(Tca {
                model: tca,
                red: match tca {
                    TcaModel::Linear => [1.013, 0., 0., 0., 0., 0.],
                    TcaModel::Poly3 => [1.012, 0.002, -0.001, 0., 0., 0.],
                    TcaModel::Acm => [1.013, 0.002, -0.001, 0.0003, 0.004, -0.003],
                },
                blue: match tca {
                    TcaModel::Linear => [0.989, 0., 0., 0., 0., 0.],
                    TcaModel::Poly3 => [0.989, -0.003, 0.002, 0., 0., 0.],
                    TcaModel::Acm => [0.989, -0.003, 0.002, -0.0004, -0.002, 0.003],
                },
                enabled: true,
            });
            for vg in [VignetteModel::Pa, VignetteModel::Acm] {
                base.vignetting = Arc::new(vec![Vignetting {
                    model: vg,
                    aperture: 2.8,
                    distance: 10.,
                    coefficients: [-0.07, 0.015, -0.001],
                }]);
                base.vignetting_selected = Some(0);
                base.vignetting_enabled = true;
                assert!(base.valid());
                for sampling in [Sampling::Nearest, Sampling::Bilinear, Sampling::Bicubic] {
                    let native = base.apply_frame(&source, [17, 11], 0, sampling);
                    for orientation in 1..=8 {
                        let input = oriented(&source, orientation);
                        let expected = oriented(&native, orientation);
                        let mut p = base.clone();
                        p.orientation = orientation;
                        let frame = [input.width(), input.height()];
                        let actual = p.apply_frame(&input, frame, 0, sampling);
                        assert_eq!(actual.dimensions(), expected.dimensions());
                        for (a, b) in actual.as_raw().iter().zip(expected.as_raw()) {
                            max_error = max_error.max((*a - *b).abs());
                            assert!(
                                (*a - *b).abs() < 3e-5,
                                "{model:?}/{tca:?}/{vg:?}/{sampling:?}/{orientation}: {a} != {b}"
                            );
                        }
                        let name = format!("case-{}.rgba32.le", cases.len());
                        std::fs::write(
                            orientation_output().join(&name),
                            actual
                                .as_raw()
                                .iter()
                                .flat_map(|v| v.to_le_bytes())
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                        cases.push(serde_json::json!({"profile":p,"sampling":sampling,"orientation":orientation,"file":name,"size":frame}));
                    }
                }
            }
        }
    }
    std::fs::write(
        orientation_output().join("source.rgba32.le"),
        source
            .as_raw()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    std::fs::write(
        orientation_output().join("cases.json"),
        serde_json::to_vec(
            &serde_json::json!({"cases":cases,"size":[17,11],"max_error":max_error}),
        )
        .unwrap(),
    )
    .unwrap();
    // Inactive geometry keeps all original channel bits; padding is rotated with the frame.
    let mut p = db.profiles[0].clone();
    p.enabled = false;
    p.tca = None;
    p.vignetting_enabled = false;
    for orientation in 1..=8 {
        p.orientation = orientation;
        let input = oriented(&source, orientation);
        assert_eq!(
            p.apply_frame(
                &input,
                [input.width(), input.height()],
                0,
                Sampling::Bicubic
            ),
            input
        );
    }
    for (distortion, tca_enabled, vignetting_enabled) in [
        (false, true, false),
        (false, false, true),
        (false, true, true),
    ] {
        let mut p = db.profiles[0].clone();
        p.center = [0.2, -0.1];
        p.enabled = distortion;
        p.tca.as_mut().unwrap().enabled = tca_enabled;
        p.vignetting_selected = Some(0);
        p.vignetting_enabled = vignetting_enabled;
        for sampling in [Sampling::Nearest, Sampling::Bilinear, Sampling::Bicubic] {
            p.orientation = 1;
            let corrected = p.apply_frame(&source, [17, 11], 0, sampling);
            for direction in 1..=8 {
                p.orientation = direction;
                let input = oriented(&source, direction);
                let expected = oriented(&corrected, direction);
                let actual = p.apply_frame(&input, [input.width(), input.height()], 0, sampling);
                assert!(actual == expected);
            }
        }
    }
    let mut p = db.profiles[0].clone();
    p.center = [0.21, -0.19];
    let padded = Rgba32FImage::from_fn(21, 15, |x, y| {
        if (2..19).contains(&x) && (2..13).contains(&y) {
            *source.get_pixel(x - 2, y - 2)
        } else {
            Rgba([3., -2., 1., 0.3])
        }
    });
    let corrected = p.apply_frame(&padded, [17, 11], 2, Sampling::Bilinear);
    for orientation in 1..=8 {
        p.orientation = orientation;
        let input = oriented(&padded, orientation);
        let frame = super::orientation::size([17, 11], orientation);
        let actual = p.apply_frame(&input, frame, 2, Sampling::Bilinear);
        let expected = oriented(&corrected, orientation);
        for (a, b) in actual.as_raw().iter().zip(expected.as_raw()) {
            assert!((*a - *b).abs() < 3e-5);
        }
    }
}
#[test]
fn lens_orientation_raw_match_old_schema_interpolation_saved_project_history_and_native_outputs() {
    use crate::document::Editor;
    let db = load(&root().join("tests/fixtures/lens-profiles/synthetic.xml")).unwrap();
    let mut doc = files::open(&root().join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let raw = doc.layers[0].raw_source.clone().unwrap();
    let source = doc.layers[0].precision.clone().unwrap();
    let mut info = raw.record.info.clone();
    info.orientation = 6;
    let mut p = db.suggest(&info).unwrap();
    assert_eq!(p.orientation, 6);
    assert_eq!(p.at_focal(37.).unwrap().orientation, 6);
    let value = serde_json::to_value(&db.profiles[0]).unwrap();
    assert!(value.get("orientation").is_none());
    assert_eq!(
        serde_json::from_value::<Profile>(value)
            .unwrap()
            .orientation,
        1
    );
    let mut invalid = p.clone();
    invalid.orientation = 9;
    assert!(!invalid.valid());
    invalid.orientation = 0;
    assert!(!invalid.valid());
    p.center = [0.2, -0.1];
    doc.layers[0].adjustments.lens_profile = Some(p.clone());
    let result = crate::precision::composite(&doc);
    let mut editor = Editor::new(doc, true);
    editor.edit("교정 촬영 방향", |d| {
        d.layers[0]
            .adjustments
            .lens_profile
            .as_mut()
            .unwrap()
            .orientation = 3
    });
    assert!(crate::precision::composite(&editor.document) != result);
    editor.undo();
    assert_eq!(crate::precision::composite(&editor.document), result);
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &source
    ));
    assert!(Arc::ptr_eq(
        editor.document.layers[0].raw_source.as_ref().unwrap(),
        &raw
    ));
    let path = orientation_output().join("oriented.rphoto");
    files::save_project(&editor.document, &path).unwrap();
    let restored = files::load_project(&path).unwrap();
    assert_eq!(restored.layers[0].adjustments.lens_profile, Some(p));
    assert_eq!(crate::precision::composite(&restored), result);
    for ext in ["psd", "psb", "exr"] {
        let path = orientation_output().join(format!("oriented.{ext}"));
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
        let reopened = files::open(&path).unwrap().document;
        for (a, b) in crate::precision::composite(&reopened)
            .as_raw()
            .iter()
            .zip(result.as_raw())
        {
            assert!((*a - *b).abs() < 3e-5);
        }
    }
    let before = std::fs::read(&path).unwrap();
    let mut bad = restored;
    bad.layers[0]
        .adjustments
        .lens_profile
        .as_mut()
        .unwrap()
        .orientation = 9;
    assert!(files::save_project(&bad, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
