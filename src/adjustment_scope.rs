use crate::{
    document::{Document, Layer, Selection},
    matrix::{self as m, Matrix},
};
use image::GrayImage;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// The selection snapshot uses its original document coordinates. Leaf mappings
// start at source pixels; group mappings start at current document pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub min: [i32; 2],
    pub max: [i32; 2],
    pub inverted: bool,
    pub extent: [u32; 2],
    pub mapping: Matrix,
    pub mask_size: Option<[u32; 2]>,
    #[serde(skip)]
    pub mask: Option<Arc<GrayImage>>,
}
impl Scope {
    pub fn capture(layer: &Layer, selection: &Selection, extent: [u32; 2]) -> Result<Self, String> {
        let scope = Self {
            min: selection.min,
            max: selection.max,
            inverted: selection.inverted,
            extent,
            mapping: if layer.canvas_space() {
                m::IDENTITY
            } else {
                layer
                    .transform
                    .mapping(layer.offset, [layer.pixels.width(), layer.pixels.height()])
            },
            mask_size: selection
                .mask
                .as_ref()
                .map(|mask| [mask.width(), mask.height()]),
            mask: selection.mask.clone(),
        };
        if !scope.valid() {
            return Err("보정 선택 영역이 유효하지 않습니다.".into());
        }
        Ok(scope)
    }
    pub fn valid(&self) -> bool {
        crate::document::validate_size(self.extent[0], self.extent[1]).is_ok()
            && self.min.iter().zip(self.max).all(|(a, b)| *a <= b)
            && self
                .mapping
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 1e12)
            && m::inverse(self.mapping).is_some()
            && match (&self.mask, self.mask_size) {
                (None, None) => true,
                (Some(mask), Some(size)) => {
                    [mask.width(), mask.height()] == size
                        && crate::document::validate_size(size[0], size[1]).is_ok()
                        && (0..2).all(|a| {
                            i64::from(self.max[a]) - i64::from(self.min[a]) == i64::from(size[a])
                        })
                }
                _ => false,
            }
    }
    pub fn coverage(&self, point: [f32; 2]) -> f32 {
        let point = m::point(self.mapping, point);
        if point.iter().any(|p| !p.is_finite())
            || (0..2).any(|a| point[a] < 0. || point[a] >= self.extent[a] as f32)
        {
            return 0.;
        }
        let value = if let Some(mask) = &self.mask {
            crate::sampling::gray(
                [point[0] - self.min[0] as f32, point[1] - self.min[1] as f32],
                crate::sampling::Sampling::Bilinear,
                |x, y| {
                    if x < 0 || y < 0 || x >= mask.width() as i32 || y >= mask.height() as i32 {
                        0.
                    } else {
                        mask.get_pixel(x as u32, y as u32)[0] as f32 / 255.
                    }
                },
            )
        } else {
            if (0..2).all(|a| point[a] >= self.min[a] as f32 && point[a] < self.max[a] as f32) {
                1.
            } else {
                0.
            }
        };
        if self.inverted { 1. - value } else { value }
    }
    pub fn rebase(&mut self, new_to_old: Matrix) {
        self.mapping = m::multiply(self.mapping, new_to_old);
    }
    pub fn global(&mut self, world: Matrix) -> Result<(), String> {
        let inverse = m::inverse(world).ok_or("보정 영역 변형의 역행렬이 없습니다.")?;
        let old = self.mapping;
        self.rebase(inverse);
        if !self.valid() {
            self.mapping = old;
            return Err("보정 영역의 변형이 유효하지 않습니다.".into());
        }
        Ok(())
    }
}
pub fn mix(original: [f32; 4], adjusted: [f32; 4], coverage: f32) -> [f32; 4] {
    if coverage <= 0. {
        return original;
    }
    if coverage >= 1. {
        return adjusted;
    }
    let coverage = f64::from(coverage);
    let alpha = f64::from(original[3]) * (1. - coverage) + f64::from(adjusted[3]) * coverage;
    let mut out = [0.; 4];
    out[3] = alpha as f32;
    for c in 0..3 {
        out[c] = crate::precision::float_channel(if alpha > 0. {
            (f64::from(original[c]) * f64::from(original[3]) * (1. - coverage)
                + f64::from(adjusted[c]) * f64::from(adjusted[3]) * coverage)
                / alpha
        } else {
            f64::from(original[c]) * (1. - coverage) + f64::from(adjusted[c]) * coverage
        });
    }
    out
}

