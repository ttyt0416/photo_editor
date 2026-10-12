fn tca_output() -> std::path::PathBuf {
    let p = root().join("qa-output/lens-tca");
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn tca_dump(name: &str, image: &Rgba32FImage) {
    std::fs::write(
        tca_output().join(name),
        image
            .as_raw()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
#[test]
fn lens_tca_xml_models_only_tca_duplicates_invalid_defaults_and_old_project() {
    let db = load(&root().join("tests/fixtures/lens-profiles/tca.xml")).unwrap();
    assert_eq!(db.profiles.len(), 8);
    assert!(
        db.profiles[..4]
            .iter()
            .all(|p| p.tca.is_some() && p.valid())
    );
    assert!(db.profiles[4..].iter().all(|p| p.tca.is_none()));
    assert!(db.notes.iter().any(|n| n.contains("중복")));
    assert!(db.notes.iter().any(|n| n.contains("접히는")));
    let mut only = db.profiles[3].clone();
    assert!(!only.effect_enabled());
    only.tca.as_mut().unwrap().enabled = true;
    assert!(only.effect_enabled());
    assert!(!only.enabled);
    for model in [TcaModel::Linear, TcaModel::Poly3, TcaModel::Acm] {
        let identity = Tca {
            model,
            red: [1., 0., 0., 0., 0., 0.],
            blue: [1., 0., 0., 0., 0., 0.],
            enabled: true,
        };
        assert!(identity.valid_for(&only));
        assert!(!identity.effect_enabled());
    }
    let mut invalid = only.clone();
    invalid.tca.as_mut().unwrap().red[0] = 0.;
    assert!(!invalid.valid());
    invalid = only.clone();
    invalid.tca.as_mut().unwrap().red[1] = 0.1;
    assert!(!invalid.valid());
    let mut legacy = serde_json::to_value(&db.profiles[0]).unwrap();
    legacy.as_object_mut().unwrap().remove("tca");
    let parsed: Profile = serde_json::from_value(legacy).unwrap();
    assert!(parsed.tca.is_none());
    assert!(parsed.valid());
    let official = load(&root().join("tests/fixtures/lens-profiles/slr-nikon.xml")).unwrap();
    let count = official.profiles.iter().filter(|p| p.tca.is_some()).count();
    assert!(count > 100);
    std::fs::write(tca_output().join("official-summary.json"),serde_json::to_vec(&serde_json::json!({"profiles":official.profiles.len(),"tca":count,"notes":official.notes})).unwrap()).unwrap();
}
#[test]
fn lens_tca_channels_geometry_samplers_alpha_float_source_identity_and_independent_outputs() {
    use crate::sampling::Sampling;
    let db = load(&root().join("tests/fixtures/lens-profiles/tca.xml")).unwrap();
    let source = Rgba32FImage::from_fn(13, 9, |x, y| {
        Rgba([
            x as f32 * 0.23 - 0.2,
            y as f32 * 0.19 - 0.3,
            ((x * 11 + y * 17) % 23) as f32 * 0.15,
            [0., 1e-12, 0.3, 1.][((x + y) % 4) as usize],
        ])
    });
    let bits = source
        .as_raw()
        .iter()
        .map(|v| v.to_bits())
        .collect::<Vec<_>>();
    tca_dump("source.rgba32.le", &source);
    let mut cases = Vec::new();
    for base in &db.profiles[..4] {
        for distortion in [false, true] {
            for crop in [0.75, 1.5, 2.] {
                for sampling in [Sampling::Nearest, Sampling::Bilinear, Sampling::Bicubic] {
                    let mut p = base.clone();
                    p.enabled = distortion;
                    p.input_crop = crop;
                    p.tca.as_mut().unwrap().enabled = true;
                    assert!(p.valid());
                    let mut geometry = p.clone();
                    geometry.tca.as_mut().unwrap().enabled = false;
                    let reference = geometry.apply_frame(&source, [13, 9], 0, sampling);
                    let result = p.apply_frame(&source, [13, 9], 0, sampling);
                    assert!(result.as_raw().iter().all(|v| v.is_finite()));
                    for (a, b) in result.pixels().zip(reference.pixels()) {
                        assert_eq!(a[1].to_bits(), b[1].to_bits());
                        assert_eq!(a[3].to_bits(), b[3].to_bits());
                    }
                    let points = (0..9)
                        .flat_map(|y| (0..13).map(move |x| [x as f64 + 0.5, y as f64 + 0.5]))
                        .map(|pt| {
                            let pt = p.point(pt, [13, 9]);
                            [
                                p.tca.as_ref().unwrap().point(pt, [13, 9], &p, false),
                                pt,
                                p.tca.as_ref().unwrap().point(pt, [13, 9], &p, true),
                            ]
                        })
                        .collect::<Vec<_>>();
                    cases.push(serde_json::json!({"profile":p,"sampling":sampling,"points":points,"pixels":result.as_raw()}));
                }
            }
        }
    }
    assert_eq!(
        source
            .as_raw()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        bits
    );
    std::fs::write(
        tca_output().join("formula.json"),
        serde_json::to_vec(&serde_json::json!({"size":[13,9],"cases":cases})).unwrap(),
    )
    .unwrap();
    // Disabled optics bypass even bicubic sampling, retaining hidden color and tiny alpha bits.
    let p = db.profiles[3].clone();
    assert_eq!(
        p.apply_frame(&source, [13, 9], 0, Sampling::Bicubic),
        source
    );
}
#[test]
fn lens_tca_raw_scope_history_project_native_exports_and_invalid_save_keep_original() {
    use crate::document::{Editor, Selection};
    let db = load(&root().join("tests/fixtures/lens-profiles/tca.xml")).unwrap();
    let mut doc = files::open(&root().join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let raw = doc.layers[0].raw_source.clone().unwrap();
    let original = doc.layers[0].precision.clone().unwrap();
    let mut profile = db.suggest(&raw.record.info).unwrap();
    profile.tca.as_mut().unwrap().enabled = true;
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
    assert_ne!(result, *original);
    tca_dump("raw-source.rgba32.le", &original);
    tca_dump("scoped.rgba32.le", &result);
    std::fs::write(
        tca_output().join("scoped.json"),
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
    editor.edit("색수차 보정 해제", |d| {
        d.layers[0]
            .adjustments
            .lens_profile
            .as_mut()
            .unwrap()
            .tca
            .as_mut()
            .unwrap()
            .enabled = false
    });
    let disabled = crate::precision::composite(&editor.document);
    assert_ne!(disabled, result);
    editor.undo();
    assert_eq!(crate::precision::composite(&editor.document), result);
    editor.redo();
    assert_eq!(crate::precision::composite(&editor.document), disabled);
    editor.undo();
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &original
    ));
    assert!(Arc::ptr_eq(
        editor.document.layers[0].raw_source.as_ref().unwrap(),
        &raw
    ));
    let path = tca_output().join("raw-profile.rphoto");
    files::save_project(&editor.document, &path).unwrap();
    let restored = files::load_project(&path).unwrap();
    assert_eq!(restored.layers[0].adjustments.lens_profile, Some(profile));
    assert_eq!(crate::precision::composite(&restored), result);
    assert_eq!(
        restored.layers[0].raw_source.as_ref().unwrap().bytes,
        raw.bytes
    );
    for ext in ["psd", "psb", "exr"] {
        let p = tca_output().join(format!("scoped.{ext}"));
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
        .tca
        .as_mut()
        .unwrap()
        .red[0] = f64::INFINITY;
    assert!(files::save_project(&bad, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
