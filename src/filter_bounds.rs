use crate::document::Layer;
use std::sync::Arc;

pub fn valid(layer: &Layer) -> bool {
    if !layer.adjustments.valid() || !layer.effects.valid() {
        return false;
    }
    if layer.canvas_space() {
        return true;
    }
    let pad = layer.adjustments.padding() + layer.effects.padding();
    let size = [layer.pixels.width(), layer.pixels.height()].map(|v| v.saturating_add(pad * 2));
    crate::document::validate_size(size[0], size[1]).is_ok()
        && layer.transform.padded(pad as i32).valid_for(size)
        && layer
            .offset
            .iter()
            .all(|v| v.checked_sub(pad as i32).is_some())
}

// Preserve source geometry and the mask's world coordinates while materializing filters.
pub fn bake(layer: &mut Layer) -> Result<(), String> {
    if !valid(layer) {
        return Err("필터 외곽 크기 또는 원근 변형이 한계를 넘습니다. 범위를 줄이세요.".into());
    }
    let source = layer.precision.as_ref().map_or_else(
        || image::DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f(),
        |p| (**p).clone(),
    );
    let (output, pad) = layer.adjustments.apply_expanded(&source)?;
    let world = layer.mask.as_ref().map(|_| {
        (
            crate::masks::world(layer),
            crate::masks::linked(layer),
            crate::masks::sampling(layer),
        )
    });
    let mut candidate = layer.clone();
    candidate.pixels = Arc::new(crate::precision::preview(&output));
    if candidate.precision.is_some() {
        candidate.precision = Some(Arc::new(output));
    }
    candidate.offset = candidate.offset.map(|v| v - pad);
    candidate.transform = candidate.transform.padded(pad);
    candidate.content = None;
    candidate.adjustments = Default::default();
    candidate.psd_extra = Arc::new(Vec::new());
    if pad > 0
        && let Some((world, linked, sampling)) = world
    {
        crate::masks::set_world(&mut candidate, world, linked, sampling)?;
    }
    *layer = candidate;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adjustments::Adjustments,
        document::{Document, Selection},
        precision,
    };
    use image::{Luma, Rgba, Rgba32FImage};
    fn doc() -> Document {
        let mut doc = precision::document(
            "expanded HDR".into(),
            image::DynamicImage::ImageRgba32F(Rgba32FImage::from_fn(3, 3, |x, y| {
                Rgba([
                    2. + x as f32,
                    -0.25,
                    y as f32 * 0.5,
                    if x == 0 { 0.5 } else { 1. },
                ])
            })),
        )
        .unwrap();
        doc.width = 24;
        doc.height = 20;
        doc.layers[0].offset = [10, 8];
        doc
    }
    fn approx(a: &Rgba32FImage, b: &Rgba32FImage, tol: f32) {
        assert_eq!(a.dimensions(), b.dimensions());
        for (i, (a, b)) in a.pixels().zip(b.pixels()).enumerate() {
            for c in 0..4 {
                assert!(
                    (a[c] - b[c]).abs() < tol,
                    "pixel {i} channel {c}: {} != {}",
                    a[c],
                    b[c]
                );
            }
        }
    }
    #[test]
    fn gaussian_motion_and_bloom_extend_alpha_without_hidden_color_or_source_damage() {
        let source = Rgba32FImage::from_pixel(1, 1, Rgba([4., -0.25, 1., 0.5]));
        let mut a = Adjustments {
            expand_filters: true,
            motion_length: 5,
            ..Default::default()
        };
        let (out, pad) = a.apply_expanded(&source).unwrap();
        assert_eq!(pad, 2);
        assert_eq!(out.dimensions(), (5, 5));
        for x in 0..5 {
            assert_eq!(out.get_pixel(x, 2), &Rgba([4., -0.25, 1., 0.1]));
        }
        assert!((out.pixels().map(|p| p[3]).sum::<f32>() - 0.5).abs() < 1e-6);
        a.motion_length = 0;
        a.blur = 1.;
        let (out, pad) = a.apply_expanded(&source).unwrap();
        assert_eq!(pad, 3);
        assert!(out.get_pixel(2, 3)[3] > 0.);
        assert!((out.pixels().map(|p| p[3]).sum::<f32>() - 0.5).abs() < 1e-5);
        assert!(
            out.pixels()
                .filter(|p| p[3] > 0.)
                .all(|p| (p[0] - 4.).abs() < 1e-5 && (p[1] + 0.25).abs() < 1e-6)
        );
        a.blur = 0.;
        a.bloom.amount = 1.;
        a.bloom.radius = 1.;
        a.bloom.threshold = 0.;
        let (out, pad) = a.apply_expanded(&source).unwrap();
        assert!(out.get_pixel(pad as u32 - 1, pad as u32)[3] > 0.);
        assert!(out.get_pixel(pad as u32, pad as u32)[0] > 4.);
        let hidden = Rgba32FImage::from_pixel(1, 1, Rgba([100., 100., 100., 0.]));
        let (out, _) = a.apply_expanded(&hidden).unwrap();
        assert!(out.pixels().all(|p| p.0 == [0.; 4]));
        assert_eq!(source.get_pixel(0, 0), &Rgba([4., -0.25, 1., 0.5]));
        let legacy: Adjustments = serde_json::from_str("{\"blur\":1.0}").unwrap();
        assert!(!legacy.expand_filters);
        assert_eq!(
            legacy.apply_expanded(&source).unwrap().0.dimensions(),
            (1, 1)
        );
        assert!(
            !Adjustments {
                expand_filters: true,
                ..Default::default()
            }
            .active()
        );
    }
    #[test]
    fn selection_and_point_coordinates_remain_in_original_frame() {
        let mut doc = doc();
        let original = doc.layers[0].precision.clone().unwrap();
        let layer = &mut doc.layers[0];
        layer.adjustments = Adjustments {
            expand_filters: true,
            blur: 1.,
            noise: 0.2,
            vignette: 0.3,
            ..Default::default()
        };
        let selection = Selection::rectangle([9, 7], [12, 10]);
        layer.adjustments.scope =
            Some(crate::adjustment_scope::Scope::capture(layer, &selection, [24, 20]).unwrap());
        let rendered = precision::composite(&doc);
        assert!(rendered.get_pixel(9, 8)[3] > 0.);
        for y in 0..20 {
            for x in 0..24 {
                if !selection.contains(x as i32, y as i32) {
                    let expected = if (10..13).contains(&x) && (8..11).contains(&y) {
                        original.get_pixel(x - 10, y - 8).0
                    } else {
                        [0.; 4]
                    };
                    // A fully transparent composite canonically clears hidden RGB.
                    let p = rendered.get_pixel(x, y);
                    if expected[3] > 0. {
                        for c in 0..4 {
                            assert!((p[c] - expected[c]).abs() < 1e-5);
                        }
                    } else {
                        assert_eq!(p.0, [0.; 4]);
                    }
                }
            }
        }
        let mut points = layerless_points();
        points.expand_filters = true;
        points.motion_length = 3;
        let (out, pad) = points.apply_expanded(&original).unwrap();
        points.expand_filters = false;
        points.motion_length = 0;
        let mut padded = Rgba32FImage::new(5, 5);
        let mapped = points.apply_float(&original);
        image::imageops::replace(&mut padded, &mapped, 1, 1);
        let expected = Adjustments {
            motion_length: 3,
            ..Default::default()
        }
        .apply_float(&padded);
        assert_eq!(pad, 1);
        approx(&out, &expected, 1e-6);
    }
    fn layerless_points() -> Adjustments {
        Adjustments {
            noise: 0.2,
            grain: 0.3,
            vignette: 0.4,
            exposure: 0.5,
            ..Default::default()
        }
    }
    #[test]
    fn bake_preserves_projected_geometry_masks_effects_and_single_history() {
        for linked in [true, false] {
            let mut doc = doc();
            let l = &mut doc.layers[0];
            l.transform.angle = 23.;
            l.transform.scale = [1.2, 0.8];
            l.transform.translation = [0.3, -0.7];
            l.transform.warp = Some([1., 0.1, 0., 0.05, 1., 0., 0.01, 0.005, 1.]);
            l.mask = Some(Arc::new(image::GrayImage::from_fn(3, 3, |x, y| {
                Luma([(x * 50 + y * 25 + 50) as u8])
            })));
            crate::masks::set_world(
                l,
                crate::masks::world(l),
                linked,
                crate::sampling::Sampling::Bilinear,
            )
            .unwrap();
            l.adjustments = Adjustments {
                expand_filters: true,
                blur: 1.,
                ..Default::default()
            };
            l.effects.outer_glow.enabled = true;
            l.effects.outer_glow.radius = 0.5;
            l.effects.outer_glow.offset = [0; 2];
            let source = l.precision.clone().unwrap();
            let world = crate::masks::world(l);
            let before = precision::composite(&doc);
            let mut editor = crate::document::Editor::new(doc, false);
            let id = editor.document.active;
            editor.edit("expanded bake", |doc| {
                crate::adjustment_scope::bake(doc, id).unwrap()
            });
            approx(&precision::composite(&editor.document), &before, 2e-5);
            let l = &editor.document.layers[0];
            assert!(crate::masks::valid(l));
            assert_eq!(crate::masks::linked(l), linked);
            for (a, b) in crate::masks::world(l).into_iter().zip(world) {
                assert!((a - b).abs() < 1e-5);
            }
            assert_eq!(l.pixels.dimensions(), (9, 9));
            assert!(!l.adjustments.active());
            editor.undo();
            assert!(Arc::ptr_eq(
                &source,
                editor.document.layers[0].precision.as_ref().unwrap()
            ));
            editor.redo();
            approx(&precision::composite(&editor.document), &before, 2e-5);
        }
    }
    #[test]
    fn native_and_photoshop_outputs_preserve_expanded_hdr_at_all_depths() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/filter-bounds");
        std::fs::create_dir_all(&dir).unwrap();
        for depth in [8, 16, 32] {
            let mut doc = doc();
            let l = &mut doc.layers[0];
            if depth < 32 {
                let image = if depth == 16 {
                    image::DynamicImage::ImageRgba16(precision::image16(
                        l.precision.as_ref().unwrap(),
                    ))
                } else {
                    image::DynamicImage::ImageRgba8((*l.pixels).clone())
                };
                let converted = precision::document("source".into(), image).unwrap();
                l.precision = converted.layers[0].precision.clone();
            }
            l.bit_depth = depth;
            l.adjustments = Adjustments {
                expand_filters: true,
                blur: 1.,
                motion_length: 3,
                ..Default::default()
            };
            let source = l.pixels.clone();
            let expected = precision::composite(&doc);
            let native = dir.join(format!("expanded{depth}.rphoto"));
            crate::files::save_project(&doc, &native).unwrap();
            let restored = crate::files::open(&native).unwrap().document;
            assert!(restored.layers[0].adjustments.expand_filters);
            approx(
                &precision::composite(&restored),
                &expected,
                if depth == 16 { 4e-5 } else { 1e-6 },
            );
            for extension in ["psd", "psb"] {
                let path = dir.join(format!("expanded{depth}.{extension}"));
                crate::files::save_photoshop(&doc, &path).unwrap();
                let restored = crate::files::open(&path).unwrap().document;
                let actual = precision::composite(&restored);
                let tolerance = if depth == 8 {
                    0.012
                } else if depth == 16 {
                    0.0001
                } else {
                    1e-5
                };
                for (a, b) in actual.pixels().zip(expected.pixels()) {
                    assert!((a[3] - b[3]).abs() < tolerance);
                    for c in 0..3 {
                        assert!((a[c] * a[3] - b[c] * b[3]).abs() < tolerance);
                    }
                }
                assert_eq!(restored.layers[0].bit_depth, depth);
            }
            assert!(Arc::ptr_eq(&source, &doc.layers[0].pixels));
        }
    }
    #[test]
    fn broad_gaussian_kernel_keeps_all_alpha_and_tiny_sigma_is_identity() {
        let source = Rgba32FImage::from_pixel(1, 1, Rgba([3., -0.2, 1., 0.5]));
        let a = Adjustments {
            expand_filters: true,
            blur: 16.,
            ..Default::default()
        };
        let (out, pad) = a.apply_expanded(&source).unwrap();
        assert!(pad >= 52);
        assert!((out.pixels().map(|p| p[3]).sum::<f32>() - 0.5).abs() < 2e-5);
        assert_eq!(precision::blur(&source, f32::MIN_POSITIVE / 2.), source);
    }
    #[test]
    fn lens_zoom_uses_original_center_after_padding() {
        let doc = doc();
        let source = doc.layers[0].precision.as_ref().unwrap();
        let mut a = Adjustments {
            expand_filters: true,
            blur: 1.,
            ..Default::default()
        };
        let (blurred, pad) = a.apply_expanded(source).unwrap();
        a.lens.zoom = 2.;
        let (actual, _) = a.apply_expanded(source).unwrap();
        let expected = Rgba32FImage::from_fn(actual.width(), actual.height(), |x, y| {
            let center = 1.5;
            let point = [x as f32 + 0.5 - pad as f32, y as f32 + 0.5 - pad as f32]
                .map(|v| center + (v - center) / 2. + pad as f32);
            Rgba(crate::sampling::rgba(
                point,
                crate::sampling::Sampling::Bilinear,
                |x, y| {
                    if x < 0 || y < 0 || x >= blurred.width() as i32 || y >= blurred.height() as i32
                    {
                        [0.; 4]
                    } else {
                        blurred.get_pixel(x as u32, y as u32).0
                    }
                },
            ))
        });
        approx(&actual, &expected, 1e-5);
    }
    #[test]
    fn outer_horizon_and_pixel_budget_reject_atomically() {
        let mut doc = doc();
        let l = &mut doc.layers[0];
        l.transform.warp = Some([1., 0., 0., 0., 1., 0., 0.2, 0., 1.]);
        l.adjustments = Adjustments {
            expand_filters: true,
            blur: 2.,
            ..Default::default()
        };
        assert!(l.transform.valid_for([3, 3]));
        assert!(!valid(l));
        let source = l.pixels.clone();
        let offset = l.offset;
        assert!(bake(l).is_err());
        assert!(Arc::ptr_eq(&source, &l.pixels));
        assert_eq!(l.offset, offset);
        let a = Adjustments {
            expand_filters: true,
            blur: 100.,
            ..Default::default()
        };
        let tall = Rgba32FImage::new(1, 200_000);
        assert!(a.apply_expanded(&tall).is_err());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/filter-bounds");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("invalid.rphoto");
        std::fs::write(&path, b"preserve").unwrap();
        assert!(crate::files::save_project(&doc, &path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"preserve");
    }
}
