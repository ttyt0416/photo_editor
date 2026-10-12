use crate::document::{Document, Layer, Selection, validate_size};
use image::{GrayImage, Luma, Rgba, RgbaImage, imageops};
use std::sync::Arc;

pub fn ellipse(min: [i32; 2], max: [i32; 2]) -> Selection {
    crate::selection_edges::ellipse(min, max, false)
}
pub fn ellipse_smooth(min: [i32; 2], max: [i32; 2], smooth: bool) -> Selection {
    if smooth {
        crate::selection_edges::ellipse(min, max, true)
    } else {
        ellipse(min, max)
    }
}

pub fn shifted_selection(selection: &Selection, delta: [i32; 2], size: [u32; 2]) -> Selection {
    // An inverted selection is finite within the document; shifting its excluded box alone
    // would select new pixels at the opposite edge instead of moving the selected pixels.
    let mut shifted = if selection.inverted {
        Selection {
            min: [0, 0],
            max: size.map(|s| s as i32),
            mask: Some(Arc::new(GrayImage::from_fn(size[0], size[1], |x, y| {
                Luma([(selection.coverage(x as i32, y as i32) * 255.0).round() as u8])
            }))),
            inverted: false,
        }
    } else {
        selection.clone()
    };
    for (axis, d) in delta.into_iter().enumerate() {
        shifted.min[axis] = shifted.min[axis].saturating_add(d);
        shifted.max[axis] = shifted.max[axis].saturating_add(d);
    }
    shifted
}

pub struct PixelMove {
    original: Layer,
    selection: Selection,
    copy: bool,
    other_pixels: u64,
}

impl PixelMove {
    pub fn new(document: &Document, selection: &Selection, copy: bool) -> Result<Self, String> {
        let layer = document.active_layer().ok_or("픽셀 레이어를 선택하세요.")?;
        if document.is_locked(layer.id)
            || layer.canvas_space()
            || layer.content.is_some()
            || !layer.transform.valid()
        {
            return Err("잠기지 않은 픽셀 레이어에서 선택한 픽셀을 이동할 수 있습니다.".into());
        }
        if layer.transform.warp.is_some() {
            return Err("왜곡된 레이어의 선택 픽셀 이동은 레이어를 래스터화한 뒤 사용하세요. 레이어 전체 이동은 바로 사용할 수 있습니다.".into());
        }
        Ok(Self {
            original: layer.clone(),
            selection: selection.clone(),
            copy,
            other_pixels: document.pixel_count()
                - u64::from(layer.pixels.width()) * u64::from(layer.pixels.height()),
        })
    }

    pub fn translated(&self, delta: [i32; 2]) -> Result<Layer, String> {
        if delta == [0, 0] {
            return Ok(self.original.clone());
        }
        let layer = &self.original;
        let transform = layer.transform;
        let (sin, cos) = transform.angle.to_radians().sin_cos();
        let local = [
            (delta[0] as f32 * cos + delta[1] as f32 * sin) / transform.scale[0],
            (-delta[0] as f32 * sin + delta[1] as f32 * cos) / transform.scale[1],
        ]
        .map(|d| d.round() as i64);
        if local == [0, 0] {
            return Ok(layer.clone());
        }
        let old_size = [layer.pixels.width(), layer.pixels.height()];
        // Grow equally on both sides so the transform pivot stays exactly in place,
        // including rotated, flipped and scaled layers. Off-canvas pixels stay recoverable.
        let padding = local.map(|d| d.unsigned_abs());
        let new_size = std::array::from_fn::<_, 2, _>(|axis| {
            u64::from(old_size[axis]).saturating_add(padding[axis].saturating_mul(2))
        });
        if new_size.iter().any(|s| *s > i32::MAX as u64)
            || new_size[0]
                .saturating_mul(new_size[1])
                .saturating_add(self.other_pixels)
                > crate::document::MAX_PIXELS
        {
            return Err("선택 픽셀 이동이 현재 문서의 메모리 예산을 초과합니다.".into());
        }
        let padding = padding.map(|p| p as i32);
        let offset =
            std::array::from_fn::<_, 2, _>(|axis| layer.offset[axis].checked_sub(padding[axis]));
        let [Some(x), Some(y)] = offset else {
            return Err("이동할 위치가 지원하는 좌표 범위를 벗어납니다.".into());
        };
        let sample = |x: i64, y: i64, moved: bool| {
            if x < 0 || y < 0 || x >= old_size[0] as i64 || y >= old_size[1] as i64 {
                return [0.0; 4];
            }
            let mut p = layer.precision.as_ref().map_or_else(
                || {
                    layer
                        .pixels
                        .get_pixel(x as u32, y as u32)
                        .0
                        .map(|c| c as f32 / 255.0)
                },
                |high| high.get_pixel(x as u32, y as u32).0,
            );
            let point =
                transform.to_document([x as f32 + 0.5, y as f32 + 0.5], layer.offset, old_size);
            let coverage = self
                .selection
                .coverage(point[0].floor() as i32, point[1].floor() as i32);
            if moved {
                p[3] *= coverage;
            } else if !self.copy {
                p[3] *= 1.0 - coverage;
            }
            p
        };
        let high = image::Rgba32FImage::from_fn(new_size[0] as u32, new_size[1] as u32, |x, y| {
            let x = x as i64 - padding[0] as i64;
            let y = y as i64 - padding[1] as i64;
            Rgba(crate::precision::over(
                sample(x - local[0], y - local[1], true),
                sample(x, y, false),
                1.0,
                crate::blend::BlendMode::Normal,
            ))
        });
        let mut result = layer.clone();
        result.offset = [x, y];
        result.pixels = Arc::new(crate::precision::preview(&high));
        if layer.precision.is_some() {
            result.precision = Some(Arc::new(high));
        }
        crate::masks::rebase(&mut result, padding.map(|v| -(v as f32)));
        if layer.mask_placement.is_none()
            && let Some(mask) = &layer.mask
        {
            result.mask = Some(Arc::new(GrayImage::from_fn(
                new_size[0] as u32,
                new_size[1] as u32,
                |x, y| {
                    let x = x as i64 - padding[0] as i64;
                    let y = y as i64 - padding[1] as i64;
                    if x >= 0 && y >= 0 && x < mask.width() as i64 && y < mask.height() as i64 {
                        *mask.get_pixel(x as u32, y as u32)
                    } else {
                        Luma([0])
                    }
                },
            )));
        }
        Ok(result)
    }
}

pub fn refine_selection(
    selection: &Selection,
    width: u32,
    height: u32,
    feather: f32,
    expand: i32,
) -> Selection {
    let mask = image::GrayImage::from_fn(width, height, |x, y| {
        Luma([(selection.coverage(x as i32, y as i32) * 255.0).round() as u8])
    });
    let mask = if expand != 0 {
        let values: Vec<f32> = mask.as_raw().iter().map(|v| *v as f32).collect();
        let field = crate::effects::extremum(
            &values,
            width as usize,
            height as usize,
            expand.unsigned_abs() as usize,
            expand > 0,
        );
        image::GrayImage::from_raw(width, height, field.into_iter().map(|v| v as u8).collect())
            .unwrap()
    } else {
        mask
    };
    let mask = if feather > 0.0 {
        imageops::blur(&mask, feather)
    } else {
        mask
    };
    Selection {
        min: [0, 0],
        max: [width as i32, height as i32],
        mask: Some(Arc::new(mask)),
        inverted: false,
    }
}

