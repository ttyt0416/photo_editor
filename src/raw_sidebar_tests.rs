use super::*;
#[test]
fn raw_sidebar_wide_compact_short_window_dpi_transitions_footer_and_cancel_keep_state() {
    let doc = crate::files::open(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/raw/linear-32-o1.dng"),
    )
    .unwrap()
    .document;
    for dpi in [1., 2.] {
        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(dpi);
        let mut panel = Panel::for_layer(1, 0, &doc.layers[0], &ctx).unwrap();
        let mut time = 0.;
        let source = panel.source.clone();
        let settings = panel.settings.clone();
        panel.adjustments.color_mixer.hue[0] = 20.;
        let adjustments = panel.adjustments.clone();
        fn frame(
            panel: &mut Panel,
            ctx: &egui::Context,
            time: &mut f64,
            size: egui::Vec2,
            events: Vec<egui::Event>,
        ) -> (Vec<(String, egui::Rect, egui::Rect)>, Option<Action>) {
            *time += 0.05;
            let mut action = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    time: Some(*time),
                    events,
                    ..Default::default()
                },
                |_| action = panel.ui(ctx),
            );
            let labels = output
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        Some((
                            t.galley.job.text.clone(),
                            egui::Rect::from_min_size(t.pos, t.galley.size()),
                            s.clip_rect,
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            output.textures_delta.clear();
            (labels, action)
        }
        for size in [
            egui::vec2(1280., 900.),
            egui::vec2(360., 480.),
            egui::vec2(1280., 480.),
            egui::vec2(880., 620.),
            egui::vec2(1440., 1000.),
        ] {
            let mut labels = vec![];
            for _ in 0..4 {
                labels = frame(&mut panel, &ctx, &mut time, size, vec![]).0;
                std::thread::sleep(Duration::from_millis(2));
            }
            for label in ["적용", "취소"] {
                let (_, rect, clip) = labels.iter().find(|(s, _, _)| s == label).unwrap();
                assert!(
                    clip.contains(rect.center()),
                    "{size:?}/{dpi}/{label}: {rect:?} {clip:?}"
                );
                assert!(
                    egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains_rect(*rect),
                    "{size:?}/{dpi}/{label}: {rect:?}"
                );
            }
            if size.x >= 1100. {
                let wb = labels
                    .iter()
                    .find(|(s, _, _)| s == "화이트 밸런스")
                    .unwrap()
                    .1;
                let before = labels.iter().find(|(s, _, _)| s == "원본").unwrap().1;
                assert!(
                    before.right() < wb.left(),
                    "preview and sidebar overlap: {before:?} {wb:?}"
                );
                assert!(labels.iter().any(|(s, _, _)| s == "현상 보정"));
            } else {
                assert!(!labels.iter().any(|(s, _, _)| s == "현상 보정"));
            }
            assert_eq!(panel.settings, settings);
            assert_eq!(panel.adjustments, adjustments);
            assert!(Arc::ptr_eq(&panel.source, &source));
        }
        let size = egui::vec2(1440., 1000.);
        let at = frame(&mut panel, &ctx, &mut time, size, vec![])
            .0
            .iter()
            .find(|(s, _, _)| s == "취소")
            .unwrap()
            .1
            .center();
        frame(
            &mut panel,
            &ctx,
            &mut time,
            size,
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
        let action = frame(
            &mut panel,
            &ctx,
            &mut time,
            size,
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        )
        .1;
        assert!(matches!(action, Some(Action::Cancel)));
        assert!(Arc::ptr_eq(
            doc.layers[0].raw_source.as_ref().unwrap(),
            &source
        ));
    }
}
