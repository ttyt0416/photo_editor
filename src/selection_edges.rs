use crate::document::{Selection, validate_size};
use image::{GrayImage, Luma};
use std::sync::Arc;

// Scanline coverage counts sixteen subpixel samples without allocating an enlarged image.
fn scan(
    min: [i32; 2],
    max: [i32; 2],
    smooth: bool,
    mut spans: impl FnMut(f64, &mut Vec<f64>),
) -> Selection {
    let size = std::array::from_fn::<_, 2, _>(|i| (max[i] - min[i]).max(1) as u32);
    let samples = if smooth { 4 } else { 1 };
    let denominator = samples * samples;
    let mut image = GrayImage::new(size[0], size[1]);
    let mut crossings = Vec::new();
    let mut row = vec![0u8; size[0] as usize];
    for y in 0..size[1] {
        row.fill(0);
        for sub in 0..samples {
            crossings.clear();
            spans(
                min[1] as f64 + y as f64 + (sub as f64 + 0.5) / samples as f64,
                &mut crossings,
            );
            crossings.sort_unstable_by(f64::total_cmp);
            for pair in crossings.as_chunks::<2>().0 {
                let start = (((pair[0] - min[0] as f64) * samples as f64 - 0.5).ceil() as i64)
                    .clamp(0, size[0] as i64 * samples);
                let end = (((pair[1] - min[0] as f64) * samples as f64 - 0.5).ceil() as i64)
                    .clamp(0, size[0] as i64 * samples);
                if end <= start {
                    continue;
                }
                let first = (start / samples) as usize;
                let last = ((end - 1) / samples) as usize;
                if first == last {
                    row[first] += (end - start) as u8;
                } else {
                    row[first] += (samples - start % samples) as u8;
                    row[last] += ((end - 1) % samples + 1) as u8;
                    for count in &mut row[first + 1..last] {
                        *count += samples as u8;
                    }
                }
            }
        }
        for (x, count) in row.iter().enumerate() {
            image.put_pixel(
                x as u32,
                y,
                Luma([((*count as i64 * 255 + denominator / 2) / denominator) as u8]),
            );
        }
    }
    Selection {
        min,
        max,
        mask: Some(Arc::new(image)),
        inverted: false,
    }
}
pub fn ellipse(min: [i32; 2], max: [i32; 2], smooth: bool) -> Selection {
    let radius = std::array::from_fn::<_, 2, _>(|i| (max[i] - min[i]).max(1) as f64 / 2.);
    let center = std::array::from_fn::<_, 2, _>(|i| min[i] as f64 + radius[i]);
    scan(min, max, smooth, |y, spans| {
        let dy = (y - center[1]) / radius[1];
        if dy.abs() < 1. {
            let dx = radius[0] * (1. - dy * dy).sqrt();
            spans.extend([center[0] - dx, center[0] + dx]);
        }
    })
}
pub fn polygon(points: &[[f32; 2]], width: u32, height: u32, smooth: bool) -> Option<Selection> {
    if points.len() < 3
        || points.len() > 100000
        || points
            .iter()
            .flatten()
            .any(|v| !v.is_finite() || v.abs() > 1e9)
        || validate_size(width, height).is_err()
    {
        return None;
    }
    let size = [width, height];
    let min = std::array::from_fn::<_, 2, _>(|i| {
        points
            .iter()
            .map(|p| p[i])
            .fold(f32::INFINITY, f32::min)
            .floor()
            .clamp(0., size[i] as f32) as i32
    });
    let max = std::array::from_fn::<_, 2, _>(|i| {
        points
            .iter()
            .map(|p| p[i])
            .fold(f32::NEG_INFINITY, f32::max)
            .ceil()
            .clamp(0., size[i] as f32) as i32
    });
    if min.iter().zip(max).any(|(a, b)| *a >= b) {
        return None;
    }
    let mut edges = Vec::with_capacity(points.len());
    let mut previous = points[points.len() - 1];
    for &p in points {
        let (a, b) = if p[1] < previous[1] {
            (p.map(f64::from), previous.map(f64::from))
        } else {
            (previous.map(f64::from), p.map(f64::from))
        };
        if a[1] < b[1] {
            edges.push((a[1], b[1], a[0], (b[0] - a[0]) / (b[1] - a[1])));
        }
        previous = p;
    }
    edges.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
    let mut cursor = 0;
    let mut active = Vec::new();
    Some(scan(min, max, smooth, |y, spans| {
        while cursor < edges.len() && edges[cursor].0 <= y {
            if edges[cursor].1 > y {
                active.push(edges[cursor]);
            }
            cursor += 1;
        }
        active.retain(|edge| edge.1 > y);
        for &(low, _, x, slope) in &active {
            spans.push(x + (y - low) * slope);
        }
    }))
}
pub fn color(selection: Selection) -> Selection {
    let Some(mask) = &selection.mask else {
        return selection;
    };
    // A one-pixel tent filter softens the discrete color region boundary.
    let filtered = GrayImage::from_fn(mask.width(), mask.height(), |x, y| {
        let mut sum = 0u32;
        for (dy, wy) in [(-1, 1), (0, 2), (1, 1)] {
            for (dx, wx) in [(-1, 1), (0, 2), (1, 1)] {
                let sx = (x as i64 + dx).clamp(0, mask.width() as i64 - 1) as u32;
                let sy = (y as i64 + dy).clamp(0, mask.height() as i64 - 1) as u32;
                sum += mask.get_pixel(sx, sy)[0] as u32 * wx * wy;
            }
        }
        Luma([((sum + 8) / 16) as u8])
    });
    Selection {
        mask: Some(Arc::new(filtered)),
        ..selection
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smoothed_mask_fill_copy_project_and_undo_keep_fractional_alpha() {
        let selection = polygon(&[[1., 1.], [8.25, 2.25], [2.4, 8.]], 10, 10, true).unwrap();
        let (x, y, coverage) = (0..10)
            .flat_map(|y| (0..10).map(move |x| (x, y)))
            .map(|(x, y)| (x, y, selection.coverage(x, y)))
            .find(|(_, _, v)| *v > 0. && *v < 1.)
            .unwrap();
        let mut doc = crate::precision::document(
            "선택 경계".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
                10,
                10,
                image::Rgba([2.125, -0.25, 0.3456789, 0.8]),
            )),
        )
        .unwrap();
        doc.layers[0].mask = Some(Arc::new(GrayImage::from_pixel(10, 10, Luma([255]))));
        let pixels = doc.layers[0].precision.clone().unwrap();
        let raw = doc.layers[0].mask.clone().unwrap();
        let mut editor = crate::document::Editor::new(doc, true);
        editor.edit("선택 마스크 채우기", |doc| {
            crate::editing::fill_mask(&mut doc.layers[0], Some(&selection), [0, 0, 0, 255])
        });
        let expected = (255. * (1. - coverage)).round() as u8;
        assert_eq!(
            editor.document.layers[0]
                .mask
                .as_ref()
                .unwrap()
                .get_pixel(x as u32, y as u32)[0],
            expected
        );
        assert!(Arc::ptr_eq(
            &pixels,
            editor.document.layers[0].precision.as_ref().unwrap()
        ));
        let copied =
            crate::editing::copy_pixels(&editor.document, Some(&selection), false).into_rgba32f();
        assert!(copied.pixels().any(|p| p[3] > 0. && p[3] < 0.8));
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/selection-edges");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("selection.rphoto");
        crate::files::save_project(&editor.document, &path).unwrap();
        let restored = crate::files::open(&path).unwrap().document;
        assert_eq!(
            crate::precision::composite(&restored),
            crate::precision::composite(&editor.document)
        );
        editor.undo();
        assert!(Arc::ptr_eq(
            &raw,
            editor.document.layers[0].mask.as_ref().unwrap()
        ));
        editor.redo();
        assert_eq!(
            editor.document.layers[0]
                .mask
                .as_ref()
                .unwrap()
                .get_pixel(x as u32, y as u32)[0],
            expected
        );
    }
    #[test]
    fn diagonal_coverage_matches_independent_supersampling_and_hard_edges() {
        let points = [[0.25, 0.3], [7.4, 1.2], [2.6, 7.75]];
        for smooth in [false, true] {
            let selection = polygon(&points, 9, 9, smooth).unwrap();
            let n = if smooth { 4 } else { 1 };
            for y in 0..9 {
                for x in 0..9 {
                    let mut count = 0;
                    for sy in 0..n {
                        for sx in 0..n {
                            let p = [
                                x as f64 + (sx as f64 + 0.5) / n as f64,
                                y as f64 + (sy as f64 + 0.5) / n as f64,
                            ];
                            let mut inside = false;
                            for i in 0..3 {
                                let a = points[i].map(f64::from);
                                let b = points[(i + 1) % 3].map(f64::from);
                                if (a[1] > p[1]) != (b[1] > p[1])
                                    && p[0] < a[0] + (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1])
                                {
                                    inside = !inside;
                                }
                            }
                            count += usize::from(inside);
                        }
                    }
                    let expected = (count * 255 + n * n / 2) / (n * n);
                    assert_eq!((selection.coverage(x, y) * 255.).round() as usize, expected);
                }
            }
        }
        let selection = polygon(&points, 9, 9, true).unwrap();
        assert!(
            selection
                .mask
                .unwrap()
                .pixels()
                .any(|p| p[0] > 0 && p[0] < 255)
        );
        let rectangle = polygon(&[[1., 1.], [7., 1.], [7., 7.], [1., 7.]], 9, 9, true).unwrap();
        assert_eq!(rectangle.coverage(1, 1), 1.);
        assert_eq!(rectangle.coverage(0, 1), 0.);
    }
    #[test]
    fn ellipse_boundary_color_smoothing_and_invalid_polygons_are_bounded() {
        let s = ellipse([0, 0], [8, 6], true);
        assert!(
            s.mask
                .as_ref()
                .unwrap()
                .pixels()
                .any(|p| p[0] > 0 && p[0] < 255)
        );
        assert_eq!(s.coverage(4, 3), 1.);
        assert_eq!(s.coverage(8, 3), 0.);
        let hard = ellipse([0, 0], [8, 6], false);
        assert!(
            hard.mask
                .unwrap()
                .pixels()
                .all(|p| p[0] == 0 || p[0] == 255)
        );
        let input = Selection {
            min: [0, 0],
            max: [8, 6],
            mask: Some(Arc::new(GrayImage::from_fn(8, 6, |x, _| {
                Luma([if x < 4 { 255 } else { 0 }])
            }))),
            inverted: false,
        };
        let s = color(input);
        assert_eq!(s.coverage(0, 0), 1.);
        assert_eq!(s.coverage(7, 0), 0.);
        assert_eq!((s.coverage(3, 0) * 255.).round() as u8, 191);
        assert_eq!((s.coverage(4, 0) * 255.).round() as u8, 64);
        assert!(polygon(&[[f32::NAN, 0.], [1., 1.], [0., 1.]], 8, 8, true).is_none());
        assert!(polygon(&[[1., 1.]; 3], 8, 8, true).is_none());
        assert!(polygon(&[[0., 0.], [1., 1.], [0., 1.]], 0, 8, true).is_none());
    }
}