pub fn layer_selection(document: &Document, mask: bool) -> Option<Selection> {
    let layer = document.active_layer()?;
    let (transform, offset, size) = if mask {
        crate::masks::geometry(layer)?
    } else {
        (
            layer.transform,
            layer.offset,
            [layer.pixels.width(), layer.pixels.height()],
        )
    };
    let image = image::GrayImage::from_fn(document.width, document.height, |x, y| {
        let point = transform.to_local([x as f32 + 0.5, y as f32 + 0.5], offset, size);
        if point[0] < 0.0
            || point[1] < 0.0
            || point[0] >= size[0] as f32
            || point[1] >= size[1] as f32
        {
            return Luma([0]);
        }
        let (x, y) = (point[0] as u32, point[1] as u32);
        Luma([if mask {
            layer.mask.as_ref().unwrap().get_pixel(x, y)[0]
        } else {
            layer.pixels.get_pixel(x, y)[3]
        }])
    });
    Some(Selection {
        min: [0, 0],
        max: [document.width as i32, document.height as i32],
        mask: Some(Arc::new(image)),
        inverted: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_hdr_pixels_move_copy_expand_and_undo_without_accumulating_previews() {
        let original = image::Rgba32FImage::from_fn(4, 3, |x, y| {
            Rgba([2.125 + x as f32, -0.25 + y as f32, 0.3456789, 1.0])
        });
        let mut doc = crate::precision::document(
            "HDR".into(),
            image::DynamicImage::ImageRgba32F(original.clone()),
        )
        .unwrap();
        let selection = Selection::rectangle([0, 0], [1, 1]);
        let movement = PixelMove::new(&doc, &selection, false).unwrap();
        let mut editor = crate::document::Editor::new(doc.clone(), true);
        editor.begin("선택 이동");
        editor.document.layers[0] = movement.translated([2, 1]).unwrap();
        editor.document.layers[0] = movement.translated([-2, -1]).unwrap();
        editor.finish();
        let layer = &editor.document.layers[0];
        assert_eq!(layer.offset, [-2, -1]);
        let high = layer.precision.as_ref().unwrap();
        assert_eq!(high.get_pixel(0, 0), original.get_pixel(0, 0));
        assert_eq!(high.get_pixel(2, 1)[3], 0.0);
        assert_eq!(high.get_pixel(3, 1), original.get_pixel(1, 0));
        editor.undo();
        assert_eq!(
            &**editor.document.layers[0].precision.as_ref().unwrap(),
            &original
        );
        editor.redo();
        assert_eq!(editor.document.layers[0].offset, [-2, -1]);
        let copied = PixelMove::new(&doc, &selection, true)
            .unwrap()
            .translated([-2, -1])
            .unwrap();
        assert_eq!(
            copied.precision.as_ref().unwrap().get_pixel(2, 1),
            original.get_pixel(0, 0)
        );
        assert_eq!(
            copied.precision.as_ref().unwrap().get_pixel(0, 0),
            original.get_pixel(0, 0)
        );
        assert!(movement.translated([i32::MAX, 0]).is_err());
        doc.layers[0].locked = true;
        assert!(PixelMove::new(&doc, &selection, false).is_err());
    }

    #[test]
    fn moved_feathered_pixels_keep_rotated_pivot_and_stationary_layer_mask() {
        let mut doc =
            Document::from_image("회전".into(), RgbaImage::from_pixel(4, 4, Rgba([255; 4])));
        let layer = &mut doc.layers[0];
        layer.transform.angle = 90.0;
        layer.transform.scale = [2.0, 2.0];
        layer.offset = [3, 2];
        layer.mask = Some(Arc::new(GrayImage::from_fn(4, 4, |x, y| {
            Luma([(x + y * 4) as u8])
        })));
        let selection = Selection {
            min: [0, 0],
            max: [12, 12],
            inverted: false,
            mask: Some(Arc::new(GrayImage::from_pixel(12, 12, Luma([128])))),
        };
        let before = layer.clone();
        let movement = PixelMove::new(&doc, &selection, false).unwrap();
        let after = movement.translated([4, 0]).unwrap();
        let new_size = [after.pixels.width(), after.pixels.height()];
        let padding = [(new_size[0] - 4) / 2, (new_size[1] - 4) / 2];
        for point in [[0.5, 0.5], [2.5, 1.5], [3.5, 3.5]] {
            let old_world = before.transform.to_document(point, before.offset, [4, 4]);
            let new_local = [point[0] + padding[0] as f32, point[1] + padding[1] as f32];
            let new_world = after
                .transform
                .to_document(new_local, after.offset, new_size);
            assert!((old_world[0] - new_world[0]).abs() < 1e-5);
            assert!((old_world[1] - new_world[1]).abs() < 1e-5);
        }
        assert_eq!(
            after
                .mask
                .as_ref()
                .unwrap()
                .get_pixel(padding[0] + 2, padding[1] + 1)[0],
            6
        );
        assert_eq!(after.pixels.get_pixel(padding[0], padding[1] + 3)[3], 127);
        let mut inverted = Selection::rectangle([0, 0], [1, 1]);
        inverted.inverted = true;
        let shifted = shifted_selection(&inverted, [2, 0], [4, 4]);
        assert!(!shifted.contains(0, 1));
        assert!(!shifted.contains(2, 0));
        assert!(shifted.contains(3, 0));
    }

    #[test]
    fn crop_ratio_and_centered_rectangle_keep_ratio_at_document_edges() {
        let selection =
            selection_rectangle([90.0, 30.0], [150.0, 70.0], [100, 80], Some(1.0), false);
        assert_eq!((selection.min, selection.max), ([90, 30], [100, 40]));
        let selection =
            selection_rectangle([50.0, 40.0], [110.0, 90.0], [100, 80], Some(2.0), true);
        assert_eq!((selection.min, selection.max), ([0, 15], [100, 65]));
        let selection =
            selection_rectangle([90.0, 70.0], [20.0, -20.0], [100, 80], Some(1.0), false);
        assert_eq!((selection.min, selection.max), ([20, 0], [90, 70]));
    }
    #[test]
    fn merge_down_preserves_hdr_composite_and_rejects_locked_base_without_mutation() {
        let mut doc = crate::precision::document(
            "HDR".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
                3,
                2,
                Rgba([2.125, 0.3, -0.2, 1.0]),
            )),
        )
        .unwrap();
        doc.add_layer(
            "위".into(),
            RgbaImage::from_pixel(3, 2, Rgba([0, 255, 0, 128])),
        )
        .unwrap();
        let active = doc.active;
        let before = crate::precision::composite(&doc);
        doc.layers[0].locked = true;
        assert!(merge_down(&mut doc, &[active]).is_err());
        assert_eq!(doc.layers.len(), 2);
        doc.layers[0].locked = false;
        merge_down(&mut doc, &[active]).unwrap();
        assert_eq!(doc.layers.len(), 1);
        assert_eq!(doc.layers[0].bit_depth, 32);
        assert_eq!(crate::precision::composite(&doc), before);
    }
    #[test]
    fn mask_blur_changes_only_selected_mask_area_and_keeps_hdr_original_assets() {
        let mut doc = crate::precision::document(
            "HDR".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
                16,
                8,
                Rgba([3.125, -0.25, 0.3, 1.0]),
            )),
        )
        .unwrap();
        doc.layers[0].mask = Some(Arc::new(GrayImage::from_fn(16, 8, |x, _| {
            Luma([if x < 8 { 255 } else { 0 }])
        })));
        let original_pixels = doc.layers[0].pixels.clone();
        let original_precision = doc.layers[0].precision.clone().unwrap();
        let original_mask = doc.layers[0].mask.clone().unwrap();
        let source = crate::precision::blur(&retouch_source(&doc, true, true), 3.0);
        let brush = crate::document::Brush {
            radius: 4.0,
            opacity: 1.0,
            hardness: 1.0,
            color: [0; 4],
            erase: false,
            mask: true,
        };
        let selection = Selection::rectangle([8, 0], [16, 8]);
        let mut editor = crate::document::Editor::new(doc, true);
        editor.edit("마스크 블러", |d| {
            retouch_line(
                d.active_layer_mut().unwrap(),
                &source,
                [8.5, 4.5],
                [8.5, 4.5],
                [0.0; 2],
                &brush,
                Some(&selection),
                Retouch::Blur,
            )
        });
        let layer = editor.document.active_layer().unwrap();
        assert!(Arc::ptr_eq(&layer.pixels, &original_pixels));
        assert!(Arc::ptr_eq(
            layer.precision.as_ref().unwrap(),
            &original_precision
        ));
        assert_eq!(layer.mask.as_ref().unwrap().get_pixel(7, 4)[0], 255);
        assert!(layer.mask.as_ref().unwrap().get_pixel(8, 4)[0] > 0);
        editor.undo();
        assert_eq!(
            editor.document.layers[0].mask.as_ref().unwrap(),
            &original_mask
        );
        editor.redo();
        assert!(
            editor.document.layers[0]
                .mask
                .as_ref()
                .unwrap()
                .get_pixel(8, 4)[0]
                > 0
        );
    }

    #[test]
    fn selected_groups_copy_duplicate_reorder_merge_and_undo_without_losing_hdr() {
        let mut doc = Document::new(6, 4, true).unwrap();
        doc.add_dynamic(
            "HDR".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
                6,
                4,
                Rgba([2.125, 0.34567, -0.02, 1.0]),
            )),
        )
        .unwrap();
        let hdr = doc.active;
        doc.add_layer(
            "위".into(),
            RgbaImage::from_pixel(6, 4, Rgba([0, 0, 255, 128])),
        )
        .unwrap();
        let upper = doc.active;
        let before = crate::precision::composite(&doc);
        doc.group_selected(&[hdr, upper]).unwrap();
        let group = doc.active;
        assert!(doc.hierarchy_valid());
        assert_eq!(doc.selected_roots(&[group, hdr]), vec![group]);
        let source = doc.selected_document(&[group, hdr]).unwrap();
        assert_eq!(source.layers.len(), 3);
        let mut destination = Document::new(6, 4, true).unwrap();
        destination.import_layers(source).unwrap();
        assert!(destination.hierarchy_valid());
        assert_eq!(crate::precision::composite(&destination), before);
        let copies = doc.duplicate_selected(&[group, hdr]).unwrap();
        assert_eq!(copies.len(), 1);
        assert!(doc.hierarchy_valid());
        doc.delete_selected(&copies);
        assert_eq!(crate::precision::composite(&doc), before);
        assert!(doc.reorder_selected(&[group], hdr, true).is_err());
        doc.layers.iter_mut().find(|l| l.id == hdr).unwrap().locked = true;
        assert!(!doc.movable(&[group]));
        assert!(merge_selected(&mut doc, &[group]).is_err());
        doc.layers.iter_mut().find(|l| l.id == hdr).unwrap().locked = false;
        let mut editor = crate::document::Editor::new(doc, false);
        editor.edit("그룹 병합", |doc| {
            merge_selected(doc, &[group]).unwrap()
        });
        assert_eq!(editor.document.layers.len(), 2);
        assert_eq!(editor.document.active_layer().unwrap().bit_depth, 32);
        assert_eq!(crate::precision::composite(&editor.document), before);
        editor.undo();
        assert!(editor.document.layers.iter().any(|l| l.group));
        editor.redo();
        assert_eq!(crate::precision::composite(&editor.document), before);
    }
    #[test]
    fn canvas_flip_and_trim_preserve_group_masks_hdr_assets_and_undo() {
        let source =
            image::Rgba32FImage::from_fn(3, 2, |x, y| Rgba([x as f32 + 1.125, y as f32, 0.2, 1.0]));
        let mut doc =
            crate::precision::document("원본".into(), image::DynamicImage::ImageRgba32F(source))
                .unwrap();
        resize(&mut doc, 8, 5, false).unwrap();
        doc.layers[0].offset = [2, 1];
        doc.group_active().unwrap();
        doc.active_layer_mut().unwrap().mask =
            Some(Arc::new(image::GrayImage::from_fn(8, 5, |x, _| {
                image::Luma([if x < 4 { 255 } else { 64 }])
            })));
        let before = crate::precision::composite(&doc);
        let original = doc
            .layers
            .iter()
            .find(|l| !l.group)
            .unwrap()
            .precision
            .clone()
            .unwrap();
        let mut editor = crate::document::Editor::new(doc, true);
        editor.edit("뒤집기", |doc| flip_canvas(doc, true));
        assert_eq!(
            crate::precision::composite(&editor.document),
            imageops::flip_horizontal(&before)
        );
        editor.edit("뒤집기", |doc| flip_canvas(doc, false));
        assert_eq!(
            crate::precision::composite(&editor.document),
            imageops::flip_vertical(&imageops::flip_horizontal(&before))
        );
        assert!(Arc::ptr_eq(
            &original,
            editor
                .document
                .layers
                .iter()
                .find(|l| !l.group)
                .unwrap()
                .precision
                .as_ref()
                .unwrap()
        ));
        editor.edit("가장자리 자르기", |doc| {
            trim_transparent(doc).unwrap()
        });
        assert_eq!((editor.document.width, editor.document.height), (3, 2));
        editor.undo();
        editor.undo();
        editor.undo();
        assert_eq!(crate::precision::composite(&editor.document), before);
        let mut transparent = Document::new(8, 5, true).unwrap();
        assert!(trim_transparent(&mut transparent).is_err());
        assert_eq!((transparent.width, transparent.height), (8, 5));
    }
    #[test]
    fn cloned_hdr_pixels_and_resized_vector_settings_remain_editable() {
        let source = image::Rgba32FImage::from_fn(4, 4, |x, y| {
            Rgba([x as f32 + 0.1234567, y as f32, 0.1, 1.0])
        });
        let mut doc = crate::precision::document(
            "리터칭".into(),
            image::DynamicImage::ImageRgba32F(source.clone()),
        )
        .unwrap();
        let brush = crate::document::Brush {
            radius: 0.6,
            hardness: 1.0,
            opacity: 1.0,
            color: [0; 4],
            erase: false,
            mask: false,
        };
        retouch_line(
            doc.active_layer_mut().unwrap(),
            &source,
            [1.5, 1.5],
            [1.5, 1.5],
            [1.0, 0.0],
            &brush,
            None,
            Retouch::Clone,
        );
        assert_eq!(
            doc.layers[0].precision.as_ref().unwrap().get_pixel(1, 1),
            source.get_pixel(2, 1)
        );
        let content = crate::vector::Content::Gradient {
            width: 4,
            height: 4,
            start: [0.0, 0.0],
            end: [4.0, 4.0],
            colors: [[255, 0, 0, 255], [0, 0, 255, 255]],
            stops: vec![],
            interpolation: crate::vector::GradientInterpolation::Srgb,
        };
        doc.add_layer("그라디언트".into(), content.render().unwrap())
            .unwrap();
        doc.active_layer_mut().unwrap().content = Some(content);
        resize(&mut doc, 8, 12, true).unwrap();
        let content = doc.active_layer().unwrap().content.as_ref().unwrap();
        assert_eq!(content.render().unwrap().dimensions(), (8, 12));
        assert!(matches!(
            content,
            crate::vector::Content::Gradient {
                end: [8.0, 12.0],
                ..
            }
        ));
    }
    #[test]
    fn selections_invert_feather_and_color_connectivity_protect_pixels() {
        let mut ellipse = ellipse([0, 0], [10, 10]);
        assert!(!ellipse.contains(0, 0));
        assert!(ellipse.contains(5, 5));
        ellipse.inverted = true;
        assert!(ellipse.contains(0, 0));
        assert!(!ellipse.contains(5, 5));
        let polygon = polygon(&[[0.0, 0.0], [8.0, 0.0], [0.0, 8.0]], 10, 10).unwrap();
        assert!(polygon.contains(1, 1));
        assert!(!polygon.contains(6, 6));
        let image = RgbaImage::from_fn(5, 1, |x, _| {
            Rgba(if x == 2 {
                [0, 0, 255, 255]
            } else {
                [255, 0, 0, 255]
            })
        });
        let near = color_selection(&image, [0, 0], 0, true).unwrap();
        assert!(near.contains(1, 0));
        assert!(!near.contains(4, 0));
        let all = color_selection(&image, [0, 0], 0, false).unwrap();
        assert!(all.contains(4, 0));
        let combined =
            combine_selection(Some(&all), Selection::rectangle([0, 0], [1, 1]), true, 5, 1);
        assert!(!combined.contains(0, 0));
        assert!(combined.contains(1, 0));
        let softened = refine_selection(&Selection::rectangle([3, 3], [7, 7]), 10, 10, 1.0, 0);
        assert!(softened.coverage(3, 3) > 0.0 && softened.coverage(3, 3) < 1.0);
        assert!(softened.coverage(2, 3) > 0.0);
        let expanded = refine_selection(&Selection::rectangle([3, 3], [7, 7]), 10, 10, 0.0, 1);
        assert!(expanded.contains(2, 3));
        assert!(!expanded.contains(1, 3));
    }
    #[test]
    fn crop_retains_assets_resize_and_paste_survive_undo() {
        let mut doc = Document::new(10, 8, false).unwrap();
        let original = doc.layers[0].pixels.clone();
        crop(&mut doc, &Selection::rectangle([2, 1], [7, 6])).unwrap();
        assert_eq!((doc.width, doc.height), (5, 5));
        assert!(Arc::ptr_eq(&doc.layers[0].pixels, &original));
        assert_eq!(doc.layers[0].offset, [-2, -1]);
        let copy = copy_pixels(&doc, Some(&Selection::rectangle([1, 1], [3, 3])), false);
        assert_eq!((copy.width(), copy.height()), (2, 2));
        resize(&mut doc, 10, 10, true).unwrap();
        assert_eq!(doc.layers[0].pixels.dimensions(), (20, 16));
    }
}

