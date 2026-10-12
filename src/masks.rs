use crate::{
    document::Layer,
    matrix::{self as m, Matrix},
    sampling::Sampling,
    transform::Transform,
};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Placement {
    pub linked: bool,
    pub matrix: Matrix,
    pub sampling: Sampling,
}
pub fn source_size(layer: &Layer) -> [u32; 2] {
    if layer.canvas_space() {
        layer
            .mask
            .as_ref()
            .map_or([layer.pixels.width(), layer.pixels.height()], |mask| {
                [mask.width(), mask.height()]
            })
    } else {
        [layer.pixels.width(), layer.pixels.height()]
    }
}
pub fn source_matrix(layer: &Layer) -> Matrix {
    layer.transform.mapping(layer.offset, source_size(layer))
}
pub fn world(layer: &Layer) -> Matrix {
    match &layer.mask_placement {
        Some(p) if p.linked => m::multiply(source_matrix(layer), p.matrix),
        Some(p) => p.matrix,
        None => source_matrix(layer),
    }
}
pub fn linked(layer: &Layer) -> bool {
    layer.mask_placement.as_ref().is_none_or(|p| p.linked)
}
pub fn valid_mapping(matrix: Matrix, size: [u32; 2]) -> bool {
    if matrix.iter().any(|v| !v.is_finite() || v.abs() > 1e12) || m::inverse(matrix).is_none() {
        return false;
    }
    let points = [
        [0., 0.],
        [size[0] as f32, 0.],
        size.map(|v| v as f32),
        [0., size[1] as f32],
    ];
    let w = points.map(|p| matrix[6] * p[0] as f64 + matrix[7] * p[1] as f64 + matrix[8]);
    (w.iter().all(|v| *v > 1e-8) || w.iter().all(|v| *v < -1e-8))
        && points
            .into_iter()
            .all(|p| m::point(matrix, p).iter().all(|v| v.is_finite()))
}
pub fn valid(layer: &Layer) -> bool {
    match &layer.mask {
        Some(mask) => {
            crate::document::validate_size(mask.width(), mask.height()).is_ok()
                && (layer.canvas_space()
                    || layer.mask_placement.is_some()
                    || mask.dimensions() == layer.pixels.dimensions())
                && valid_mapping(world(layer), [mask.width(), mask.height()])
        }
        None => layer.mask_placement.is_none(),
    }
}
pub fn sampling(layer: &Layer) -> Sampling {
    layer
        .mask_placement
        .as_ref()
        .map_or(layer.transform.sampling, |p| p.sampling)
}
pub fn set_world(
    layer: &mut Layer,
    matrix: Matrix,
    linked: bool,
    sampling: Sampling,
) -> Result<(), String> {
    let mask = layer.mask.as_ref().ok_or("마스크가 없습니다.")?;
    if !valid_mapping(matrix, [mask.width(), mask.height()]) {
        return Err("마스크 영역을 가로지르는 투영은 적용할 수 없습니다.".into());
    }
    let stored = if linked {
        m::multiply(
            m::inverse(source_matrix(layer)).ok_or("레이어 역행렬이 없습니다.")?,
            matrix,
        )
    } else {
        matrix
    };
    layer.mask_placement = Some(Placement {
        linked,
        matrix: m::normalized(stored),
        sampling,
    });
    Ok(())
}
pub fn set_linked(layer: &mut Layer, value: bool) -> Result<(), String> {
    if layer.locked {
        return Err("잠긴 레이어의 마스크는 바꿀 수 없습니다.".into());
    }
    if linked(layer) == value {
        return Ok(());
    }
    set_world(layer, world(layer), value, sampling(layer))
}
pub fn geometry(layer: &Layer) -> Option<(Transform, [i32; 2], [u32; 2])> {
    let mask = layer.mask.as_ref()?;
    let size = [mask.width(), mask.height()];
    if layer.mask_placement.is_none() {
        return Some((layer.transform, layer.offset, size));
    }
    Some((
        Transform {
            sampling: sampling(layer),
            ..Default::default()
        }
        .world(world(layer), [0; 2], size)?,
        [0; 2],
        size,
    ))
}
pub struct Sampler<'a> {
    mask: &'a image::GrayImage,
    mapping: Matrix,
    sampling: Sampling,
}
impl Sampler<'_> {
    pub fn value(&self, point: [f32; 2]) -> f32 {
        let p = m::point(self.mapping, point);
        crate::sampling::gray(p, self.sampling, |x, y| {
            if x >= 0 && y >= 0 && x < self.mask.width() as i32 && y < self.mask.height() as i32 {
                self.mask.get_pixel(x as u32, y as u32)[0] as f32 / 255.
            } else {
                0.
            }
        })
    }
}
pub fn document(layer: &Layer) -> Option<Sampler<'_>> {
    Some(Sampler {
        mask: layer.mask.as_ref()?,
        mapping: m::inverse(world(layer))?,
        sampling: sampling(layer),
    })
}
pub fn local(layer: &Layer) -> Option<Sampler<'_>> {
    let (mapping, sampling) = if layer.mask_placement.is_some() {
        (
            m::multiply(m::inverse(world(layer))?, source_matrix(layer)),
            sampling(layer),
        )
    } else {
        (m::IDENTITY, Sampling::Nearest)
    };
    Some(Sampler {
        mask: layer.mask.as_ref()?,
        mapping,
        sampling,
    })
}
pub fn rebase(layer: &mut Layer, origin: [f32; 2]) {
    if !layer.canvas_space()
        && let Some(scope) = &mut layer.adjustments.scope
    {
        scope.rebase(m::translate(origin[0] as f64, origin[1] as f64));
    }
    if let Some(p) = &mut layer.mask_placement
        && p.linked
    {
        p.matrix = m::multiply(m::translate(-origin[0] as f64, -origin[1] as f64), p.matrix);
    }
}
pub fn global(layer: &mut Layer, matrix: Matrix) {
    if layer.canvas_space()
        && let Some(scope) = &mut layer.adjustments.scope
        && let Some(inverse) = m::inverse(matrix)
    {
        scope.rebase(inverse);
    }
    if let Some(p) = &mut layer.mask_placement
        && !p.linked
    {
        p.matrix = m::multiply(matrix, p.matrix);
    }
}
pub fn align_source(layer: &mut Layer) {
    if layer.mask_placement.is_none() {
        return;
    }
    if layer.canvas_space() {
        if let Some((transform, offset, _)) = geometry(layer) {
            layer.transform = transform;
            layer.offset = offset;
            layer.mask_placement = None;
        }
        return;
    }
    let Some(sampler) = local(layer) else {
        return;
    };
    let size = source_size(layer);
    let mask = image::GrayImage::from_fn(size[0], size[1], |x, y| {
        image::Luma([(sampler.value([x as f32 + 0.5, y as f32 + 0.5]) * 255.).round() as u8])
    });
    layer.mask = Some(std::sync::Arc::new(mask));
    layer.mask_placement = None;
}

