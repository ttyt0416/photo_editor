use super::*;
use image::{Rgba, Rgba32FImage};
fn out() -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/float-sampling");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
#[test]
fn float_sampling_hdr_extremes_tiny_alpha_sdr_hidden_border_and_negative_lobes_match_independent_weights()
 {
    let points = [
        [-2.1, -1.7],
        [-0.1, 0.2],
        [0.5, 0.5],
        [0.6, 0.6],
        [1.2, 1.7],
        [2., 1.],
        [3.1, 2.7],
        [4.5, 3.5],
        [5.2, 4.3],
    ];
    let mut cases = Vec::new();
    for alpha in [0., f32::from_bits(1), 1e-40, 1e-12, 0.3, 1.] {
        for extreme in [false, true] {
            let source = Rgba32FImage::from_fn(5, 4, |x, y| {
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
                    if extreme {
                        1e30
                    } else {
                        ((x * 11 + y * 17) % 23) as f32 * 0.15
                    },
                    if (x + y) % 4 == 0 { 0. } else { alpha },
                ])
            });
            for mode in [Sampling::Nearest, Sampling::Bilinear, Sampling::Bicubic] {
                let results = points.map(|point| {
                    rgba(point, mode, |x, y| {
                        if x < 0 || y < 0 || x >= 5 || y >= 4 {
                            [0.; 4]
                        } else {
                            source.get_pixel(x as u32, y as u32).0
                        }
                    })
                });
                assert!(results.iter().flatten().all(|v| v.is_finite()));
                cases.push(serde_json::json!({"source":source.as_raw(),"points":points,"sampling":mode,"pixels":results}));
            }
        }
    }
    for alpha in [f32::from_bits(1), 1e-40, 1e-12] {
        for mode in [Sampling::Bilinear, Sampling::Bicubic] {
            let result = rgba([1.17, 2.21], mode, |_, _| {
                [f32::MAX, -f32::MAX, 2.125, alpha]
            });
            assert!(result[3] > 0.);
            assert_eq!(result[0], f32::MAX);
            assert_eq!(result[1], -f32::MAX);
            assert_eq!(result[2], 2.125);
        }
    }
    let hidden = [-0., f32::MAX, -f32::MAX, 0.];
    let nearest = rgba([0.5, 0.5], Sampling::Nearest, |_, _| hidden);
    assert_eq!(nearest.map(f32::to_bits), hidden.map(f32::to_bits));
    for point in [[f32::NAN, 0.], [f32::INFINITY, 0.], [1.1e9, 0.]] {
        for mode in [Sampling::Nearest, Sampling::Bilinear, Sampling::Bicubic] {
            assert_eq!(
                rgba(point, mode, |_, _| panic!("invalid coordinate fetched")),
                [0.; 4]
            );
        }
    }
    std::fs::write(
        out().join("formula.json"),
        serde_json::to_vec(&serde_json::json!({"cases":cases,"size":[5,4]})).unwrap(),
    )
    .unwrap();
}
#[test]
fn float_sampling_transformed_precision_layers_project_undo_partial_render_and_native_exports_keep_tiny_alpha()
 {
    use crate::{document::Editor, files};
    use std::sync::Arc;
    let source = Arc::new(Rgba32FImage::from_fn(5, 4, |x, y| {
        Rgba([2.125 + x as f32 * 0.1, -0.5, 1.5 + y as f32 * 0.2, 1e-12])
    }));
    let mut doc = crate::precision::document(
        "Tiny alpha transformed".into(),
        image::DynamicImage::ImageRgba32F(source.as_ref().clone()),
    )
    .unwrap();
    doc.layers[0].transform.scale = [1.3, 1.4];
    doc.layers[0].transform.angle = 17.;
    doc.layers[0].transform.sampling = Sampling::Bicubic;
    let rendered = crate::precision::composite(&doc);
    assert!(rendered.as_raw().iter().all(|v| v.is_finite()));
    assert!(
        rendered
            .pixels()
            .any(|p| p[3] > 0. && p[0] > 1. && p[1] < 0.)
    );
    assert!(rendered.pixels().any(|p| p[3] > 0. && p[3] < 1e-10));
    let region = crate::document::PixelRegion {
        min: [1, 1],
        max: [4, 3],
    };
    let partial = crate::precision::composite_region(&doc, region);
    for y in 1..3 {
        for x in 1..4 {
            assert_eq!(partial.get_pixel(x - 1, y - 1), rendered.get_pixel(x, y));
        }
    }
    let original = doc.layers[0].precision.clone().unwrap();
    let mut editor = Editor::new(doc, true);
    editor.edit("보간 변경", |d| {
        d.layers[0].transform.sampling = Sampling::Bilinear
    });
    editor.undo();
    assert_eq!(crate::precision::composite(&editor.document), rendered);
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &original
    ));
    let path = out().join("tiny-alpha.rphoto");
    files::save_project(&editor.document, &path).unwrap();
    let restored = files::load_project(&path).unwrap();
    assert_eq!(crate::precision::composite(&restored), rendered);
    assert_eq!(
        restored.layers[0].precision.as_ref().unwrap().as_raw(),
        source.as_raw()
    );
    for ext in ["psd", "psb", "exr"] {
        let path = out().join(format!("tiny-alpha.{ext}"));
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
        let output = crate::precision::composite(&reopened);
        assert!(output.pixels().any(|p| p[3] > 0. && p[3] < 1e-10));
        for (a, b) in output.as_raw().iter().zip(rendered.as_raw()) {
            assert!((*a - *b).abs() < 3e-5);
        }
    }
}