pub fn polygon(points: &[[f32; 2]], width: u32, height: u32) -> Option<Selection> {
    crate::selection_edges::polygon(points, width, height, false)
}
pub fn polygon_smooth(
    points: &[[f32; 2]],
    width: u32,
    height: u32,
    smooth: bool,
) -> Option<Selection> {
    if smooth {
        crate::selection_edges::polygon(points, width, height, true)
    } else {
        polygon(points, width, height)
    }
}

pub fn color_selection(
    image: &RgbaImage,
    origin: [i32; 2],
    tolerance: u8,
    contiguous: bool,
) -> Option<Selection> {
    let (w, h) = image.dimensions();
    if origin[0] < 0 || origin[1] < 0 || origin[0] >= w as i32 || origin[1] >= h as i32 {
        return None;
    }
    let source = image.get_pixel(origin[0] as u32, origin[1] as u32);
    let matches = |x: u32, y: u32| {
        image
            .get_pixel(x, y)
            .0
            .iter()
            .zip(source.0)
            .all(|(a, b)| a.abs_diff(b) <= tolerance)
    };
    let mut mask = GrayImage::new(w, h);
    if contiguous {
        let mut stack = vec![(origin[0] as u32, origin[1] as u32)];
        while let Some((x, y)) = stack.pop() {
            if mask.get_pixel(x, y)[0] != 0 {
                continue;
            }
            mask.put_pixel(x, y, Luma([1]));
            if !matches(x, y) {
                continue;
            }
            mask.put_pixel(x, y, Luma([255]));
            if x > 0 {
                stack.push((x - 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if x + 1 < w {
                stack.push((x + 1, y));
            }
            if y + 1 < h {
                stack.push((x, y + 1));
            }
        }
        for p in mask.pixels_mut() {
            if p[0] == 1 {
                p[0] = 0;
            }
        }
    } else {
        for (x, y, p) in mask.enumerate_pixels_mut() {
            p[0] = if matches(x, y) { 255 } else { 0 };
        }
    }
    Some(Selection {
        min: [0, 0],
        max: [w as i32, h as i32],
        mask: Some(Arc::new(mask)),
        inverted: false,
    })
}

pub fn combine_selection(
    previous: Option<&Selection>,
    next: Selection,
    subtract: bool,
    width: u32,
    height: u32,
) -> Selection {
    let Some(previous) = previous else {
        return next;
    };
    let mask = GrayImage::from_fn(width, height, |x, y| {
        let a = previous.coverage(x as i32, y as i32);
        let b = next.coverage(x as i32, y as i32);
        Luma([((if subtract { a * (1.0 - b) } else { a.max(b) }) * 255.0).round() as u8])
    });
    Selection {
        min: [0, 0],
        max: [width as i32, height as i32],
        mask: Some(Arc::new(mask)),
        inverted: false,
    }
}

pub fn selection_rectangle(
    start: [f32; 2],
    end: [f32; 2],
    size: [u32; 2],
    ratio: Option<f32>,
    centered: bool,
) -> Selection {
    let start = std::array::from_fn::<_, 2, _>(|axis| start[axis].clamp(0.0, size[axis] as f32));
    let mut delta = [end[0] - start[0], end[1] - start[1]];
    if let Some(ratio) = ratio.filter(|r| r.is_finite() && *r > 0.0) {
        if delta[0].abs() / ratio > delta[1].abs() {
            delta[1] = delta[0].abs() / ratio * if delta[1] < 0.0 { -1.0 } else { 1.0 };
        } else {
            delta[0] = delta[1].abs() * ratio * if delta[0] < 0.0 { -1.0 } else { 1.0 };
        }
    }
    let mut scale = 1.0_f32;
    for axis in 0..2 {
        let available = if centered {
            start[axis].min(size[axis] as f32 - start[axis])
        } else if delta[axis] < 0.0 {
            start[axis]
        } else {
            size[axis] as f32 - start[axis]
        };
        if delta[axis] != 0.0 {
            scale = scale.min(available / delta[axis].abs());
        }
    }
    delta = delta.map(|d| d * scale);
    let a = if centered {
        [start[0] - delta[0], start[1] - delta[1]]
    } else {
        start
    };
    let b = [start[0] + delta[0], start[1] + delta[1]];
    Selection::rectangle(
        std::array::from_fn(|axis| a[axis].min(b[axis]).floor() as i32),
        std::array::from_fn(|axis| a[axis].max(b[axis]).ceil().min(size[axis] as f32) as i32),
    )
}

pub fn crop(document: &mut Document, selection: &Selection) -> Result<(), String> {
    let min = [selection.min[0].max(0), selection.min[1].max(0)];
    let max = [
        selection.max[0].min(document.width as i32),
        selection.max[1].min(document.height as i32),
    ];
    let w = (max[0] - min[0]).max(0) as u32;
    let h = (max[1] - min[1]).max(0) as u32;
    validate_size(w, h)?;
    for layer in &mut document.layers {
        layer.offset[0] -= min[0];
        layer.offset[1] -= min[1];
        crate::masks::global(
            layer,
            crate::matrix::translate(-min[0] as f64, -min[1] as f64),
        );
    }
    document.width = w;
    document.height = h;
    document.layout.shift([-min[0] as f32, -min[1] as f32]);
    Ok(())
}

pub fn resize(
    document: &mut Document,
    width: u32,
    height: u32,
    resample: bool,
) -> Result<(), String> {
    validate_size(width, height)?;
    if resample {
        let sx = width as f64 / document.width as f64;
        let sy = height as f64 / document.height as f64;
        let mask_worlds: Vec<_> = document
            .layers
            .iter()
            .map(|l| {
                l.mask_placement.as_ref().map(|p| {
                    (
                        crate::matrix::multiply(
                            [sx, 0., 0., 0., sy, 0., 0., 0., 1.],
                            crate::masks::world(l),
                        ),
                        p.linked,
                        p.sampling,
                    )
                })
            })
            .collect();
        let dimensions: Vec<_> = document
            .layers
            .iter()
            .map(|l| {
                if l.adjustment_layer {
                    return (1, 1);
                }
                (
                    (l.pixels.width() as f64 * sx).round().max(1.0) as u32,
                    (l.pixels.height() as f64 * sy).round().max(1.0) as u32,
                )
            })
            .collect();
        if dimensions
            .iter()
            .map(|(w, h)| u64::from(*w) * u64::from(*h))
            .sum::<u64>()
            > crate::document::MAX_PIXELS
        {
            return Err("크기 변경이 메모리 예산을 초과합니다.".into());
        }
        let transformations: Vec<_> = document
            .layers
            .iter()
            .zip(&dimensions)
            .map(|(l, &(w, h))| {
                let old = if l.canvas_space() {
                    l.mask
                        .as_ref()
                        .map_or([document.width, document.height], |m| {
                            [m.width(), m.height()]
                        })
                } else {
                    [l.pixels.width(), l.pixels.height()]
                };
                let new = if l.canvas_space() {
                    old.map(|v| v)
                } else {
                    [w, h]
                };
                let new = if l.canvas_space() && l.mask_placement.is_none() {
                    [
                        (new[0] as f64 * sx).round().max(1.) as u32,
                        (new[1] as f64 * sy).round().max(1.) as u32,
                    ]
                } else {
                    new
                };
                let offset = [
                    (l.offset[0] as f64 * sx).round() as i32,
                    (l.offset[1] as f64 * sy).round() as i32,
                ];
                let full = crate::matrix::multiply(
                    [sx, 0., 0., 0., sy, 0., 0., 0., 1.],
                    crate::matrix::multiply(
                        l.transform.mapping(l.offset, old),
                        [
                            old[0] as f64 / new[0] as f64,
                            0.,
                            0.,
                            0.,
                            old[1] as f64 / new[1] as f64,
                            0.,
                            0.,
                            0.,
                            1.,
                        ],
                    ),
                );
                let base = crate::transform::Transform {
                    sampling: l.transform.sampling,
                    ..Default::default()
                };
                let relative = crate::matrix::multiply(
                    full,
                    crate::matrix::inverse(base.mapping(offset, new))
                        .ok_or("크기 변경 역행렬이 없습니다.")?,
                );
                let transform = base
                    .world(relative, offset, new)
                    .ok_or("크기 변경이 투영 한계를 넘습니다.")?;
                Ok((transform, offset))
            })
            .collect::<Result<Vec<_>, String>>()?;
        for (((layer, (w, h)), (transform, offset)), mask_world) in document
            .layers
            .iter_mut()
            .zip(dimensions)
            .zip(transformations)
            .zip(mask_worlds)
        {
            if let Some(scope) = &mut layer.adjustments.scope {
                if layer.group || layer.adjustment_layer {
                    scope.rebase([1. / sx, 0., 0., 0., 1. / sy, 0., 0., 0., 1.]);
                } else {
                    scope.rebase([
                        layer.pixels.width() as f64 / w as f64,
                        0.,
                        0.,
                        0.,
                        layer.pixels.height() as f64 / h as f64,
                        0.,
                        0.,
                        0.,
                        1.,
                    ]);
                }
            }
            if let Some(content) = &mut layer.content {
                content.resize([layer.pixels.width(), layer.pixels.height()], [w, h]);
            }
            layer.pixels = Arc::new(imageops::resize(
                &*layer.pixels,
                w,
                h,
                imageops::FilterType::Lanczos3,
            ));
            if let Some(pixels) = &layer.precision {
                let pixels = imageops::resize(&**pixels, w, h, imageops::FilterType::Lanczos3);
                layer.pixels = Arc::new(crate::precision::preview(&pixels));
                layer.precision = Some(Arc::new(pixels));
            }
            if layer.mask_placement.is_none()
                && let Some(mask) = &layer.mask
            {
                let (w, h) = if layer.canvas_space() {
                    (
                        (mask.width() as f64 * sx).round().max(1.0) as u32,
                        (mask.height() as f64 * sy).round().max(1.0) as u32,
                    )
                } else {
                    (w, h)
                };
                layer.mask = Some(Arc::new(imageops::resize(
                    &**mask,
                    w,
                    h,
                    imageops::FilterType::Triangle,
                )));
            }
            layer.offset = offset;
            layer.transform = transform;
            if let Some((matrix, linked, sampling)) = mask_world {
                crate::masks::set_world(layer, matrix, linked, sampling)?;
            }
        }
    }
    if resample {
        document.layout.scale([
            width as f32 / document.width as f32,
            height as f32 / document.height as f32,
        ]);
    }
    document.width = width;
    document.height = height;
    Ok(())
}

pub fn trim_transparent(document: &mut Document) -> Result<(), String> {
    let pixels = crate::precision::composite(document);
    let mut min = [document.width as i32, document.height as i32];
    let mut max = [0, 0];
    for (x, y, _) in pixels.enumerate_pixels().filter(|(_, _, p)| p[3] > 0.0) {
        min[0] = min[0].min(x as i32);
        min[1] = min[1].min(y as i32);
        max[0] = max[0].max(x as i32 + 1);
        max[1] = max[1].max(y as i32 + 1);
    }
    if max[0] <= min[0] || max[1] <= min[1] {
        return Err("문서 전체가 투명해 자를 영역이 없습니다.".into());
    }
    crop(document, &Selection::rectangle(min, max))
}

pub fn flip_canvas(document: &mut Document, horizontal: bool) {
    let axis = usize::from(!horizontal);
    let length = [document.width, document.height][axis] as i32;
    for guide in &mut document.layout.guides {
        if guide.axis == axis {
            guide.position = length as f32 - guide.position;
        }
    }
    let size = [document.width, document.height];
    let reflection = if horizontal {
        [-1., 0., length as f64, 0., 1., 0., 0., 0., 1.]
    } else {
        [1., 0., 0., 0., -1., length as f64, 0., 0., 1.]
    };
    for layer in &mut document.layers {
        let local_size = if layer.canvas_space() {
            layer
                .mask
                .as_ref()
                .map_or(size, |m| [m.width(), m.height()])
        } else {
            [layer.pixels.width(), layer.pixels.height()]
        };
        if let Some(transform) = layer.transform.world(reflection, layer.offset, local_size) {
            layer.transform = transform;
            crate::masks::global(layer, reflection);
        }
        if layer.canvas_space() {
            layer.effects.shadow.offset[axis] *= -1;
            layer.effects.inner_shadow.offset[axis] *= -1;
            layer.adjustments.motion_angle *= -1.;
        }
    }
}

pub fn copy_pixels(
    document: &Document,
    selection: Option<&Selection>,
    merged: bool,
) -> image::DynamicImage {
    let mut isolated = document.clone();
    if !merged {
        let ids: Vec<u64> = document
            .layers
            .iter()
            .filter(|l| document.descendant_of(l.id, document.active))
            .map(|l| l.id)
            .collect();
        isolated.layers.retain(|l| ids.contains(&l.id));
        if let Some(active) = isolated.active_layer_mut() {
            active.parent = None;
            active.visible = true;
            active.clipped = false;
        }
    }
    let depth = isolated
        .layers
        .iter()
        .map(|l| l.bit_depth)
        .max()
        .unwrap_or(8);
    let pixels = crate::precision::composite(&isolated);
    let (min, max) = selection.filter(|s| !s.inverted).map_or(
        ([0, 0], [document.width as i32, document.height as i32]),
        |s| (s.min, s.max),
    );
    let min = [
        min[0].clamp(0, document.width as i32),
        min[1].clamp(0, document.height as i32),
    ];
    let max = [
        max[0].clamp(min[0], document.width as i32),
        max[1].clamp(min[1], document.height as i32),
    ];
    let copy = image::Rgba32FImage::from_fn(
        (max[0] - min[0]).max(1) as u32,
        (max[1] - min[1]).max(1) as u32,
        |x, y| {
            let dx = x as i32 + min[0];
            let dy = y as i32 + min[1];
            if dx >= document.width as i32 || dy >= document.height as i32 {
                return Rgba([0.0; 4]);
            }
            let mut p = *pixels.get_pixel(dx as u32, dy as u32);
            p[3] *= selection.map_or(1.0, |s| s.coverage(dx, dy));
            p
        },
    );
    match depth {
        32 => image::DynamicImage::ImageRgba32F(copy),
        16 => image::DynamicImage::ImageRgba16(crate::precision::image16(&copy)),
        _ => image::DynamicImage::ImageRgba8(crate::precision::preview(&copy)),
    }
}

pub fn merge_visible(document: &mut Document) -> Result<(), String> {
    if !document.layers.iter().any(|l| l.visible) {
        return Ok(());
    }
    let pixels = copy_pixels(document, None, true);
    let hidden: Vec<u64> = document
        .layers
        .iter()
        .filter(|l| {
            document
                .layers
                .iter()
                .any(|p| !p.visible && document.descendant_of(l.id, p.id))
        })
        .map(|l| l.id)
        .collect();
    document.layers.retain(|l| hidden.contains(&l.id));
    let ids: Vec<u64> = document.layers.iter().map(|l| l.id).collect();
    for l in &mut document.layers {
        if l.parent.is_some_and(|p| !ids.contains(&p)) {
            l.parent = None;
        }
    }
    let insertion = document.layers.len();
    document.active = 0;
    document.add_dynamic("병합된 이미지".into(), pixels)?;
    let merged = document.layers.pop().unwrap();
    document.layers.insert(insertion, merged);
    Ok(())
}

pub fn merge_down(document: &mut Document, ids: &[u64]) -> Result<(), String> {
    let roots = document.selected_roots(ids);
    if roots.len() != 1 {
        return Err("아래와 병합할 레이어 하나를 선택하세요.".into());
    }
    let index = document
        .layers
        .iter()
        .position(|l| l.id == roots[0])
        .unwrap();
    let parent = document.layers[index].parent;
    let below = document.layers[..index]
        .iter()
        .rev()
        .find(|l| l.parent == parent)
        .ok_or("아래에 병합할 레이어가 없습니다.")?
        .id;
    merge_selected(document, &[below, roots[0]])
}

pub fn merge_selected(document: &mut Document, ids: &[u64]) -> Result<(), String> {
    if !document
        .layers
        .iter()
        .any(|l| l.adjustment_layer && ids.iter().any(|id| document.descendant_of(l.id, *id)))
    {
        return merge_selected_inner(document, ids);
    }
    let original = crate::precision::composite(document);
    let depth = document
        .layers
        .iter()
        .map(|l| l.bit_depth)
        .max()
        .unwrap_or(8);
    let tolerance = match depth {
        8 => 1. / 255.,
        16 => 1. / 65535.,
        _ => 1e-5,
    };
    let mut candidate = document.clone();
    merge_selected_inner(&mut candidate, ids)?;
    let changed = crate::precision::composite(&candidate);
    if original
        .pixels()
        .zip(changed.pixels())
        .any(|(a, b)| (0..4).any(|c| (a[c] - b[c]).abs() > tolerance * (1. + a[c].abs())))
    {
        return Err("이 조정은 선택하지 않은 아래 합성에도 적용됩니다. 적용 대상 레이어를 모두 함께 선택하거나 전체 합성을 병합하세요.".into());
    }
    *document = candidate;
    Ok(())
}
fn merge_selected_inner(document: &mut Document, ids: &[u64]) -> Result<(), String> {
    let roots = document.selected_roots(ids);
    if roots.is_empty() {
        return Err("레이어를 선택하세요.".into());
    }
    let parent = document
        .layers
        .iter()
        .find(|l| l.id == roots[0])
        .unwrap()
        .parent;
    if roots
        .iter()
        .any(|id| document.layers.iter().find(|l| l.id == *id).unwrap().parent != parent)
    {
        return Err("같은 그룹의 레이어를 선택해 병합하세요.".into());
    }
    if !document.movable(&roots) {
        return Err("잠긴 레이어는 병합할 수 없습니다.".into());
    }
    let siblings: Vec<_> = document
        .layers
        .iter()
        .filter(|l| l.parent == parent)
        .collect();
    for (i, _) in siblings
        .iter()
        .enumerate()
        .filter(|(_, l)| roots.contains(&l.id) && l.clipped)
    {
        let base = siblings[..i].iter().rev().find(|l| !l.clipped);
        if base.is_none_or(|l| !roots.contains(&l.id)) {
            return Err("클리핑 기준 레이어도 함께 선택해 병합하세요.".into());
        }
    }
    let isolated = document.selected_document(&roots)?;
    if document.pixel_count() - isolated.pixel_count()
        + u64::from(document.width) * u64::from(document.height)
        > crate::document::MAX_PIXELS
    {
        return Err("병합이 현재 문서 메모리 예산을 초과합니다.".into());
    }
    let image = crate::precision::composite(&isolated);
    let depth = isolated
        .layers
        .iter()
        .map(|l| l.bit_depth)
        .max()
        .unwrap_or(8);
    let pixels = match depth {
        32 => image::DynamicImage::ImageRgba32F(image),
        16 => image::DynamicImage::ImageRgba16(crate::precision::image16(&image)),
        _ => image::DynamicImage::ImageRgba8(crate::precision::preview(&image)),
    };
    let removed: Vec<u64> = isolated.layers.iter().map(|l| l.id).collect();
    let insertion = document
        .layers
        .iter()
        .rposition(|l| roots.contains(&l.id))
        .unwrap();
    let before_count = document.layers[..insertion]
        .iter()
        .filter(|l| !removed.contains(&l.id))
        .count();
    document.layers.retain(|l| !removed.contains(&l.id));
    // Append through the regular precision-aware constructor, then put it at the selected stack position.
    document.active = 0;
    document.add_dynamic("병합된 레이어".into(), pixels)?;
    let mut layer = document.layers.pop().unwrap();
    layer.parent = parent;
    document.layers.insert(before_count, layer);
    Ok(())
}

pub fn fill(layer: &mut Layer, selection: Option<&Selection>, color: [u8; 4], clear: bool) {
    if layer.locked || layer.adjustment_layer {
        return;
    }
    let offset = layer.offset;
    let transform = layer.transform;
    let size = [layer.pixels.width(), layer.pixels.height()];
    let mut precision = layer.precision.as_mut().map(Arc::make_mut);
    for (x, y, p) in Arc::make_mut(&mut layer.pixels).enumerate_pixels_mut() {
        let point = transform.to_document([x as f32 + 0.5, y as f32 + 0.5], offset, size);
        let alpha = selection.map_or(1.0, |s| {
            s.coverage(point[0].floor() as i32, point[1].floor() as i32)
        });
        if let Some(high) = &mut precision {
            let value = high.get_pixel_mut(x, y);
            if clear {
                value[3] *= 1.0 - alpha;
            } else {
                value.0 = crate::precision::over(
                    color.map(|c| c as f32 / 255.0),
                    value.0,
                    alpha,
                    crate::blend::BlendMode::Normal,
                );
            }
            p.0 = value.0.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        } else if clear {
            p[3] = (p[3] as f32 * (1.0 - alpha)).round() as u8;
        } else {
            p.0 = crate::document::over(color, p.0, alpha);
        }
    }
}

pub fn fill_mask(layer: &mut Layer, selection: Option<&Selection>, color: [u8; 4]) {
    if layer.locked {
        return;
    }
    let Some((transform, offset, size)) = crate::masks::geometry(layer) else {
        return;
    };
    let mask = layer.mask.as_mut().unwrap();
    let value =
        (color[0] as f32 * 0.2126 + color[1] as f32 * 0.7152 + color[2] as f32 * 0.0722).round();
    for (x, y, p) in Arc::make_mut(mask).enumerate_pixels_mut() {
        let point = transform.to_document([x as f32 + 0.5, y as f32 + 0.5], offset, size);
        let amount = selection.map_or(1.0, |s| {
            s.coverage(point[0].floor() as i32, point[1].floor() as i32)
        }) * color[3] as f32
            / 255.0;
        p[0] = (p[0] as f32 * (1.0 - amount) + value * amount).round() as u8;
    }
}

#[derive(Clone, Copy)]
pub enum Retouch {
    Clone,
    Heal,
    Blur,
}

pub fn rasterize(layer: &mut Layer) -> Result<(), String> {
    if layer.adjustment_layer {
        return Err("조정 레이어는 아래 레이어와 병합하세요.".into());
    }
    let mut prepared = layer.clone();
    crate::masks::align_source(&mut prepared);
    rasterize_source(&mut prepared)?;
    *layer = prepared;
    Ok(())
}
fn rasterize_source(layer: &mut Layer) -> Result<(), String> {
    let size = [layer.pixels.width(), layer.pixels.height()];
    if !layer.transform.valid_for(size) {
        return Err("원본 영역의 변형이 유효하지 않습니다.".into());
    }
    let (min, max) = layer.transform.bounds(layer.offset, size);
    let width = i64::from(max[0]) - i64::from(min[0]);
    let height = i64::from(max[1]) - i64::from(min[1]);
    if width <= 0 || height <= 0 || width > u32::MAX as i64 || height > u32::MAX as i64 {
        return Err("변형 크기가 너무 큽니다.".into());
    }
    validate_size(width as u32, height as u32)?;
    let inverse = crate::matrix::inverse(layer.transform.mapping(layer.offset, size))
        .ok_or("변형 역행렬이 없습니다.")?;
    let local = |x: u32, y: u32| {
        crate::matrix::point(
            inverse,
            [
                x as f32 + min[0] as f32 + 0.5,
                y as f32 + min[1] as f32 + 0.5,
            ],
        )
    };
    let inside = |x: i32, y: i32| x >= 0 && y >= 0 && x < size[0] as i32 && y < size[1] as i32;
    let coverage = |x: i32, y: i32| {
        if !inside(x, y) {
            0.
        } else {
            layer
                .mask
                .as_ref()
                .map_or(1., |m| m.get_pixel(x as u32, y as u32)[0] as f32 / 255.)
        }
    };
    let sampling = layer.transform.sampling;
    let mask = layer.mask.as_ref().map(|_| {
        Arc::new(GrayImage::from_fn(width as u32, height as u32, |x, y| {
            Luma([(crate::sampling::gray(local(x, y), sampling, coverage) * 255.).round() as u8])
        }))
    });
    let floats = image::Rgba32FImage::from_fn(width as u32, height as u32, |x, y| {
        let mut rgba = crate::sampling::rgba(local(x, y), sampling, |sx, sy| {
            if !inside(sx, sy) {
                return [0.; 4];
            }
            let mut p = layer.precision.as_ref().map_or_else(
                || {
                    layer
                        .pixels
                        .get_pixel(sx as u32, sy as u32)
                        .0
                        .map(|v| v as f32 / 255.)
                },
                |p| p.get_pixel(sx as u32, sy as u32).0,
            );
            if layer.mask_enabled {
                p[3] *= coverage(sx, sy);
            }
            p
        });
        if layer.mask_enabled
            && let Some(mask) = &mask
        {
            let alpha = mask.get_pixel(x, y)[0] as f32 / 255.;
            rgba[3] = if alpha > 0. {
                (rgba[3] / alpha).clamp(0., 1.)
            } else {
                0.
            };
        }
        Rgba(rgba)
    });
    if let Some(scope) = &mut layer.adjustments.scope {
        scope.rebase(crate::matrix::multiply(
            inverse,
            crate::matrix::translate(min[0] as f64, min[1] as f64),
        ));
    }
    layer.pixels = Arc::new(crate::precision::preview(&floats));
    if layer.precision.is_some() {
        layer.precision = Some(Arc::new(floats));
    }
    layer.mask = mask;
    layer.offset = min;
    layer.transform = Default::default();
    layer.content = None;
    Ok(())
}

pub fn retouch_source(document: &Document, merged: bool, mask: bool) -> image::Rgba32FImage {
    if !mask {
        return if merged {
            crate::precision::composite(document)
        } else {
            copy_pixels(document, None, false).into_rgba32f()
        };
    }
    let layer = document.active_layer().unwrap();
    let Some((transform, offset, size)) = crate::masks::geometry(layer) else {
        return image::Rgba32FImage::new(document.width, document.height);
    };
    let mask = layer.mask.as_ref().unwrap();
    image::Rgba32FImage::from_fn(document.width, document.height, |x, y| {
        let local = transform.to_local([x as f32 + 0.5, y as f32 + 0.5], offset, size);
        let value = if local[0] >= 0.0
            && local[1] >= 0.0
            && local[0] < size[0] as f32
            && local[1] < size[1] as f32
        {
            mask.get_pixel(local[0] as u32, local[1] as u32)[0] as f32 / 255.0
        } else {
            0.0
        };
        Rgba([value, value, value, 1.0])
    })
}

#[allow(clippy::too_many_arguments)] // A stroke needs both its source and destination coordinate spaces.
pub fn retouch_line(
    layer: &mut Layer,
    source: &image::Rgba32FImage,
    from: [f32; 2],
    to: [f32; 2],
    delta: [f32; 2],
    brush: &crate::document::Brush,
    selection: Option<&Selection>,
    mode: Retouch,
) {
    if layer.locked
        || (layer.adjustment_layer && !brush.mask)
        || (brush.mask && layer.mask.is_none())
    {
        return;
    }
    let steps = (((to[0] - from[0]).hypot(to[1] - from[1])) / (brush.radius * 0.4).max(1.0))
        .ceil()
        .max(1.0) as usize;
    let (transform, offset, size) = if brush.mask {
        let Some(geometry) = crate::masks::geometry(layer) else {
            return;
        };
        geometry
    } else {
        (
            layer.transform,
            layer.offset,
            [layer.pixels.width(), layer.pixels.height()],
        )
    };
    let mut mask = if brush.mask {
        layer.mask.as_mut().map(Arc::make_mut)
    } else {
        None
    };
    let mut precision = if brush.mask {
        None
    } else {
        layer.precision.as_mut().map(Arc::make_mut)
    };
    let mut image = (!brush.mask).then(|| Arc::make_mut(&mut layer.pixels));
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let center = [
            from[0] + (to[0] - from[0]) * t,
            from[1] + (to[1] - from[1]) * t,
        ];
        let radius = brush.radius.max(0.5);
        let mut surrounding = [0.0; 4];
        let mut samples = 0.0;
        if matches!(mode, Retouch::Heal) {
            for i in 0..24 {
                let a = i as f32 * std::f32::consts::TAU / 24.0;
                let x = (center[0] + a.cos() * radius * 1.8) as i32;
                let y = (center[1] + a.sin() * radius * 1.8) as i32;
                if x >= 0 && y >= 0 && x < source.width() as i32 && y < source.height() as i32 {
                    let p = source.get_pixel(x as u32, y as u32);
                    for c in 0..4 {
                        surrounding[c] += p[c];
                    }
                    samples += 1.0;
                }
            }
        }
        let local = transform.to_local(center, offset, size);
        let bound_radius = radius / transform.scale[0].abs().min(transform.scale[1].abs());
        let min = [
            (local[0] - bound_radius).floor().max(0.0) as i32,
            (local[1] - bound_radius).floor().max(0.0) as i32,
        ];
        let max = [
            (local[0] + bound_radius).ceil().min(size[0] as f32) as i32,
            (local[1] + bound_radius).ceil().min(size[1] as f32) as i32,
        ];
        let (min, max) = if transform.warp.is_some() {
            transform.local_bounds(
                [center[0] - radius, center[1] - radius],
                [center[0] + radius, center[1] + radius],
                offset,
                size,
            )
        } else {
            (min, max)
        };
        for y in min[1]..max[1] {
            for x in min[0]..max[0] {
                let world = transform.to_document([x as f32 + 0.5, y as f32 + 0.5], offset, size);
                let (dx, dy) = (world[0].floor() as i32, world[1].floor() as i32);
                let distance = (world[0] - center[0]).hypot(world[1] - center[1]);
                if distance >= radius {
                    continue;
                }
                let sx = (dx as f32 + delta[0]).floor() as i32;
                let sy = (dy as f32 + delta[1]).floor() as i32;
                if sx < 0 || sy < 0 || sx >= source.width() as i32 || sy >= source.height() as i32 {
                    continue;
                }
                let sample = if matches!(mode, Retouch::Heal) && samples > 0.0 {
                    surrounding.map(|c| c / samples)
                } else {
                    source.get_pixel(sx as u32, sy as u32).0
                };
                let coverage = ((radius - distance) / (radius * (1.0 - brush.hardness.min(0.99))))
                    .clamp(0.0, 1.0)
                    * brush.opacity
                    * selection.map_or(1.0, |s| s.coverage(dx, dy));
                if let Some(mask) = &mut mask {
                    let p = mask.get_pixel_mut(x as u32, y as u32);
                    p[0] = (p[0] as f32 * (1.0 - coverage) + sample[0] * 255.0 * coverage)
                        .clamp(0.0, 255.0)
                        .round() as u8;
                    continue;
                }
                let p = image.as_mut().unwrap().get_pixel_mut(x as u32, y as u32);
                if let Some(high) = &mut precision {
                    let p_high = high.get_pixel_mut(x as u32, y as u32);
                    for c in 0..4 {
                        p_high[c] = p_high[c] * (1.0 - coverage) + sample[c] * coverage;
                    }
                    p.0 = p_high.0.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
                    continue;
                }
                for c in 0..4 {
                    p[c] = (p[c] as f32 * (1.0 - coverage) + sample[c] * 255.0 * coverage)
                        .clamp(0.0, 255.0)
                        .round() as u8;
                }
            }
        }
    }
}
