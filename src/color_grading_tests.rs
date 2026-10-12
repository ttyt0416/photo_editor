use super::*;
use crate::{
    adjustments::Adjustments,
    document::{Editor, Selection},
    files,
};
use image::{DynamicImage, Rgba, Rgba32FImage};
use std::{path::Path, sync::Arc};
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/color-grading");
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
fn grading_weights_identity_luminosity_continuity_hdr_validation_and_independent_samples() {
    let mut states = vec![ColorGrading::default()];
    for tone in 0..4 {
        for blend in [0., 50., 100.] {
            for balance in [-100., 0., 100.] {
                let mut g = ColorGrading::default();
                g.tones[tone] = Grade {
                    hue: 220. + tone as f32 * 20.,
                    saturation: 0.7,
                    luminance: 0.,
                };
                g.blending = blend;
                g.balance = balance;
                states.push(g);
            }
        }
    }
    let mut all = states[1].clone();
    all.tones = std::array::from_fn(|i| Grade {
        hue: i as f32 * 80.,
        saturation: 0.35,
        luminance: if i == 3 { 0.1 } else { i as f32 * 0.2 - 0.2 },
    });
    states.push(all);
    let mut samples = Vec::new();
    for h in (0..360).step_by(12) {
        for (gain, offset) in [(0.8, 0.1), (3., -0.5)] {
            samples.push(basis(h as f64).map(|v| (v * gain + offset) as f32));
        }
    }
    for i in 0..=100 {
        samples.push([i as f32 / 100.; 3]);
    }
    samples.extend([
        [-0.0; 3],
        [-2.; 3],
        [2.125, -0.2, 0.4],
        [-f32::MAX, f32::MAX, 0.],
    ]);
    let mut outputs = Vec::new();
    for g in &states {
        assert!(g.valid());
        for &c in &samples {
            let out = g.apply(c);
            assert!(out.iter().all(|v| v.is_finite()));
            if !g.enabled() {
                assert_eq!(out.map(f32::to_bits), c.map(f32::to_bits));
            }
            if g.tones.iter().all(|t| t.luminance == 0.) {
                let y = lum(c.map(f64::from));
                assert!((lum(out.map(f64::from)) - y).abs() < 1e-6 * y.abs().max(1.));
            }
            outputs.push(out);
        }
        for i in 0..=100 {
            let w = g.weights(i as f64 / 100.);
            assert!((w.iter().sum::<f64>() - 1.).abs() < 1e-12);
            assert!(w.iter().all(|v| *v >= 0.));
        }
    }
    std::fs::write(
        output().join("formula.json"),
        serde_json::to_vec(
            &serde_json::json!({"states":states,"samples":samples,"outputs":outputs}),
        )
        .unwrap(),
    )
    .unwrap();
    let mut weights = ColorGrading::default();
    let neutral_weights = weights.weights(0.5);
    weights.balance = 100.;
    assert!(weights.weights(0.5)[2] > neutral_weights[2]);
    assert!(weights.weights(0.5)[0] < neutral_weights[0]);
    weights.balance = 0.;
    weights.blending = 0.;
    let separated = weights.weights(0.25);
    weights.blending = 100.;
    assert!(weights.weights(0.25)[2] > separated[2]);
    weights.tones[0].saturation = 0.7;
    weights.tones[2] = Grade {
        hue: 240.,
        saturation: 0.6,
        luminance: 0.,
    };
    for i in 1..1000 {
        let a = weights.apply([i as f32 / 1000.; 3]);
        let b = weights.apply([(i + 1) as f32 / 1000.; 3]);
        assert!(a.iter().zip(b).all(|(a, b)| (*a - b).abs() < 0.01));
    }
    let g = &states[1];
    assert_eq!(g.apply([0.; 3]), [0.; 3]);
    assert_eq!(g.apply([1.; 3]), [1.; 3]);
    assert_ne!(g.apply([0.25; 3]), [0.25; 3]);
    assert!(
        g.apply([2.125, -0.2, 0.4])
            .iter()
            .any(|v| *v > 1. || *v < 0.)
    );
    let mut neutral = ColorGrading::default();
    neutral.tones[2].hue = 240.;
    neutral.blending = 81.;
    neutral.balance = -10.;
    let a = Adjustments {
        color_grading: neutral.clone(),
        ..Default::default()
    };
    assert!(!a.active());
    assert_eq!(
        neutral.apply([-0.0, 2., -1.]).map(f32::to_bits),
        [-0.0f32, 2., -1.].map(f32::to_bits)
    );
    assert_eq!(
        serde_json::from_str::<Adjustments>(&serde_json::to_string(&a).unwrap())
            .unwrap()
            .color_grading,
        neutral
    );
    assert_eq!(
        serde_json::from_str::<Adjustments>("{}")
            .unwrap()
            .color_grading,
        ColorGrading::default()
    );
    let mut bad = neutral;
    bad.tones[1].saturation = f32::NAN;
    assert!(!bad.valid());
    bad.tones[1].saturation = 1.1;
    assert!(!bad.valid());
    bad.tones[1].saturation = 0.;
    bad.balance = 101.;
    assert!(!bad.valid());
}
#[test]
fn grading_wheel_pointer_drag_center_and_boundary() {
    use eframe::egui::{self, Event, Pos2, Vec2};
    let ctx = egui::Context::default();
    let mut g = Grade::default();
    let mut time = 0.;
    let mut frame = |events: Vec<Event>, g: &mut Grade| {
        time += 0.05;
        let mut rect = egui::Rect::NOTHING;
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::new(300., 200.))),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ctx| {
                rect = wheel(ctx, g).rect;
            },
        );
        out.textures_delta.clear();
        rect
    };
    let r = frame(vec![], &mut g);
    frame(vec![], &mut g);
    let point = r.center() + Vec2::new(0., -33.);
    frame(
        vec![
            Event::PointerMoved(point),
            Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        &mut g,
    );
    frame(
        vec![Event::PointerButton {
            pos: point,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        &mut g,
    );
    assert!((g.hue - 270.).abs() < 1e-4);
    assert!((g.saturation - 0.75).abs() < 1e-4);
    let start = r.center();
    frame(
        vec![
            Event::PointerMoved(start),
            Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        &mut g,
    );
    let end = start + Vec2::new(80., 0.);
    frame(vec![Event::PointerMoved(end)], &mut g);
    frame(
        vec![Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        &mut g,
    );
    assert_eq!(g.saturation, 1.);
    assert!(g.hue.abs() < 1e-4);
    frame(
        vec![
            Event::PointerMoved(start),
            Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ],
        &mut g,
    );
    frame(
        vec![Event::PointerButton {
            pos: start,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
        &mut g,
    );
    assert_eq!(g.saturation, 0.);
}
#[test]
fn grading_scoped_raw_and_adjustment_layers_project_exports_history_keep_precision_alpha_original()
{
    let mut doc = files::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw/linear-32-o1.dng"),
    )
    .unwrap()
    .document;
    let source = doc.active_layer().unwrap().precision.clone().unwrap();
    let raw = doc.active_layer().unwrap().raw_source.clone().unwrap();
    let mut m = ColorGrading::default();
    m.tones[0] = Grade {
        hue: 220.,
        saturation: 0.6,
        luminance: 0.1,
    };
    m.tones[1] = Grade {
        hue: 35.,
        saturation: 0.4,
        luminance: -0.1,
    };
    m.tones[2] = Grade {
        hue: 290.,
        saturation: 0.7,
        luminance: 0.2,
    };
    m.tones[3] = Grade {
        hue: 135.,
        saturation: 0.1,
        luminance: 0.,
    };
    m.balance = 25.;
    m.blending = 72.;
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
        color_grading: m.clone(),
        scope: Some(scope.clone()),
        ..Default::default()
    };
    let result = crate::precision::composite(&doc);
    assert_ne!(&result, source.as_ref());
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
    editor.edit("그레이딩 초기화", |d| {
        d.active_layer_mut().unwrap().adjustments.color_grading = Default::default()
    });
    editor.undo();
    assert_eq!(crate::precision::composite(&editor.document), result);
    editor.redo();
    editor.undo();
    let project = output().join("raw-grading.rphoto");
    files::save_project(&editor.document, &project).unwrap();
    let restored = files::load_project(&project).unwrap();
    assert_eq!(
        restored.active_layer().unwrap().adjustments.color_grading,
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
        color_grading: m.clone(),
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
        .color_grading
        .tones[3]
        .luminance = f32::INFINITY;
    assert!(files::save_project(&invalid, &project).is_err());
    assert_eq!(std::fs::read(&project).unwrap(), before);
}
