fn interpolation_output() -> std::path::PathBuf {
    let p = root().join("qa-output/lens-interpolation");
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn lens_interpolation_exact_neighbors_models_calibrations_ambiguity_range_and_raw_match() {
    let db = load(&root().join("tests/fixtures/lens-profiles/interpolation.xml")).unwrap();
    assert_eq!(db.profiles.len(), 24);
    let mut samples = Vec::new();
    for model in [Model::Poly3, Model::Poly5, Model::Ptlens, Model::Acm] {
        let name = if model == Model::Poly5 {
            "Known Prime 35mm".to_owned()
        } else {
            format!(
                "Interpolated {}",
                match model {
                    Model::Poly3 => "poly3",
                    Model::Ptlens => "ptlens",
                    Model::Acm => "acm",
                    _ => unreachable!(),
                }
            )
        };
        let base = db
            .profiles
            .iter()
            .find(|p| p.name == name && p.calibration == 0 && p.focal == 35.)
            .unwrap();
        let mut base = base.clone();
        base.tca.as_mut().unwrap().enabled = true;
        assert_eq!(base.at_focal(35.).unwrap(), base);
        assert!(Arc::ptr_eq(&base.at_focal(35.).unwrap().xml, &base.xml));
        for focal in [20., 25., 34., 35., 37., 42.5, 50., 60., 75., 85.] {
            let p = base.at_focal(focal).unwrap();
            assert!(p.valid());
            assert_eq!(p.calibration, 0);
            assert_eq!(p.focal, focal);
            assert!(p.tca.as_ref().unwrap().enabled);
            assert!(Arc::ptr_eq(&p.xml, &base.xml));
            let exact = [20., 35., 50., 85.].contains(&focal);
            assert_eq!(p.interpolated.is_none(), exact);
            assert_eq!(!p.vignetting.is_empty(), exact);
            samples.push(serde_json::json!({"profile":p,"target":focal}));
        }
        for focal in [10., 100., f64::NAN] {
            assert!(base.at_focal(focal).is_err());
        }
        let mut bad = base;
        bad.xml = Arc::new("<lens>bad".into());
        assert!(bad.at_focal(40.).is_err());
    }
    let doc = files::open(&root().join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let mut info = doc.layers[0]
        .raw_source
        .as_ref()
        .unwrap()
        .record
        .info
        .clone();
    info.shooting.focal_length = Some([37, 1]);
    // Two calibration sets make automatic selection ambiguous, while manual interpolation stays scoped.
    assert_eq!(db.suggest(&info).unwrap().calibration, 0);
    let single = load(&root().join("tests/fixtures/lens-profiles/synthetic.xml")).unwrap();
    assert_eq!(single.suggest(&info).unwrap().focal, 37.);
    let duplicate=xml::parse("<lensdatabase><lens><model>duplicate</model><calibration><distortion model='poly5' focal='20' k1='0.01'/><distortion model='poly5' focal='20' k1='0.02'/><distortion model='poly5' focal='50' k1='0.03'/></calibration></lens></lensdatabase>").unwrap();
    assert!(duplicate.profiles[0].at_focal(35.).is_err());
    std::fs::write(
        interpolation_output().join("profiles.json"),
        serde_json::to_vec(&samples).unwrap(),
    )
    .unwrap();
}
#[test]
fn lens_interpolation_float_pixels_scope_saved_xml_reopen_history_and_native_outputs_keep_source() {
    use crate::document::{Editor, Selection};
    let db = load(&root().join("tests/fixtures/lens-profiles/interpolation.xml")).unwrap();
    let mut doc = files::open(&root().join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let raw = doc.layers[0].raw_source.clone().unwrap();
    let source = doc.layers[0].precision.clone().unwrap();
    let mut base = db
        .profiles
        .iter()
        .find(|p| p.name == "Known Prime 35mm" && p.calibration == 0 && p.focal == 35.)
        .unwrap()
        .clone();
    base.tca.as_mut().unwrap().enabled = true;
    let profile = base.at_focal(42.5).unwrap();
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
            interpolation_output().join(name),
            pixels
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    std::fs::write(
        interpolation_output().join("scoped.json"),
        serde_json::to_vec(&profile).unwrap(),
    )
    .unwrap();
    let mut editor = Editor::new(doc, true);
    editor.edit("초점거리 변경", |d| {
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
    let path = interpolation_output().join("raw-profile.rphoto");
    files::save_project(&editor.document, &path).unwrap();
    let restored = files::load_project(&path).unwrap();
    assert_eq!(
        restored.layers[0].adjustments.lens_profile,
        Some(profile.clone())
    );
    let recovered = restored.layers[0]
        .adjustments
        .lens_profile
        .as_ref()
        .unwrap()
        .at_focal(60.)
        .unwrap();
    assert_eq!(recovered, profile.at_focal(60.).unwrap());
    assert_eq!(
        restored.layers[0].raw_source.as_ref().unwrap().bytes,
        raw.bytes
    );
    for ext in ["psd", "psb", "exr"] {
        let p = interpolation_output().join(format!("scoped.{ext}"));
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
    let mut invalid = restored;
    invalid.layers[0]
        .adjustments
        .lens_profile
        .as_mut()
        .unwrap()
        .interpolated = Some([50., 35.]);
    assert!(files::save_project(&invalid, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
