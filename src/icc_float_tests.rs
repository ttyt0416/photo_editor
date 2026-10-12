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
fn icc_float_rgb_rgba_hdr_negative_tiff_project_working_export() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/icc-float");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("working.icc"), srgb_profile()).unwrap();
    let source = Rgba32FImage::from_fn(17, 9, |x, y| {
        Rgba([
            [-2., -0.125, -0.0, 0.001, 0.5, 1., 2., 16., 1e8][y as usize],
            x as f32 / 4. - 1.,
            (x * 19 + y * 13) as f32 / 11.,
            [0., -0.0, 0.125, 0.33333334, 1.][(x as usize + y as usize) % 5],
        ])
    });
    let mut cases = vec![];
    for name in [
        "srgb", "p3", "adobe", "linear", "table", "gamma", "para1", "para2", "para4",
    ] {
        let mut profile = match name {
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
                converted.pixels().any(|p| p.0[..3].iter().any(|v| *v > 1.)),
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
            editor.edit("rename", |d| d.layers[0].name = "HDR".into());
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
