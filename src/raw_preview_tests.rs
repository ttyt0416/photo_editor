use super::*;
use std::path::Path;
fn output() -> std::path::PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/raw-preview");
    std::fs::create_dir_all(&p).unwrap();
    p
}
#[test]
fn raw_preview_bounded_alpha_area_and_exact_detail_preserve_float_original_independent_pixels() {
    let mut manifest = Vec::new();
    for (index, (width, height)) in [
        (1, 1),
        (127, 31),
        (513, 1),
        (1, 257),
        (520, 270),
        (640, 480),
    ]
    .into_iter()
    .enumerate()
    {
        let source = Rgba32FImage::from_fn(width, height, |x, y| {
            image::Rgba([
                if (x + y) % 7 == 0 {
                    3.
                } else {
                    (x % 31) as f32 / 30.
                },
                if (x + y) % 11 == 0 {
                    -0.4
                } else {
                    (y % 23) as f32 / 22.
                },
                ((x * 13 + y * 19) % 41) as f32 / 40.,
                [0., 0.1, 0.5, 1.][((x + y) % 4) as usize],
            ])
        });
        let original = source
            .as_raw()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>();
        let fit = thumbnail(&source);
        assert!(fit.width() <= 512 && fit.height() <= 256);
        let name = format!("case-{index}");
        fit.save(output().join(format!("{name}-fit.png"))).unwrap();
        std::fs::write(
            output().join(format!("{name}.rgba32.le")),
            source
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let focuses = [[0., 0.], [0.5, 0.5], [1., 1.], [0.2, 0.8]];
        for (i, focus) in focuses.into_iter().enumerate() {
            let p = detail(&source, focus);
            assert_eq!(p.width(), width.min(160));
            assert_eq!(p.height(), height.min(160));
            p.save(output().join(format!("{name}-detail-{i}.png")))
                .unwrap();
        }
        assert_eq!(
            source
                .as_raw()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            original
        );
        manifest.push(serde_json::json!({"name":name,"size":[width,height],"focuses":focuses}));
    }
    std::fs::write(
        output().join("manifest.json"),
        serde_json::to_vec(&manifest).unwrap(),
    )
    .unwrap();
    let transparent = Rgba32FImage::from_fn(1024, 1, |x, _| {
        if x % 2 == 0 {
            image::Rgba([1., 0., 0., 0.])
        } else {
            image::Rgba([0., 0., 1., 1.])
        }
    });
    assert!(
        thumbnail(&transparent)
            .pixels()
            .all(|p| p.0 == [0, 0, 255, 128])
    );
}
#[test]
fn raw_preview_pointer_shared_focus_physical_pixels_dpi_latest_clear_restore_and_source_identity() {
    use egui::{Event, Pos2};
    let before = Arc::new(Rgba32FImage::from_fn(320, 240, |x, y| {
        image::Rgba([x as f32 / 319., y as f32 / 239., 0.25, 0.5])
    }));
    let after = Arc::new(Rgba32FImage::from_fn(320, 240, |x, y| {
        image::Rgba([x as f32 / 159., y as f32 / 119., 0.5, 0.5])
    }));
    for dpi in [1., 2.] {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(dpi);
        let mut view = View::new(before.clone());
        view.before_ready(&ctx, &thumbnail(&before));
        view.after_ready(&ctx, after.clone(), &thumbnail(&after));
        let mut time = 0.;
        fn frame(
            ctx: &egui::Context,
            view: &mut View,
            time: &mut f64,
            events: Vec<Event>,
        ) -> Vec<(egui::TextureId, Rect)> {
            *time += 0.05;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(440., 620.))),
                    time: Some(*time),
                    events,
                    ..Default::default()
                },
                |ui| view.ui(ui),
            );
            let meshes = out
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Mesh(m) = &s.shape {
                        Some((m.texture_id, m.calc_bounds()))
                    } else {
                        None
                    }
                })
                .collect();
            out.textures_delta.clear();
            meshes
        }
        frame(&ctx, &mut view, &mut time, vec![]);
        let meshes = frame(&ctx, &mut view, &mut time, vec![]);
        let id = view.before_fit.as_ref().unwrap().id();
        let rect = meshes.iter().find(|(i, _)| *i == id).unwrap().1;
        let point = rect.min + rect.size() * Vec2::new(0.8, 0.3);
        frame(
            &ctx,
            &mut view,
            &mut time,
            vec![
                Event::PointerMoved(point),
                Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        frame(
            &ctx,
            &mut view,
            &mut time,
            vec![Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        assert!((view.focus[0] - 0.8).abs() < 1e-5);
        assert!((view.focus[1] - 0.3).abs() < 1e-5);
        let meshes = frame(&ctx, &mut view, &mut time, vec![]);
        for t in view.detail_textures.as_ref().unwrap() {
            let rect = meshes.iter().find(|(i, _)| *i == t.id()).unwrap().1;
            assert!((rect.width() * ctx.pixels_per_point() - 160.).abs() < 1e-4);
            assert!((rect.height() * ctx.pixels_per_point() - 160.).abs() < 1e-4);
        }
        let focus = view.focus;
        view.clear_after();
        assert!(view.after.is_none() && view.after_fit.is_none() && view.detail_textures.is_none());
        assert!(view.before_fit.is_some());
        assert_eq!(view.focus, focus);
        view.restore_before();
        assert!(Arc::ptr_eq(view.after.as_ref().unwrap(), &before));
        assert_eq!(
            view.after_fit.as_ref().unwrap().id(),
            view.before_fit.as_ref().unwrap().id()
        );
        view.after_ready(&ctx, after.clone(), &thumbnail(&after));
        assert!(Arc::ptr_eq(view.after.as_ref().unwrap(), &after));
        assert_eq!(view.focus, focus);
        assert!(Arc::ptr_eq(&view.before, &before));
    }
}

#[test]
fn raw_preview_sized_fit_larger_picture_pointer_sensor_coordinate_and_physical_detail() {
    let original = Arc::new(Rgba32FImage::from_pixel(
        768,
        768,
        image::Rgba([0.2, 0.4, 0.6, 1.]),
    ));
    for dpi in [1., 2.] {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(dpi);
        let mut view = View::new(original.clone());
        view.before_ready(&ctx, &thumbnail(&original));
        view.after_ready(&ctx, original.clone(), &thumbnail(&original));
        let mut time = 0.;
        fn frame(
            ctx: &egui::Context,
            view: &mut View,
            time: &mut f64,
            height: f32,
            events: Vec<egui::Event>,
        ) -> (Vec<(egui::TextureId, Rect)>, Option<[f32; 2]>) {
            *time += 0.05;
            let mut pick = None;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(1000., 1000.),
                    )),
                    time: Some(*time),
                    events,
                    ..Default::default()
                },
                |ui| pick = view.ui_pick_sized(ui, true, height),
            );
            let meshes = out
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Mesh(m) = &s.shape {
                        Some((m.texture_id, m.calc_bounds()))
                    } else {
                        None
                    }
                })
                .collect();
            out.textures_delta.clear();
            (meshes, pick)
        }
        frame(&ctx, &mut view, &mut time, 180., vec![]);
        let small = frame(&ctx, &mut view, &mut time, 180., vec![]).0;
        let id = view.after_fit.as_ref().unwrap().id();
        let small = small.iter().find(|(i, _)| *i == id).unwrap().1;
        frame(&ctx, &mut view, &mut time, 240., vec![]);
        let large = frame(&ctx, &mut view, &mut time, 240., vec![]).0;
        let large = large.iter().find(|(i, _)| *i == id).unwrap().1;
        assert!((small.height() - 180.).abs() < 0.01);
        assert!((large.height() - 240.).abs() < 0.01);
        let at = large.min + large.size() * Vec2::new(0.75, 0.25);
        frame(
            &ctx,
            &mut view,
            &mut time,
            240.,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        let (meshes, pick) = frame(
            &ctx,
            &mut view,
            &mut time,
            240.,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        let pick = pick.unwrap();
        assert!((pick[0] - 0.75).abs() < 1e-6 && (pick[1] - 0.25).abs() < 1e-6);
        let detail_id = view.detail_textures.as_ref().unwrap()[0].id();
        let rect = meshes.iter().find(|(i, _)| *i == detail_id).unwrap().1;
        assert!((rect.width() * dpi - 160.).abs() < 0.01);
        assert_eq!(view.before.as_ref(), original.as_ref());
    }
}