// Grow the mask bitmap without changing the existing mask's document coordinates.
pub fn expand_local(layer: &mut Layer, min: [i32; 2], max: [i32; 2]) -> Result<bool, String> {
    if layer.locked {
        return Err("잠긴 마스크는 확장할 수 없습니다.".into());
    }
    let mask = layer.mask.as_ref().ok_or("마스크가 없습니다.")?;
    let old = [mask.width(), mask.height()];
    let min = min.map(|v| v.min(0));
    let max = std::array::from_fn::<_, 2, _>(|i| max[i].max(old[i] as i32));
    if min == [0, 0] && max == old.map(|v| v as i32) {
        return Ok(false);
    }
    let new = std::array::from_fn::<_, 2, _>(|i| (i64::from(max[i]) - i64::from(min[i])) as u64);
    if new.iter().any(|v| *v > u32::MAX as u64) {
        return Err("마스크 확장 크기가 너무 큽니다.".into());
    }
    let new = new.map(|v| v as u32);
    crate::document::validate_size(new[0], new[1])?;
    let new_world = m::multiply(world(layer), m::translate(min[0] as f64, min[1] as f64));
    if !valid_mapping(new_world, new) {
        return Err("마스크 확장이 원근 투영의 한계를 넘습니다.".into());
    }
    let transform = if layer.canvas_space() {
        layer
            .transform
            .rebased(layer.offset, old, new, min.map(|v| v as f32))
            .ok_or("그룹 마스크 확장이 투영 한계를 넘습니다.")?
    } else {
        layer.transform
    };
    let linked = linked(layer);
    let sampling = sampling(layer);
    let mut bytes = Vec::new();
    let len = new[0] as usize * new[1] as usize;
    bytes
        .try_reserve_exact(len)
        .map_err(|_| "마스크 확장 메모리를 확보하지 못했습니다.")?;
    bytes.resize(len, 0);
    let mut image = image::GrayImage::from_raw(new[0], new[1], bytes).unwrap();
    image::imageops::replace(&mut image, &**mask, -i64::from(min[0]), -i64::from(min[1]));
    // Calculate placement against the new group-mask source size before publishing edits.
    let size = if layer.canvas_space() {
        new
    } else {
        source_size(layer)
    };
    let base = transform.mapping(layer.offset, size);
    let matrix = if linked {
        m::multiply(
            m::inverse(base).ok_or("마스크 역행렬이 없습니다.")?,
            new_world,
        )
    } else {
        new_world
    };
    layer.mask = Some(std::sync::Arc::new(image));
    layer.transform = transform;
    layer.mask_placement = Some(Placement {
        linked,
        matrix: m::normalized(matrix),
        sampling,
    });
    Ok(true)
}
pub fn expand_document(layer: &mut Layer, min: [f32; 2], max: [f32; 2]) -> Result<bool, String> {
    if min.iter().chain(&max).any(|v| !v.is_finite()) || min[0] >= max[0] || min[1] >= max[1] {
        return Err("마스크 편집 영역이 유효하지 않습니다.".into());
    }
    let inverse = m::inverse(world(layer)).ok_or("마스크 역행렬이 없습니다.")?;
    let corners = [min, [max[0], min[1]], max, [min[0], max[1]]];
    let w = corners.map(|p| inverse[6] * p[0] as f64 + inverse[7] * p[1] as f64 + inverse[8]);
    if !(w.iter().all(|v| *v > 1e-8) || w.iter().all(|v| *v < -1e-8)) {
        return Err("원근 투영 경계를 넘어 마스크를 편집할 수 없습니다.".into());
    }
    let local = corners.map(|p| m::point(inverse, p));
    if local
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 1e9)
    {
        return Err("마스크 확장 좌표가 너무 큽니다.".into());
    }
    let min = std::array::from_fn::<_, 2, _>(|i| {
        local
            .iter()
            .map(|p| p[i])
            .fold(f32::INFINITY, f32::min)
            .floor() as i32
    });
    let max = std::array::from_fn::<_, 2, _>(|i| {
        local
            .iter()
            .map(|p| p[i])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil() as i32
    });
    let old = layer.mask.as_ref().ok_or("마스크가 없습니다.")?;
    let size = [old.width() as i32, old.height() as i32];
    if min.iter().all(|v| *v >= 0) && max.iter().zip(size).all(|(v, s)| *v <= s) {
        return Ok(false);
    }
    // A little spare space avoids copying the entire bitmap on every stroke event.
    let padded_min = min.map(|v| if v < 0 { v.div_euclid(64) * 64 } else { 0 });
    let padded_max = std::array::from_fn(|i| {
        if max[i] > size[i] {
            (max[i] + 63).div_euclid(64) * 64
        } else {
            size[i]
        }
    });
    if expand_local(layer, padded_min, padded_max).is_ok() {
        return Ok(true);
    }
    expand_local(layer, min, max)
}
pub fn expand_stroke(
    layer: &mut Layer,
    from: [f32; 2],
    to: [f32; 2],
    radius: f32,
) -> Result<(), String> {
    if !radius.is_finite() {
        return Err("브러시 크기가 유효하지 않습니다.".into());
    }
    let radius = radius.max(0.5);
    expand_document(
        layer,
        std::array::from_fn(|i| from[i].min(to[i]) - radius),
        std::array::from_fn(|i| from[i].max(to[i]) + radius),
    )?;
    Ok(())
}
pub fn paint(
    layer: &mut Layer,
    from: [f32; 2],
    to: [f32; 2],
    brush: &crate::document::Brush,
    selection: Option<&crate::document::Selection>,
) -> Result<(), String> {
    if brush.mask {
        expand_stroke(layer, from, to, brush.radius)?;
    }
    crate::document::paint_line(layer, from, to, brush, selection);
    Ok(())
}
#[allow(clippy::too_many_arguments)]
pub fn retouch(
    layer: &mut Layer,
    source: &image::Rgba32FImage,
    from: [f32; 2],
    to: [f32; 2],
    delta: [f32; 2],
    brush: &crate::document::Brush,
    selection: Option<&crate::document::Selection>,
    mode: crate::editing::Retouch,
) -> Result<(), String> {
    if brush.mask {
        expand_stroke(layer, from, to, brush.radius)?;
    }
    crate::editing::retouch_line(layer, source, from, to, delta, brush, selection, mode);
    Ok(())
}
pub fn fill(
    layer: &mut Layer,
    selection: Option<&crate::document::Selection>,
    color: [u8; 4],
    size: [u32; 2],
) -> Result<(), String> {
    expand_document(layer, [0., 0.], size.map(|v| v as f32))?;
    crate::editing::fill_mask(layer, selection, color);
    Ok(())
}
pub fn feather(layer: &mut Layer, sigma: f32) -> Result<(), String> {
    if !sigma.is_finite() || !(0.0..=100.).contains(&sigma) {
        return Err("마스크 페더 크기가 유효하지 않습니다.".into());
    }
    if sigma == 0. {
        return Ok(());
    }
    let mask = layer.mask.as_ref().ok_or("마스크가 없습니다.")?;
    let pad = (sigma * 3.).ceil() as i32;
    let max = [mask.width() as i32 + pad, mask.height() as i32 + pad];
    expand_local(layer, [-pad; 2], max)?;
    layer.mask = layer
        .mask
        .as_ref()
        .map(|m| std::sync::Arc::new(image::imageops::blur(&**m, sigma)));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{Brush, Document, Editor, Selection},
        transform_batch::Frame,
    };
    use image::{GrayImage, Luma, Rgba, RgbaImage};
    use std::sync::Arc;
    fn fixture() -> Document {
        let mut doc = Document::from_image(
            "독립 마스크".into(),
            RgbaImage::from_pixel(8, 6, Rgba([200, 100, 40, 255])),
        );
        doc.width = 24;
        doc.height = 20;
        let l = &mut doc.layers[0];
        l.offset = [4, 5];
        l.transform.sampling = Sampling::Nearest;
        l.mask = Some(Arc::new(GrayImage::from_fn(8, 6, |x, y| {
            Luma([if x < 4 && y < 5 { 255 } else { 0 }])
        })));
        doc
    }
    fn near(a: Matrix, b: Matrix) {
        for (a, b) in m::normalized(a).into_iter().zip(m::normalized(b)) {
            assert!((a - b).abs() < 0.0001, "{a} != {b}");
        }
    }
    fn folder() -> std::path::PathBuf {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/independent-mask");
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn expanded_strokes_keep_old_world_values_and_store_new_coverage_beyond_color_bitmap() {
        for linked in [false, true] {
            let mut doc = fixture();
            let l = &mut doc.layers[0];
            set_linked(l, false).unwrap();
            set_linked(l, linked).unwrap();
            let pixels = l.pixels.clone();
            let original = l.mask.clone().unwrap();
            let world_before = world(l);
            let sample_before = document(l).unwrap().value([5.5, 6.5]);
            let brush = Brush {
                radius: 1.2,
                opacity: 1.,
                hardness: 1.,
                color: [255; 4],
                erase: false,
                mask: true,
            };
            paint(l, [1.5, 4.5], [3.5, 4.5], &brush, None).unwrap();
            assert_eq!(linked, super::linked(l));
            assert!(Arc::ptr_eq(&pixels, &l.pixels));
            assert_eq!(original.get_pixel(0, 0)[0], 255);
            assert_eq!(document(l).unwrap().value([5.5, 6.5]), sample_before);
            assert_eq!(document(l).unwrap().value([1.5, 4.5]), 1.);
            assert!(l.mask.as_ref().unwrap().width() > 8);
            assert!(valid(l));
            let movement =
                crate::editing::PixelMove::new(&doc, &Selection::rectangle([4, 5], [5, 6]), true)
                    .unwrap();
            doc.layers[0] = movement.translated([-3, -1]).unwrap();
            assert_eq!(doc.composite().get_pixel(1, 4)[3], 255);
            let p = folder().join(format!("expanded-{linked}.rphoto"));
            crate::files::save_project(&doc, &p).unwrap();
            let d = crate::files::open(&p).unwrap().document;
            assert_eq!(document(&d.layers[0]).unwrap().value([1.5, 4.5]), 1.);
            assert_eq!(doc.composite(), d.composite());
            assert!(valid_mapping(world_before, [8, 6]));
        }
    }
    #[test]
    fn mask_expansion_handles_rotated_flipped_perspective_and_groups_without_moving_children() {
        for group in [false, true] {
            let mut doc = fixture();
            let child = doc.active;
            let child_pixels = doc.layers[0].pixels.clone();
            if group {
                doc.group_active().unwrap();
                let l = doc.active_layer_mut().unwrap();
                l.mask = Some(Arc::new(GrayImage::from_pixel(8, 6, Luma([128]))));
                l.offset = [4, 5];
            }
            let l = doc.active_layer_mut().unwrap();
            l.transform.angle = 27.;
            l.transform.scale = [-1.1, 1.3];
            l.transform.warp = Some([1., 0.1, 0., 0., 1., 0., 0.003, 0., 1.]);
            let old = world(l);
            set_linked(l, false).unwrap();
            set_linked(l, true).unwrap();
            let expected = m::point(old, [2.5, 3.5]);
            let value = document(l).unwrap().value(expected);
            expand_local(l, [-4, -3], [12, 10]).unwrap();
            assert!(valid(l));
            near(world(l), m::multiply(old, m::translate(-4., -3.)));
            assert!((document(l).unwrap().value(expected) - value).abs() < 0.00001);
            if group {
                let l = doc.layers.iter().find(|l| l.id == child).unwrap();
                assert_eq!(l.offset, [4, 5]);
                assert!(Arc::ptr_eq(&child_pixels, &l.pixels));
            }
        }
    }
    #[test]
    fn extended_fill_feather_clone_and_undo_preserve_color_original() {
        let mut doc = fixture();
        let pixels = doc.layers[0].pixels.clone();
        let raw = doc.layers[0].mask.clone().unwrap();
        let mut editor = Editor::new(doc.clone(), true);
        editor.begin("바깥 마스크");
        fill(
            editor.document.active_layer_mut().unwrap(),
            Some(&Selection::rectangle([1, 1], [3, 3])),
            [255; 4],
            [24, 20],
        )
        .unwrap();
        editor.finish();
        assert_eq!(
            document(editor.document.active_layer().unwrap())
                .unwrap()
                .value([1.5, 1.5]),
            1.
        );
        assert_eq!(editor.history().count(), 1);
        editor.undo();
        assert!(Arc::ptr_eq(
            &raw,
            editor.document.layers[0].mask.as_ref().unwrap()
        ));
        editor.redo();
        feather(&mut editor.document.layers[0], 2.).unwrap();
        let alpha = document(&editor.document.layers[0])
            .unwrap()
            .value([0.5, 1.5]);
        assert!(alpha > 0. && alpha < 1.);
        assert!(Arc::ptr_eq(&pixels, &editor.document.layers[0].pixels));
        let src = image::Rgba32FImage::from_pixel(24, 20, Rgba([0.25, 0.25, 0.25, 1.]));
        let brush = Brush {
            radius: 0.75,
            opacity: 1.,
            hardness: 1.,
            color: [0; 4],
            erase: false,
            mask: true,
        };
        retouch(
            &mut doc.layers[0],
            &src,
            [15.5, 16.5],
            [15.5, 16.5],
            [0., 0.],
            &brush,
            None,
            crate::editing::Retouch::Clone,
        )
        .unwrap();
        assert!((document(&doc.layers[0]).unwrap().value([15.5, 16.5]) - 0.25).abs() < 0.003);
    }
    #[test]
    fn excessive_or_horizon_crossing_extension_fails_without_mutation() {
        let mut doc = fixture();
        let l = &mut doc.layers[0];
        let raw = l.mask.clone().unwrap();
        let original = world(l);
        assert!(expand_local(l, [-100000, -100000], [100000, 100000]).is_err());
        assert!(Arc::ptr_eq(&raw, l.mask.as_ref().unwrap()));
        near(world(l), original);
        set_world(
            l,
            [1., 0., 0., 0., 1., 0., 0.1, 0., 1.],
            false,
            Sampling::Nearest,
        )
        .unwrap();
        let original = world(l);
        assert!(expand_document(l, [9., 0.], [11., 2.]).is_err());
        assert!(Arc::ptr_eq(&raw, l.mask.as_ref().unwrap()));
        near(world(l), original);
        l.locked = true;
        assert!(expand_local(l, [-1, -1], [10, 10]).is_err());
    }
    #[test]
    fn unlink_relink_freeze_geometry_and_layer_motion_preserves_originals() {
        let mut doc = fixture();
        doc.layers[0].transform.angle = 31.;
        doc.layers[0].transform.scale = [-1.2, 0.8];
        let expected = doc.composite();
        let l = &mut doc.layers[0];
        let pixels = l.pixels.clone();
        let mask = l.mask.clone().unwrap();
        let original = world(l);
        set_linked(l, false).unwrap();
        near(world(l), original);
        assert!(!linked(l));
        assert_eq!(doc.composite(), expected);
        let l = &mut doc.layers[0];
        l.offset[0] += 3;
        near(world(l), original);
        set_linked(l, true).unwrap();
        near(world(l), original);
        l.offset[0] += 2;
        near(world(l), m::multiply(m::translate(2., 0.), original));
        assert!(Arc::ptr_eq(&pixels, &l.pixels));
        assert!(Arc::ptr_eq(&mask, l.mask.as_ref().unwrap()));
        l.locked = true;
        assert!(set_linked(l, false).is_err());
    }
    #[test]
    fn mask_transform_preview_cancel_commit_undo_only_changes_geometry() {
        let doc = fixture();
        let id = doc.active;
        let original = doc.composite();
        let pixels = doc.layers[0].pixels.clone();
        let mask = doc.layers[0].mask.clone().unwrap();
        let mut e = Editor::new(doc, true);
        let f = Frame::new_mask(&e.document, id).unwrap();
        assert!(!f.legacy());
        assert_eq!(f.handles().len(), 8);
        assert!(!f.apply(&mut e, f.quad).unwrap());
        assert_eq!(e.history().count(), 0);
        let quad = f.mapped(m::translate(2., 0.));
        f.apply(&mut e, quad).unwrap();
        assert!(!linked(&e.document.layers[0]));
        assert_eq!(e.document.layers[0].offset, [4, 5]);
        assert!(Arc::ptr_eq(&pixels, &e.document.layers[0].pixels));
        assert!(Arc::ptr_eq(
            &mask,
            e.document.layers[0].mask.as_ref().unwrap()
        ));
        assert_eq!(e.document.composite().get_pixel(4, 5)[3], 0);
        assert_eq!(e.document.composite().get_pixel(9, 5)[3], 255);
        e.cancel();
        assert_eq!(e.document.composite(), original);
        assert!(e.document.layers[0].mask_placement.is_none());
        f.apply(&mut e, quad).unwrap();
        e.finish();
        let transformed = e.document.composite();
        assert_eq!(e.history().count(), 1);
        e.undo();
        assert_eq!(e.document.composite(), original);
        e.redo();
        assert_eq!(e.document.composite(), transformed);
        e.document.layers[0].locked = true;
        assert!(Frame::new_mask(&e.document, id).is_err());
        assert!(f.apply(&mut e, f.quad).is_err());
    }
    #[test]
    fn mask_brush_selection_fill_and_clone_use_own_document_coordinates() {
        let mut doc = fixture();
        let l = &mut doc.layers[0];
        let pixels = l.pixels.clone();
        let shifted = m::multiply(m::translate(3., 1.), world(l));
        set_world(l, shifted, false, Sampling::Nearest).unwrap();
        let brush = Brush {
            radius: 0.75,
            opacity: 1.,
            hardness: 1.,
            color: [0, 0, 0, 255],
            erase: false,
            mask: true,
        };
        crate::document::paint_line(l, [7.5, 6.5], [7.5, 6.5], &brush, None);
        assert_eq!(l.mask.as_ref().unwrap().get_pixel(0, 0)[0], 0);
        assert_eq!(l.mask.as_ref().unwrap().get_pixel(3, 1)[0], 255);
        let selected = crate::editing::layer_selection(&doc, true).unwrap();
        assert_eq!(selected.coverage(7, 6), 0.);
        assert_eq!(selected.coverage(8, 6), 1.);
        let l = &mut doc.layers[0];
        crate::editing::fill_mask(
            l,
            Some(&Selection::rectangle([8, 6], [9, 7])),
            [0, 0, 0, 255],
        );
        assert_eq!(l.mask.as_ref().unwrap().get_pixel(1, 0)[0], 0);
        let src = crate::editing::retouch_source(&doc, false, true);
        assert_eq!(src.get_pixel(8, 6)[0], 0.);
        assert_eq!(src.get_pixel(9, 6)[0], 1.);
        let l = &mut doc.layers[0];
        let src = image::Rgba32FImage::from_pixel(24, 20, Rgba([0.5, 0.5, 0.5, 1.]));
        crate::editing::retouch_line(
            l,
            &src,
            [9.5, 6.5],
            [9.5, 6.5],
            [0., 0.],
            &brush,
            None,
            crate::editing::Retouch::Clone,
        );
        assert_eq!(l.mask.as_ref().unwrap().get_pixel(2, 0)[0], 128);
        assert!(Arc::ptr_eq(&pixels, &l.pixels));
    }
    #[test]
    fn crop_flip_resize_and_source_padding_keep_independent_mask_bytes() {
        for linked in [false, true] {
            let mut doc = fixture();
            let l = &mut doc.layers[0];
            set_linked(l, false).unwrap();
            set_linked(l, linked).unwrap();
            let mask = l.mask.clone().unwrap();
            let before = world(l);
            crate::editing::crop(&mut doc, &Selection::rectangle([2, 3], [22, 19])).unwrap();
            near(
                world(&doc.layers[0]),
                m::multiply(m::translate(-2., -3.), before),
            );
            let before = world(&doc.layers[0]);
            crate::editing::flip_canvas(&mut doc, true);
            near(
                world(&doc.layers[0]),
                m::multiply([-1., 0., 20., 0., 1., 0., 0., 0., 1.], before),
            );
            let before = world(&doc.layers[0]);
            crate::editing::resize(&mut doc, 40, 48, true).unwrap();
            near(
                world(&doc.layers[0]),
                m::multiply([2., 0., 0., 0., 3., 0., 0., 0., 1.], before),
            );
            assert!(Arc::ptr_eq(&mask, doc.layers[0].mask.as_ref().unwrap()));
            assert!(valid(&doc.layers[0]));
            let before = world(&doc.layers[0]);
            let move_pixels =
                crate::editing::PixelMove::new(&doc, &Selection::rectangle([0, 0], [40, 48]), true)
                    .unwrap();
            let moved = move_pixels.translated([4, 2]).unwrap();
            near(world(&moved), before);
            assert!(Arc::ptr_eq(&mask, moved.mask.as_ref().unwrap()));
            doc.layers[0] = moved;
            let p = folder().join(format!("resized-{linked}.rphoto"));
            crate::files::save_project(&doc, &p).unwrap();
            let reopened = crate::files::open(&p).unwrap().document;
            assert_eq!(
                reopened.layers[0].mask_placement,
                doc.layers[0].mask_placement
            );
            assert_eq!(reopened.composite(), doc.composite());
        }
    }
    #[test]
    fn independent_group_masks_transform_without_moving_children_and_export() {
        for pass in [false, true] {
            let mut doc = fixture();
            doc.layers[0].mask = None;
            let child = doc.active;
            doc.group_active().unwrap();
            let id = doc.active;
            let g = doc.active_layer_mut().unwrap();
            g.pass_through = pass;
            g.mask = Some(Arc::new(GrayImage::from_pixel(8, 6, Luma([128]))));
            g.offset = [4, 5];
            let pixels = doc
                .layers
                .iter()
                .find(|l| l.id == child)
                .unwrap()
                .pixels
                .clone();
            let mut e = Editor::new(doc, true);
            let f = Frame::new_mask(&e.document, id).unwrap();
            f.apply(&mut e, f.mapped(m::translate(2., 0.))).unwrap();
            e.finish();
            let child_layer = e.document.layers.iter().find(|l| l.id == child).unwrap();
            assert_eq!(child_layer.offset, [4, 5]);
            assert!(Arc::ptr_eq(&pixels, &child_layer.pixels));
            let expected = e.document.composite();
            assert_eq!(expected.get_pixel(4, 5)[3], 0);
            assert_eq!(expected.get_pixel(6, 5)[3], 128);
            for ext in ["rphoto", "psd", "psb"] {
                let p = folder().join(format!("group-{pass}.{ext}"));
                if ext == "rphoto" {
                    crate::files::save_project(&e.document, &p).unwrap();
                } else {
                    crate::files::save_photoshop(&e.document, &p).unwrap();
                }
                let actual = crate::files::open(&p).unwrap().document;
                assert_eq!(actual.layers.len(), 2);
                assert_eq!(actual.composite(), expected);
            }
            let raw = e.document.active_layer().unwrap().mask.clone().unwrap();
            let before = world(e.document.active_layer().unwrap());
            crate::editing::resize(&mut e.document, 48, 40, true).unwrap();
            near(
                world(e.document.active_layer().unwrap()),
                m::multiply([2., 0., 0., 0., 2., 0., 0., 0., 1.], before),
            );
            assert!(Arc::ptr_eq(
                &raw,
                e.document.active_layer().unwrap().mask.as_ref().unwrap()
            ));
        }
    }
    #[test]
    fn native_conversion_preserves_hdr_masked_composite_without_mutating_project() {
        let mut doc = fixture();
        let l = &mut doc.layers[0];
        l.precision = Some(Arc::new(image::Rgba32FImage::from_pixel(
            8,
            6,
            Rgba([2.125, -0.2, 0.3456789, 1.]),
        )));
        l.bit_depth = 32;
        let shifted = m::multiply(m::translate(2., 0.), world(l));
        set_world(l, shifted, false, Sampling::Nearest).unwrap();
        let original = l.pixels.clone();
        let raw = l.mask.clone().unwrap();
        let expected = crate::precision::composite(&doc);
        for ext in ["psd", "psb"] {
            let p = folder().join(format!("hdr.{ext}"));
            assert!(crate::files::save_document(&doc, &p).is_err());
            crate::files::save_photoshop(&doc, &p).unwrap();
            let d = crate::files::open(&p).unwrap().document;
            assert_eq!(crate::precision::composite(&d), expected);
        }
        let p = folder().join("fallback.svg");
        assert!(crate::svg_export::report(&doc).contains("독립 마스크"));
        crate::svg_export::save(&doc, &p).unwrap();
        let d = crate::files::open(&p).unwrap().document;
        assert_eq!(d.composite(), doc.composite());
        assert!(Arc::ptr_eq(&original, &doc.layers[0].pixels));
        assert!(Arc::ptr_eq(&raw, doc.layers[0].mask.as_ref().unwrap()));
        let mut raster = doc.layers[0].clone();
        crate::editing::rasterize(&mut raster).unwrap();
        assert!(raster.mask_placement.is_none());
        let mut d = doc.clone();
        d.layers[0] = raster;
        assert_eq!(crate::precision::composite(&d), expected);
    }
    #[test]
    fn failed_rasterization_preserves_independent_mask_and_original_pixels() {
        let mut doc = fixture();
        let l = &mut doc.layers[0];
        set_linked(l, false).unwrap();
        let raw = l.mask.clone().unwrap();
        let pixels = l.pixels.clone();
        let placement = l.mask_placement.clone();
        l.transform.scale = [100., 100.];
        l.transform.warp = Some([1e9, 0., 0., 0., 1e9, 0., 0., 0., 1.]);
        assert!(crate::editing::rasterize(l).is_err());
        assert_eq!(l.mask_placement, placement);
        assert!(Arc::ptr_eq(&raw, l.mask.as_ref().unwrap()));
        assert!(Arc::ptr_eq(&pixels, &l.pixels));
    }
    #[test]
    fn invalid_mask_geometry_or_missing_assets_cannot_replace_existing_project() {
        let mut doc = fixture();
        let p = folder().join("safe.rphoto");
        crate::files::save_project(&doc, &p).unwrap();
        let original = std::fs::read(&p).unwrap();
        let l = &mut doc.layers[0];
        let old = world(l);
        assert!(
            set_world(
                l,
                [1., 0., 0., 0., 1., 0., -0.25, 0., 1.],
                false,
                Sampling::Nearest
            )
            .is_err()
        );
        near(world(l), old);
        l.mask_placement = Some(Placement {
            linked: false,
            matrix: [0.; 9],
            sampling: Sampling::Nearest,
        });
        assert!(crate::files::save_project(&doc, &p).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), original);
        doc.layers[0].mask = None;
        assert!(crate::files::save_project(&doc, &p).is_err());
        assert_eq!(std::fs::read(&p).unwrap(), original);
    }
}
