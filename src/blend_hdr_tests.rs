use super::*;
use crate::{
    document::{Editor, PixelRegion},
    files, precision,
};
use image::{Rgba, Rgba32FImage};
use std::sync::Arc;
fn out() -> std::path::PathBuf {
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/blend-hdr");
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
#[test]
fn all_non_normal_blends_extreme_small_alpha_precision_and_independent_decimal_formulas() {
    let colors = [
        [f32::MAX; 3],
        [-f32::MAX; 3],
        [f32::MAX, -f32::MAX, 1e30],
        [-f32::MAX, f32::MAX, -1e30],
        [-0.5, 1.5, 2.125],
        [0.25, 0.75, 1.],
        [1e-30, -1e-30, 0.],
        [0., 0.5, 1.],
    ];
    let mut cases = Vec::new();
    let mut color_cases = Vec::new();
    for mode in BlendMode::ALL.into_iter().skip(2) {
        for s in colors {
            for d in colors {
                let color = mode.color(s, d);
                assert!(
                    color.iter().all(|v| v.is_finite()),
                    "{} {s:?} {d:?} {color:?}",
                    mode.name()
                );
                color_cases
                    .push(serde_json::json!({"mode":mode,"source":s,"target":d,"pixels":color}));
                for (sa, da) in [
                    (0., 1.),
                    (1., 0.),
                    (1., 1.),
                    (0.4, 0.75),
                    (1e-12, 1e-40),
                    (f32::from_bits(1), f32::from_bits(1)),
                ] {
                    for opacity in [0., f32::from_bits(1), 0.1, 0.5, 1.] {
                        let source = [s[0], s[1], s[2], sa];
                        let target = [d[0], d[1], d[2], da];
                        let pixels = precision::over(source, target, opacity, mode);
                        assert!(
                            pixels.iter().all(|v| v.is_finite()),
                            "{} {source:?} {target:?} {opacity} {pixels:?}",
                            mode.name()
                        );
                        if sa == 0. || opacity == 0. {
                            assert_eq!(pixels.map(f32::to_bits), target.map(f32::to_bits));
                        }
                        cases.push(serde_json::json!({"mode":mode,"source":source,"target":target,"opacity":opacity,"pixels":pixels}));
                    }
                }
            }
        }
    }
    assert_eq!(BlendMode::Screen.channel(f32::MAX, 1.), 1.);
    assert_eq!(BlendMode::Screen.channel(1., f32::MAX), 1.);
    assert_eq!(BlendMode::Exclusion.channel(f32::MAX, 0.5), 0.5);
    assert_eq!(BlendMode::SoftLight.channel(1., f32::MAX), f32::MAX.sqrt());
    for mode in [
        BlendMode::Screen,
        BlendMode::LinearBurn,
        BlendMode::LinearLight,
        BlendMode::ColorBurn,
    ] {
        let (s, d) = match mode {
            BlendMode::Screen => (1e-30, 0.),
            BlendMode::LinearLight => (0.5, 1e-30),
            _ => (1., 1e-30),
        };
        assert_eq!(mode.channel(s, d), 1e-30, "{mode:?}");
    }
    // A product wider than f32 becomes representable after small source coverage.
    let p = precision::over(
        [1e30, 0., 0., 1e-30],
        [1e30, 0., 0., 1.],
        1.,
        BlendMode::Multiply,
    );
    assert!((p[0] as f64 / 2e30 - 1.).abs() < 1e-6);
    // Gray inputs retain target luminance even at opposing storage extrema.
    for mode in [
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ] {
        assert!(
            mode.color([f32::MAX; 3], [-f32::MAX; 3])
                .iter()
                .all(|v| (0.0..=1.0).contains(v))
        );
    }
    std::fs::write(
        out().join("formula.json"),
        serde_json::to_vec(&serde_json::json!({"cases":cases,"color_cases":color_cases})).unwrap(),
    )
    .unwrap();
}
#[test]
fn all_non_normal_blends_pipeline_partial_source_history_project_and_native_outputs() {
    let source = Rgba32FImage::from_fn(5, 3, |x, y| {
        Rgba([
            if x % 2 == 0 { f32::MAX } else { -f32::MAX },
            if y % 2 == 0 { -f32::MAX } else { f32::MAX },
            2.125,
            1.,
        ])
    });
    let mut doc = precision::document(
        "Blend HDR".into(),
        image::DynamicImage::ImageRgba32F(source.clone()),
    )
    .unwrap();
    doc.add_dynamic(
        "Above".into(),
        image::DynamicImage::ImageRgba32F(Rgba32FImage::from_fn(5, 3, |x, y| {
            Rgba([
                -source.get_pixel(x, y)[0],
                -source.get_pixel(x, y)[1],
                -0.5,
                0.4,
            ])
        })),
    )
    .unwrap();
    let mut rendered_cases = Vec::new();
    for mode in BlendMode::ALL.into_iter().skip(2) {
        doc.layers[1].blend = mode;
        let rendered = precision::composite(&doc);
        assert!(rendered.as_raw().iter().all(|v| v.is_finite()));
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
        let mut editor = Editor::new(doc.clone(), true);
        editor.edit("혼합 변경", |d| d.layers[1].blend = BlendMode::Normal);
        editor.undo();
        assert!(Arc::ptr_eq(
            editor.document.layers[0].precision.as_ref().unwrap(),
            &original
        ));
        assert_eq!(precision::composite(&editor.document), rendered);
        let path = out().join(format!("{mode:?}.rphoto"));
        files::save_project(&editor.document, &path).unwrap();
        let restored = files::load_project(&path).unwrap();
        assert_eq!(restored.layers[1].blend, mode);
        assert_eq!(precision::composite(&restored), rendered);
        assert_eq!(
            restored.layers[0].precision.as_ref().unwrap().as_raw(),
            source.as_raw()
        );
        for ext in ["psd", "psb", "exr"] {
            let path = out().join(format!("{mode:?}.{ext}"));
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
                assert!(
                    (*a as f64 - *b as f64).abs() < 3e-5 * f64::from(*b).abs().max(1.),
                    "{mode:?}/{ext}: {a}!={b}"
                );
            }
        }
        rendered_cases
            .push(serde_json::json!({"mode":mode,"size":[5,3],"pixels":rendered.as_raw()}));
    }
    std::fs::write(
        out().join("rendered.json"),
        serde_json::to_vec(&rendered_cases).unwrap(),
    )
    .unwrap();
}
