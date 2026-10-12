use super::*;
use crate::{
    adjustments::Adjustments,
    document::{Editor, Selection},
    files,
};
use image::{DynamicImage, Rgba, Rgba32FImage};
use std::{path::Path, sync::Arc};
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/color-mixer");
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
fn mixer_bands_neutral_identity_luminosity_continuity_hdr_validation_and_independent_samples() {
    let mut states = vec![ColorMixer::default()];
    for i in 0..8 {
        let mut m = ColorMixer::default();
        m.hue[i] = 47.;
        m.saturation[i] = -0.4;
        m.luminance[i] = 0.2;
        states.push(m);
    }
    states.push(ColorMixer {
        hue: [-33.; 8],
        saturation: [0.6; 8],
        luminance: [0.; 8],
    });
    let mut samples = Vec::new();
    for h in (0..360).step_by(3) {
        for (gain, offset) in [(0.8, 0.1), (3., -0.5), (0.0001, 0.)] {
            samples.push(basis(h as f64).map(|v| (v * gain + offset) as f32));
        }
    }
    samples.extend([
        [0.3; 3],
        [-0.0; 3],
        [2.125, -0.2, 0.4],
        [-2.0, -3., -4.],
        [-f32::MAX, f32::MAX, 0.],
    ]);
    let mut outputs = Vec::new();
    for m in &states {
        for &c in &samples {
            let p = m.apply(c);
            assert!(p.iter().all(|v| v.is_finite()));
            if m.disabled() {
                assert_eq!(p.map(f32::to_bits), c.map(f32::to_bits));
            }
            outputs.push(p);
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
    let m = ColorMixer {
        hue: [60.; 8],
        saturation: [0.; 8],
        luminance: [0.; 8],
    };
    let yellow = m.apply([1., 0., 0.]);
    assert!((yellow[0] - yellow[1]).abs() < 1e-6 && yellow[2].abs() < 1e-6);
    assert!((lum(yellow.map(f64::from)) - 0.2126).abs() < 1e-6);
    let gray = ColorMixer {
        hue: [0.; 8],
        saturation: [-1.; 8],
        luminance: [0.; 8],
    }
    .apply([0.7, 0.2, 0.4]);
    assert_eq!(gray[0], gray[1]);
    assert_eq!(gray[1], gray[2]);
    for m in &states {
        for h in CENTERS {
            let a = m.apply(basis(h - 0.001).map(|v| (v * 0.6 + 0.1) as f32));
            let b = m.apply(basis(h + 0.001).map(|v| (v * 0.6 + 0.1) as f32));
            assert!(a.iter().zip(b).all(|(a, b)| (*a - b).abs() < 0.0001));
        }
    }
    let m = &states[1];
    let green = [0., 1., 0.];
    assert_eq!(m.apply(green), green);
    assert!(
        states[9]
            .apply([2.125, -0.2, 0.4])
            .iter()
            .any(|v| *v < 0. || *v > 1.)
    );
    let old: Adjustments = serde_json::from_str("{}").unwrap();
    assert!(old.color_mixer.disabled());
    let mut bad = ColorMixer::default();
    bad.hue[2] = f32::NAN;
    assert!(!bad.valid());
    bad.hue[2] = 181.;
    assert!(!bad.valid());
    bad.hue[2] = 0.;
    bad.saturation[4] = 1.1;
    assert!(!bad.valid());
}
#[test]
fn mixer_scoped_raw_and_adjustment_layers_project_exports_history_keep_precision_alpha_original() {
    let mut doc = files::open(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/raw/linear-32-o1.dng"),
    )
    .unwrap()
    .document;
    let source = doc.active_layer().unwrap().precision.clone().unwrap();
    let raw = doc.active_layer().unwrap().raw_source.clone().unwrap();
    let mut m = ColorMixer::default();
    m.hue[0] = 55.;
    m.hue[1] = 30.;
    m.saturation[5] = -0.8;
    m.luminance[1] = 0.7;
    m.luminance[6] = 0.4;
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
        color_mixer: m.clone(),
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
    editor.edit("믹서 초기화", |d| {
        d.active_layer_mut().unwrap().adjustments.color_mixer = Default::default()
    });
    editor.undo();
    assert_eq!(crate::precision::composite(&editor.document), result);
    editor.redo();
    editor.undo();
    let project = output().join("raw-mixer.rphoto");
    files::save_project(&editor.document, &project).unwrap();
    let restored = files::load_project(&project).unwrap();
    assert_eq!(restored.active_layer().unwrap().adjustments.color_mixer, m);
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
        color_mixer: m.clone(),
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
        .color_mixer
        .luminance[7] = f32::INFINITY;
    assert!(files::save_project(&invalid, &project).is_err());
    assert_eq!(std::fs::read(&project).unwrap(), before);
}
