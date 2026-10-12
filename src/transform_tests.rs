use crate::{
    document::{Document, Editor, Selection},
    matrix as m,
    sampling::{self, Sampling},
    transform::{Handle, Transform},
    transform_batch::Frame,
};
use image::{GrayImage, Luma, Rgba, Rgba32FImage, RgbaImage};
use std::sync::Arc;
fn close(a: [f32; 2], b: [f32; 2]) {
    for i in 0..2 {
        assert!((a[i] - b[i]).abs() < 0.002, "{a:?} != {b:?}");
    }
}
fn folder() -> std::path::PathBuf {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/transform-crop");
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn pattern() -> Document {
    let mut d = Document::from_image(
        "원본".into(),
        RgbaImage::from_fn(20, 16, |x, y| {
            Rgba([
                (x * 12) as u8,
                (y * 15) as u8,
                80,
                if x < 2 { 0 } else { 255 },
            ])
        }),
    );
    crate::editing::resize(&mut d, 64, 64, false).unwrap();
    d.layers[0].offset = [12, 14];
    d
}
#[test]
fn perspective_maps_known_trapezoid_and_inverse_without_crossing_horizon() {
    let q = [[10., 10.], [50., 10.], [40., 40.], [20., 40.]];
    let matrix = m::unit_quad(q).unwrap();
    for (p, expected) in [
        ([0., 0.], q[0]),
        ([1., 0.], q[1]),
        ([1., 1.], q[2]),
        ([0., 1.], q[3]),
        ([0.5, 0.5], [30., 30.]),
    ] {
        close(m::point(matrix, p), expected);
        close(m::point(m::inverse(matrix).unwrap(), expected), p);
    }
    assert!(m::unit_quad([[0., 0.], [10., 10.], [10., 0.], [0., 10.]]).is_none());
    assert!(m::unit_quad([[0.; 2]; 4]).is_none());
    let transform = Transform {
        warp: Some([1., 0., 0., 0., 1., 0., -0.1, 0., 1.]),
        ..Default::default()
    };
    assert!(transform.valid());
    assert!(!transform.valid_for([20, 20]));
    let old: Transform = serde_json::from_str(r#"{"scale":[1,1],"angle":0}"#).unwrap();
    assert_eq!(old, Transform::default());
}
#[test]
fn interpolation_preserves_hdr_and_ignores_transparent_rgb_halos() {
    for mode in [Sampling::Bilinear, Sampling::Bicubic] {
        let p = sampling::rgba([1., 0.5], mode, |x, _| {
            if x < 1 {
                [2.5, -0.5, 0.25, 1.]
            } else {
                [0., 100., 50., 0.]
            }
        });
        assert!((p[0] - 2.5).abs() < 1e-5);
        assert!((p[1] + 0.5).abs() < 1e-5);
        assert!((p[3] - 0.5).abs() < 1e-5);
        let p = sampling::rgba([4.75, 3.25], mode, |x, y| [x as f32, y as f32, 2., 1.]);
        assert!((p[0] - 4.25).abs() < 1e-5);
        assert!((p[1] - 2.75).abs() < 1e-5);
        assert_eq!(p[3], 1.);
    }
    assert_eq!(
        sampling::rgba([0.9, 1.9], Sampling::Nearest, |x, y| [
            x as f32, y as f32, 4., 1.
        ]),
        [0., 1., 4., 1.]
    );
    assert_eq!(
        sampling::rgba([f32::NAN, 0.], Sampling::Bicubic, |_, _| [1.; 4]),
        [0.; 4]
    );
}
#[test]
fn common_nonuniform_scale_of_rotated_layers_keeps_shear_source_and_undo() {
    let mut d = pattern();
    d.layers[0].transform.angle = 33.;
    d.add_layer(
        "둘째".into(),
        RgbaImage::from_pixel(12, 8, Rgba([20, 200, 80, 255])),
    )
    .unwrap();
    d.layers[1].offset = [35, 20];
    d.layers[1].transform.angle = -24.;
    let roots = d.layers.iter().map(|l| l.id).collect();
    let before = d.clone();
    let mut e = Editor::new(d, true);
    let f = Frame::new(&e.document, roots).unwrap();
    let world = [1.5, 0., 3., 0., 0.8, 5., 0., 0., 1.];
    assert!(f.apply(&mut e, f.mapped(world)).unwrap());
    for (old, new) in before.layers.iter().zip(&e.document.layers) {
        let size = [old.pixels.width(), old.pixels.height()];
        for p in [[0., 0.], [3., 4.], size.map(|v| v as f32)] {
            close(
                new.transform.to_document(p, new.offset, size),
                m::point(world, old.transform.to_document(p, old.offset, size)),
            );
        }
        assert!(new.transform.warp.is_some());
        assert!(Arc::ptr_eq(&old.pixels, &new.pixels));
    }
    assert_eq!(e.history().count(), 0);
    e.finish();
    assert_eq!(e.history().count(), 1);
    e.undo();
    assert_eq!(e.document.composite(), before.composite());
    e.redo();
    assert!(e.document.layers[0].transform.warp.is_some());
}
#[test]
fn nested_group_masks_transform_once_with_hdr_clipping_and_native_content() {
    let mut d = crate::precision::document(
        "HDR".into(),
        image::DynamicImage::ImageRgba32F(Rgba32FImage::from_pixel(
            12,
            12,
            Rgba([2.5, -0.125, 0.3, 1.]),
        )),
    )
    .unwrap();
    crate::editing::resize(&mut d, 64, 64, false).unwrap();
    d.layers[0].offset = [10, 12];
    d.group_active().unwrap();
    let inner = d.active;
    d.active_layer_mut().unwrap().mask = Some(Arc::new(GrayImage::from_fn(64, 64, |x, _| {
        Luma([if x < 16 { 255 } else { 100 }])
    })));
    let content = crate::vector::Content::Shape {
        kind: crate::vector::ShapeKind::Rectangle,
        width: 10,
        height: 10,
        fill: [30, 220, 40, 128],
        stroke: [0; 4],
        thickness: 0.,
    };
    d.add_layer("벡터".into(), content.render().unwrap())
        .unwrap();
    let l = d.active_layer_mut().unwrap();
    l.content = Some(content);
    l.offset = [15, 15];
    l.parent = Some(inner);
    l.clipped = true;
    d.active = inner;
    d.group_active().unwrap();
    let root = d.active;
    d.active_layer_mut().unwrap().mask = Some(Arc::new(GrayImage::from_pixel(64, 64, Luma([200]))));
    let before = d.clone();
    let mut e = Editor::new(d, true);
    let f = Frame::new(&e.document, vec![root]).unwrap();
    let world = [1.2, 0.15, 2., -0.1, 1.1, 3., 0., 0., 1.];
    f.apply(&mut e, f.mapped(world)).unwrap();
    for (old, new) in before.layers.iter().zip(&e.document.layers) {
        let size = if old.group {
            [64, 64]
        } else {
            [old.pixels.width(), old.pixels.height()]
        };
        close(
            new.transform.to_document([5., 7.], new.offset, size),
            m::point(world, old.transform.to_document([5., 7.], old.offset, size)),
        );
        assert_eq!(new.parent, old.parent);
        assert_eq!(new.clipped, old.clipped);
        assert_eq!(new.content, old.content);
        assert!(Arc::ptr_eq(&old.pixels, &new.pixels));
        if let (Some(a), Some(b)) = (&old.mask, &new.mask) {
            assert!(Arc::ptr_eq(a, b));
        }
        if let (Some(a), Some(b)) = (&old.precision, &new.precision) {
            assert!(Arc::ptr_eq(a, b));
        }
    }
    assert!(
        crate::precision::composite(&e.document)
            .pixels()
            .any(|p| p[0] > 1.)
    );
    e.cancel();
    assert_eq!(
        crate::precision::composite(&e.document),
        crate::precision::composite(&before)
    );
    assert_eq!(e.history().count(), 0);
}
#[test]
fn preview_reuses_original_snapshot_and_invalid_or_locked_batch_is_atomic() {
    let d = pattern();
    let original = d.layers[0].pixels.clone();
    let mut e = Editor::new(d, true);
    let mut f = Frame::new(&e.document, vec![1]).unwrap();
    f.distort = true;
    let q = f
        .drag(Handle::Resize([0, 0]), f.quad[0], [15., 12.], false, false)
        .unwrap();
    f.apply(&mut e, q).unwrap();
    let result = e.document.composite();
    f.apply(&mut e, q).unwrap();
    assert_eq!(e.document.composite(), result);
    assert!(Arc::ptr_eq(&original, &e.document.layers[0].pixels));
    let transform = e.document.layers[0].transform;
    assert!(f.apply(&mut e, [[0.; 2]; 4]).is_err());
    assert_eq!(e.document.layers[0].transform, transform);
    e.document.layers[0].locked = true;
    assert!(f.apply(&mut e, f.quad).is_err());
    assert_eq!(e.document.layers[0].transform, transform);
    assert!(
        f.drag(Handle::Resize([0, 0]), f.quad[0], [50., 50.], false, false)
            .is_err()
    );
}
#[test]
fn batch_handle_constraints_keep_opposite_or_center_and_angle_steps() {
    let d = pattern();
    let f = Frame::new(&d, vec![1]).unwrap();
    let q = f
        .drag(Handle::Resize([2, 2]), f.quad[2], [40., 38.], true, false)
        .unwrap();
    close(q[0], f.quad[0]);
    assert!(((q[1][0] - q[0][0]) / (q[3][1] - q[0][1]) - 20. / 16.).abs() < 0.001);
    let q = f
        .drag(Handle::Resize([2, 2]), f.quad[2], [40., 38.], true, true)
        .unwrap();
    close(m::point(m::unit_quad(q).unwrap(), [0.5; 2]), f.center());
    let c = f.center();
    let q = f
        .drag(
            Handle::Rotate,
            [c[0], c[1] - 10.],
            [c[0] + 10., c[1]],
            true,
            false,
        )
        .unwrap();
    close(m::point(m::unit_quad(q).unwrap(), [0.5; 2]), c);
    assert!((q[1][0] - q[0][0]).abs() < 0.002);
    let mut distort = f.clone();
    distort.distort = true;
    let q = distort
        .drag(Handle::Resize([1, 0]), [22., 14.], [27., 14.], false, false)
        .unwrap();
    close(q[0], [17., 14.]);
    close(q[1], [37., 14.]);
    close(q[2], f.quad[2]);
}
#[test]
fn crop_eight_handles_fixed_anchors_center_ratio_and_bounds() {
    let s = Selection::rectangle([20, 20], [80, 60]);
    assert_eq!(crate::crop_handles::handles(&s).len(), 8);
    for (handle, start) in crate::crop_handles::handles(&s) {
        let Handle::Resize(h) = handle else {
            unreachable!()
        };
        let changed = crate::crop_handles::drag(
            &s,
            handle,
            start,
            [start[0] + 3., start[1] + 4.],
            [100, 100],
            None,
            false,
        );
        for (i, value) in h.iter().enumerate() {
            if *value == 0 {
                assert_eq!(changed.max[i], s.max[i]);
            } else if *value == 2 {
                assert_eq!(changed.min[i], s.min[i]);
            } else {
                assert_eq!(changed.min[i], s.min[i]);
                assert_eq!(changed.max[i], s.max[i]);
            }
        }
    }
    let centered = crate::crop_handles::drag(
        &s,
        Handle::Resize([0, 0]),
        [20., 20.],
        [-40., -20.],
        [100, 100],
        None,
        true,
    );
    assert_eq!(centered.min[0] + centered.max[0], 100);
    assert_eq!(centered.min[1] + centered.max[1], 80);
    assert!(centered.min[0] >= 0 && centered.max[1] <= 100);
    let ratio = crate::crop_handles::drag(
        &s,
        Handle::Resize([2, 2]),
        [80., 60.],
        [130., 130.],
        [100, 100],
        Some(1.5),
        false,
    );
    assert_eq!(ratio.min, s.min);
    assert!(
        ((ratio.max[0] - ratio.min[0]) as f32 / (ratio.max[1] - ratio.min[1]) as f32 - 1.5).abs()
            < 0.03
    );
    let moved = crate::crop_handles::drag(
        &s,
        Handle::Move,
        [0., 0.],
        [100., -100.],
        [100, 100],
        None,
        false,
    );
    assert_eq!(moved, Selection::rectangle([40, 0], [100, 40]));
}
#[test]
fn warp_sampling_project_roundtrip_crop_and_invalid_save_preserve_originals() {
    let d = pattern();
    let mut e = Editor::new(d, true);
    let mut f = Frame::new(&e.document, vec![1]).unwrap();
    f.distort = true;
    f.sampling = Sampling::Bicubic;
    let q = [[14., 12.], [32., 14.], [30., 32.], [10., 28.]];
    f.apply(&mut e, q).unwrap();
    e.finish();
    let before = e.document.clone();
    let path = folder().join("원근-변형.rphoto");
    crate::files::save_project(&before, &path).unwrap();
    let loaded = crate::files::load_project(&path).unwrap();
    assert_eq!(loaded.layers[0].transform, before.layers[0].transform);
    assert_eq!(loaded.composite(), before.composite());
    loaded
        .composite()
        .save(folder().join("원근-변형.png"))
        .unwrap();
    let original = e.document.layers[0].pixels.clone();
    let mut bad = before.clone();
    bad.layers[0].transform.warp = Some([1., 0., 0., 0., 1., 0., -0.1, 0., 1.]);
    let bytes = std::fs::read(&path).unwrap();
    assert!(crate::files::save_project(&bad, &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    e.edit("자르기", |doc| {
        crate::editing::crop(doc, &Selection::rectangle([8, 8], [40, 40])).unwrap()
    });
    assert_eq!(
        e.document.composite(),
        image::imageops::crop_imm(&before.composite(), 8, 8, 32, 32).to_image()
    );
    assert!(Arc::ptr_eq(&original, &e.document.layers[0].pixels));
    e.undo();
    assert_eq!(e.document.composite(), before.composite());
}
#[test]
fn warped_hdr_rasterization_preserves_masked_composite_and_precision() {
    let mut d = crate::precision::document(
        "HDR".into(),
        image::DynamicImage::ImageRgba32F(Rgba32FImage::from_fn(12, 10, |x, _| {
            Rgba([2.75, -0.25, x as f32 / 12., 1.])
        })),
    )
    .unwrap();
    crate::editing::resize(&mut d, 40, 40, false).unwrap();
    d.layers[0].offset = [8, 9];
    d.layers[0].mask = Some(Arc::new(GrayImage::from_fn(12, 10, |x, _| {
        Luma([if x < 6 { 255 } else { 100 }])
    })));
    let mut e = Editor::new(d, true);
    let mut f = Frame::new(&e.document, vec![1]).unwrap();
    f.sampling = Sampling::Bicubic;
    f.apply(&mut e, [[9., 8.], [23., 10.], [20., 24.], [7., 20.]])
        .unwrap();
    e.finish();
    let before = crate::precision::composite(&e.document);
    e.edit("래스터화", |d| {
        crate::editing::rasterize(&mut d.layers[0]).unwrap()
    });
    assert_eq!(e.document.layers[0].bit_depth, 32);
    assert!(e.document.layers[0].mask.is_some());
    let actual = crate::precision::composite(&e.document);
    for (a, b) in actual.pixels().zip(before.pixels()) {
        for c in 0..3 {
            assert!((a[c] * a[3] - b[c] * b[3]).abs() < 0.012, "{a:?} {b:?}");
        }
        assert!((a[3] - b[3]).abs() < 0.005);
    }
    assert!(actual.pixels().any(|p| p[0] > 2. && p[1] < 0.));
    e.undo();
    assert!(e.document.layers[0].transform.warp.is_some());
}
#[test]
fn transformed_group_mask_psd_psb_export_preserves_children_and_appearance() {
    let mut d = pattern();
    d.group_active().unwrap();
    let id = d.active;
    d.active_layer_mut().unwrap().mask = Some(Arc::new(GrayImage::from_fn(64, 64, |x, _| {
        Luma([if x < 22 { 255 } else { 90 }])
    })));
    let mut e = Editor::new(d, true);
    let f = Frame::new(&e.document, vec![id]).unwrap();
    let world = [1.2, 0.1, 1., 0., 1.1, 2., 0., 0., 1.];
    f.apply(&mut e, f.mapped(world)).unwrap();
    e.finish();
    let before = e.document.composite();
    for ext in ["psd", "psb"] {
        let path = folder().join(format!("그룹-변형.{ext}"));
        crate::files::save_photoshop(&e.document, &path).unwrap();
        let loaded = crate::files::open(&path).unwrap().document;
        assert_eq!(loaded.layers.iter().filter(|l| l.group).count(), 1);
        assert_eq!(loaded.layers.len(), e.document.layers.len());
        let actual = loaded.composite();
        for (a, b) in actual.pixels().zip(before.pixels()) {
            for c in 0..4 {
                assert!(a[c].abs_diff(b[c]) <= 2, "{ext}: {a:?} {b:?}");
            }
        }
    }
}
#[test]
fn affine_svg_keeps_matrix_and_perspective_svg_reports_pixel_fallback() {
    let mut d = pattern();
    d.layers[0].transform = d.layers[0]
        .transform
        .world(
            [1., 0.3, 0., 0.2, 1., 0., 0., 0., 1.],
            d.layers[0].offset,
            [20, 16],
        )
        .unwrap();
    let source = crate::svg_export::source(&d).unwrap();
    assert!(source.contains("matrix(1 0.2 0.3 1"));
    assert!(crate::svg_export::report(&d).contains("사진 레이어"));
    let mut e = Editor::new(d, true);
    let f = Frame::new(&e.document, vec![1]).unwrap();
    let mut quad = f.quad;
    quad[0][0] += 3.;
    f.apply(&mut e, quad).unwrap();
    assert!(crate::svg_export::report(&e.document).contains("원근 왜곡"));
    let source = crate::svg_export::source(&e.document).unwrap();
    assert!(!source.contains("matrix("));
    let rendered = crate::vector::render_svg(&source).unwrap();
    assert_eq!(rendered, e.document.composite());
    crate::svg_export::save(&e.document, &folder().join("원근-외관.svg")).unwrap();
}
#[test]
fn anisotropic_document_resize_and_flip_keep_projective_geometry() {
    let mut d = pattern();
    d.layers[0].transform.angle = 28.;
    let mut e = Editor::new(d, true);
    let f = Frame::new(&e.document, vec![1]).unwrap();
    let mut q = f.quad;
    q[0][0] += 3.;
    f.apply(&mut e, q).unwrap();
    e.finish();
    let before = e.document.clone();
    let size = [20, 16];
    let p = [5., 7.];
    let world = before.layers[0]
        .transform
        .to_document(p, before.layers[0].offset, size);
    crate::editing::resize(&mut e.document, 128, 96, true).unwrap();
    close(
        e.document.layers[0].transform.to_document(
            [10., 10.5],
            e.document.layers[0].offset,
            [40, 24],
        ),
        [world[0] * 2., world[1] * 1.5],
    );
    let image = e.document.composite();
    crate::editing::flip_canvas(&mut e.document, true);
    let flipped = e.document.composite();
    for (a, b) in flipped
        .pixels()
        .zip(image::imageops::flip_horizontal(&image).pixels())
    {
        for c in 0..4 {
            assert!(a[c].abs_diff(b[c]) <= 1, "{a:?} {b:?}");
        }
    }
}
#[test]
fn painting_warped_layer_visits_exact_world_brush_footprint() {
    let mut d = Document::new(30, 24, true).unwrap();
    d.layers[0].transform.warp = Some([1., 0.15, 0., 0., 1., 0., 0.015, 0., 1.]);
    let t = d.layers[0].transform;
    let center = t.to_document([20.5, 12.5], [0, 0], [30, 24]);
    let brush = crate::document::Brush {
        radius: 3.,
        hardness: 0.9,
        opacity: 1.,
        color: [255, 0, 0, 255],
        erase: false,
        mask: false,
    };
    crate::document::paint_line(&mut d.layers[0], center, center, &brush, None);
    for (x, y, p) in d.layers[0].pixels.enumerate_pixels() {
        let world = t.to_document([x as f32 + 0.5, y as f32 + 0.5], [0, 0], [30, 24]);
        let distance = (world[0] - center[0]).hypot(world[1] - center[1]);
        if distance < 2.7 {
            assert_eq!(p[3], 255);
        }
        if distance >= 3. {
            assert_eq!(p[3], 0);
        }
    }
}

#[test]
fn projective_source_expansion_keeps_every_point_not_only_origin() {
    let t = Transform {
        warp: Some([1., 0.1, 0., 0.05, 1., 0., 0.003, 0.001, 1.]),
        angle: 21.,
        scale: [-1.2, 0.8],
        ..Default::default()
    };
    let new = t.rebased([10, 12], [20, 16], [30, 26], [-5., -5.]).unwrap();
    for p in [[0., 0.], [12., 6.], [20., 16.]] {
        close(
            t.to_document(p, [10, 12], [20, 16]),
            new.to_document([p[0] + 5., p[1] + 5.], [10, 12], [30, 26]),
        );
    }
}

#[test]
fn perspective_effect_padding_psd_and_regional_preview_match_full_render() {
    let mut d = pattern();
    let l = &mut d.layers[0];
    l.effects.shadow.enabled = true;
    l.effects.shadow.radius = 1.;
    l.effects.shadow.offset = [1, 1];
    l.mask = Some(Arc::new(GrayImage::from_pixel(20, 16, Luma([220]))));
    let mut e = Editor::new(d, true);
    let f = Frame::new(&e.document, vec![1]).unwrap();
    let mut q = f.quad;
    q[0][0] += 2.;
    f.apply(&mut e, q).unwrap();
    e.finish();
    let expected = e.document.composite();
    for region in [
        crate::document::PixelRegion {
            min: [8, 9],
            max: [35, 35],
        },
        crate::document::PixelRegion {
            min: [0, 0],
            max: [12, 12],
        },
    ] {
        let [w, h] = region.size();
        assert_eq!(
            e.document.composite_region(region),
            image::imageops::crop_imm(&expected, region.min[0], region.min[1], w, h).to_image()
        );
    }
    let path = folder().join("왜곡-그림자.psd");
    crate::files::save_photoshop(&e.document, &path).unwrap();
    let actual = crate::files::open(&path).unwrap().document.composite();
    for (a, b) in actual.pixels().zip(expected.pixels()) {
        for c in 0..4 {
            assert!(a[c].abs_diff(b[c]) <= 2, "{a:?} {b:?}");
        }
    }
    expected.save(folder().join("왜곡-그림자.png")).unwrap();
}
#[test]
fn transform_reopen_retains_sampling_and_tiny_geometry_has_handles() {
    let mut d = Document::new(1, 1, true).unwrap();
    d.layers[0].transform.scale = [0.01; 2];
    d.layers[0].transform.sampling = Sampling::Bicubic;
    let f = Frame::new(&d, vec![1]).unwrap();
    assert_eq!(f.sampling, Sampling::Bicubic);
    assert_eq!(f.handles().len(), 8);
}

#[test]
fn ordinary_eight_bit_photos_use_selected_interpolation_in_full_and_partial_preview() {
    let mut d = Document::from_image(
        "8비트".into(),
        RgbaImage::from_fn(2, 1, |x, _| {
            Rgba([
                if x == 0 { 0 } else { 255 },
                if x == 0 { 0 } else { 255 },
                if x == 0 { 0 } else { 255 },
                255,
            ])
        }),
    );
    crate::editing::resize(&mut d, 3, 1, false).unwrap();
    d.layers[0].transform.translation = [0.5, 0.];
    assert_eq!(d.composite().get_pixel(1, 0).0, [255; 4]);
    for mode in [Sampling::Bilinear, Sampling::Bicubic] {
        d.layers[0].transform.sampling = mode;
        let rendered = d.composite();
        assert_eq!(rendered.get_pixel(1, 0).0, [128, 128, 128, 255]);
        let region = crate::document::PixelRegion {
            min: [1, 0],
            max: [2, 1],
        };
        assert_eq!(
            d.composite_region(region).get_pixel(0, 0),
            rendered.get_pixel(1, 0)
        );
    }
}
