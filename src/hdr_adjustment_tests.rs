use super::*;
use image::{Rgba, Rgba32FImage};
fn out() -> std::path::PathBuf {
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/hdr-adjustments");
    std::fs::create_dir_all(&path).unwrap();
    path
}
#[test]
fn hdr_adjustments_extreme_finite_points_identity_alpha_curves_and_scope_match_independent_formulas()
 {
    let values = [
        f32::MAX,
        -f32::MAX,
        1e30,
        -1e30,
        8.,
        -4.,
        1.,
        0.25,
        0.,
        -0.,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
    ];
    let pixels: Vec<_> = values
        .iter()
        .enumerate()
        .flat_map(|(i, &r)| {
            [0., f32::from_bits(1), 1e-12, 0.4, 1.].map(|alpha| {
                [
                    r,
                    values[(i + 3) % values.len()],
                    values[(i + 7) % values.len()],
                    alpha,
                ]
            })
        })
        .collect();
    let source = Rgba32FImage::from_fn(pixels.len() as u32, 1, |x, _| Rgba(pixels[x as usize]));
    let bits = source
        .as_raw()
        .iter()
        .map(|v| v.to_bits())
        .collect::<Vec<_>>();
    let mut configs = vec![
        Adjustments::default(),
        Adjustments {
            exposure: 3.,
            contrast: 0.8,
            brightness: 0.2,
            ..Default::default()
        },
        Adjustments {
            saturation: 1.,
            ..Default::default()
        },
        Adjustments {
            hue: 75.,
            saturation: 0.3,
            ..Default::default()
        },
        Adjustments {
            black: 0.95,
            gamma: 0.1,
            ..Default::default()
        },
        Adjustments {
            monochrome: true,
            invert: true,
            ..Default::default()
        },
        Adjustments {
            gradient_map: true,
            gradient_colors: [[0, 32, 255, 11], [255, 160, 0, 222]],
            ..Default::default()
        },
        Adjustments {
            vignette: -1.,
            grain: 1.,
            noise: 1.,
            ..Default::default()
        },
    ];
    let mut curve = Adjustments::default();
    curve.curves[0].points = vec![[0., 1.], [0.001, 0.], [1., 1.]];
    configs.push(curve);
    let mut balance = Adjustments::default();
    balance.color_balance.shifts = [[100., -100., 50.], [-100., 70., -80.], [100., -100., 100.]];
    configs.push(balance);
    let mut mixer = Adjustments::default();
    mixer.color_mixer.hue = [120.; 8];
    mixer.color_mixer.saturation = [1.; 8];
    mixer.color_mixer.luminance = [-0.5; 8];
    configs.push(mixer);
    let mut grading = Adjustments::default();
    grading.color_grading.tones[0].hue = 120.;
    grading.color_grading.tones[0].saturation = 1.;
    grading.color_grading.tones[0].luminance = -0.7;
    configs.push(grading);
    let mut records = Vec::new();
    for (i, a) in configs.iter().enumerate() {
        assert!(a.valid());
        let result = a.apply_float(&source);
        for (x, _) in pixels.iter().enumerate() {
            let direct = a.sample(pixels[x], [x as u32, 0], [pixels.len() as u32, 1]);
            assert_eq!(direct, result.get_pixel(x as u32, 0).0);
            assert!(direct.iter().all(|v| v.is_finite()), "config {i},pixel{x}");
            if !a.gradient_map {
                assert_eq!(direct[3].to_bits(), pixels[x][3].to_bits());
            }
        }
        if i == 0 {
            assert_eq!(
                result
                    .as_raw()
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                bits
            );
        }
        records.push(serde_json::json!({"settings":a,"pixels":result.as_raw()}));
    }
    let mut scopes = Vec::new();
    for original in &pixels {
        for adjusted in &pixels {
            for coverage in [0., 0.01, 0.5, 0.99, 1.] {
                let mixed = crate::adjustment_scope::mix(*original, *adjusted, coverage);
                assert!(mixed.iter().all(|v| v.is_finite()));
                scopes.push(serde_json::json!({"original":original,"adjusted":adjusted,"coverage":coverage,"pixels":mixed}));
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
    std::fs::write(out().join("formula.json"),serde_json::to_vec(&serde_json::json!({"source":pixels,"size":[pixels.len(),1],"cases":records,"scopes":scopes})).unwrap()).unwrap();
    assert!(Curve::default().value(f32::MAX).is_finite());
    assert_eq!(Curve::default().value(-f32::MAX), -f32::MAX);
}
#[test]
fn hdr_adjustments_layer_lens_pipeline_project_undo_and_failed_save_preserve_extreme_source() {
    use crate::{document::Editor, files};
    use std::sync::Arc;
    let source = Arc::new(Rgba32FImage::from_fn(5, 3, |x, y| {
        Rgba([
            if x % 2 == 0 { f32::MAX } else { -f32::MAX },
            if y % 2 == 0 { -f32::MAX } else { f32::MAX },
            1e30,
            [0., 1e-12, 1.][y as usize],
        ])
    }));
    let mut doc = crate::precision::document(
        "HDR extreme".into(),
        image::DynamicImage::ImageRgba32F(source.as_ref().clone()),
    )
    .unwrap();
    doc.layers[0].adjustments.hue = 75.;
    doc.layers[0].adjustments.saturation = 1.;
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/lens-profiles/synthetic.xml");
    let db = crate::lens_profile::load(&path).unwrap();
    doc.layers[0].adjustments.lens_profile = Some(db.profiles[0].clone());
    let adjusted = doc.layers[0].adjustments.apply_float(&source);
    assert!(adjusted.as_raw().iter().all(|v| v.is_finite()));
    let original = doc.layers[0].precision.clone().unwrap();
    let mut editor = Editor::new(doc, true);
    editor.edit("HDR 노출", |d| d.layers[0].adjustments.exposure = 4.);
    editor.undo();
    assert_eq!(editor.document.layers[0].adjustments.exposure, 0.);
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &original
    ));
    let path = out().join("extreme.rphoto");
    files::save_project(&editor.document, &path).unwrap();
    let restored = files::load_project(&path).unwrap();
    assert_eq!(
        restored.layers[0].precision.as_ref().unwrap().as_raw(),
        source.as_raw()
    );
    assert_eq!(
        restored.layers[0].adjustments.apply_float(&source),
        adjusted
    );
    let before = std::fs::read(&path).unwrap();
    let mut invalid = restored;
    invalid.layers[0].adjustments.gamma = f32::NAN;
    assert!(files::save_project(&invalid, &path).is_err());
    assert_eq!(std::fs::read(path).unwrap(), before);
}
