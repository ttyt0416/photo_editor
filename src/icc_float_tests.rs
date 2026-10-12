use super::*;
use crate::files::{ExportOptions, export, load_project, open, save_project};
use image::{ImageFormat, Rgba, Rgba32FImage};
use moxcms::ToneReprCurve;
use std::path::Path;

fn write_float(path: &Path, image: &Rgba32FImage) {
    std::fs::write(
        path,
        image
            .as_raw()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
}

#[test]
fn icc_float_sdr_matrix_reference_keeps_out_of_gamut_and_alpha() {
    // Independent NumPy XYZ-matrix/TRC calculation from serialized ICC tags.
    let source = Rgba32FImage::from_raw(
        4,
        1,
        vec![
            0., 1., 0., -0., 0.25, 0.5, 0.75, 0., 0.1, 0.2, 0.3, 0.125, 1., 1., 1., 1.,
        ],
    )
    .unwrap();
    let expected = [
        [
            -3.1120597136961483,
            1.0220987529247985,
            0.024353907175998314,
        ],
        [-0.3452852974443335, 0.5168620196011476, 0.7515238652363996],
        [-0.04599507463761619, 0.19189564219230074, 0.293208528057279],
        [1.0001868539464738, 0.9999769957803065, 0.9995859727672171],
    ];
    let profile = include_bytes!("../tests/fixtures/tiff-rgb-associated/matrix.icc");
    let converted = convert(&DynamicImage::ImageRgba32F(source.clone()), profile)
        .unwrap()
        .to_rgba32f();
    let mut maximum = 0f64;
    for ((actual, source), reference) in converted.pixels().zip(source.pixels()).zip(expected) {
        assert_eq!(actual[3].to_bits(), source[3].to_bits());
        for c in 0..3 {
            maximum = maximum
                .max((f64::from(actual[c]) - reference[c]).abs() / reference[c].abs().max(1.));
        }
    }
    assert!(
        maximum < 0.0008,
        "SDR matrix reference error {maximum} exceeds 0.0008"
    );
}

#[test]
fn icc_float_rgb_rgba_hdr_negative_tiff_project_working_export() {
    icc_float_rgb_cases(true);
}

#[test]
fn icc_float_rgb_rgba_sdr_tiff_project_working_export() {
    icc_float_rgb_cases(false);
}

fn icc_float_rgb_cases(hdr: bool) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(if hdr {
        "qa-output/icc-float"
    } else {
        "qa-output/icc-float-sdr"
    });
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("working.icc"), srgb_profile()).unwrap();
    let source = Rgba32FImage::from_fn(17, 9, |x, y| {
        if !hdr {
            return Rgba([
                if x == 0 { -0. } else { x as f32 / 16. },
                y as f32 / 8.,
                ((x * 19 + y * 13) % 113) as f32 / 112.,
                [0., -0., 0.125, 0.33333334, 1.][(x as usize + y as usize) % 5],
            ]);
        }
        Rgba([
            [-2., -0.125, -0.0, 0.001, 0.5, 1., 2., 16., 1e8][y as usize],
            x as f32 / 4. - 1.,
            (x * 19 + y * 13) as f32 / 11.,
            [0., -0.0, 0.125, 0.33333334, 1.][(x as usize + y as usize) % 5],
        ])
    });
    let mut cases = vec![];
    let mut profiles = vec![
        "srgb", "p3", "adobe", "linear", "table", "gamma", "para1", "para2", "para4",
    ];
    if !hdr {
        profiles.push("matrix");
    }
    for name in profiles {
        let mut profile = match name {
            "matrix" => ColorProfile::new_from_slice(include_bytes!(
                "../tests/fixtures/tiff-rgb-associated/matrix.icc"
            ))
            .unwrap(),
            "srgb" => ColorProfile::new_srgb(),
            "adobe" => ColorProfile::new_adobe_rgb(),
            _ => ColorProfile::new_display_p3(),
        };
        let curve = match name {
            "linear" => Some(ToneReprCurve::Lut(vec![])),
            "gamma" => Some(ToneReprCurve::Lut(vec![563])),
            "table" => Some(ToneReprCurve::Lut(
                (0..=32)
                    .map(|i| ((i as f64 / 32.).powf(1.8) * 65535.).round() as u16)
                    .collect(),
            )),
            "para1" => Some(ToneReprCurve::Parametric(vec![2.2, 1., 0.])),
            "para2" => Some(ToneReprCurve::Parametric(vec![2.2, 1., 0., 0.01])),
            "para4" => Some(ToneReprCurve::Parametric(vec![
                2.2, 1., 0., 0.1, 0.05, 0.02, 0.003,
            ])),
            _ => None,
        };
        if let Some(curve) = curve {
            profile.red_trc = Some(curve.clone());
            profile.green_trc = Some(curve.clone());
            profile.blue_trc = Some(curve);
        }
        let icc = profile.encode().unwrap();
        std::fs::write(root.join(format!("{name}.icc")), &icc).unwrap();
        for alpha in [false, true] {
            let id = format!("{name}-alpha{alpha}");
            let input = if alpha {
                DynamicImage::ImageRgba32F(source.clone())
            } else {
                DynamicImage::ImageRgb32F(DynamicImage::ImageRgba32F(source.clone()).to_rgb32f())
            };
            let native = input.to_rgba32f();
            let converted = convert(&input, &icc).unwrap().to_rgba32f();
            assert!(
                !hdr || converted.pixels().any(|p| p.0[..3].iter().any(|v| *v > 1.)),
                "{id}"
            );
            assert!(
                converted
                    .pixels()
                    .zip(native.pixels())
                    .all(|(d, s)| d[3].to_bits() == s[3].to_bits())
            );
            write_float(&root.join(format!("{id}.src.f32")), &native);
            write_float(&root.join(format!("{id}.dst.f32")), &converted);
            let path = root.join(format!("{id}.tiff"));
            let mut encoder =
                tiff::encoder::TiffEncoder::new(std::fs::File::create(&path).unwrap()).unwrap();
            if alpha {
                let mut image = encoder
                    .new_image::<tiff::encoder::colortype::RGBA32Float>(17, 9)
                    .unwrap();
                image
                    .encoder()
                    .write_tag(tiff::tags::Tag::IccProfile, icc.as_slice())
                    .unwrap();
                image.write_data(native.as_raw()).unwrap();
            } else {
                let mut image = encoder
                    .new_image::<tiff::encoder::colortype::RGB32Float>(17, 9)
                    .unwrap();
                image
                    .encoder()
                    .write_tag(tiff::tags::Tag::IccProfile, icc.as_slice())
                    .unwrap();
                image.write_data(input.to_rgb32f().as_raw()).unwrap();
            }
            let original = std::fs::read(&path).unwrap();
            let imported = open(&path).unwrap();
            assert!(
                imported.notes.iter().any(|n| n.contains("적용해")),
                "{id}: {:?}",
                imported.notes
            );
            let expected = crate::precision::document(
                "reference".into(),
                DynamicImage::ImageRgba32F(converted),
            )
            .unwrap();
            let pixels = crate::precision::composite(&expected);
            assert_eq!(crate::precision::composite(&imported.document), pixels);
            assert_eq!(imported.document.layers[0].bit_depth, 32);
            assert_eq!(
                imported.document.source_icc.as_deref().unwrap().as_slice(),
                icc
            );
            let mut editor = crate::document::Editor::new(imported.document, false);
            editor.edit("rename", |d| {
                d.layers[0].name = if hdr { "HDR" } else { "SDR" }.into()
            });
            editor.undo();
            editor.redo();
            save_project(&editor.document, &root.join(format!("{id}.rphoto"))).unwrap();
            let restored = load_project(&root.join(format!("{id}.rphoto"))).unwrap();
            assert_eq!(restored.source_icc, editor.document.source_icc);
            assert_eq!(crate::precision::composite(&restored), pixels);
            write_float(&root.join(format!("{id}.comp.f32")), &pixels);
            for (ext, format) in [("tiff", ImageFormat::Tiff), ("exr", ImageFormat::OpenExr)] {
                let output = root.join(format!("{id}-working.{ext}"));
                export(
                    &restored,
                    &output,
                    format,
                    &ExportOptions {
                        tiff_depth: 3,
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(
                    crate::precision::composite(&open(&output).unwrap().document),
                    pixels
                );
            }
            assert_eq!(std::fs::read(path).unwrap(), original);
            cases.push(serde_json::json!({"id":id,"profile":name,"alpha":alpha,"size":[17,9]}));
        }
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

#[test]
fn icc_float_nonfinite_rgb_or_alpha_and_overflow_keep_all_source_bits() {
    let icc = ColorProfile::new_display_p3().encode().unwrap();
    for (channel, value) in [
        (0, f32::MAX),
        (0, f32::INFINITY),
        (1, f32::NAN),
        (3, f32::NAN),
        (3, f32::NEG_INFINITY),
    ] {
        let mut source = Rgba32FImage::from_pixel(2, 1, Rgba([-1., 2., 4., 0.125]));
        source.get_pixel_mut(1, 0)[channel] = value;
        let expected = source
            .as_raw()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>();
        let mut notes = vec![];
        let output = apply(DynamicImage::ImageRgba32F(source), &icc, &mut notes).to_rgba32f();
        assert_eq!(
            output
                .as_raw()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(notes[0].contains("못했습니다"));
    }
}
