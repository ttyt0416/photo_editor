fn vignette_output() -> std::path::PathBuf {
    let p = root().join("qa-output/lens-vignetting");
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn vignette_dump(name: &str, image: &Rgba32FImage) {
    std::fs::write(
        vignette_output().join(name),
        image
            .as_raw()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
#[test]
fn lens_vignetting_xml_aperture_distance_duplicates_invalid_models_old_profile_and_camera_choice() {
    let db = load(&root().join("tests/fixtures/lens-profiles/vignetting.xml")).unwrap();
    assert_eq!(db.profiles.len(), 5);
    assert_eq!(db.profiles[0].vignetting.len(), 3);
    assert_eq!(db.profiles[1].vignetting.len(), 1);
    assert_eq!(db.profiles[2].vignetting.len(), 1);
    assert!(db.profiles[3..].iter().all(|p| p.vignetting.is_empty()));
    assert!(db.notes.iter().any(|n| n.contains("중복")));
    assert!(db.notes.iter().any(|n| n.contains("0에 가까운")));
    let doc = files::open(&root().join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let info = &doc.layers[0].raw_source.as_ref().unwrap().record.info;
    let matched = db.suggest(info).unwrap();
    assert_eq!(matched.vignetting_selected, Some(0));
    assert!(!matched.vignetting_enabled);
    let mut p = matched.clone();
    let mut info = info.clone();
    info.shooting.aperture = Some([4, 1]);
    p.choose_vignetting(&info);
    assert_eq!(p.vignetting_selected, None);
    info.shooting.aperture = Some([5, 1]);
    p.choose_vignetting(&info);
    assert_eq!(p.vignetting_selected, None);
    let mut old = serde_json::to_value(&matched).unwrap();
    for key in ["vignetting", "vignetting_selected", "vignetting_enabled"] {
        old.as_object_mut().unwrap().remove(key);
    }
    let legacy: Profile = serde_json::from_value(old).unwrap();
    assert!(legacy.vignetting.is_empty());
    assert!(legacy.valid());
    p = matched;
    p.vignetting_selected = Some(99);
    assert!(!p.valid());
    p.vignetting_selected = None;
    p.vignetting_enabled = true;
    assert!(!p.valid());
    let official = load(&root().join("tests/fixtures/lens-profiles/slr-nikon.xml")).unwrap();
    let rows = official
        .profiles
        .iter()
        .map(|p| p.vignetting.len())
        .sum::<usize>();
    assert!(rows > 1000);
    std::fs::write(vignette_output().join("official-summary.json"),serde_json::to_vec(&serde_json::json!({"profiles":official.profiles.len(),"vignetting_connections":rows,"notes":official.notes})).unwrap()).unwrap();
}
#[test]
fn lens_vignetting_linear_light_geometry_alpha_hidden_hdr_precision_padding_and_independent_outputs()
 {
    use crate::sampling::Sampling;
    let db = load(&root().join("tests/fixtures/lens-profiles/vignetting.xml")).unwrap();
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
    vignette_dump("source.rgba32.le", &source);
    let mut cases = Vec::new();
    for base in &db.profiles[..3] {
        for selected in 0..base.vignetting.len() {
            for optics in [false, true] {
                for crop in [1., 1.5, 2.] {
                    for sampling in [Sampling::Nearest, Sampling::Bilinear, Sampling::Bicubic] {
                        let mut p = base.clone();
                        p.enabled = optics;
                        if let Some(t) = &mut p.tca {
                            t.enabled = optics;
                        }
                        p.input_crop = crop;
                        p.vignetting_selected = Some(selected);
                        p.vignetting_enabled = true;
                        assert!(p.valid());
                        let pure = p.vignetting[selected].apply(&source, [13, 9], 0, &p);
                        assert!(pure.as_raw().iter().all(|v| v.is_finite()));
                        for (a, b) in pure.pixels().zip(source.pixels()) {
                            assert_eq!(a[3].to_bits(), b[3].to_bits());
                            if b[3] == 0. {
                                assert_eq!(a.0.map(f32::to_bits), b.0.map(f32::to_bits));
                            }
                        }
                        let result = p.apply_frame(&source, [13, 9], 0, sampling);
                        assert!(result.as_raw().iter().all(|v| v.is_finite()));
                        cases.push(serde_json::json!({"profile":p,"sampling":sampling,"pixels":result.as_raw(),"prewarp":pure.as_raw()}));
                    }
                }
            }
        }
    }
    std::fs::write(
        vignette_output().join("formula.json"),
        serde_json::to_vec(&serde_json::json!({"size":[13,9],"cases":cases})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        source
            .as_raw()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        bits
    );
    let mut p = db.profiles[2].clone();
    p.vignetting_selected = Some(0);
    p.vignetting_enabled = true;
    let input = Rgba32FImage::from_pixel(13, 9, Rgba([f32::MAX, -f32::MAX, 2., 1e-40]));
    let out = p.apply_frame(&input, [13, 9], 0, Sampling::Bicubic);
    assert!(out.as_raw().iter().all(|v| v.is_finite()));
    assert!(out.pixels().all(|v| v[0] == f32::MAX && v[3] > 0.));
    p.vignetting_enabled = false;
    assert_eq!(
        p.apply_frame(&source, [13, 9], 0, Sampling::Bicubic),
        source
    );
    p.vignetting_enabled = true;
    let padded = p.vignetting[0].apply(&source, [7, 3], 3, &p);
    for (y, row) in padded.enumerate_rows() {
        for (x, _, color) in row {
            if !(3..10).contains(&x) || !(3..6).contains(&y) {
                assert_eq!(
                    color.0.map(f32::to_bits),
                    source.get_pixel(x, y).0.map(f32::to_bits)
                );
            }
        }
    }
}
#[test]
fn lens_vignetting_raw_scope_history_project_native_exports_and_invalid_save_keep_original() {
    use crate::document::{Editor, Selection};
    let db = load(&root().join("tests/fixtures/lens-profiles/vignetting.xml")).unwrap();
    let mut doc = files::open(&root().join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let raw = doc.layers[0].raw_source.clone().unwrap();
    let original = doc.layers[0].precision.clone().unwrap();
    let mut profile = db.suggest(&raw.record.info).unwrap();
    profile.vignetting_enabled = true;
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
    vignette_dump("raw-source.rgba32.le", &original);
    vignette_dump("scoped.rgba32.le", &result);
    std::fs::write(
        vignette_output().join("scoped.json"),
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
    editor.edit("주변부 밝기 해제", |d| {
        d.layers[0]
            .adjustments
            .lens_profile
            .as_mut()
            .unwrap()
            .vignetting_enabled = false
    });
    let off = crate::precision::composite(&editor.document);
    assert_ne!(off, result);
    editor.undo();
    assert_eq!(crate::precision::composite(&editor.document), result);
    editor.redo();
    assert_eq!(crate::precision::composite(&editor.document), off);
    editor.undo();
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &original
    ));
    assert!(Arc::ptr_eq(
        editor.document.layers[0].raw_source.as_ref().unwrap(),
        &raw
    ));
    assert!(Arc::ptr_eq(
        &editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .vignetting,
        &profile.vignetting
    ));
    let path = vignette_output().join("raw-profile.rphoto");
    files::save_project(&editor.document, &path).unwrap();
    let restored = files::load_project(&path).unwrap();
    assert_eq!(restored.layers[0].adjustments.lens_profile, Some(profile));
    assert_eq!(crate::precision::composite(&restored), result);
    assert_eq!(
        restored.layers[0].raw_source.as_ref().unwrap().bytes,
        raw.bytes
    );
    for ext in ["psd", "psb", "exr"] {
        let p = vignette_output().join(format!("scoped.{ext}"));
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
        .vignetting_selected = Some(999);
    assert!(files::save_project(&bad, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}
