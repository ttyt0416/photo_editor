#[test]
fn raw_camera_calibration_camera_temperature_custom_dialog_cancel_latest_apply_undo() {
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Camera,
        "raw-calibration/raw-analog-raw-dual-linear-3c-32-ill17-21-analog-cc-match.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Temperature,
        "raw-calibration/raw-white-raw-dual-linear-4c-32-ill21-17-unit-xyA-cc-match.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Custom,
        "raw-calibration/raw-white-raw-dual-cfa-4c-16-ill17-21-analog-xyD65-cc-match.dng",
    );
}
#[test]
fn raw_dng_white_camera_temperature_custom_dialog_cancel_latest_apply_undo() {
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Camera,
        "raw-white/raw-dual-linear-3c-32-ill17-21-analog-xyD65.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Temperature,
        "raw-white/raw-dual-linear-4c-32-ill21-17-analog-xyA.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Custom,
        "raw-white/raw-dual-cfa-4c-16-ill17-21-analog-xyD65.dng",
    );
}
#[test]
fn raw_analog_camera_temperature_custom_dialog_cancel_latest_apply_undo() {
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Camera,
        "raw-analog/raw-dual-linear-3c-32-ill17-21-analog.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Temperature,
        "raw-analog/raw-dual-linear-4c-32-ill21-17-analog.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Custom,
        "raw-analog/raw-dual-cfa-4c-16-ill17-21-analog.dng",
    );
}
#[test]
fn raw_dual_custom_dialog_cancel_latest_fourth_gain_apply_undo_and_small_window() {
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Custom,
        "raw-dual/linear-3c-32-ill17-21.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Custom,
        "raw-dual/linear-4c-32-ill21-17.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Custom,
        "raw-dual/cfa-4c-16-ill17-21.dng",
    );
}
#[test]
fn raw_dual_camera_dialog_cancel_latest_apply_undo_and_small_window() {
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Camera,
        "raw-dual/linear-3c-32-ill17-21.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Camera,
        "raw-dual/linear-4c-32-ill21-17.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Camera,
        "raw-dual/cfa-4c-16-ill17-21.dng",
    );
}
#[test]
fn raw_dual_temperature_dialog_cancel_latest_apply_undo_and_small_window() {
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Temperature,
        "raw-dual/linear-3c-32-ill17-21.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Temperature,
        "raw-dual/linear-4c-32-ill17-21.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Temperature,
        "raw-dual/cfa-4c-16-ill17-21.dng",
    );
}
#[test]
fn raw_dialog_pointer_cancel_latest_apply_undo_small_window_and_modal_guards() {
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Custom,
        "raw/linear-32-o1.dng",
    );
}
#[test]
fn raw_temperature_dialog_mode_cancel_latest_kelvin_tint_apply_undo_and_small_window() {
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Temperature,
        "raw/linear-32-o1.dng",
    );
}
#[test]
fn raw_four_channel_dialog_custom_temperature_cfa_labels_cancel_apply_and_undo() {
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Custom,
        "raw-color/linear-4c-32-ill17.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Temperature,
        "raw-color/linear-4c-32-ill17.dng",
    );
    raw_dialog_flow(
        crate::raw_input::WhiteBalance::Custom,
        "raw-color/cfa-4c-16-ill213451.dng",
    );
}
fn raw_dialog_flow(mode: crate::raw_input::WhiteBalance, fixture: &str) {
    use crate::{commands::Command, raw_input::WhiteBalance};
    let (mut app, ctx, mut time) = input_app(Tool::Brush);
    assert!(app.command_reason(Command::RawDevelop).is_some());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(fixture);
    let mut doc = files::open(&path).unwrap().document;
    let layer = doc.active_layer_mut().unwrap();
    layer.offset = [3, 5];
    layer.mask = Some(Arc::new(image::GrayImage::from_pixel(
        12,
        10,
        image::Luma([177]),
    )));
    layer.transform.angle = 12.;
    let before = layer.precision.clone().unwrap();
    let mask = layer.mask.clone().unwrap();
    let raw = layer.raw_source.clone().unwrap();
    app.tabs[0].editor = Editor::new(doc, false);
    fn frame(
        app: &mut PhotoEditor,
        ctx: &egui::Context,
        time: &mut f64,
        events: Vec<egui::Event>,
        size: Vec2,
    ) -> Vec<(String, Rect)> {
        *time += 0.1;
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(*time),
                events,
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
                ..Default::default()
            },
            |_| {
                app.shortcuts(ctx);
                app.dialogs(ctx);
            },
        );
        let labels = output
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    Some((
                        t.galley.job.text.clone(),
                        Rect::from_min_size(t.pos, t.galley.size()),
                    ))
                } else {
                    None
                }
            })
            .collect();
        output.textures_delta.clear();
        labels
    }
    fn click(app: &mut PhotoEditor, ctx: &egui::Context, time: &mut f64, at: Pos2) {
        let size = Vec2::new(880., 620.);
        frame(app, ctx, time, vec![egui::Event::PointerMoved(at)], size);
        frame(
            app,
            ctx,
            time,
            vec![
                egui::Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                },
            ],
            size,
        );
    }
    let size = Vec2::new(880., 620.);
    app.execute_command(Command::RawDevelop, &ctx);
    assert!(app.raw_dialog.is_some());
    assert!(
        app.command_reason(Command::Action(crate::shortcuts::Action::Undo))
            .is_some()
    );
    let mut labels = Vec::new();
    for _ in 0..3 {
        labels = frame(&mut app, &ctx, &mut time, vec![], size);
    }
    let at = labels
        .iter()
        .find(|(s, _)| s == "촬영값")
        .unwrap()
        .1
        .center();
    click(&mut app, &ctx, &mut time, at);
    for _ in 0..2 {
        labels = frame(&mut app, &ctx, &mut time, vec![], size);
    }
    let selected = if mode == WhiteBalance::Temperature {
        "색온도·틴트"
    } else {
        "중립"
    };
    let at = labels
        .iter()
        .find(|(s, _)| s == selected)
        .unwrap()
        .1
        .center();
    click(&mut app, &ctx, &mut time, at);
    assert_eq!(
        app.raw_dialog.as_ref().unwrap().settings.white_balance,
        if mode == WhiteBalance::Temperature {
            mode
        } else {
            WhiteBalance::Neutral
        }
    );
    labels = frame(&mut app, &ctx, &mut time, vec![], size);
    let at = labels.iter().find(|(s, _)| s == "취소").unwrap().1.center();
    click(&mut app, &ctx, &mut time, at);
    assert!(app.raw_dialog.is_none());
    assert!(Arc::ptr_eq(
        app.tab().unwrap().editor.document.layers[0]
            .precision
            .as_ref()
            .unwrap(),
        &before
    ));
    assert!(!app.tab().unwrap().editor.can_undo());
    app.execute_command(Command::RawDevelop, &ctx);
    let panel = app.raw_dialog.as_mut().unwrap();
    panel.settings.white_balance = mode;
    panel.settings.temperature = if mode == WhiteBalance::Temperature {
        2856.
    } else {
        6500.
    };
    panel.settings.tint = if mode == WhiteBalance::Temperature {
        30.
    } else {
        0.
    };
    panel.settings.gains = [0.5, 2., 3.];
    let gain_fourth = if (fixture.starts_with("raw-dual/")
        || fixture.starts_with("raw-analog/")
        || fixture.starts_with("raw-white/")
        || fixture.starts_with("raw-calibration/"))
        && mode == WhiteBalance::Custom
    {
        1.25
    } else {
        8.
    };
    if raw.record.info.camera_wb_fourth.is_some() {
        panel.settings.gains_fourth = Some(gain_fourth);
    }
    panel.settings.exposure = 1.;
    for _ in 0..3 {
        frame(&mut app, &ctx, &mut time, vec![], size);
    }
    std::thread::sleep(Duration::from_millis(220));
    frame(&mut app, &ctx, &mut time, vec![], size);
    // Change parameters while a worker exists: only matching current parameters may apply.
    app.raw_dialog.as_mut().unwrap().settings.exposure = 2.;
    if mode == WhiteBalance::Temperature {
        app.raw_dialog.as_mut().unwrap().settings.temperature = 4000.;
        app.raw_dialog.as_mut().unwrap().settings.tint = 75.;
    }
    let deadline = Instant::now() + Duration::from_secs(8);
    while !app.raw_dialog.as_ref().unwrap().ready() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
        frame(&mut app, &ctx, &mut time, vec![], size);
    }
    assert!(app.raw_dialog.as_ref().unwrap().ready());
    for _ in 0..4 {
        labels = frame(&mut app, &ctx, &mut time, vec![], size);
    }
    if mode == WhiteBalance::Custom && raw.record.info.camera_wb_fourth.is_some() {
        let label = format!("{} 배율", raw.record.info.channel_label(3));
        assert!(labels.iter().any(|(text, _)| text == &label));
    }
    let at = labels.iter().find(|(s, _)| s == "적용").unwrap().1.center();
    click(&mut app, &ctx, &mut time, at);
    assert!(app.raw_dialog.is_none());
    let editor = &mut app.tabs[0].editor;
    let layer = editor.document.active_layer().unwrap();
    assert_eq!(
        layer.raw_source.as_ref().unwrap().record.settings.exposure,
        2.
    );
    if raw.record.info.camera_wb_fourth.is_some() {
        assert_eq!(
            layer
                .raw_source
                .as_ref()
                .unwrap()
                .record
                .settings
                .gains_fourth,
            Some(gain_fourth)
        );
    }
    if mode == WhiteBalance::Temperature {
        let settings = &layer.raw_source.as_ref().unwrap().record.settings;
        assert_eq!(settings.temperature, 4000.);
        assert_eq!(settings.tint, 75.);
        assert_eq!(settings.white_balance, mode);
        let expected = raw.develop(settings).unwrap();
        assert_eq!(
            layer.precision.as_ref().unwrap().as_raw(),
            expected.as_raw()
        );
    }
    assert!(Arc::ptr_eq(
        &layer.raw_source.as_ref().unwrap().bytes,
        &raw.bytes
    ));
    assert!(Arc::ptr_eq(layer.mask.as_ref().unwrap(), &mask));
    assert_eq!(layer.offset, [3, 5]);
    assert_eq!(layer.transform.angle, 12.);
    assert!(
        layer
            .precision
            .as_ref()
            .unwrap()
            .pixels()
            .any(|p| p.0[..3].iter().any(|v| *v > 1.))
    );
    editor.undo();
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &before
    ));
    assert!(!editor.can_undo());
    editor.redo();
    assert!(!editor.can_redo());
    for dpi in [1., 2.] {
        ctx.set_pixels_per_point(dpi);
        app.execute_command(Command::RawDevelop, &ctx);
        for _ in 0..3 {
            labels = frame(&mut app, &ctx, &mut time, vec![], Vec2::new(640., 320.));
        }
        for label in ["적용", "취소"] {
            let rect = labels.iter().find(|(s, _)| s == label).unwrap().1;
            assert!(
                Rect::from_min_size(Pos2::ZERO, Vec2::new(640., 320.)).contains_rect(rect),
                "{dpi}: {label} {rect:?}"
            );
        }
        frame(
            &mut app,
            &ctx,
            &mut time,
            vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
            Vec2::new(640., 320.),
        );
        assert!(app.raw_dialog.is_none());
    }
    app.execute_command(Command::RawDevelop, &ctx);
    app.tabs[0]
        .editor
        .document
        .active_layer_mut()
        .unwrap()
        .locked = true;
    frame(&mut app, &ctx, &mut time, vec![], size);
    assert!(app.raw_dialog.is_none());
    assert!(app.command_reason(Command::RawDevelop).is_some());
}
