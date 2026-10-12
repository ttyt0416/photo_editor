fn model_frame(
    panel: &mut Panel,
    ctx: &egui::Context,
    time: &mut f64,
    size: egui::Vec2,
    fg: &mut [u8; 4],
    bg: &mut [u8; 4],
    events: Vec<egui::Event>,
) -> Vec<(String, egui::Rect)> {
    *time += 0.1;
    let mut out = ctx.run_ui(
        egui::RawInput {
            time: Some(*time),
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |_| panel.ui(ctx, fg, bg),
    );
    let labels = out
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                let rect = egui::Rect::from_min_size(t.pos, t.galley.size());
                s.clip_rect
                    .intersects(rect)
                    .then(|| (t.galley.job.text.clone(), rect.intersect(s.clip_rect)))
            } else {
                None
            }
        })
        .collect();
    out.textures_delta.clear();
    labels
}
fn model_pointer(
    panel: &mut Panel,
    ctx: &egui::Context,
    time: &mut f64,
    size: egui::Vec2,
    fg: &mut [u8; 4],
    bg: &mut [u8; 4],
    control: (&str, f32),
) {
    let (label, distance) = control;
    model_frame(panel, ctx, time, size, fg, bg, vec![]);
    let labels = model_frame(panel, ctx, time, size, fg, bg, vec![]);
    let at = labels
        .iter()
        .find(|(s, _)| s == label)
        .unwrap_or_else(|| panic!("missing {label}: {labels:?}"))
        .1
        .center();
    model_frame(
        panel,
        ctx,
        time,
        size,
        fg,
        bg,
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
    if distance != 0. {
        model_frame(
            panel,
            ctx,
            time,
            size,
            fg,
            bg,
            vec![egui::Event::PointerMoved(at + egui::vec2(distance, 0.))],
        );
    }
    model_frame(
        panel,
        ctx,
        time,
        size,
        fg,
        bg,
        vec![egui::Event::PointerButton {
            pos: at + egui::vec2(distance, 0.),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
}
#[test]
fn color_model_modal_switch_gray_hue_components_alpha_targets_external_sync_and_compact_close() {
    for model in [Model::Hsv, Model::Hsl] {
        for dpi in [1., 2.] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(dpi);
            let mut panel = Panel::default();
            let mut fg = [85, 85, 85, 83];
            let mut bg = [20, 40, 60, 70];
            let mut time = 0.;
            let size = egui::vec2(360., 480.);
            panel.show(fg, bg);
            for _ in 0..5 {
                model_frame(&mut panel, &ctx, &mut time, size, &mut fg, &mut bg, vec![]);
            }
            model_pointer(
                &mut panel,
                &ctx,
                &mut time,
                size,
                &mut fg,
                &mut bg,
                (model.label(), 0.),
            );
            assert_eq!(panel.library.model, model);
            assert_eq!(fg, [85, 85, 85, 83]);
            model_pointer(
                &mut panel,
                &ctx,
                &mut time,
                size,
                &mut fg,
                &mut bg,
                ("색조 °", 100.),
            );
            assert!((panel.model_values[0] - 180.).abs() < 1e-4);
            assert_eq!(fg, [85, 85, 85, 83]);
            model_frame(&mut panel, &ctx, &mut time, size, &mut fg, &mut bg, vec![]);
            assert!((panel.model_values[0] - 180.).abs() < 1e-4);
            model_pointer(
                &mut panel,
                &ctx,
                &mut time,
                size,
                &mut fg,
                &mut bg,
                ("채도 %", 200.),
            );
            let expected = if model == Model::Hsv {
                [0, 85, 85, 83]
            } else {
                [0, 170, 170, 83]
            };
            assert_eq!(fg, expected);
            assert_eq!(bg, [20, 40, 60, 70]);
            assert_eq!(panel.value, hex(fg));
            model_pointer(
                &mut panel,
                &ctx,
                &mut time,
                size,
                &mut fg,
                &mut bg,
                ("배경색", 0.),
            );
            assert_eq!(fg, expected);
            assert_eq!(bg, [20, 40, 60, 70]);
            model_pointer(
                &mut panel,
                &ctx,
                &mut time,
                size,
                &mut fg,
                &mut bg,
                ("색조 °", 20.),
            );
            assert_ne!(bg, [20, 40, 60, 70]);
            assert_eq!(bg[3], 70);
            assert_eq!(fg, expected);
            model_pointer(
                &mut panel,
                &ctx,
                &mut time,
                size,
                &mut fg,
                &mut bg,
                ("전경색", 0.),
            );
            fg = [200, 10, 50, 0];
            model_frame(&mut panel, &ctx, &mut time, size, &mut fg, &mut bg, vec![]);
            assert_eq!(panel.model_values, model.components(fg));
            assert_eq!(panel.value, hex(fg));
            model_pointer(
                &mut panel,
                &ctx,
                &mut time,
                size,
                &mut fg,
                &mut bg,
                ("RGB", 0.),
            );
            assert_eq!(fg, [200, 10, 50, 0]);
            let labels = model_frame(&mut panel, &ctx, &mut time, size, &mut fg, &mut bg, vec![]);
            let close = labels.iter().find(|(s, _)| s == "닫기").unwrap().1;
            assert!(
                close.min.x >= 0.
                    && close.max.x <= size.x
                    && close.min.y >= 0.
                    && close.max.y <= size.y
            );
            model_pointer(
                &mut panel,
                &ctx,
                &mut time,
                size,
                &mut fg,
                &mut bg,
                ("닫기", 0.),
            );
            assert!(!panel.open);
        }
    }
}
#[test]
fn color_model_library_legacy_default_current_mode_palette_recent_and_bad_file_preservation() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/color-models");
    std::fs::create_dir_all(&root).unwrap();
    let mut old = serde_json::to_value(Library::default()).unwrap();
    old.as_object_mut().unwrap().remove("model");
    let legacy = root.join("legacy.json");
    std::fs::write(&legacy, serde_json::to_vec(&old).unwrap()).unwrap();
    let loaded = Panel::load(legacy).unwrap();
    assert_eq!(loaded.library.model, Model::Rgb);
    let mut panel = Panel {
        path: Some(root.join("library.json")),
        ..Default::default()
    };
    panel.library.model = Model::Hsl;
    panel.library.palette.push([0, 170, 170, 83]);
    panel
        .observe([0, 170, 170, 83], [20, 40, 60, 70], false)
        .unwrap();
    assert_eq!(panel.saved.model, Model::Rgb);
    panel
        .observe([0, 170, 170, 83], [20, 40, 60, 70], true)
        .unwrap();
    let loaded = Panel::load(root.join("library.json")).unwrap();
    assert!(loaded.library == panel.library);
    assert_eq!(loaded.library.model, Model::Hsl);
    assert!(loaded.library.recent.contains(&[0, 170, 170, 83]));
    let mut bad = serde_json::to_value(Library::default()).unwrap();
    bad["model"] = serde_json::json!("unrecognized");
    let bytes = serde_json::to_vec(&bad).unwrap();
    let path = root.join("invalid-model.json");
    std::fs::write(&path, &bytes).unwrap();
    assert!(Panel::load(path.clone()).is_err());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