pub fn needs_backdrop(document: &Document, layer: &Layer) -> bool {
    layer.group
        && layer.pass_through
        && layer.blend == crate::blend::BlendMode::Normal
        && !layer.effects.enabled()
        && document.layers.iter().any(|child| {
            child.id != layer.id
                && document.descendant_of(child.id, layer.id)
                && (child.adjustment_layer
                    || (layer.adjustments.scope.is_some()
                        && layer.adjustments.active()
                        && child.blend != crate::blend::BlendMode::Normal))
        })
}
pub fn bake(document: &mut Document, id: u64) -> Result<(), String> {
    let layer = document
        .layers
        .iter()
        .find(|l| l.id == id)
        .ok_or("보정할 레이어가 없습니다.")?;
    if document.is_locked(id)
        || (layer.group
            && document
                .layers
                .iter()
                .any(|l| document.descendant_of(l.id, id) && document.is_locked(l.id)))
    {
        return Err("잠긴 레이어·그룹에는 원본 적용을 할 수 없습니다.".into());
    }
    if layer.adjustment_layer {
        return Err("조정 레이어는 아래 레이어와 병합하거나 보정 설정으로 유지하세요.".into());
    }
    if needs_backdrop(document, layer) {
        return Err("다른 레이어와 블렌드되는 통과 그룹은 보정 설정으로 유지하거나 그룹 통과를 끈 뒤 원본에 적용하세요.".into());
    }
    if !layer.adjustments.valid() {
        return Err("보정 값이 유효하지 않습니다.".into());
    }
    if layer.group {
        let members: Vec<_> = document
            .layers
            .iter()
            .filter(|l| document.descendant_of(l.id, id))
            .map(|l| l.id)
            .collect();
        let removed: u64 = document
            .layers
            .iter()
            .filter(|l| members.contains(&l.id))
            .map(|l| l.pixels.width() as u64 * l.pixels.height() as u64)
            .sum();
        if document.pixel_count() - removed + document.width as u64 * document.height as u64
            > crate::document::MAX_PIXELS
        {
            return Err("그룹 원본 적용이 메모리 예산을 초과합니다.".into());
        }
        let mut isolated = document.clone();
        isolated.active = id;
        let group = isolated.active_layer_mut().unwrap();
        group.opacity = 1.;
        group.blend = crate::blend::BlendMode::Normal;
        group.pass_through = false;
        let pixels = crate::editing::copy_pixels(&isolated, None, false);
        let rendered = crate::precision::document("보정 그룹".into(), pixels)?
            .layers
            .remove(0);
        document
            .layers
            .retain(|l| l.id == id || !members.contains(&l.id));
        let group = document.layers.iter_mut().find(|l| l.id == id).unwrap();
        group.pixels = rendered.pixels;
        group.precision = rendered.precision;
        group.bit_depth = rendered.bit_depth;
        group.offset = [0; 2];
        group.transform = Default::default();
        group.mask = None;
        group.mask_placement = None;
        group.group = false;
        group.content = None;
        group.adjustments = Default::default();
        group.effects = Default::default();
        group.psd_extra = Arc::new(Vec::new());
    } else {
        let pad = layer.adjustments.padding();
        let old = u64::from(layer.pixels.width()) * u64::from(layer.pixels.height());
        let new =
            u64::from(layer.pixels.width() + pad * 2) * u64::from(layer.pixels.height() + pad * 2);
        if document.pixel_count() - old + new > crate::document::MAX_PIXELS {
            return Err("필터 원본 적용이 문서 메모리 예산을 초과합니다.".into());
        }
        let layer = document.layers.iter_mut().find(|l| l.id == id).unwrap();
        crate::filter_bounds::bake(layer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        adjustments::Adjustments,
        document::{Editor, PixelRegion},
        precision,
    };
    use image::{Luma, Rgba, Rgba32FImage};
    fn hdr() -> Document {
        precision::document(
            "선택 보정 HDR".into(),
            image::DynamicImage::ImageRgba32F(Rgba32FImage::from_fn(12, 8, |x, y| {
                Rgba([
                    x as f32 * 0.31 - 0.4,
                    y as f32 * 0.19,
                    0.125,
                    (x % 3 + 1) as f32 / 3.,
                ])
            })),
        )
        .unwrap()
    }
    fn scoped(doc: &mut Document, selection: Selection) {
        let scope = Scope::capture(
            doc.active_layer().unwrap(),
            &selection,
            [doc.width, doc.height],
        )
        .unwrap();
        doc.active_layer_mut().unwrap().adjustments.scope = Some(scope);
    }
    fn approx(a: &Rgba32FImage, b: &Rgba32FImage, epsilon: f32) {
        assert_eq!(a.dimensions(), b.dimensions());
        for (index, (a, b)) in a.pixels().zip(b.pixels()).enumerate() {
            for c in 0..4 {
                assert!(
                    (a[c] - b[c]).abs() <= epsilon,
                    "pixel {index} channel {c}: {} != {}",
                    a[c],
                    b[c]
                );
            }
        }
    }
    #[test]
    fn selected_adjustments_and_spatial_filters_leave_outside_originals_unchanged() {
        let mut doc = hdr();
        let source = doc.layers[0].precision.as_ref().unwrap().clone();
        scoped(&mut doc, Selection::rectangle([3, 2], [9, 6]));
        let mut variants = Vec::new();
        macro_rules! adjustment {
            ($field:ident,$value:expr) => {{
                let mut a = Adjustments::default();
                a.$field = $value;
                variants.push(a);
            }};
        }
        adjustment!(brightness, 0.2);
        adjustment!(contrast, 0.4);
        adjustment!(exposure, 1.);
        adjustment!(saturation, -0.5);
        adjustment!(hue, 35.);
        adjustment!(temperature, 0.3);
        adjustment!(tint, -0.2);
        adjustment!(gamma, 1.5);
        adjustment!(black, 0.2);
        adjustment!(white, 0.8);
        adjustment!(monochrome, true);
        adjustment!(invert, true);
        adjustment!(blur, 1.2);
        adjustment!(sharpen, 1.);
        adjustment!(grain, 0.3);
        adjustment!(noise, 0.2);
        adjustment!(vignette, 0.4);
        adjustment!(motion_length, 5);
        adjustment!(gradient_map, true);
        let mut balance = Adjustments::default();
        balance.color_balance.shifts[1][0] = 0.2;
        variants.push(balance);
        let mut bloom = Adjustments::default();
        bloom.bloom.amount = 0.5;
        variants.push(bloom);
        let mut lens = Adjustments::default();
        lens.lens.radial[0] = 0.08;
        variants.push(lens);
        let mut curves = Adjustments::default();
        curves.curves[0].points = vec![[0., 0.], [0.5, 0.7], [1., 1.]];
        variants.push(curves);
        let scope = doc.layers[0].adjustments.scope.clone();
        for mut a in variants {
            let expected = a.apply_float(&source);
            a.scope = scope.clone();
            let actual = a.apply_float(&source);
            for (x, y, p) in actual.enumerate_pixels() {
                assert_eq!(
                    *p,
                    if (3..9).contains(&x) && (2..6).contains(&y) {
                        *expected.get_pixel(x, y)
                    } else {
                        *source.get_pixel(x, y)
                    }
                );
            }
            if a.local() {
                for (x, y, p) in actual.enumerate_pixels() {
                    let sampled = a.sample(source.get_pixel(x, y).0, [x, y], [12, 8]);
                    for c in 0..4 {
                        assert!((p[c] - sampled[c]).abs() < 1e-6);
                    }
                }
            }
        }
        assert!(Arc::ptr_eq(
            &source,
            doc.layers[0].precision.as_ref().unwrap()
        ));
        assert_eq!(
            mix([2., -0.5, 1., 0.25], [8., 1.5, 3., 0.75], 0.5),
            [6.5, 1., 2.5, 0.5]
        );
    }
    #[test]
    fn inverted_feathered_selection_keeps_fractional_alpha_hdr_and_document_clip() {
        let mut doc = hdr();
        let mut selection = Selection::rectangle([2, 2], [5, 4]);
        selection.mask = Some(Arc::new(
            GrayImage::from_raw(3, 2, vec![0, 128, 255, 255, 128, 0]).unwrap(),
        ));
        selection.inverted = true;
        scoped(&mut doc, selection.clone());
        let scope = doc.layers[0].adjustments.scope.as_ref().unwrap();
        assert_eq!(scope.coverage([-0.5, 0.5]), 0.);
        assert_eq!(scope.coverage([12.5, 4.5]), 0.);
        for y in 0..8 {
            for x in 0..12 {
                assert!(
                    (scope.coverage([x as f32 + 0.5, y as f32 + 0.5]) - selection.coverage(x, y))
                        .abs()
                        < 1e-6
                );
            }
        }
        doc.layers[0].adjustments.exposure = 1.;
        let source = doc.layers[0].precision.as_ref().unwrap();
        let output = doc.layers[0].adjustments.apply_float(source);
        let old = source.get_pixel(3, 2);
        let new = output.get_pixel(3, 2);
        let weight = 127. / 255.;
        assert!((new[0] - old[0] * (1. + weight)).abs() < 1e-6);
        assert_eq!(new[3], old[3]);
        assert_eq!(output.get_pixel(4, 2), source.get_pixel(4, 2));
        assert!(output.get_pixel(10, 3)[0] > 1.);
    }
    #[test]
    fn scoped_layer_moves_flips_crops_resizes_and_rasterizes_with_original_region() {
        let mut doc = hdr();
        scoped(&mut doc, Selection::rectangle([3, 2], [8, 6]));
        doc.layers[0].adjustments.exposure = 1.;
        let id = doc.active;
        let original = precision::composite(&doc);
        let scope = doc.layers[0].adjustments.scope.clone();
        doc.move_layer(id, [2, 0]);
        assert_eq!(doc.layers[0].adjustments.scope, scope);
        let moved = precision::composite(&doc);
        for y in 0..8 {
            for x in 2..12 {
                assert_eq!(moved.get_pixel(x, y), original.get_pixel(x - 2, y));
            }
        }
        crate::editing::crop(&mut doc, &Selection::rectangle([2, 0], [12, 8])).unwrap();
        approx(
            &precision::composite(&doc),
            &image::imageops::crop_imm(&original, 0, 0, 10, 8).to_image(),
            1e-6,
        );
        crate::editing::flip_canvas(&mut doc, true);
        let flipped = precision::composite(&doc);
        approx(
            &flipped,
            &image::imageops::flip_horizontal(
                &image::imageops::crop_imm(&original, 0, 0, 10, 8).to_image(),
            ),
            1e-5,
        );
        crate::editing::rasterize(&mut doc.layers[0]).unwrap();
        approx(&precision::composite(&doc), &flipped, 1e-5);
        let saved_scope = doc.layers[0].adjustments.scope.clone().unwrap();
        let size = [doc.layers[0].pixels.width(), doc.layers[0].pixels.height()];
        crate::editing::resize(&mut doc, 20, 16, true).unwrap();
        let now = doc.layers[0].adjustments.scope.as_ref().unwrap();
        for p in [[2.5, 3.5], [5.5, 4.5], [8.5, 6.5]] {
            let q = [
                p[0] * doc.layers[0].pixels.width() as f32 / size[0] as f32,
                p[1] * doc.layers[0].pixels.height() as f32 / size[1] as f32,
            ];
            assert!((now.coverage(q) - saved_scope.coverage(p)).abs() < 1e-6);
        }
        assert_eq!(
            precision::preview(&precision::composite(&doc)),
            doc.composite()
        );
    }
    #[test]
    fn group_selection_follows_batch_transform_repeated_preview_and_source_padding() {
        let mut doc = hdr();
        let child = doc.active;
        doc.group_active().unwrap();
        let group = doc.active;
        scoped(&mut doc, Selection::rectangle([3, 2], [8, 6]));
        doc.active_layer_mut().unwrap().adjustments.exposure = 1.;
        let source = doc
            .layers
            .iter()
            .find(|l| l.id == child)
            .unwrap()
            .precision
            .clone()
            .unwrap();
        let original = precision::composite(&doc);
        let mut editor = Editor::new(doc, false);
        let frame = crate::transform_batch::Frame::new(&editor.document, vec![group]).unwrap();
        let quad = frame.mapped(m::translate(2., 0.));
        frame.apply(&mut editor, quad).unwrap();
        let scope = editor
            .document
            .active_layer()
            .unwrap()
            .adjustments
            .scope
            .clone();
        assert!(!frame.apply(&mut editor, quad).unwrap());
        assert_eq!(
            editor.document.active_layer().unwrap().adjustments.scope,
            scope
        );
        let moved = precision::composite(&editor.document);
        for y in 0..8 {
            for x in 2..12 {
                for c in 0..4 {
                    assert!(
                        (moved.get_pixel(x, y)[c] - original.get_pixel(x - 2, y)[c]).abs() < 1e-5
                    );
                }
            }
        }
        editor.cancel();
        approx(&precision::composite(&editor.document), &original, 1e-6);
        editor.document.move_layer(group, [2, 1]);
        crate::editing::crop(&mut editor.document, &Selection::rectangle([2, 1], [12, 8])).unwrap();
        approx(
            &precision::composite(&editor.document),
            &image::imageops::crop_imm(&original, 0, 0, 10, 7).to_image(),
            1e-5,
        );
        assert!(Arc::ptr_eq(
            &source,
            editor
                .document
                .layers
                .iter()
                .find(|l| l.id == child)
                .unwrap()
                .precision
                .as_ref()
                .unwrap()
        ));
        let cropped = precision::composite(&editor.document);
        crate::editing::flip_canvas(&mut editor.document, true);
        approx(
            &precision::composite(&editor.document),
            &image::imageops::flip_horizontal(&cropped),
            1e-5,
        );
        let old_scope = editor
            .document
            .active_layer()
            .unwrap()
            .adjustments
            .scope
            .clone()
            .unwrap();
        crate::editing::resize(&mut editor.document, 20, 14, true).unwrap();
        let new_scope = editor
            .document
            .active_layer()
            .unwrap()
            .adjustments
            .scope
            .as_ref()
            .unwrap();
        for point in [[2.5, 2.5], [6.5, 4.5], [9.5, 5.5]] {
            assert_eq!(
                new_scope.coverage(point.map(|p| p * 2.)),
                old_scope.coverage(point)
            );
        }
        let mut doc = hdr();
        scoped(&mut doc, Selection::rectangle([3, 2], [8, 6]));
        let old = doc.layers[0].adjustments.scope.clone().unwrap();
        crate::masks::rebase(&mut doc.layers[0], [-4., -2.]);
        let now = doc.layers[0].adjustments.scope.as_ref().unwrap();
        for p in [[3.5, 2.5], [7.5, 5.5], [9.5, 2.5]] {
            assert_eq!(now.coverage([p[0] + 4., p[1] + 2.]), old.coverage(p));
        }
    }
    #[test]
    fn bake_leaf_and_group_preserves_composite_precision_masks_and_single_undo() {
        for group in [false, true] {
            let mut doc = hdr();
            if group {
                doc.group_active().unwrap();
            }
            scoped(&mut doc, Selection::rectangle([3, 2], [8, 6]));
            let layer = doc.active_layer_mut().unwrap();
            layer.adjustments.exposure = 0.8;
            layer.opacity = 0.7;
            if !group {
                layer.mask = Some(Arc::new(GrayImage::from_pixel(12, 8, Luma([180]))));
            }
            let before = doc.clone();
            let expected = precision::composite(&doc);
            let id = doc.active;
            let mut editor = Editor::new(doc, false);
            editor.begin("원본 보정");
            bake(&mut editor.document, id).unwrap();
            editor.finish();
            approx(&precision::composite(&editor.document), &expected, 1e-5);
            assert_eq!(editor.history().count(), 1);
            let layer = editor.document.active_layer().unwrap();
            assert!(!layer.group);
            assert_eq!(layer.bit_depth, 32);
            assert_eq!(layer.adjustments, Adjustments::default());
            editor.undo();
            approx(&precision::composite(&editor.document), &expected, 1e-6);
            assert_eq!(editor.document.layers.len(), before.layers.len());
            assert_eq!(
                editor.document.active_layer().unwrap().adjustments,
                before.active_layer().unwrap().adjustments
            );
            editor.redo();
            approx(&precision::composite(&editor.document), &expected, 1e-5);
        }
        let mut doc = hdr();
        doc.layers[0].locked = true;
        let source = doc.layers[0].precision.clone().unwrap();
        let id = doc.active;
        assert!(bake(&mut doc, id).is_err());
        assert!(Arc::ptr_eq(
            &source,
            doc.layers[0].precision.as_ref().unwrap()
        ));
    }
    #[test]
    fn scoped_local_patch_matches_full_render_and_auto_levels_ignores_unselected_pixels() {
        let mut doc = hdr();
        scoped(&mut doc, Selection::rectangle([3, 2], [8, 6]));
        doc.layers[0].adjustments.exposure = 0.6;
        assert!(doc.local_preview());
        let region = PixelRegion {
            min: [2, 1],
            max: [10, 7],
        };
        let full = precision::composite(&doc);
        approx(
            &precision::composite_region(&doc, region),
            &image::imageops::crop_imm(&full, 2, 1, 8, 6).to_image(),
            1e-6,
        );
        let source = Rgba32FImage::from_fn(12, 8, |x, y| {
            let value = if (3..8).contains(&x) && (2..6).contains(&y) {
                if x % 2 == 0 { 0.6 } else { 0.3 }
            } else {
                if x % 2 == 0 { 0. } else { 1. }
            };
            Rgba([value, value, value, 1.])
        });
        let mut a = doc.layers[0].adjustments.clone();
        a.auto_levels(&source);
        assert!((a.black - 0.3).abs() < 0.005);
        assert!((a.white - 0.6).abs() < 0.005);
        a.scope = None;
        a.auto_levels(&source);
        assert_eq!((a.black, a.white), (0., 1.));
    }
    #[test]
    fn selection_snapshot_without_adjustment_does_not_isolate_pass_through_group() {
        let mut doc = Document::from_image(
            "배경".into(),
            image::RgbaImage::from_pixel(12, 8, Rgba([100, 80, 60, 255])),
        );
        doc.add_layer(
            "multiply".into(),
            image::RgbaImage::from_pixel(12, 8, Rgba([160, 180, 200, 180])),
        )
        .unwrap();
        doc.layers[1].blend = crate::blend::BlendMode::Multiply;
        doc.group_active().unwrap();
        let before = precision::composite(&doc);
        scoped(&mut doc, Selection::rectangle([2, 2], [8, 6]));
        assert!(!doc.active_layer().unwrap().adjustments.active());
        assert!(doc.local_preview());
        approx(&precision::composite(&doc), &before, 0.);
        let source = hdr().layers[0].precision.clone().unwrap();
        let a = Adjustments {
            scope: doc.active_layer().unwrap().adjustments.scope.clone(),
            ..Default::default()
        };
        assert_eq!(a.apply_float(&source), *source);
    }
    #[test]
    fn scoped_project_assets_and_native_conversion_restore_hdr_and_geometry() {
        use std::io::{Read, Write};
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/adjustment-scope");
        std::fs::create_dir_all(&root).unwrap();
        let mut doc = hdr();
        let selection = crate::editing::ellipse_smooth([2, 1], [10, 7], true);
        scoped(&mut doc, selection);
        doc.layers[0].adjustments.exposure = 0.7;
        doc.layers[0].transform.angle = 17.;
        let expected = precision::composite(&doc);
        let path = root.join("selected-hdr.rphoto");
        crate::files::save_project(&doc, &path).unwrap();
        let restored = crate::files::open(&path).unwrap().document;
        assert_eq!(restored.layers[0].adjustments, doc.layers[0].adjustments);
        approx(&precision::composite(&restored), &expected, 1e-6);
        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let mut json = String::new();
        archive
            .by_name("manifest.json")
            .unwrap()
            .read_to_string(&mut json)
            .unwrap();
        let manifest: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest["version"], 8);
        assert!(archive.by_name("layers/1-adjustment-selection.png").is_ok());
        assert!(json.len() < 8192);
        for extension in ["psd", "psb"] {
            let target = root.join(format!("selected-hdr.{extension}"));
            crate::files::save_photoshop(&doc, &target).unwrap();
            let loaded = crate::files::open(&target).unwrap().document;
            approx(&precision::composite(&loaded), &expected, 1e-4);
            assert_eq!(loaded.layers[0].bit_depth, 32);
        }
        let invalid_path = root.join("invalid-scope.rphoto");
        std::fs::copy(&path, &invalid_path).unwrap();
        let old = std::fs::read(&invalid_path).unwrap();
        let mut invalid = doc.clone();
        invalid.layers[0]
            .adjustments
            .scope
            .as_mut()
            .unwrap()
            .mapping = [0.; 9];
        assert!(crate::files::save_project(&invalid, &invalid_path).is_err());
        assert_eq!(std::fs::read(&invalid_path).unwrap(), old);
        for variant in ["missing", "shape", "legacy"] {
            let target = root.join(format!("bad-{variant}.rphoto"));
            let mut writer = zip::ZipWriter::new(std::fs::File::create(&target).unwrap());
            for index in 0..archive.len() {
                let mut asset = archive.by_index(index).unwrap();
                let name = asset.name().to_owned();
                if variant == "missing" && name.ends_with("-adjustment-selection.png") {
                    continue;
                }
                writer
                    .start_file(&name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                if name == "manifest.json" {
                    let mut manifest = manifest.clone();
                    if variant == "shape" {
                        manifest["layers"][0]["adjustments"]["scope"]["mask_size"] =
                            serde_json::json!([1, 1]);
                    }
                    if variant == "legacy" {
                        manifest["version"] = serde_json::json!(6);
                    }
                    writer
                        .write_all(&serde_json::to_vec(&manifest).unwrap())
                        .unwrap();
                } else {
                    std::io::copy(&mut asset, &mut writer).unwrap();
                }
            }
            writer.finish().unwrap();
            assert!(crate::files::open(&target).is_err(), "{variant}");
        }
    }
    #[test]
    fn pass_through_group_keeps_unselected_backdrop_and_feathers_once_with_native_conversion() {
        let mut doc = Document::from_image(
            "배경".into(),
            image::RgbaImage::from_pixel(12, 8, Rgba([100, 80, 60, 255])),
        );
        doc.add_layer(
            "multiply".into(),
            image::RgbaImage::from_pixel(12, 8, Rgba([160, 180, 200, 180])),
        )
        .unwrap();
        doc.layers[1].blend = crate::blend::BlendMode::Multiply;
        doc.group_active().unwrap();
        let group = doc.active;
        doc.active_layer_mut().unwrap().opacity = 0.65;
        doc.active_layer_mut().unwrap().mask =
            Some(Arc::new(GrayImage::from_pixel(12, 8, Luma([180]))));
        let baseline = precision::composite(&doc);
        let mut selection = Selection::rectangle([3, 1], [9, 7]);
        selection.mask = Some(Arc::new(GrayImage::from_fn(6, 6, |x, _| {
            Luma([match x % 3 {
                0 => 0,
                1 => 128,
                _ => 255,
            }])
        })));
        scoped(&mut doc, selection.clone());
        doc.active_layer_mut().unwrap().adjustments.exposure = 0.5;
        let actual = precision::composite(&doc);
        let mut whole = doc.clone();
        let layer = whole.active_layer_mut().unwrap();
        layer.pass_through = false;
        layer.adjustments.scope = None;
        let changed = precision::composite(&whole);
        for (x, y, p) in actual.enumerate_pixels() {
            let coverage = selection.coverage(x as i32, y as i32);
            for c in 0..3 {
                let expected = baseline.get_pixel(x, y)[c] * (1. - coverage)
                    + changed.get_pixel(x, y)[c] * coverage;
                assert!((p[c] - expected).abs() < 1e-6);
            }
            assert_eq!(p[3], 1.);
        }
        assert!(needs_backdrop(&doc, doc.active_layer().unwrap()));
        let before = doc.layers.len();
        assert!(bake(&mut doc, group).is_err());
        assert_eq!(doc.layers.len(), before);
        approx(&precision::composite(&doc), &actual, 0.);
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/adjustment-scope");
        std::fs::create_dir_all(&root).unwrap();
        let project = root.join("pass-through.rphoto");
        crate::files::save_project(&doc, &project).unwrap();
        approx(
            &precision::composite(&crate::files::open(&project).unwrap().document),
            &actual,
            0.,
        );
        for extension in ["psd", "psb"] {
            let path = root.join(format!("pass-through.{extension}"));
            crate::files::save_photoshop(&doc, &path).unwrap();
            let loaded = crate::files::open(&path).unwrap().document;
            assert_eq!(loaded.layers.len(), 1);
            assert_eq!(loaded.composite(), doc.composite());
        }
    }
}
