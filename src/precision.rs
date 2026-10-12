use crate::{
    blend::BlendMode,
    document::{Document, PixelRegion},
};
use image::{DynamicImage, Rgba, Rgba32FImage, RgbaImage};
use std::sync::Arc;

pub fn preview(image: &Rgba32FImage) -> RgbaImage {
    RgbaImage::from_fn(image.width(), image.height(), |x, y| {
        Rgba(
            image
                .get_pixel(x, y)
                .0
                .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8),
        )
    })
}
pub fn bit_depth(image: &DynamicImage) -> u8 {
    match image.color() {
        image::ColorType::L16
        | image::ColorType::La16
        | image::ColorType::Rgb16
        | image::ColorType::Rgba16 => 16,
        image::ColorType::Rgb32F | image::ColorType::Rgba32F => 32,
        _ => 8,
    }
}
pub fn document(name: String, image: DynamicImage) -> Result<Document, String> {
    crate::document::validate_size(image.width(), image.height())?;
    let depth = bit_depth(&image);
    let mut document = Document::from_image(name, image.to_rgba8());
    if depth > 8 {
        let pixels = image.into_rgba32f();
        if pixels.pixels().any(|p| p.0.iter().any(|c| !c.is_finite())) {
            return Err("이미지에 유효하지 않은 부동소수점 값이 있습니다.".into());
        }
        document.layers[0].precision = Some(Arc::new(pixels));
        document.layers[0].bit_depth = depth;
    }
    Ok(document)
}
pub fn over(source: [f32; 4], target: [f32; 4], opacity: f32, mode: BlendMode) -> [f32; 4] {
    if matches!(mode, BlendMode::Normal | BlendMode::Dissolve) {
        let sa = f64::from(source[3]).clamp(0., 1.) * f64::from(opacity);
        let da = f64::from(target[3]).clamp(0., 1.);
        if sa <= 0. {
            return [target[0], target[1], target[2], da as f32];
        }
        if da <= 0. || sa >= 1. {
            return [source[0], source[1], source[2], sa as f32];
        }
        let alpha = sa + da * (1. - sa);
        let color = std::array::from_fn::<_, 3, _>(|i| {
            float_channel(
                (sa * f64::from(source[i]) + (1. - sa) * da * f64::from(target[i])) / alpha,
            )
        });
        return [color[0], color[1], color[2], alpha as f32];
    }
    let sa = f64::from(source[3]).clamp(0., 1.) * f64::from(opacity);
    let da = f64::from(target[3]).clamp(0., 1.);
    if sa <= 0. {
        return [target[0], target[1], target[2], da as f32];
    }
    if da <= 0. {
        return [source[0], source[1], source[2], sa as f32];
    }
    let alpha = sa + da * (1. - sa);
    let blended = mode.color_precise(
        [source[0], source[1], source[2]].map(f64::from),
        [target[0], target[1], target[2]].map(f64::from),
    );
    let color = std::array::from_fn::<_, 3, _>(|c| {
        float_channel(
            ((sa * (1. - da) * f64::from(source[c]) + (1. - sa) * da * f64::from(target[c]))
                + sa * da * blended[c])
                / alpha,
        )
    });
    [color[0], color[1], color[2], alpha as f32]
}

