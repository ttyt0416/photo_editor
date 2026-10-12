use crate::document::{Document, Layer, PixelRegion};
use image::{Rgba32FImage, RgbaImage};

// An adjustment owns settings and a mask, not a duplicate of the image below it.
pub fn add(document: &mut Document) -> Result<(), String> {
    let parent = document
        .active_layer()
        .and_then(|l| if l.group { Some(l.id) } else { l.parent });
    if parent.is_some_and(|id| document.is_locked(id)) {
        return Err("잠긴 그룹에는 조정 레이어를 추가할 수 없습니다.".into());
    }
    let anchor = document.active;
    let group = document.active_layer().is_some_and(|l| l.group);
    let depth = document
        .layers
        .iter()
        .map(|l| l.bit_depth)
        .max()
        .unwrap_or(8);
    document.add_layer(format!("조정 {}", document.next_id), RgbaImage::new(1, 1))?;
    let mut layer = document.layers.pop().unwrap();
    layer.adjustment_layer = true;
    layer.bit_depth = depth;
    // A one-pixel sentinel keeps the existing file asset layout small.
    if depth > 8 {
        layer.precision = Some(std::sync::Arc::new(Rgba32FImage::new(1, 1)));
    }
    let insertion = if group {
        document
            .layers
            .iter()
            .rposition(|l| document.descendant_of(l.id, anchor))
    } else {
        document.layers.iter().position(|l| l.id == anchor)
    }
    .map_or(document.layers.len(), |i| i + 1);
    document.layers.insert(insertion, layer);
    Ok(())
}
pub fn valid(layer: &Layer) -> bool {
    !layer.adjustment_layer
        || (!layer.group
            && layer.content.is_none()
            && layer.pixels.dimensions() == (1, 1)
            && layer.pixels.get_pixel(0, 0)[3] == 0
            && layer
                .precision
                .as_ref()
                .is_none_or(|p| p.get_pixel(0, 0)[3] == 0.))
}
pub fn apply(layer: &Layer, target: &mut Rgba32FImage, region: PixelRegion, extent: [u32; 2]) {
    if !layer.visible
        || layer.opacity <= 0.
        || (!layer.adjustments.active()
            && !layer.effects.enabled()
            && layer.blend == crate::blend::BlendMode::Normal)
    {
        return;
    }
    let mut changed = if layer.adjustments.local() {
        Rgba32FImage::from_fn(target.width(), target.height(), |x, y| {
            image::Rgba(layer.adjustments.sample(
                target.get_pixel(x, y).0,
                [x + region.min[0], y + region.min[1]],
                extent,
            ))
        })
    } else {
        layer.adjustments.apply_float_at(target, region.min)
    };
    if layer.effects.enabled() {
        let (pixels, pad) = crate::effects::apply(&changed, &layer.effects);
        changed = image::imageops::crop_imm(
            &pixels,
            pad as u32,
            pad as u32,
            target.width(),
            target.height(),
        )
        .to_image();
    }
    let mask = layer
        .mask_enabled
        .then(|| crate::masks::document(layer))
        .flatten();
    for ((x, y, p), adjusted) in target.enumerate_pixels_mut().zip(changed.pixels()) {
        let point = [x + region.min[0], y + region.min[1]];
        let coverage = layer.opacity
            * mask
                .as_ref()
                .map_or(1., |mask| mask.value(point.map(|v| v as f32 + 0.5)));
        let alpha = p[3];
        let mut original = p.0;
        original[3] = 1.;
        let mut source = adjusted.0;
        source[3] = 1.;
        p.0 = crate::precision::over_at(source, original, coverage, layer.blend, point, layer.id);
        p[3] = alpha;
    }
}
// Auto levels must inspect the stack entering the adjustment, not its sentinel.
pub fn input(document: &Document, id: u64) -> Rgba32FImage {
    let mut source = document.clone();
    let Some(layer) = source.layers.iter().find(|l| l.id == id) else {
        return Rgba32FImage::new(document.width, document.height);
    };
    if layer.clipped {
        let siblings: Vec<_> = document
            .layers
            .iter()
            .filter(|l| l.parent == layer.parent)
            .collect();
        let at = siblings.iter().position(|l| l.id == id).unwrap();
        if let Some(base) = siblings[..at].iter().rposition(|l| !l.clipped)
            && !siblings[base].adjustment_layer
        {
            let ids: Vec<_> = siblings[base..at].iter().map(|l| l.id).collect();
            source
                .layers
                .retain(|l| ids.iter().any(|id| document.descendant_of(l.id, *id)));
            for l in &mut source.layers {
                if ids.contains(&l.id) {
                    l.parent = None;
                }
            }
            if let Some(base) = source.layers.iter_mut().find(|l| l.id == ids[0]) {
                base.opacity = 1.;
                base.blend = crate::blend::BlendMode::Normal;
            }
            return crate::precision::composite(&source);
        }
    }
    let mut ancestors = Vec::new();
    let mut parent = layer.parent;
    while let Some(id) = parent {
        if ancestors.len() >= 64 || ancestors.contains(&id) {
            return Rgba32FImage::new(document.width, document.height);
        }
        let l = document.layers.iter().find(|l| l.id == id).unwrap();
        ancestors.push(id);
        parent = l.parent;
    }
    let isolated = ancestors
        .iter()
        .find(|id| {
            let l = document.layers.iter().find(|l| l.id == **id).unwrap();
            !l.pass_through
                || l.blend != crate::blend::BlendMode::Normal
                || l.adjustments.active()
                || l.effects.enabled()
                || {
                    let siblings: Vec<_> = document
                        .layers
                        .iter()
                        .filter(|s| s.parent == l.parent)
                        .collect();
                    let at = siblings.iter().position(|s| s.id == l.id).unwrap();
                    siblings.get(at + 1).is_some_and(|s| s.clipped)
                }
        })
        .copied();
    let mut hidden = Vec::new();
    for target in std::iter::once(id).chain(ancestors.iter().copied()) {
        let l = document.layers.iter().find(|l| l.id == target).unwrap();
        let mut later = false;
        for sibling in &document.layers {
            if sibling.parent == l.parent {
                if sibling.id == target {
                    later = true;
                }
                if later && sibling.id != target {
                    hidden.push(sibling.id);
                }
            }
        }
        if Some(target) == isolated {
            break;
        }
    }
    hidden.push(id);
    source.layers.retain(|l| {
        isolated.is_none_or(|root| document.descendant_of(l.id, root))
            && !hidden.iter().any(|id| document.descendant_of(l.id, *id))
    });
    for l in &mut source.layers {
        if ancestors.contains(&l.id) {
            l.opacity = 1.;
            l.mask = None;
            l.mask_placement = None;
            l.adjustments = Default::default();
            l.effects = Default::default();
            if Some(l.id) == isolated {
                l.parent = None;
                l.blend = crate::blend::BlendMode::Normal;
                l.clipped = false;
            }
        }
    }
    crate::precision::composite(&source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        blend::BlendMode,
        document::{Editor, Selection},
        precision,
    };
    use image::{GrayImage, Luma, Rgba};
    use std::sync::Arc;
    fn hdr() -> Document {
        precision::document(
            "original".into(),
            image::DynamicImage::ImageRgba32F(Rgba32FImage::from_fn(12, 8, |x, y| {
                Rgba([
                    1.25 + x as f32 / 10.,
                    -0.15 + y as f32 / 15.,
                    0.375,
                    [0., 0.3, 0.7, 1.][(x % 4) as usize],
                ])
            })),
        )
        .unwrap()
    }
    fn approx(a: &Rgba32FImage, b: &Rgba32FImage, tolerance: f32) {
        assert_eq!(a.dimensions(), b.dimensions());
        for (a, b) in a.pixels().zip(b.pixels()) {
            for c in 0..4 {
                assert!(
                    (a[c] - b[c]).abs() <= tolerance,
                    "{} != {} at channel {}",
                    a[c],
                    b[c],
                    c
                );
            }
        }
    }
    #[test]
    fn adjustment_stack_mask_opacity_blend_visibility_keep_hdr_alpha_and_original_assets() {
        let mut doc = hdr();
        let original = doc.layers[0].precision.clone().unwrap();
        let baseline = precision::composite(&doc);
        add(&mut doc).unwrap();
        let id = doc.active;
        assert_eq!(doc.pixel_count(), 97);
        assert_eq!(doc.layers[1].pixels.dimensions(), (1, 1));
        let layer = doc.active_layer_mut().unwrap();
        layer.adjustments.exposure = 1.;
        layer.opacity = 0.5;
        layer.mask = Some(Arc::new(GrayImage::from_fn(12, 8, |x, _| {
            Luma([if x < 6 { 128 } else { 0 }])
        })));
        let mut expected = baseline.clone();
        for (x, _, p) in expected.enumerate_pixels_mut() {
            let weight = if x < 6 { 0.5 * 128. / 255. } else { 0. };
            for c in 0..3 {
                p[c] *= 1. + weight;
            }
        }
        approx(&precision::composite(&doc), &expected, 1e-6);
        doc.active_layer_mut().unwrap().visible = false;
        approx(&precision::composite(&doc), &baseline, 0.);
        doc.active_layer_mut().unwrap().visible = true;
        doc.active_layer_mut().unwrap().mask_enabled = false;
        for (p, original) in expected.pixels_mut().zip(baseline.pixels()) {
            for c in 0..3 {
                p[c] = original[c] * 1.5;
            }
        }
        approx(&precision::composite(&doc), &expected, 1e-6);
        doc.active_layer_mut().unwrap().blend = BlendMode::Multiply;
        let blended = precision::composite(&doc);
        for (p, old) in blended.pixels().zip(baseline.pixels()) {
            assert_eq!(p[3], old[3]);
        }
        assert!(Arc::ptr_eq(
            &original,
            doc.layers[0].precision.as_ref().unwrap()
        ));
        doc.active = id;
        doc.duplicate_active().unwrap();
        assert!(doc.active_layer().unwrap().adjustment_layer);
        assert_eq!(
            doc.active_layer().unwrap().adjustments,
            doc.layers[1].adjustments
        );
        // Stacking two unmasked half-opacity one-stop adjustments compounds 1.5 twice.
        doc.layers[1].blend = BlendMode::Normal;
        doc.active_layer_mut().unwrap().blend = BlendMode::Normal;
        for (p, old) in precision::composite(&doc).pixels().zip(baseline.pixels()) {
            for c in 0..3 {
                assert!((p[c] - old[c] * 2.25).abs() < 1e-6);
            }
        }
    }
    #[test]
    fn group_pass_through_isolation_clipping_and_histogram_input_follow_the_actual_stack() {
        for pass in [false, true] {
            let mut doc = Document::from_image(
                "background".into(),
                RgbaImage::from_pixel(12, 8, Rgba([70, 80, 90, 255])),
            );
            doc.add_layer(
                "child".into(),
                RgbaImage::from_pixel(12, 8, Rgba([100, 130, 160, 128])),
            )
            .unwrap();
            doc.group_active().unwrap();
            let group = doc.active;
            doc.active_layer_mut().unwrap().pass_through = pass;
            doc.active_layer_mut().unwrap().opacity = 0.75;
            add(&mut doc).unwrap();
            let id = doc.active;
            doc.active_layer_mut().unwrap().adjustments.exposure = 0.5;
            let mut reference = doc.clone();
            reference.layers.retain(|l| l.id != id);
            if pass {
                let before = precision::composite(&reference);
                let actual = precision::composite(&doc);
                // Group opacity is applied after the adjustment to the child+backdrop stack.
                let mut unattenuated = reference.clone();
                unattenuated
                    .layers
                    .iter_mut()
                    .find(|l| l.id == group)
                    .unwrap()
                    .opacity = 1.;
                let entering = precision::composite(&unattenuated);
                approx(&input(&doc, id), &entering, 1e-6);
                for ((p, old), full) in actual.pixels().zip(before.pixels()).zip(entering.pixels())
                {
                    for c in 0..3 {
                        assert!(
                            (p[c] - (old[c] + full[c] * (2f32.sqrt() - 1.) * 0.75)).abs() < 1e-6
                        );
                    }
                }
            } else {
                let child = reference
                    .layers
                    .iter_mut()
                    .find(|l| !l.group && l.id != 1)
                    .unwrap();
                child.adjustments.exposure = 0.5;
                approx(
                    &precision::composite(&doc),
                    &precision::composite(&reference),
                    1e-6,
                );
                let entering = input(&doc, id);
                assert!((entering.get_pixel(3, 3)[0] - 100. / 255.).abs() < 1e-6);
                assert_eq!(entering.get_pixel(3, 3)[3], 128. / 255.);
            }
        }
        let mut doc = Document::from_image(
            "background".into(),
            RgbaImage::from_pixel(12, 8, Rgba([80, 70, 60, 255])),
        );
        doc.add_layer(
            "base".into(),
            RgbaImage::from_pixel(12, 8, Rgba([100, 120, 140, 100])),
        )
        .unwrap();
        let base = doc.active;
        doc.active_layer_mut().unwrap().opacity = 0.6;
        doc.active_layer_mut().unwrap().blend = BlendMode::Multiply;
        add(&mut doc).unwrap();
        let id = doc.active;
        doc.active_layer_mut().unwrap().clipped = true;
        doc.active_layer_mut().unwrap().adjustments.exposure = 0.5;
        let mut expected = doc.clone();
        expected.layers.retain(|l| l.id != id);
        expected
            .layers
            .iter_mut()
            .find(|l| l.id == base)
            .unwrap()
            .adjustments
            .exposure = 0.5;
        approx(
            &precision::composite(&doc),
            &precision::composite(&expected),
            1e-6,
        );
        let entering = input(&doc, id);
        assert!((entering.get_pixel(0, 0)[0] - 100. / 255.).abs() < 1e-6);
        assert_eq!(entering.get_pixel(0, 0)[3], 100. / 255.);
        doc.active_layer_mut().unwrap().visible = false;
        let before = precision::composite(&doc);
        doc.layers
            .iter_mut()
            .find(|l| l.id == base)
            .unwrap()
            .visible = false;
        assert_ne!(precision::composite(&doc), before);
    }
    #[test]
    fn local_noise_vignette_scope_and_spatial_filters_match_full_render_and_preserve_alpha() {
        let mut doc = hdr();
        let base = precision::composite(&doc);
        add(&mut doc).unwrap();
        let selection = Selection::rectangle([2, 1], [10, 7]);
        let scope = crate::adjustment_scope::Scope::capture(
            doc.active_layer().unwrap(),
            &selection,
            [12, 8],
        )
        .unwrap();
        let layer = doc.active_layer_mut().unwrap();
        layer.adjustments.scope = Some(scope);
        layer.adjustments.noise = 0.35;
        layer.adjustments.vignette = 0.3;
        let region = PixelRegion {
            min: [3, 2],
            max: [8, 6],
        };
        assert!(doc.local_preview());
        let full = precision::composite(&doc);
        approx(
            &precision::composite_region(&doc, region),
            &image::imageops::crop_imm(&full, 3, 2, 5, 4).to_image(),
            0.,
        );
        for (x, y, p) in full.enumerate_pixels() {
            assert_eq!(p[3], base.get_pixel(x, y)[3]);
            if !selection.contains(x as i32, y as i32) {
                assert_eq!(p, base.get_pixel(x, y));
            }
        }
        let layer = doc.active_layer_mut().unwrap();
        layer.adjustments.blur = 1.5;
        layer.adjustments.motion_length = 3;
        layer.adjustments.bloom.amount = 0.4;
        assert!(!doc.local_preview());
        let full = precision::composite(&doc);
        approx(
            &precision::composite_region(&doc, region),
            &image::imageops::crop_imm(&full, 3, 2, 5, 4).to_image(),
            0.,
        );
        for (p, old) in full.pixels().zip(base.pixels()) {
            assert_eq!(p[3], old[3]);
        }
    }
    #[test]
    fn adjustment_mask_editing_global_geometry_and_history_keep_sentinel_and_scope() {
        let mut doc = hdr();
        add(&mut doc).unwrap();
        let id = doc.active;
        let selection = Selection::rectangle([2, 2], [8, 6]);
        doc.active_layer_mut().unwrap().adjustments.scope = Some(
            crate::adjustment_scope::Scope::capture(
                doc.active_layer().unwrap(),
                &selection,
                [12, 8],
            )
            .unwrap(),
        );
        let layer = doc.active_layer_mut().unwrap();
        layer.adjustments.exposure = 0.7;
        layer.mask = Some(Arc::new(GrayImage::from_pixel(12, 8, Luma([255]))));
        let original = doc.layers[0].precision.clone().unwrap();
        let sentinel = doc.active_layer().unwrap().pixels.clone();
        let mut editor = Editor::new(doc, true);
        editor.begin("mask");
        let brush = crate::document::Brush {
            radius: 2.,
            opacity: 1.,
            hardness: 1.,
            color: [0, 0, 0, 255],
            erase: false,
            mask: true,
        };
        crate::document::paint_line(
            editor.document.active_layer_mut().unwrap(),
            [4., 4.],
            [6., 4.],
            &brush,
            None,
        );
        assert_eq!(
            editor
                .document
                .active_layer()
                .unwrap()
                .mask
                .as_ref()
                .unwrap()
                .get_pixel(4, 4)[0],
            0
        );
        editor.finish();
        let painted = precision::composite(&editor.document);
        editor.undo();
        editor.redo();
        approx(&precision::composite(&editor.document), &painted, 0.);
        let cropped = image::imageops::crop_imm(&painted, 1, 1, 10, 6).to_image();
        crate::editing::crop(&mut editor.document, &Selection::rectangle([1, 1], [11, 7])).unwrap();
        approx(&precision::composite(&editor.document), &cropped, 1e-6);
        crate::editing::flip_canvas(&mut editor.document, true);
        approx(
            &precision::composite(&editor.document),
            &image::imageops::flip_horizontal(&cropped),
            1e-6,
        );
        crate::editing::resize(&mut editor.document, 20, 12, true).unwrap();
        assert!(valid(editor.document.active_layer().unwrap()));
        assert!(crate::masks::valid(editor.document.active_layer().unwrap()));
        assert_eq!(
            editor.document.active_layer().unwrap().pixels.dimensions(),
            (1, 1)
        );
        // A raw pixel brush may not silently turn an adjustment into a painted image.
        let brush = crate::document::Brush {
            mask: false,
            ..brush
        };
        let mask = editor.document.active_layer().unwrap().mask.clone();
        crate::document::paint_line(
            editor.document.active_layer_mut().unwrap(),
            [0., 0.],
            [1., 1.],
            &brush,
            None,
        );
        assert_eq!(editor.document.active_layer().unwrap().mask, mask);
        assert_eq!(*editor.document.active_layer().unwrap().pixels, *sentinel);
        assert!(crate::adjustment_scope::bake(&mut editor.document, id).is_err());
        assert!(crate::editing::rasterize(editor.document.active_layer_mut().unwrap()).is_err());
        assert_eq!(original.dimensions(), (12, 8));
    }
    #[test]
    fn adjustment_mask_transform_preview_cancel_and_group_transform_keep_below_originals() {
        let mut doc = hdr();
        doc.group_active().unwrap();
        let group = doc.active;
        add(&mut doc).unwrap();
        let id = doc.active;
        let selection = Selection::rectangle([2, 1], [10, 7]);
        let scope = crate::adjustment_scope::Scope::capture(
            doc.active_layer().unwrap(),
            &selection,
            [12, 8],
        )
        .unwrap();
        let layer = doc.active_layer_mut().unwrap();
        layer.adjustments.scope = Some(scope);
        layer.adjustments.exposure = 0.5;
        layer.mask = Some(Arc::new(GrayImage::from_fn(12, 8, |x, _| {
            Luma([if x < 6 { 255 } else { 0 }])
        })));
        let original = doc
            .layers
            .iter()
            .find(|l| l.id == 1)
            .unwrap()
            .precision
            .clone()
            .unwrap();
        let before = precision::composite(&doc);
        let mut editor = Editor::new(doc, true);
        let frame = crate::transform_batch::Frame::new_mask(&editor.document, id).unwrap();
        let quad = frame.mapped(crate::matrix::translate(2., 0.));
        frame.apply(&mut editor, quad).unwrap();
        assert_ne!(precision::composite(&editor.document), before);
        assert!(Arc::ptr_eq(
            &original,
            editor
                .document
                .layers
                .iter()
                .find(|l| l.id == 1)
                .unwrap()
                .precision
                .as_ref()
                .unwrap()
        ));
        editor.cancel();
        approx(&precision::composite(&editor.document), &before, 0.);
        let frame = crate::transform_batch::Frame::new(&editor.document, vec![group]).unwrap();
        let quad = frame.mapped(crate::matrix::translate(2., 0.));
        frame.apply(&mut editor, quad).unwrap();
        let moved = precision::composite(&editor.document);
        for y in 0..8 {
            for x in 2..12 {
                for c in 0..4 {
                    assert!(
                        (moved.get_pixel(x, y)[c] - before.get_pixel(x - 2, y)[c]).abs() < 1e-5
                    );
                }
            }
        }
        assert!(!frame.apply(&mut editor, quad).unwrap());
        editor.cancel();
        approx(&precision::composite(&editor.document), &before, 0.);
    }
    #[test]
    fn project_v8_keeps_adjustment_type_assets_and_hdr_outputs_with_explicit_flattening() {
        use std::io::{Read, Write};
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/adjustment-layer");
        std::fs::create_dir_all(&root).unwrap();
        let mut doc = hdr();
        add(&mut doc).unwrap();
        let id = doc.active;
        let selection = crate::editing::ellipse_smooth([2, 1], [10, 7], true);
        let scope = crate::adjustment_scope::Scope::capture(
            doc.active_layer().unwrap(),
            &selection,
            [12, 8],
        )
        .unwrap();
        let layer = doc.active_layer_mut().unwrap();
        layer.name = "조정 HDR 한글".into();
        layer.adjustments.exposure = 0.7;
        layer.adjustments.scope = Some(scope);
        layer.mask = Some(Arc::new(GrayImage::from_pixel(12, 8, Luma([180]))));
        let expected = precision::composite(&doc);
        let path = root.join("selected-hdr.rphoto");
        crate::files::save_project(&doc, &path).unwrap();
        let loaded = crate::files::open(&path).unwrap().document;
        assert!(loaded.active_layer().unwrap().adjustment_layer);
        assert_eq!(
            loaded.active_layer().unwrap().adjustments,
            doc.active_layer().unwrap().adjustments
        );
        assert_eq!(
            loaded.active_layer().unwrap().mask,
            doc.active_layer().unwrap().mask
        );
        approx(&precision::composite(&loaded), &expected, 1e-6);
        for ext in ["psd", "psb"] {
            let path = root.join(format!("selected-hdr.{ext}"));
            crate::files::save_photoshop(&doc, &path).unwrap();
            let loaded = crate::files::open(&path).unwrap().document;
            assert_eq!(loaded.layers.len(), 1);
            assert_eq!(loaded.layers[0].bit_depth, 32);
            approx(&precision::composite(&loaded), &expected, 1e-6);
        }
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut json = String::new();
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut json)
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest["version"], 8);
        assert_eq!(manifest["layers"][1]["adjustment_layer"], true);
        for variant in ["legacy", "group", "content"] {
            let target = root.join(format!("bad-{variant}.rphoto"));
            let mut writer = zip::ZipWriter::new(std::fs::File::create(&target).unwrap());
            for i in 0..archive.len() {
                let mut asset = archive.by_index(i).unwrap();
                let name = asset.name().to_owned();
                writer
                    .start_file(&name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                if name == "manifest.json" {
                    let mut bad = manifest.clone();
                    if variant == "legacy" {
                        bad["version"] = 7.into();
                    }
                    if variant == "group" {
                        bad["layers"][1]["group"] = true.into();
                    }
                    if variant == "content" {
                        bad["layers"][1]["content"] =
                            serde_json::to_value(crate::vector::Content::Solid {
                                width: 1,
                                height: 1,
                                color: [0; 4],
                            })
                            .unwrap();
                    }
                    writer
                        .write_all(&serde_json::to_vec(&bad).unwrap())
                        .unwrap();
                } else {
                    std::io::copy(&mut asset, &mut writer).unwrap();
                }
            }
            writer.finish().unwrap();
            assert!(crate::files::open(&target).is_err(), "{variant}");
        }
        let old = std::fs::read(&path).unwrap();
        let mut invalid = doc.clone();
        invalid.active_layer_mut().unwrap().group = true;
        assert!(crate::files::save_project(&invalid, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), old);
        assert!(crate::svg_export::report(&doc).contains("PNG"));
        assert_eq!(doc.active, id);
    }
    #[test]
    fn adjustment_merges_keep_appearance_or_reject_backdrop_dependencies_atomically() {
        let mut doc = hdr();
        add(&mut doc).unwrap();
        let id = doc.active;
        doc.active_layer_mut().unwrap().adjustments.exposure = 0.6;
        let expected = precision::composite(&doc);
        crate::editing::merge_down(&mut doc, &[id]).unwrap();
        assert_eq!(doc.layers.len(), 1);
        approx(&precision::composite(&doc), &expected, 1e-6);
        let mut doc = Document::from_image(
            "background".into(),
            RgbaImage::from_pixel(12, 8, Rgba([80, 100, 120, 255])),
        );
        doc.add_layer(
            "child".into(),
            RgbaImage::from_pixel(12, 8, Rgba([150, 170, 190, 128])),
        )
        .unwrap();
        add(&mut doc).unwrap();
        let id = doc.active;
        doc.active_layer_mut().unwrap().adjustments.exposure = 0.4;
        let expected = precision::composite(&doc);
        let count = doc.layers.len();
        assert!(crate::editing::merge_down(&mut doc, &[id]).is_err());
        assert_eq!(doc.layers.len(), count);
        approx(&precision::composite(&doc), &expected, 0.);
        let ids = doc.layers.iter().map(|l| l.id).collect::<Vec<_>>();
        crate::editing::merge_selected(&mut doc, &ids).unwrap();
        approx(&precision::composite(&doc), &expected, 1. / 255. + 1e-6);
        let mut locked = hdr();
        locked.group_active().unwrap();
        locked.active_layer_mut().unwrap().locked = true;
        let before = locked.layers.len();
        assert!(add(&mut locked).is_err());
        assert_eq!(locked.layers.len(), before);
    }
}
