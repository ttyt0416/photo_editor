use super::*;
use crate::{
    adjustments::Adjustments,
    document::{Editor, PixelRegion},
    files, precision,
};
use std::sync::Arc;
fn out() -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/bloom-hdr");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
#[test]
fn bloom_hdr_extremes_tiny_alpha_hidden_rgb_edges_constant_and_independent_formulas() {
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
                for (amount, radius, threshold, knee) in [
                    (5., 0., 32., 1.),
                    (1., 1., 1., 0.5),
                    (0.125, 0.1, 0., 0.),
                    (3., 3., 2., 1.),
                ] {
                    let bloom = Bloom {
                        amount,
                        radius,
                        threshold,
                        knee,
                    };
                    assert!(bloom.valid());
                    for expand in [false, true] {
                        let output = bloom.apply_alpha(&source, expand);
                        assert!(output.as_raw().iter().all(|v| v.is_finite()));
                        assert_eq!(output.dimensions(), source.dimensions());
                        if !expand {
                            for (p, s) in output.pixels().zip(source.pixels()) {
                                assert_eq!(p[3].to_bits(), s[3].to_bits());
                                if s[3] == 0. {
                                    assert_eq!(p, s);
                                }
                            }
                        }
                        cases.push(serde_json::json!({"source":source.as_raw(),"pixels":output.as_raw(),"size":size,"bloom":bloom,"expand":expand}));
                    }
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
    let constant = Rgba32FImage::from_pixel(3, 2, Rgba([f32::MAX, 2.125, -f32::MAX, 1e-40]));
    for expand in [false, true] {
        assert_eq!(Bloom::default().apply_alpha(&constant, expand), constant);
        assert_eq!(
            Bloom {
                amount: 1.,
                radius: f32::NAN,
                ..Default::default()
            }
            .apply_alpha(&constant, expand),
            constant
        );
        let bloom = Bloom {
            amount: 5.,
            radius: 1.,
            threshold: 1.,
            knee: 0.5,
        };
        assert_eq!(
            bloom
                .apply_alpha(&Rgba32FImage::new(0, 0), expand)
                .dimensions(),
            (0, 0)
        );
        for p in bloom.apply_alpha(&constant, expand).pixels() {
            assert!(p.0.iter().all(|v| v.is_finite()));
            assert!(p[3] > 0.);
            assert_eq!(p[0], f32::MAX);
        }
    }
    std::fs::write(
        out().join("formula.json"),
        serde_json::to_vec(&serde_json::json!({"cases":cases})).unwrap(),
    )
    .unwrap();
}
#[test]
fn bloom_hdr_pipeline_extended_project_bake_history_partial_and_native_outputs() {
    let source = Rgba32FImage::from_fn(5, 3, |x, y| {
        Rgba([
            if x % 2 == 0 { f32::MAX } else { -f32::MAX },
            if y % 2 == 0 { -f32::MAX } else { f32::MAX },
            2.125,
            1e-12,
        ])
    });
    let mut doc = precision::document(
        "Bloom HDR".into(),
        image::DynamicImage::ImageRgba32F(source.clone()),
    )
    .unwrap();
    doc.layers[0].adjustments = Adjustments {
        bloom: Bloom {
            amount: 3.,
            radius: 1.,
            threshold: 1.,
            knee: 0.5,
        },
        expand_filters: true,
        ..Default::default()
    };
    let rendered = precision::composite(&doc);
    assert!(rendered.as_raw().iter().all(|v| v.is_finite()));
    assert!(rendered.pixels().any(|p| p[3] > 0. && p[3] < 1e-10));
    assert_eq!(
        precision::composite_region(
            &doc,
            PixelRegion {
                min: [1, 1],
                max: [4, 3]
            }
        ),
        image::imageops::crop_imm(&rendered, 1, 1, 3, 2).to_image()
    );
    let original = doc.layers[0].precision.clone().unwrap();
    let mut editor = Editor::new(doc, true);
    let id = editor.document.active;
    editor.edit("원본 적용", |d| {
        crate::adjustment_scope::bake(d, id).unwrap()
    });
    assert_eq!(precision::composite(&editor.document), rendered);
    editor.undo();
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &original
    ));
    editor.redo();
    assert_eq!(precision::composite(&editor.document), rendered);
    editor.undo();
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
        let actual = precision::composite(&files::open(&path).unwrap().document);
        assert!(actual.as_raw().iter().all(|v| v.is_finite()));
        for (a, b) in actual.as_raw().iter().zip(rendered.as_raw()) {
            assert!((*a as f64 - *b as f64).abs() < 3e-5 * f64::from(*b).abs().max(1.));
        }
    }
}
