use super::*;
fn out() -> std::path::PathBuf {
    let dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/float-composite");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
#[test]
fn float_composite_normal_extremes_tiny_alpha_opacity_endpoints_dissolve_and_independent_over() {
    let colors = [
        [f32::MAX, f32::MAX, -f32::MAX],
        [-f32::MAX, -f32::MAX, f32::MAX],
        [1e30, -1e30, 2.],
        [-0., 0., 0.],
        [-0.5, 1.5, 2.125],
        [0.25, 0.75, 1.],
    ];
    let alphas = [0., f32::from_bits(1), 1e-40, 1e-12, 0.4, 1.];
    let mut cases = Vec::new();
    for s in colors {
        for d in colors {
            for sa in alphas {
                for da in alphas {
                    for opacity in [0., f32::from_bits(1), 0.1, 0.5, 1.] {
                        let source = [s[0], s[1], s[2], sa];
                        let target = [d[0], d[1], d[2], da];
                        let result = over(source, target, opacity, BlendMode::Normal);
                        assert!(result.iter().all(|v| v.is_finite()));
                        if opacity == 0. || sa == 0. {
                            assert_eq!(result.map(f32::to_bits), target.map(f32::to_bits));
                        }
                        if sa == 1. && opacity == 1. {
                            assert_eq!(result.map(f32::to_bits), source.map(f32::to_bits));
                        }
                        cases.push(serde_json::json!({"source":source,"target":target,"opacity":opacity,"pixels":result}));
                    }
                }
            }
        }
    }
    let source = [f32::MAX, -f32::MAX, 2.125, 0.5];
    let target = [-f32::MAX, f32::MAX, -0.5, 1.];
    let mut opaque = 0;
    let mut dissolved = Vec::new();
    for y in 0..32 {
        for x in 0..32 {
            let p = over_at(source, target, 0.5, BlendMode::Dissolve, [x, y], 7);
            assert!(p.iter().all(|v| v.is_finite()));
            assert_eq!(
                p,
                over_at(source, target, 0.5, BlendMode::Dissolve, [x, y], 7)
            );
            if p[0] == source[0] {
                opaque += 1;
            } else {
                assert_eq!(p, target);
            }
            dissolved.push(p);
        }
    }
    assert!((180..340).contains(&opaque));
    std::fs::write(out().join("formula.json"),serde_json::to_vec(&serde_json::json!({"cases":cases,"dissolve":dissolved,"source":source,"target":target,"opacity":0.5,"seed":7,"size":[32,32]})).unwrap()).unwrap();
}
#[test]
fn float_composite_pass_through_group_layer_mask_project_history_partial_and_native_outputs_preserve_extremes()
 {
    use crate::{document::Editor, files};
    let source = Arc::new(Rgba32FImage::from_fn(5, 3, |x, y| {
        Rgba([
            if x % 2 == 0 { f32::MAX } else { -f32::MAX },
            if y % 2 == 0 { -f32::MAX } else { f32::MAX },
            2.125,
            1.,
        ])
    }));
    let mut doc = document(
        "Composite extrema".into(),
        DynamicImage::ImageRgba32F(source.as_ref().clone()),
    )
    .unwrap();
    doc.add_dynamic(
        "mixed".into(),
        DynamicImage::ImageRgba32F(Rgba32FImage::from_fn(5, 3, |x, y| {
            Rgba([
                if x % 2 == 0 { -f32::MAX } else { f32::MAX },
                if y % 2 == 0 { f32::MAX } else { -f32::MAX },
                -0.5,
                0.4,
            ])
        })),
    )
    .unwrap();
    doc.layers[1].opacity = 0.5;
    let rendered = composite(&doc);
    assert!(rendered.as_raw().iter().all(|v| v.is_finite()));
    let region = PixelRegion {
        min: [1, 1],
        max: [4, 3],
    };
    let partial = composite_region(&doc, region);
    for y in 1..3 {
        for x in 1..4 {
            assert_eq!(partial.get_pixel(x - 1, y - 1), rendered.get_pixel(x, y));
        }
    }
    let mut editor = Editor::new(doc, true);
    let original = editor.document.layers[0].precision.clone().unwrap();
    editor.edit("불투명도", |d| d.layers[1].opacity = 0.7);
    assert!(
        composite(&editor.document)
            .as_raw()
            .iter()
            .all(|v| v.is_finite())
    );
    editor.undo();
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &original
    ));
    assert_eq!(composite(&editor.document), rendered);
    let path = out().join("extreme.rphoto");
    files::save_project(&editor.document, &path).unwrap();
    let restored = files::load_project(&path).unwrap();
    assert_eq!(composite(&restored), rendered);
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
        let actual = composite(&reopened);
        assert!(actual.as_raw().iter().all(|v| v.is_finite()));
        if ext == "exr" {
            assert_eq!(actual, rendered);
        } else {
            let mut quantized = restored.clone();
            quantized.layers[1].opacity = 128. / 255.;
            let expected = composite(&quantized);
            for (a, b) in actual.as_raw().iter().zip(expected.as_raw()) {
                assert!((*a as f64 - *b as f64).abs() < 3e-6 * f64::from(*b).abs().max(1.));
            }
        }
    }
    let mut group = restored;
    group.group_active().unwrap();
    let id = group.active;
    let g = group.layers.iter_mut().find(|l| l.id == id).unwrap();
    g.pass_through = true;
    g.opacity = 0.5;
    g.mask = Some(Arc::new(image::GrayImage::from_pixel(
        5,
        3,
        image::Luma([128]),
    )));
    assert!(composite(&group).as_raw().iter().all(|v| v.is_finite()));
    let grouped = composite(&group);
    let coverage = 0.5 * (128f32 / 255.);
    for (i, pixel) in grouped.pixels().enumerate() {
        let old = source.as_raw()[i * 4..i * 4 + 4].try_into().unwrap();
        let changed = rendered.get_pixel((i % 5) as u32, (i / 5) as u32).0;
        let expected = crate::adjustment_scope::mix(old, changed, coverage);
        assert_eq!(pixel.0, expected);
    }
    let group_path = out().join("grouped.rphoto");
    files::save_project(&group, &group_path).unwrap();
    assert_eq!(
        composite(&files::load_project(&group_path).unwrap()),
        grouped
    );
    std::fs::write(
        out().join("grouped.rgba32.le"),
        grouped
            .as_raw()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();

    std::fs::write(
        out().join("rendered.rgba32.le"),
        rendered
            .as_raw()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}
