use super::*;
fn fixture() -> crate::document::Document {
    crate::files::open(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/raw/linear-32-o1.dng"),
    )
    .unwrap()
    .document
}
fn frame(
    panel: &mut Panel,
    ctx: &egui::Context,
    time: &mut f64,
    events: Vec<egui::Event>,
    controls: bool,
) -> Vec<(String, egui::Rect)> {
    *time += 0.05;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760., 1100.),
            )),
            time: Some(*time),
            events,
            ..Default::default()
        },
        |root| {
            if controls {
                egui::CentralPanel::default().show(root, |ui| panel.advanced_ui(ui));
            } else {
                let _ = panel.ui(ctx);
            }
        },
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
    labels
}
fn click(panel: &mut Panel, ctx: &egui::Context, time: &mut f64, label: &str) {
    frame(panel, ctx, time, vec![], true);
    let at = frame(panel, ctx, time, vec![], true)
        .iter()
        .find(|(s, _)| s == label)
        .unwrap_or_else(|| panic!("missing {label}"))
        .1
        .center();
    for pressed in [true, false] {
        frame(
            panel,
            ctx,
            time,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
            true,
        );
    }
    frame(panel, ctx, time, vec![], true);
}
#[test]
fn raw_advanced_sections_curve_channel_reset_scope_and_sensor_picker_controls() {
    let doc = fixture();
    let ctx = egui::Context::default();
    let mut panel = Panel::for_layer(1, 0, &doc.layers[0], &ctx).unwrap();
    let mut time = 0.;
    let labels = frame(&mut panel, &ctx, &mut time, vec![], true);
    for label in [
        "톤·곡선",
        "톤별 대비",
        "색상 믹서",
        "컬러 그레이딩",
        "노이즈 감소",
        "세부 선명화",
        "렌즈 프로파일",
        "수동 렌즈 보정",
        "기하 보정",
    ] {
        assert!(labels.iter().any(|(s, _)| s == label), "{label}");
    }
    click(&mut panel, &ctx, &mut time, "톤·곡선");
    click(&mut panel, &ctx, &mut time, "R");
    assert_eq!(panel.curve_channel, 1);
    panel.adjustments.curves[1].points = vec![[0., 0.], [0.4, 0.6], [1., 1.]];
    click(&mut panel, &ctx, &mut time, "커브 초기화");
    assert_eq!(panel.adjustments.curves[1], Default::default());
    let selection = crate::document::Selection {
        min: [1, 1],
        max: [4, 4],
        inverted: true,
        mask: None,
    };
    panel.adjustments.scope = Some(
        crate::adjustment_scope::Scope::capture(
            &doc.layers[0],
            &selection,
            [doc.width, doc.height],
        )
        .unwrap(),
    );
    let scope = panel.adjustments.scope.clone();
    panel.adjustments.color_mixer.hue[0] = 20.;
    panel.adjustments.denoise.color = 0.4;
    click(&mut panel, &ctx, &mut time, "현상 보정 초기화");
    assert_eq!(panel.adjustments.scope, scope);
    assert_eq!(panel.adjustments.color_mixer, Default::default());
    assert_eq!(panel.adjustments.denoise, Default::default());
    panel.picker.enabled = true;
    assert!(
        frame(&mut panel, &ctx, &mut time, vec![], true)
            .iter()
            .any(|(s, _)| s.contains("보정 전 현상 이미지"))
    );
}
#[test]
fn raw_advanced_worker_latest_adjustments_keeps_edited_pixels_and_picker_uses_raw() {
    let mut doc = fixture();
    let ctx = egui::Context::default();
    let mut pixels = (*doc.layers[0].precision.as_ref().unwrap())
        .as_ref()
        .clone();
    pixels.put_pixel(1, 1, image::Rgba([2., -0.25, 0.4, 0.7]));
    doc.layers[0].precision = Some(Arc::new(pixels));
    let original = doc.layers[0].precision.clone().unwrap();
    let source = doc.layers[0].raw_source.clone().unwrap();
    let mut panel = Panel::for_layer(1, 0, &doc.layers[0], &ctx).unwrap();
    let mut time = 0.;
    panel.adjustments.color_mixer.hue[0] = 15.;
    panel.adjustments.color_grading.tones[1].saturation = 0.2;
    panel.adjustments.denoise.color = 0.1;
    panel.adjustments.detail_sharpen.amount = 0.3;
    panel.adjustments.lens.zoom = 1.03;
    panel.invalidate_changed_settings();
    let old_key = panel.key();
    let (sender, receiver) = mpsc::channel();
    panel.job = Some((old_key.clone(), receiver, false));
    panel.adjustments.curves[0].points = vec![[0., 0.], [0.5, 0.6], [1., 1.]];
    sender
        .send(prepare(original.clone(), &old_key))
        .ok()
        .unwrap();
    frame(&mut panel, &ctx, &mut time, vec![], false);
    assert!(panel.result.is_none() && !panel.ready());
    panel.changed_at = Instant::now() - Duration::from_millis(250);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !panel.ready() && Instant::now() < deadline {
        frame(&mut panel, &ctx, &mut time, vec![], false);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(panel.ready());
    let prepared = panel.result.as_ref().unwrap().1.as_ref().unwrap();
    assert!(Arc::ptr_eq(&prepared.pixels, &original));
    assert_eq!(
        prepared.shown.as_deref().unwrap(),
        &panel.adjustments.apply_float(&original)
    );
    assert_eq!(panel.source.bytes, source.bytes);
    assert_eq!(
        doc.layers[0].precision.as_ref().unwrap().get_pixel(1, 1).0,
        [2., -0.25, 0.4, 0.7]
    );
    let mut picking = panel.key();
    picking.picking = true;
    let preview = prepare(original.clone(), &picking).unwrap();
    assert!(preview.shown.is_none());
    assert!(Arc::ptr_eq(&preview.pixels, &original));
    let mut bad = panel.key();
    bad.adjustments.lens.zoom = f32::NAN;
    assert!(prepare(original.clone(), &bad).is_err());
}

#[test]
fn raw_geometry_controls_reset_preview_key_and_sensor_bypass() {
    let doc = fixture();
    let ctx = egui::Context::default();
    let mut panel = Panel::for_layer(1, 0, &doc.layers[0], &ctx).unwrap();
    let mut time = 0.;
    click(&mut panel, &ctx, &mut time, "기하 보정");
    let labels = frame(&mut panel, &ctx, &mut time, vec![], true);
    for label in [
        "회전 °",
        "수평 원근",
        "수직 원근",
        "기하 확대",
        "가로세로 비율",
        "수평 이동",
        "수직 이동",
    ] {
        assert!(labels.iter().any(|(s, _)| s == label), "{label}");
    }
    let old = panel.key();
    panel.adjustments.geometry.angle = 13.;
    panel.adjustments.geometry.perspective = [0.2, -0.1];
    panel.invalidate_changed_settings();
    assert!(old != panel.key());
    assert!(!panel.ready());
    let original = panel.original_pixels.clone();
    let p = prepare(original.clone(), &panel.key()).unwrap();
    assert_eq!(
        p.shown.as_deref().unwrap(),
        &panel.adjustments.apply_float(&original)
    );
    assert!(Arc::ptr_eq(&p.pixels, &original));
    let mut picking = panel.key();
    picking.picking = true;
    assert!(prepare(original, &picking).unwrap().shown.is_none());
    click(&mut panel, &ctx, &mut time, "기하 초기화");
    assert!(panel.adjustments.geometry.is_default());
}
