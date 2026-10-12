use super::*;
fn out() -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/float-blur");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
#[test]
fn float_blur_extremes_tiny_alpha_borders_gaussian_reference_constant_empty_and_independent_kernel()
{
    let mut cases = Vec::new();
    for size in [[1, 1], [1, 7], [9, 1], [7, 5]] {
        for alpha in [f32::from_bits(1), 1e-40, 1e-12, 0.4, 1.] {
            for extreme in [false, true] {
                let source = Rgba32FImage::from_fn(size[0], size[1], |x, y| {
                    Rgba([
                        if extreme {
                            if x % 2 == 0 { f32::MAX } else { -f32::MAX }
                        } else {
                            x as f32 * 0.23 - 0.2
                        },
                        if extreme {
                            if y % 2 == 0 { -f32::MAX } else { f32::MAX }
                        } else {
                            y as f32 * 0.19 - 0.3
                        },
                        if extreme { 1e30 } else { 2.125 },
                        if (x + y) % 4 == 0 { 0. } else { alpha },
                    ])
                });
                let before = source
                    .as_raw()
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>();
                for sigma in [0.1, 0.8, 1., 3., 16.] {
                    let result = blur(&source, sigma);
                    assert!(result.as_raw().iter().all(|v| v.is_finite()));
                    assert_eq!(result.dimensions(), source.dimensions());
                    cases.push(serde_json::json!({"source":source.as_raw(),"pixels":result.as_raw(),"size":size,"sigma":sigma}));
                }
                assert_eq!(
                    source
                        .as_raw()
                        .iter()
                        .map(|v| v.to_bits())
                        .collect::<Vec<_>>(),
                    before
                );
            }
        }
    }
    let constant = Rgba32FImage::from_pixel(3, 2, Rgba([f32::MAX, -f32::MAX, 2.125, 1e-40]));
    for sigma in [0.1, 1., 16.] {
        let result = blur(&constant, sigma);
        for p in result.pixels() {
            assert_eq!(p[0], f32::MAX);
            assert_eq!(p[1], -f32::MAX);
            assert_eq!(p[2], 2.125);
            assert!(p[3] > 0.);
        }
    }
    for sigma in [0., -1., f32::NAN, f32::INFINITY, f32::MIN_POSITIVE / 2.] {
        assert_eq!(blur(&constant, sigma), constant);
    }
    assert_eq!(blur(&Rgba32FImage::new(0, 0), 1.).dimensions(), (0, 0));
    // Compare ordinary alpha-weighted images with the installed independent implementation.
    let ordinary = Rgba32FImage::from_fn(13, 9, |x, y| {
        Rgba([
            x as f32 * 0.11,
            y as f32 * 0.07,
            -0.2,
            if (x + y) % 3 == 0 { 0. } else { 0.4 },
        ])
    });
    for sigma in [0.1, 0.8, 1., 3., 16.] {
        let mut prem = ordinary.clone();
        for p in prem.pixels_mut() {
            for c in 0..3 {
                p[c] *= p[3];
            }
        }
        let mut expected = image::imageops::blur(&prem, sigma);
        for p in expected.pixels_mut() {
            if p[3] > 0. {
                for c in 0..3 {
                    p[c] /= p[3];
                }
            }
        }
        let actual = blur(&ordinary, sigma);
        for (a, b) in actual.as_raw().iter().zip(expected.as_raw()) {
            assert!((*a - *b).abs() < 3e-6, "sigma{sigma}: {a}!={b}");
        }
    }
    std::fs::write(
        out().join("formula.json"),
        serde_json::to_vec(&serde_json::json!({"cases":cases})).unwrap(),
    )
    .unwrap();
}
#[test]
fn float_blur_extended_bounds_pipeline_project_history_and_native_outputs_preserve_extreme_source()
{
    use crate::{adjustments::Adjustments, document::Editor, files, precision};
    use std::sync::Arc;
    let source = Arc::new(Rgba32FImage::from_fn(5, 3, |x, y| {
        Rgba([
            if x % 2 == 0 { f32::MAX } else { -f32::MAX },
            if y % 2 == 0 { -f32::MAX } else { f32::MAX },
            2.125,
            1e-12,
        ])
    }));
    let mut doc = precision::document(
        "Extreme blur".into(),
        image::DynamicImage::ImageRgba32F(source.as_ref().clone()),
    )
    .unwrap();
    doc.layers[0].adjustments = Adjustments {
        blur: 1.,
        expand_filters: true,
        ..Default::default()
    };
    let rendered = precision::composite(&doc);
    assert!(rendered.as_raw().iter().all(|v| v.is_finite()));
    assert!(rendered.pixels().any(|p| p[3] > 0. && p[3] < 1e-10));
    let original = doc.layers[0].precision.clone().unwrap();
    let mut editor = Editor::new(doc, true);
    editor.edit("흐림 반경", |d| d.layers[0].adjustments.blur = 3.);
    editor.undo();
    assert_eq!(precision::composite(&editor.document), rendered);
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &original
    ));
    let path = out().join("extreme.rphoto");
    files::save_project(&editor.document, &path).unwrap();
    let restored = files::load_project(&path).unwrap();
    assert_eq!(precision::composite(&restored), rendered);
    assert_eq!(
        restored.layers[0].precision.as_ref().unwrap().as_raw(),
        source.as_raw()
    );
    for ext in ["psd", "psb", "exr"] {
        let path = out().join(format!("extreme.{ext}"));
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
        let actual = precision::composite(&reopened);
        assert!(actual.as_raw().iter().all(|v| v.is_finite()));
        for (a, b) in actual.as_raw().iter().zip(rendered.as_raw()) {
            assert!((*a as f64 - *b as f64).abs() < 3e-5 * f64::from(*b).abs().max(1.));
        }
    }
}
