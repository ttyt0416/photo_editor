use super::*;
use crate::{
    adjustments::Adjustments,
    document::{Editor, Selection},
    files,
};
use image::{DynamicImage, Rgba, Rgba32FImage};
use std::{path::Path, sync::Arc};
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/detail-sharpen");
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn dump(name: &str, p: &Rgba32FImage) {
    std::fs::write(
        output().join(name),
        p.as_raw()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
#[test]
fn detail_sharpen_edges_threshold_masking_alpha_hdr_identity_validation_and_independent_samples() {
    let step = Rgba32FImage::from_fn(32, 16, |x, y| {
        let v = if x < 16 { 0.2 } else { 0.8 };
        let noise = (((x * 17 + y * 31) % 13) as f32 - 6.) * 0.001;
        Rgba([v + noise, v + noise + 0.1, v + noise - 0.05, 1.])
    });
    let d = DetailSharpen {
        amount: 1.,
        radius: 1.5,
        threshold: 0.,
        masking: 0.,
    };
    let out = d.apply(&step);
    assert!(
        out.get_pixel(16, 8)[0] - out.get_pixel(15, 8)[0]
            > step.get_pixel(16, 8)[0] - step.get_pixel(15, 8)[0]
    );
    let restricted = DetailSharpen {
        masking: 1.,
        ..d.clone()
    }
    .apply(&step);
    let energy = |p: &Rgba32FImage| {
        p.enumerate_pixels()
            .filter(|(x, _, _)| *x < 10)
            .map(|(x, y, a)| f64::from(a[0] - step.get_pixel(x, y)[0]).powi(2))
            .sum::<f64>()
    };
    assert!(energy(&restricted) < energy(&out) * 0.01);
    assert!(restricted.get_pixel(16, 8)[0] - restricted.get_pixel(15, 8)[0] > 0.7);
    assert_eq!(
        DetailSharpen {
            threshold: 1.,
            ..d.clone()
        }
        .apply(&step),
        step
    );
    let source = Rgba32FImage::from_fn(13, 9, |x, y| {
        let v = (((x * 11 + y * 17) % 19) as f32 - 9.) * 0.04 + if x < 6 { 0.3 } else { 1.7 };
        Rgba([
            v,
            v - 0.3,
            v + 0.2,
            if x == 0 {
                0.
            } else {
                [0.2, 0.5, 0.75, 1.][(y % 4) as usize]
            },
        ])
    });
    let mut states = vec![DetailSharpen::default()];
    for radius in [0.3, 1.5, 8.] {
        for (threshold, masking) in [(0., 0.), (0.02, 0.), (0.01, 0.7)] {
            states.push(DetailSharpen {
                amount: 1.4,
                radius,
                threshold,
                masking,
            });
        }
    }
    let mut outputs = Vec::new();
    for state in &states {
        let actual = state.apply(&source);
        assert!(actual.as_raw().iter().all(|v| v.is_finite()));
        for (a, b) in actual.pixels().zip(source.pixels()) {
            assert_eq!(a[3].to_bits(), b[3].to_bits());
            if b[3] == 0. || !state.enabled() {
                assert_eq!(a.0.map(f32::to_bits), b.0.map(f32::to_bits));
            }
            assert!(((a[0] - a[1]) - (b[0] - b[1])).abs() < 1e-6);
        }
        outputs.push(actual.as_raw().clone());
    }
    dump("formula-source.rgba32.le", &source);
    std::fs::write(
        output().join("formula.json"),
        serde_json::to_vec(&serde_json::json!({"states":states,"size":[13,9],"outputs":outputs}))
            .unwrap(),
    )
    .unwrap();
    let mut hidden = source.clone();
    for p in hidden.pixels_mut() {
        if p[3] == 0. {
            p.0 = [f32::MAX, -f32::MAX, 2., 0.];
        }
    }
    let a = d.apply(&source);
    let b = d.apply(&hidden);
    for (a, b) in a.pixels().zip(b.pixels()) {
        if a[3] > 0. {
            assert_eq!(a.0.map(f32::to_bits), b.0.map(f32::to_bits));
        }
    }
    for size in [(1, 1), (1, 7), (7, 1), (0, 0)] {
        let img = Rgba32FImage::from_pixel(size.0, size.1, Rgba([2., -1., 0.5, 0.75]));
        let result = d.apply(&img);
        assert!(result.pixels().all(|p| {
            p.0.iter()
                .zip([2., -1., 0.5, 0.75])
                .all(|(a, b)| (*a - b).abs() < 1e-6)
        }));
    }
    let extreme = Rgba32FImage::from_fn(3, 3, |x, y| {
        Rgba([
            if x == 1 { f32::MAX } else { -f32::MAX },
            if y == 1 { f32::MAX } else { -f32::MAX },
            -0.0,
            1.,
        ])
    });
    assert!(d.apply(&extreme).as_raw().iter().all(|v| v.is_finite()));
    let mut inactive = DetailSharpen {
        radius: 8.,
        threshold: 0.4,
        masking: 0.8,
        ..Default::default()
    };
    let settings = Adjustments {
        detail_sharpen: inactive.clone(),
        ..Default::default()
    };
    assert!(!settings.active());
    assert!(settings.local());
    assert_eq!(
        serde_json::from_str::<Adjustments>(&serde_json::to_string(&settings).unwrap())
            .unwrap()
            .detail_sharpen,
        inactive
    );
    inactive.amount = 0.5;
    assert!(
        !Adjustments {
            detail_sharpen: inactive.clone(),
            ..Default::default()
        }
        .local()
    );
    assert_eq!(
        serde_json::from_str::<Adjustments>("{}")
            .unwrap()
            .detail_sharpen,
        DetailSharpen::default()
    );
    inactive.radius = 0.;
    assert!(!inactive.valid());
    inactive.radius = 2.;
    inactive.threshold = f32::NAN;
    assert!(!inactive.valid());
}
#[test]
fn detail_sharpen_scoped_raw_and_adjustment_layers_project_exports_history_keep_precision_alpha_original()
 {
    let mut doc = files::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw/linear-32-o1.dng"),
    )
    .unwrap()
    .document;
    let source = doc.active_layer().unwrap().precision.clone().unwrap();
    let raw = doc.active_layer().unwrap().raw_source.clone().unwrap();
    let m = DetailSharpen {
        amount: 1.5,
        radius: 1.8,
        threshold: 0.01,
        masking: 0.3,
    };
    let mask = Arc::new(image::GrayImage::from_fn(12, 10, |x, y| {
        image::Luma([((x + y) * 13).min(255) as u8])
    }));
    let selection = Selection {
        min: [0, 0],
        max: [12, 10],
        mask: Some(mask),
        inverted: false,
    };
    let scope =
        crate::adjustment_scope::Scope::capture(doc.active_layer().unwrap(), &selection, [12, 10])
            .unwrap();
    doc.active_layer_mut().unwrap().adjustments = Adjustments {
        detail_sharpen: m.clone(),
        scope: Some(scope.clone()),
        ..Default::default()
    };
    let result = crate::precision::composite(&doc);
    assert_ne!(&result, source.as_ref());
    let region = crate::document::PixelRegion {
        min: [2, 1],
        max: [10, 8],
    };
    assert_eq!(
        crate::precision::composite_region(&doc, region),
        image::imageops::crop_imm(&result, 2, 1, 8, 7).to_image()
    );
    assert!(!doc.active_layer().unwrap().adjustments.local());

    assert_eq!(
        result.get_pixel(0, 0).0.map(f32::to_bits),
        source.get_pixel(0, 0).0.map(f32::to_bits)
    );
    for (a, b) in result.pixels().zip(source.pixels()) {
        assert_eq!(a[3].to_bits(), b[3].to_bits());
    }
    dump("source.rgba32.le", &source);
    dump("scoped.rgba32.le", &result);
    std::fs::write(
        output().join("scoped.json"),
        serde_json::to_vec(&m).unwrap(),
    )
    .unwrap();
    let mut editor = Editor::new(doc, true);
    editor.edit("세부 선명화 초기화", |d| {
        d.active_layer_mut().unwrap().adjustments.detail_sharpen = Default::default()
    });
    editor.undo();
    assert_eq!(crate::precision::composite(&editor.document), result);
    editor.redo();
    editor.undo();
    let project = output().join("raw-detail_sharpen.rphoto");
    files::save_project(&editor.document, &project).unwrap();
    let restored = files::load_project(&project).unwrap();
    assert_eq!(
        restored.active_layer().unwrap().adjustments.detail_sharpen,
        m
    );
    assert_eq!(crate::precision::composite(&restored), result);
    assert!(
        restored
            .active_layer()
            .unwrap()
            .precision
            .as_ref()
            .unwrap()
            .as_raw()
            .iter()
            .zip(source.as_raw())
            .all(|(a, b)| a.to_bits() == b.to_bits())
    );
    assert_eq!(
        *restored
            .active_layer()
            .unwrap()
            .raw_source
            .as_ref()
            .unwrap()
            .bytes,
        *raw.bytes
    );
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
            assert!(a.0.iter().zip(b.0).all(|(a, b)| (*a - b).abs() < 0.00001));
        }
    }

    let image = Rgba32FImage::from_fn(4, 2, |x, y| {
        Rgba([
            0.7,
            x as f32 * 0.1,
            0.2,
            [0., 0.2, 0.5, 1.][((x + y) % 4) as usize] as f32,
        ])
    });
    let mut layered =
        crate::precision::document("alpha".into(), DynamicImage::ImageRgba32F(image.clone()))
            .unwrap();
    let active = layered.active;
    let original_layer = layered.active_layer().unwrap().precision.clone().unwrap();
    crate::adjustment_layer::add(&mut layered).unwrap();
    layered.active_layer_mut().unwrap().adjustments = Adjustments {
        detail_sharpen: m.clone(),
        ..Default::default()
    };
    let actual = crate::precision::composite(&layered);
    for (a, b) in actual.pixels().zip(image.pixels()) {
        assert_eq!(a[3].to_bits(), b[3].to_bits());
    }
    assert!(Arc::ptr_eq(
        layered
            .layers
            .iter()
            .find(|l| l.id == active)
            .unwrap()
            .precision
            .as_ref()
            .unwrap(),
        &original_layer
    ));
    let before = std::fs::read(&project).unwrap();
    let mut invalid = restored.clone();
    invalid
        .active_layer_mut()
        .unwrap()
        .adjustments
        .detail_sharpen
        .threshold = f32::INFINITY;
    assert!(files::save_project(&invalid, &project).is_err());
    assert_eq!(std::fs::read(&project).unwrap(), before);
}
