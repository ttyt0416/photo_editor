fn vignette_interpolation_output() -> std::path::PathBuf {
    let p = root().join("qa-output/lens-vignette-interpolation");
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn lens_vignette_interpolation_dimensions_models_exact_near_single_focal_range_schema_and_saved_queries()
 {
    let db = load(&root().join("tests/fixtures/lens-profiles/vignette-interpolation.xml")).unwrap();
    let mut cases = Vec::new();
    for name in ["PA Zoom", "ACM Zoom"] {
        let mut base = db
            .profiles
            .iter()
            .find(|p| p.name == name && p.calibration == 0 && p.focal == 35.)
            .unwrap()
            .clone();
        base.vignetting_selected = Some(0);
        base.vignetting_enabled = true;
        for query in [
            [20., 2.8, 1.],
            [35., 2.8, 10.],
            [50., 4., 10.],
            [85., 5.6, 1000.],
            [25., 3.2, 2.],
            [37., 3.5, 5.],
            [42.5, 4., 10.],
            [60., 5., 100.],
            [75., 5.6, 900.],
            [85., 5.6, 900.],
        ] {
            let p = base.at_optics(query[0], query[1], query[2]).unwrap();
            assert!(p.valid());
            assert!(p.vignetting_enabled);
            assert_eq!(p.vignetting_query, Some(query));
            assert_eq!(p.focal, query[0]);
            assert_eq!(p.calibration, 0);
            assert!(Arc::ptr_eq(&p.xml, &base.xml));
            let selected = p.vignetting_selected.unwrap();
            let v = &p.vignetting[selected];
            assert!(v.valid_for(&p));
            let encoded = serde_json::to_vec(&p).unwrap();
            let decoded: Profile = serde_json::from_slice(&encoded).unwrap();
            assert_eq!(decoded, p);
            cases.push(serde_json::json!({"profile":p,"query":query}));
        }
        assert_eq!(
            base.at_optics(35., 2.8, 10.).unwrap().vignetting[0],
            base.vignetting[0]
        );
        for query in [
            [35., 0.1, 10.],
            [1000., 2.8, 10.],
            [35., 2.8, f64::NAN],
            [0., 2.8, 10.],
        ] {
            assert!(base.at_optics(query[0], query[1], query[2]).is_err());
        }
        // An invalid geometric interpolation must not silently switch off existing correction.
        assert!(base.at_optics(15., 2.8, 10.).is_err());
    }
    let base = db
        .profiles
        .iter()
        .find(|p| p.name == "Single focal brightness")
        .unwrap();
    let p = base.at_optics(35., 3.5, 5.).unwrap();
    assert!(!p.enabled);
    assert_eq!(p.vignetting_query, Some([35., 3.5, 5.]));
    assert!(base.at_optics(40., 3.5, 5.).is_err());
    cases.push(serde_json::json!({"profile":p,"query":[35.,3.5,5.]}));
    std::fs::write(
        vignette_interpolation_output().join("profiles.json"),
        serde_json::to_vec(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn lens_vignette_interpolation_raw_pixels_scope_history_project_recompute_and_native_outputs_keep_source()
 {
    use crate::document::{Editor, Selection};
    let db = load(&root().join("tests/fixtures/lens-profiles/vignette-interpolation.xml")).unwrap();
    let mut doc = files::open(&root().join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let raw = doc.layers[0].raw_source.clone().unwrap();
    let source = doc.layers[0].precision.clone().unwrap();
    let mut base = db
        .profiles
        .iter()
        .find(|p| p.name == "PA Zoom" && p.calibration == 0 && p.focal == 35.)
        .unwrap()
        .clone();
    base.vignetting_selected = Some(0);
    base.vignetting_enabled = true;
    let profile = base.at_optics(42.5, 4., 10.).unwrap();
    let selection = Selection {
        min: [0, 0],
        max: [12, 10],
        mask: Some(Arc::new(image::GrayImage::from_fn(12, 10, |x, y| {
            image::Luma([((x + y) * 13).min(255) as u8])
        }))),
        inverted: false,
    };
    let scope =
        crate::adjustment_scope::Scope::capture(&doc.layers[0], &selection, [12, 10]).unwrap();
    doc.layers[0].adjustments = Adjustments {
        lens_profile: Some(profile.clone()),
        scope: Some(scope),
        ..Default::default()
    };
    let result = crate::precision::composite(&doc);
    assert_ne!(result, *source);
    for (name, pixels) in [
        ("raw-source.rgba32.le", source.as_ref()),
        ("scoped.rgba32.le", &result),
    ] {
        std::fs::write(
            vignette_interpolation_output().join(name),
            pixels
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    std::fs::write(
        vignette_interpolation_output().join("scoped.json"),
        serde_json::to_vec(&profile).unwrap(),
    )
    .unwrap();
    let region = crate::document::PixelRegion {
        min: [2, 1],
        max: [10, 8],
    };
    assert_eq!(
        crate::precision::composite_region(&doc, region),
        image::imageops::crop_imm(&result, 2, 1, 8, 7).to_image()
    );
    let mut editor = Editor::new(doc, true);
    editor.edit("주변부 보간 변경", |d| {
        d.layers[0].adjustments.lens_profile = Some(base)
    });
    editor.undo();
    assert_eq!(crate::precision::composite(&editor.document), result);
    editor.redo();
    editor.undo();
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &source
    ));
    assert!(Arc::ptr_eq(
        editor.document.layers[0].raw_source.as_ref().unwrap(),
        &raw
    ));
    let path = vignette_interpolation_output().join("raw-profile.rphoto");
    files::save_project(&editor.document, &path).unwrap();
    let restored = files::load_project(&path).unwrap();
    assert_eq!(
        restored.layers[0].adjustments.lens_profile,
        Some(profile.clone())
    );
    assert_eq!(
        restored.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .at_optics(60., 5., 100.)
            .unwrap(),
        profile.at_optics(60., 5., 100.).unwrap()
    );
    assert_eq!(
        restored.layers[0].raw_source.as_ref().unwrap().bytes,
        raw.bytes
    );
    for ext in ["psd", "psb", "exr"] {
        let p = vignette_interpolation_output().join(format!("scoped.{ext}"));
        if ext == "exr" {
            files::export(
                &restored,
                &p,
                image::ImageFormat::OpenExr,
                &Default::default(),
            )
            .unwrap();
        } else {
            files::save_photoshop(&restored, &p).unwrap();
        }
        let reopened = files::open(&p).unwrap().document;
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
        .vignetting_query = Some([42.5, 0., 10.]);
    assert!(files::save_project(&bad, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