pub(crate) fn over_at(
    mut source: [f32; 4],
    target: [f32; 4],
    opacity: f32,
    mode: BlendMode,
    position: [u32; 2],
    seed: u64,
) -> [f32; 4] {
    if mode != BlendMode::Dissolve {
        return over(source, target, opacity, mode);
    }
    let mut hash = seed
        ^ (position[0] as u64).wrapping_mul(0x9e3779b97f4a7c15)
        ^ (position[1] as u64).wrapping_mul(0xbf58476d1ce4e5b9);
    hash = (hash ^ (hash >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    hash = (hash ^ (hash >> 27)).wrapping_mul(0x94d049bb133111eb);
    hash ^= hash >> 31;
    let threshold = ((hash as u32) as f64 + 0.5) / 4294967296.0;
    source[3] = if threshold < (source[3].clamp(0.0, 1.0) * opacity) as f64 {
        1.0
    } else {
        0.0
    };
    over(source, target, 1.0, BlendMode::Normal)
}
pub fn composite(document: &Document) -> Rgba32FImage {
    composite_region(
        document,
        PixelRegion {
            min: [0; 2],
            max: [document.width, document.height],
        },
    )
}

pub fn composite_region(document: &Document, region: PixelRegion) -> Rgba32FImage {
    let full = PixelRegion {
        min: [0; 2],
        max: [document.width, document.height],
    };
    if region != full && !document.local_preview() {
        return image::imageops::crop_imm(
            &composite(document),
            region.min[0],
            region.min[1],
            region.size()[0],
            region.size()[1],
        )
        .to_image();
    }
    let [width, height] = region.size();
    let mut output = Rgba32FImage::new(width, height);
    composite_children(document, None, &mut output, 0, region);
    output
}

fn composite_children(
    document: &Document,
    parent: Option<u64>,
    output: &mut Rgba32FImage,
    depth: usize,
    region: PixelRegion,
) {
    if depth >= 64 {
        return;
    }
    let layers: Vec<_> = document
        .layers
        .iter()
        .filter(|l| l.parent == parent)
        .collect();
    let mut index = 0;
    while index < layers.len() {
        let layer = layers[index];
        let end = (index + 1..layers.len())
            .find(|i| !layers[*i].clipped)
            .unwrap_or(layers.len());
        if !layer.visible || layer.opacity <= 0.0 || layer.clipped {
            index = end;
            continue;
        }
        if layer.adjustment_layer {
            crate::adjustment_layer::apply(
                layer,
                output,
                region,
                [document.width, document.height],
            );
            // Clipped adjustments attached to another adjustment operate on the same stack.
            for clipped in &layers[index + 1..end] {
                if clipped.adjustment_layer {
                    crate::adjustment_layer::apply(
                        clipped,
                        output,
                        region,
                        [document.width, document.height],
                    );
                } else if clipped.visible {
                    let rendered = render_layer(document, clipped, depth, region);
                    for ((x, y, p), s) in output.enumerate_pixels_mut().zip(rendered.pixels()) {
                        let alpha = p[3];
                        let mut opaque = p.0;
                        opaque[3] = 1.;
                        p.0 = over_at(
                            s.0,
                            opaque,
                            clipped.opacity,
                            clipped.blend,
                            [x + region.min[0], y + region.min[1]],
                            clipped.id,
                        );
                        p[3] = alpha;
                    }
                }
            }
            index = end;
            continue;
        }
        if layer.group
            && layer.pass_through
            && layer.blend == BlendMode::Normal
            && !layer.effects.enabled()
            && layer.adjustments.active()
            && end == index + 1
            && let Some(scope) = &layer.adjustments.scope
        {
            let before = output.clone();
            composite_children(document, Some(layer.id), output, depth + 1, region);
            let mask = layer
                .mask_enabled
                .then(|| crate::masks::document(layer))
                .flatten();
            let mut unrestricted = layer.clone();
            unrestricted.adjustments.scope = None;
            let adjusted = render_layer(document, &unrestricted, depth, region);
            for ((x, y, p), source) in output.enumerate_pixels_mut().zip(adjusted.pixels()) {
                let point = [
                    (x + region.min[0]) as f32 + 0.5,
                    (y + region.min[1]) as f32 + 0.5,
                ];
                let coverage = layer.opacity * mask.as_ref().map_or(1., |mask| mask.value(point));
                let old = before.get_pixel(x, y).0;
                let unadjusted = crate::adjustment_scope::mix(old, p.0, coverage);
                let changed = over_at(
                    source.0,
                    old,
                    layer.opacity,
                    layer.blend,
                    [x + region.min[0], y + region.min[1]],
                    layer.id,
                );
                p.0 = crate::adjustment_scope::mix(unadjusted, changed, scope.coverage(point));
            }
            index = end;
            continue;
        }
        if layer.group
            && layer.pass_through
            && layer.blend == BlendMode::Normal
            && !layer.effects.enabled()
            && !layer.adjustments.active()
            && end == index + 1
        {
            let before = output.clone();
            composite_children(document, Some(layer.id), output, depth + 1, region);
            let mask = layer
                .mask_enabled
                .then(|| crate::masks::document(layer))
                .flatten();
            for (x, y, p) in output.enumerate_pixels_mut() {
                let coverage = layer.opacity
                    * mask.as_ref().map_or(1., |mask| {
                        mask.value([
                            (x + region.min[0]) as f32 + 0.5,
                            (y + region.min[1]) as f32 + 0.5,
                        ])
                    });
                let old = before.get_pixel(x, y).0;
                let new = p.0;
                let mixed = crate::adjustment_scope::mix(old, new, coverage);
                p.0 = if mixed[3] > 0. { mixed } else { [0.; 4] };
            }
        } else {
            let mut source = render_layer(document, layer, depth, region);
            for clipped in &layers[index + 1..end] {
                if !clipped.visible {
                    continue;
                }
                if clipped.adjustment_layer {
                    crate::adjustment_layer::apply(
                        clipped,
                        &mut source,
                        region,
                        [document.width, document.height],
                    );
                    continue;
                }
                let image = render_layer(document, clipped, depth, region);
                for ((x, y, p), s) in source.enumerate_pixels_mut().zip(image.pixels()) {
                    if p[3] <= 0.0 {
                        continue;
                    }
                    let alpha = p[3];
                    let mut opaque = p.0;
                    opaque[3] = 1.0;
                    p.0 = over_at(
                        s.0,
                        opaque,
                        clipped.opacity,
                        clipped.blend,
                        [x + region.min[0], y + region.min[1]],
                        clipped.id,
                    );
                    p[3] = alpha;
                }
            }
            for ((x, y, p), s) in output.enumerate_pixels_mut().zip(source.pixels()) {
                p.0 = over_at(
                    s.0,
                    p.0,
                    layer.opacity,
                    layer.blend,
                    [x + region.min[0], y + region.min[1]],
                    layer.id,
                );
            }
        }
        index = end;
    }
}
fn render_layer(
    document: &Document,
    layer: &crate::document::Layer,
    depth: usize,
    region: PixelRegion,
) -> Rgba32FImage {
    let [width, height] = region.size();
    let mut output = Rgba32FImage::new(width, height);
    let mask = layer
        .mask_enabled
        .then(|| {
            if layer.group {
                crate::masks::document(layer)
            } else {
                crate::masks::local(layer)
            }
        })
        .flatten();
    if layer.group {
        composite_children(document, Some(layer.id), &mut output, depth + 1, region);
        if layer.adjustments.active() {
            output = layer.adjustments.apply_float_at(&output, region.min);
        }
        for (x, y, p) in output.enumerate_pixels_mut() {
            p[3] *= mask.as_ref().map_or(1., |mask| {
                mask.value([
                    (x + region.min[0]) as f32 + 0.5,
                    (y + region.min[1]) as f32 + 0.5,
                ])
            });
        }
        if layer.effects.enabled() {
            let (image, pad) = crate::effects::apply(&output, &layer.effects);
            output = image::imageops::crop_imm(
                &image,
                pad as u32,
                pad as u32,
                document.width,
                document.height,
            )
            .to_image();
        }
        return output;
    }
    // Borrow unadjusted originals: a small brush patch must not clone/convert a full layer.
    if layer.adjustments.local() && !layer.effects.enabled() {
        let sample_adjustment = layer.adjustments.active();
        let size = [layer.pixels.width(), layer.pixels.height()];

        let (min, max) = layer.transform.bounds(layer.offset, size);
        let inverse = crate::matrix::inverse(layer.transform.mapping(layer.offset, size))
            .unwrap_or(crate::matrix::IDENTITY);
        for y in min[1].max(region.min[1] as i32)..max[1].min(region.max[1] as i32) {
            for x in min[0].max(region.min[0] as i32)..max[0].min(region.max[0] as i32) {
                let local = crate::matrix::point(inverse, [x as f32 + 0.5, y as f32 + 0.5]);
                let source = crate::sampling::rgba(local, layer.transform.sampling, |lx, ly| {
                    if lx < 0 || ly < 0 || lx >= size[0] as i32 || ly >= size[1] as i32 {
                        return [0.; 4];
                    }
                    let mut p = layer.precision.as_ref().map_or_else(
                        || {
                            layer
                                .pixels
                                .get_pixel(lx as u32, ly as u32)
                                .0
                                .map(|v| v as f32 / 255.)
                        },
                        |p| p.get_pixel(lx as u32, ly as u32).0,
                    );
                    if sample_adjustment {
                        p = layer.adjustments.sample(p, [lx as u32, ly as u32], size);
                    }
                    p[3] *= mask
                        .as_ref()
                        .map_or(1., |mask| mask.value([lx as f32 + 0.5, ly as f32 + 0.5]));
                    p
                });
                output
                    .get_pixel_mut(x as u32 - region.min[0], y as u32 - region.min[1])
                    .0 = source;
            }
        }
        return output;
    }
    let original = layer.precision.as_ref().map_or_else(
        || DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f(),
        |p| (**p).clone(),
    );
    let (mut pixels, filter_pad) = if crate::filter_bounds::valid(layer) {
        layer
            .adjustments
            .apply_expanded(&original)
            .unwrap_or_else(|_| (layer.adjustments.apply_float(&original), 0))
    } else {
        (layer.adjustments.apply_float(&original), 0)
    };
    if let Some(mask) = &mask {
        for (x, y, p) in pixels.enumerate_pixels_mut() {
            p[3] *= mask.value([
                x as f32 + 0.5 - filter_pad as f32,
                y as f32 + 0.5 - filter_pad as f32,
            ]);
        }
    }
    let (pixels, pad) = if layer.effects.enabled() {
        crate::effects::apply(&pixels, &layer.effects)
    } else {
        (pixels, 0)
    };
    let pad = pad + filter_pad;
    let offset = [
        layer.offset[0].saturating_sub(pad),
        layer.offset[1].saturating_sub(pad),
    ];
    let size = [pixels.width(), pixels.height()];

    let transform = layer.transform.padded(pad);
    let (min, max) = transform.bounds(offset, size);
    let inverse =
        crate::matrix::inverse(transform.mapping(offset, size)).unwrap_or(crate::matrix::IDENTITY);
    for y in min[1].max(region.min[1] as i32)..max[1].min(region.max[1] as i32) {
        for x in min[0].max(region.min[0] as i32)..max[0].min(region.max[0] as i32) {
            let local = crate::matrix::point(inverse, [x as f32 + 0.5, y as f32 + 0.5]);
            let source = crate::sampling::rgba(local, transform.sampling, |lx, ly| {
                if lx < 0 || ly < 0 || lx >= size[0] as i32 || ly >= size[1] as i32 {
                    [0.; 4]
                } else {
                    pixels.get_pixel(lx as u32, ly as u32).0
                }
            });
            output
                .get_pixel_mut(x as u32 - region.min[0], y as u32 - region.min[1])
                .0 = source;
        }
    }
    output
}
// image 0.25 uses an odd Gaussian kernel derived from sigma, which can exceed 3*sigma.
pub fn blur_padding(sigma: f32) -> u32 {
    if sigma <= 0. || !sigma.is_normal() {
        return 0;
    }
    let kernel = ((((sigma - 0.8) / 0.3 + 1.) * 2.) + 1.).max(3.) as u32;
    (kernel / 2).max((sigma * 3.).ceil() as u32)
}
#[path = "float_blur.rs"]
mod float_blur;
pub use float_blur::blur;
pub fn image16(image: &Rgba32FImage) -> image::ImageBuffer<image::Rgba<u16>, Vec<u16>> {
    image::ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
        image::Rgba(
            image
                .get_pixel(x, y)
                .0
                .map(|c| (c.clamp(0.0, 1.0) * 65535.0).round() as u16),
        )
    })
}

