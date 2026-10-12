use super::*;
use crate::{adjustments::Adjustments, files};
use image::{Rgba, Rgba32FImage};
use std::{path::Path, sync::Arc};
fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).to_owned()
}
fn output() -> std::path::PathBuf {
    let p = root().join("qa-output/lens-profiles");
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn dump(name: &str, image: &Rgba32FImage) {
    std::fs::write(
        output().join(name),
        image
            .as_raw()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
#[test]
fn lens_profile_xml_real_database_all_models_exact_camera_match_ambiguity_and_invalid_data() {
    let db = load(&root().join("tests/fixtures/lens-profiles/synthetic.xml")).unwrap();
    assert_eq!(db.profiles.len(), 5);
    assert!(db.notes.iter().any(|n| n.contains("future-model")));
    assert!(db.notes.iter().any(|n| n.contains("접히는")));
    assert_eq!(db.profiles[0].vignetting.len(), 1);
    assert!(db.profiles[0].xml.contains("vignetting"));
    assert!(Arc::ptr_eq(&db.profiles[0].xml, &db.profiles[1].xml));
    let official = load(&root().join("tests/fixtures/lens-profiles/slr-nikon.xml")).unwrap();
    assert!(official.profiles.len() > 100);
    assert!(official.bodies.len() > 30);
    assert!(official.profiles.iter().all(Profile::valid));
    std::fs::write(output().join("official-summary.json"),serde_json::to_vec(&serde_json::json!({"profiles":official.profiles.len(),"cameras":official.bodies.len(),"notes":official.notes})).unwrap()).unwrap();
    let doc = files::open(&root().join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let info = &doc.layers[0].raw_source.as_ref().unwrap().record.info;
    let found = db.suggest(info).unwrap();
    assert_eq!(found.name, "Known Prime 35mm");
    assert_eq!(found.focal, 35.);
    assert_eq!(found.input_crop, 1.5);
    let mut ambiguous = db.clone();
    ambiguous.profiles.push(db.profiles[0].clone());
    assert!(ambiguous.suggest(info).is_none());
    let mut unknown = info.clone();
    unknown.model = "Unknown camera".into();
    assert!(db.suggest(&unknown).is_none());
    unknown = info.clone();
    unknown.shooting.focal_length = Some([37, 1]);
    assert_eq!(db.suggest(&unknown).unwrap().focal, 37.);
    let mut bad = found.clone();
    bad.coefficients[0] = f64::NAN;
    assert!(!bad.valid());
    bad = found.clone();
    bad.coefficients[0] = -5.;
    assert!(!bad.valid());
    bad = found.clone();
    bad.input_crop = 0.;
    assert!(!bad.valid());
    bad = found;
    bad.model = Model::Poly3;
    bad.coefficients[1] = 0.3;
    assert!(!bad.valid());
    assert!(xml::parse("<bad/>").is_err());
    assert!(xml::parse("<lensdatabase><lens><model>bad</model><cropfactor>NaN</cropfactor><calibration><distortion model='poly5' focal='35'/></calibration></lens></lensdatabase>").is_err());
    assert!(xml::parse("<lensdatabase><lens><model>bad</model><calibration><distortion model='poly5' focal='35' k1='no'/></calibration></lens></lensdatabase>").is_err());
}
#[test]
fn lens_profile_coordinates_three_sampling_modes_float_alpha_source_identity_and_independent_pixels()
 {
    use crate::sampling::Sampling;
    let db = load(&root().join("tests/fixtures/lens-profiles/synthetic.xml")).unwrap();
    let source = Rgba32FImage::from_fn(13, 9, |x, y| {
        Rgba([
            x as f32 * 0.23 - 0.2,
            y as f32 * 0.19 - 0.3,
            ((x * 11 + y * 17) % 23) as f32 * 0.15,
            [0., 1e-12, 0.3, 1.][((x + y) % 4) as usize],
        ])
    });
    let original = source
        .as_raw()
        .iter()
        .map(|v| v.to_bits())
        .collect::<Vec<_>>();
    dump("source.rgba32.le", &source);
    let mut cases = Vec::new();
    for base in &db.profiles {
        for crop in [0.75, 1.5, 2.] {
            let mut p = base.clone();
            p.input_crop = crop;
            assert!(p.valid());
            let center = [6.5 + 4. * p.center[0], 4.5 + 4. * p.center[1]];
            let mapped = p.point(center, [13, 9]);
            assert!(
                mapped
                    .iter()
                    .zip(center)
                    .all(|(a, b)| (*a - b).abs() < 1e-10)
            );
            for mode in [Sampling::Nearest, Sampling::Bilinear, Sampling::Bicubic] {
                let image = p.apply_frame(&source, [13, 9], 0, mode);
                assert!(image.as_raw().iter().all(|v| v.is_finite()));
                assert!(image.pixels().all(|p| (0.0..=1.).contains(&p[3])));
                let points = (0..9)
                    .flat_map(|y| (0..13).map(move |x| [x as f64 + 0.5, y as f64 + 0.5]))
                    .map(|point| p.point(point, [13, 9]))
                    .collect::<Vec<_>>();
                cases.push(serde_json::json!({"profile":p,"sampling":mode,"points":points,"pixels":image.as_raw()}));
            }
        }
        let mut off = base.clone();
        off.enabled = false;
        assert_eq!(
            off.apply_frame(&source, [13, 9], 0, Sampling::Bicubic)
                .as_raw()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            original
        );
        off.enabled = true;
        off.coefficients = [0.; 5];
        assert_eq!(
            off.apply_frame(&source, [13, 9], 0, Sampling::Bilinear)
                .as_raw()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            original
        );
    }
    std::fs::write(
        output().join("formula.json"),
        serde_json::to_vec(&serde_json::json!({"size":[13,9],"cases":cases})).unwrap(),
    )
    .unwrap();
    assert_eq!(
        source
            .as_raw()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        original
    );
    let tiny = crate::sampling::rgba_precise([0.9, 0.9], Sampling::Bilinear, |_, _| {
        [2.125, -0.2, 0.5, 1e-40]
    });
    assert!(tiny[3] > 0.);
    assert!((tiny[0] - 2.125).abs() < 1e-6);
    let huge = crate::sampling::rgba_precise([0.9, 0.9], Sampling::Bicubic, |_, _| {
        [f32::MAX, -f32::MAX, 0., 0.5]
    });
    assert!(huge.iter().all(|v| v.is_finite()));
    assert_eq!(huge[0], f32::MAX);
    let old: Adjustments = serde_json::from_str("{}").unwrap();
    assert!(old.lens_profile.is_none());
    let mut off = db.profiles[0].clone();
    off.enabled = false;
    let settings = Adjustments {
        lens_profile: Some(off),
        ..Default::default()
    };
    assert!(!settings.active());
    assert!(settings.local());
    assert_eq!(
        serde_json::from_str::<Adjustments>(&serde_json::to_string(&settings).unwrap()).unwrap(),
        settings
    );
}
#[test]
fn lens_profile_raw_fractional_scope_project_exports_history_and_shared_xml_keep_original() {
    use crate::document::{Editor, Selection};
    let db = load(&root().join("tests/fixtures/lens-profiles/synthetic.xml")).unwrap();
    let mut doc = files::open(&root().join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let raw = doc.layers[0].raw_source.clone().unwrap();
    let original = doc.layers[0].precision.clone().unwrap();
    let profile = db.suggest(&raw.record.info).unwrap();
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
    assert!(!doc.layers[0].adjustments.local());
    let result = crate::precision::composite(&doc);
    assert_ne!(result, *original);
    assert_eq!(
        result.get_pixel(0, 0).0.map(f32::to_bits),
        original.get_pixel(0, 0).0.map(f32::to_bits)
    );
    let region = crate::document::PixelRegion {
        min: [2, 1],
        max: [10, 8],
    };
    assert_eq!(
        crate::precision::composite_region(&doc, region),
        image::imageops::crop_imm(&result, 2, 1, 8, 7).to_image()
    );
    dump("raw-source.rgba32.le", &original);
    dump("scoped.rgba32.le", &result);
    std::fs::write(
        output().join("scoped.json"),
        serde_json::to_vec(&profile).unwrap(),
    )
    .unwrap();
    let mut editor = Editor::new(doc, true);
    editor.edit("프로파일 제거", |d| {
        d.layers[0].adjustments.lens_profile = None
    });
    editor.undo();
    assert_eq!(crate::precision::composite(&editor.document), result);
    editor.redo();
    editor.undo();
    let path = output().join("raw-profile.rphoto");
    files::save_project(&editor.document, &path).unwrap();
    let restored = files::load_project(&path).unwrap();
    assert_eq!(
        restored.layers[0].adjustments.lens_profile.as_ref(),
        Some(&profile)
    );
    assert_eq!(
        restored.layers[0].raw_source.as_ref().unwrap().bytes,
        raw.bytes
    );
    assert_eq!(
        restored.layers[0]
            .raw_source
            .as_ref()
            .unwrap()
            .record
            .info
            .shooting,
        raw.record.info.shooting
    );
    assert_eq!(crate::precision::composite(&restored), result);
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &original
    ));
    assert!(Arc::ptr_eq(
        &editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .xml,
        &profile.xml
    ));
    for ext in ["psd", "psb", "exr"] {
        let path = output().join(format!("scoped.{ext}"));
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
        let imported = files::open(&path).unwrap().document;
        for (a, b) in crate::precision::composite(&imported)
            .pixels()
            .zip(result.pixels())
        {
            assert!(a.0.iter().zip(b.0).all(|(a, b)| (*a - b).abs() < 3e-5));
        }
    }
    let before = std::fs::read(&path).unwrap();
    let mut bad = restored.clone();
    bad.layers[0]
        .adjustments
        .lens_profile
        .as_mut()
        .unwrap()
        .input_crop = f64::INFINITY;
    assert!(files::save_project(&bad, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

include!("lens_tca_tests.rs");

include!("lens_vignetting_tests.rs");

include!("lens_interpolation_tests.rs");

include!("lens_vignette_interpolation_tests.rs");

include!("lens_orientation_tests.rs");
