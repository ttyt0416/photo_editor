use super::*;
#[test]
fn raw_picker_preview_failure_stays_idle_until_explicit_retry_and_cancel() {
    let doc = crate::files::open(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/raw/linear-32-o1.dng"),
    )
    .unwrap()
    .document;
    let original = doc.layers[0].precision.clone().unwrap();
    let mut bad = (**doc.layers[0].raw_source.as_ref().unwrap()).clone();
    bad.bytes = Arc::new(vec![0]);
    let source = Arc::new(bad);
    let ctx = egui::Context::default();
    let mut panel = Panel::new(1, doc.active, 0, source.clone(), original.clone(), &ctx);
    panel.job = None;
    panel.picker.enabled = true;
    panel.displayed_key = panel.key();
    panel.result = Some((panel.key(), Err("검사용 미리보기 실패".into())));
    fn frame(
        panel: &mut Panel,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> (Vec<(String, egui::Rect)>, Option<Action>) {
        let mut action = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000., 950.),
                )),
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
                    ))
                } else {
                    None
                }
            })
            .collect();
        output.textures_delta.clear();
        (labels, action)
    }
    assert!(!panel.needs_pick_preview());
    let mut labels = Vec::new();
    for _ in 0..3 {
        labels = frame(&mut panel, &ctx, vec![]).0;
        assert!(panel.job.is_none());
    }
    let pos = labels
        .iter()
        .find(|(s, _)| s == "미리보기 다시 계산")
        .unwrap()
        .1
        .center();
    frame(&mut panel, &ctx, vec![egui::Event::PointerMoved(pos)]);
    for pressed in [true, false] {
        frame(
            &mut panel,
            &ctx,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Default::default(),
            }],
        );
    }
    assert!(panel.result.is_none() && panel.needs_pick_preview());
    let deadline = Instant::now() + Duration::from_secs(5);
    while panel.result.is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(4));
        frame(&mut panel, &ctx, vec![]);
    }
    assert!(panel.result.as_ref().unwrap().1.is_err());
    assert!(!panel.needs_pick_preview());
    for _ in 0..4 {
        frame(&mut panel, &ctx, vec![]);
        assert!(panel.job.is_none());
    }
    let (_, action) = frame(
        &mut panel,
        &ctx,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    assert!(matches!(action, Some(Action::Cancel)));
    assert!(Arc::ptr_eq(&panel.source, &source));
    assert!(Arc::ptr_eq(
        doc.layers[0].precision.as_ref().unwrap(),
        &original
    ));
}