/// Preserve extended RGB while limiting only values beyond the finite f32 storage range.
pub(crate) fn float_channel(value: f64) -> f32 {
    value.clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32
}

#[cfg(test)]
#[path = "float_composite_tests.rs"]
mod float_composite_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Brush, Editor, Selection, paint_line};

    #[test]
    fn dissolve_uses_stable_binary_coverage_and_survives_save() {
        let mut doc = Document::from_image(
            "디졸브".into(),
            RgbaImage::from_pixel(64, 64, Rgba([255, 0, 0, 128])),
        );
        doc.layers[0].blend = BlendMode::Dissolve;
        doc.layers[0].opacity = 0.5;
        let pixels = doc.composite();
        assert_eq!(pixels, doc.composite());
        assert!(pixels.pixels().all(|p| p[3] == 0 || p[3] == 255));
        let opaque = pixels.pixels().filter(|p| p[3] == 255).count();
        assert!((900..=1150).contains(&opaque));
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/dissolve");
        std::fs::create_dir_all(&dir).unwrap();
        for extension in ["rphoto", "psd", "psb"] {
            let path = dir.join(format!("coverage.{extension}"));
            crate::files::save_document(&doc, &path).unwrap();
            let restored = crate::files::open(&path).unwrap().document;
            assert_eq!(restored.layers[0].blend, BlendMode::Dissolve);
            let mut expected = doc.clone();
            if extension != "rphoto" {
                // Photoshop's layer opacity field is an eight-bit integer.
                expected.layers[0].opacity = 128.0 / 255.0;
            }
            assert!(
                restored.composite() == expected.composite(),
                "{extension} coverage changed"
            );
        }
    }

    #[test]
    fn groups_isolate_blends_pass_through_and_clip_without_growing_base_alpha() {
        let mut doc = Document::from_image(
            "배경".into(),
            RgbaImage::from_pixel(2, 1, Rgba([100, 100, 100, 255])),
        );
        doc.add_layer(
            "위".into(),
            RgbaImage::from_pixel(2, 1, Rgba([200, 200, 200, 255])),
        )
        .unwrap();
        doc.layers[1].blend = BlendMode::Multiply;
        doc.group_active().unwrap();
        let folder = doc.active;
        assert_eq!(doc.composite().get_pixel(0, 0)[0], 78);
        doc.active_layer_mut().unwrap().pass_through = false;
        assert_eq!(doc.composite().get_pixel(0, 0)[0], 200);
        doc.active_layer_mut().unwrap().opacity = 0.5;
        assert_eq!(doc.composite().get_pixel(0, 0)[0], 150);
        doc.move_layer(folder, [1, 0]);
        assert_eq!(
            doc.layers.iter().find(|l| l.name == "위").unwrap().offset,
            [1, 0]
        );
        assert_eq!(doc.composite().get_pixel(0, 0)[0], 100);
        let child = doc.layers.iter().find(|l| l.name == "위").unwrap().id;
        doc.active_layer_mut().unwrap().locked = true;
        assert!(doc.is_locked(child));
        doc.move_layer(child, [4, 0]);
        assert_eq!(
            doc.layers.iter().find(|l| l.id == child).unwrap().offset,
            [1, 0]
        );
        doc.active_layer_mut().unwrap().locked = false;
        doc.duplicate_active().unwrap();
        assert!(doc.hierarchy_valid());
        doc.delete_active();
        assert!(doc.hierarchy_valid());
        let mut destination = Document::new(2, 1, false).unwrap();
        destination.group_active().unwrap();
        let parent = destination.active;
        let original_ids: Vec<u64> = destination.layers.iter().map(|l| l.id).collect();
        destination.import_layers(doc.clone()).unwrap();
        assert!(destination.hierarchy_valid());
        for layer in destination
            .layers
            .iter()
            .filter(|l| !original_ids.contains(&l.id))
        {
            assert!(destination.descendant_of(layer.id, parent));
        }
        let mut clipped = Document::from_image(
            "베이스".into(),
            RgbaImage::from_pixel(2, 1, Rgba([255, 0, 0, 128])),
        );
        Arc::make_mut(&mut clipped.layers[0].pixels).get_pixel_mut(1, 0)[3] = 0;
        clipped
            .add_layer(
                "클리핑".into(),
                RgbaImage::from_pixel(2, 1, Rgba([0, 0, 255, 255])),
            )
            .unwrap();
        clipped.active_layer_mut().unwrap().clipped = true;
        assert_eq!(clipped.composite().get_pixel(0, 0).0, [0, 0, 255, 128]);
        assert_eq!(clipped.composite().get_pixel(1, 0)[3], 0);
    }

    #[test]
    fn high_precision_edits_clipboard_merge_and_undo_preserve_untouched_values() {
        let source = Rgba32FImage::from_pixel(8, 8, Rgba([2.25, 0.1234567, -0.125, 1.0]));
        let doc = document("HDR".into(), DynamicImage::ImageRgba32F(source.clone())).unwrap();
        assert_eq!(composite(&doc), source);
        let mut blanks = doc.clone();
        blanks.add_blank_layer("정밀도 유지".into()).unwrap();
        assert_eq!(blanks.active_layer().unwrap().bit_depth, 32);
        assert_eq!(composite(&blanks), source);
        let mut editor = Editor::new(doc, true);
        let brush = Brush {
            radius: 1.0,
            opacity: 0.5,
            hardness: 0.9,
            color: [255, 0, 0, 255],
            erase: false,
            mask: false,
        };
        editor.edit("획", |doc| {
            paint_line(
                doc.active_layer_mut().unwrap(),
                [2.5, 2.5],
                [2.5, 2.5],
                &brush,
                Some(&Selection::rectangle([2, 2], [3, 3])),
            )
        });
        assert_eq!(
            editor.document.layers[0]
                .precision
                .as_ref()
                .unwrap()
                .get_pixel(6, 6),
            source.get_pixel(6, 6)
        );
        assert_eq!(
            editor.document.layers[0]
                .precision
                .as_ref()
                .unwrap()
                .get_pixel(2, 2)[0],
            1.625
        );
        editor.undo();
        assert_eq!(composite(&editor.document), source);
        editor.redo();
        let copied = crate::editing::copy_pixels(
            &editor.document,
            Some(&Selection::rectangle([5, 5], [8, 8])),
            false,
        );
        assert_eq!(copied.color(), image::ColorType::Rgba32F);
        assert_eq!(copied.to_rgba32f().get_pixel(0, 0), source.get_pixel(5, 5));
        editor
            .document
            .add_layer("숨긴 원본".into(), RgbaImage::new(1, 1))
            .unwrap();
        editor.document.active_layer_mut().unwrap().visible = false;
        crate::editing::merge_visible(&mut editor.document).unwrap();
        assert_eq!(editor.document.layers.len(), 2);
        assert_eq!(editor.document.layers[0].name, "숨긴 원본");
        assert!(!editor.document.layers[0].visible);
        assert_eq!(editor.document.layers[1].bit_depth, 32);
        assert_eq!(
            editor.document.layers[1]
                .precision
                .as_ref()
                .unwrap()
                .get_pixel(6, 6),
            source.get_pixel(6, 6)
        );
    }

    #[test]
    fn mask_strokes_leave_original_pixels_and_hdr_buffer_untouched() {
        let mut doc = document(
            "원본".into(),
            DynamicImage::ImageRgba32F(Rgba32FImage::from_pixel(4, 4, Rgba([3.0, 0.2, 0.1, 1.0]))),
        )
        .unwrap();
        let original = doc.layers[0].pixels.clone();
        let high = doc.layers[0].precision.clone().unwrap();
        let layer = doc.active_layer_mut().unwrap();
        layer.mask = Some(Arc::new(image::GrayImage::from_pixel(
            4,
            4,
            image::Luma([255]),
        )));
        paint_line(
            layer,
            [1.5, 1.5],
            [1.5, 1.5],
            &Brush {
                radius: 1.0,
                opacity: 1.0,
                hardness: 0.9,
                color: [0, 0, 0, 255],
                erase: false,
                mask: true,
            },
            None,
        );
        assert!(Arc::ptr_eq(&original, &layer.pixels));
        assert!(Arc::ptr_eq(&high, layer.precision.as_ref().unwrap()));
        assert_eq!(composite(&doc).get_pixel(1, 1)[3], 0.0);
        assert_eq!(composite(&doc).get_pixel(3, 3)[0], 3.0);
    }
}
