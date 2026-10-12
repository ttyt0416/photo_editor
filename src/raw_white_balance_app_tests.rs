#[test]
fn raw_white_balance_pointer_fresh_sensor_preview_cancel_apply_project_undo_preserve_edited_baseline()
 {
    raw_white_balance_app_flow("raw/linear-32-o1.dng");
}
#[test]
fn raw_dual_white_balance_pointer_preview_cancel_apply_native_psd_psb_exr_and_undo() {
    raw_white_balance_app_flow("raw-dual/linear-3c-32-ill17-21.dng");
    raw_white_balance_app_flow("raw-dual/linear-4c-32-ill21-17.dng");
    raw_white_balance_app_flow("raw-dual/cfa-4c-16-ill17-21.dng");
    raw_white_balance_app_flow("raw-color/cfa-4c-16-ill213451.dng");
    raw_white_balance_app_flow("raw-white/raw-dual-linear-3c-32-ill17-21-analog-xyD65.dng");
    raw_white_balance_app_flow("raw-white/raw-dual-linear-4c-32-ill21-17-analog-xyA.dng");
}
#[test]
fn raw_camera_calibration_white_balance_pointer_cancel_apply_native_outputs_and_undo() {
    raw_white_balance_app_flow(
        "raw-calibration/raw-analog-raw-dual-linear-3c-32-ill17-21-analog-cc-match.dng",
    );
    raw_white_balance_app_flow(
        "raw-calibration/raw-white-raw-dual-linear-4c-32-ill21-17-unit-xyA-cc-match.dng",
    );
    raw_white_balance_app_flow(
        "raw-calibration/raw-white-raw-dual-cfa-4c-16-ill17-21-analog-xyD65-cc-mismatch.dng",
    );
}
fn raw_white_balance_app_flow(fixture: &str) {
    use crate::{commands::Command, precision, raw_input::WhiteBalance};
    struct State {
        labels: Vec<(String, Rect)>,
        images: Vec<Rect>,
    }
    fn frame(
        app: &mut PhotoEditor,
        ctx: &egui::Context,
        time: &mut f64,
        events: Vec<egui::Event>,
    ) -> State {
        *time += 0.05;
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(*time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1000., 950.))),
                events,
                ..Default::default()
            },
            |_| {
                app.shortcuts(ctx);
                app.dialogs(ctx);
            },
        );
        let mut state = State {
            labels: Vec::new(),
            images: Vec::new(),
        };
        for shape in &output.shapes {
            match &shape.shape {
                egui::Shape::Text(t) => state.labels.push((
                    t.galley.job.text.clone(),
                    Rect::from_min_size(t.pos, t.galley.size()),
                )),
                egui::Shape::Mesh(m) if m.texture_id != egui::TextureId::default() => {
                    let mut rect = Rect::NOTHING;
                    for v in &m.vertices {
                        rect.extend_with(v.pos);
                    }
                    state.images.push(rect);
                }
                _ => {}
            }
        }
        output.textures_delta.clear();
        state
    }
    fn click(app: &mut PhotoEditor, ctx: &egui::Context, time: &mut f64, point: Pos2) {
        frame(app, ctx, time, vec![egui::Event::PointerMoved(point)]);
        for pressed in [true, false] {
            frame(
                app,
                ctx,
                time,
                vec![egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
            );
        }
    }
    let (mut app, ctx, mut time) = input_app(Tool::Brush);
    let mut doc = files::open(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(fixture),
    )
    .unwrap()
    .document;
    let raw = doc.layers[0].raw_source.clone().unwrap();
    let bytes = raw.bytes.clone();
    let before = Arc::new(image::Rgba32FImage::from_pixel(
        2,
        2,
        image::Rgba([2., 0.4, 0.1, 1.]),
    ));
    let mask = Arc::new(image::GrayImage::from_pixel(2, 2, image::Luma([177])));
    let layer = &mut doc.layers[0];
    layer.precision = Some(before.clone());
    layer.pixels = Arc::new(precision::preview(&before));
    layer.mask = Some(mask.clone());
    layer.offset = [3, 5];
    layer.transform.angle = 12.;
    let transform = layer.transform;
    let mask_world = crate::masks::world(layer);
    app.tabs[0].editor = Editor::new(doc, false);
    for apply in [false, true] {
        app.execute_command(Command::RawDevelop, &ctx);
        for _ in 0..3 {
            frame(&mut app, &ctx, &mut time, vec![]);
        }
        let state = frame(&mut app, &ctx, &mut time, vec![]);
        let point = state
            .labels
            .iter()
            .find(|(s, _)| s == "화이트 밸런스 스포이드")
            .unwrap()
            .1
            .center();
        click(&mut app, &ctx, &mut time, point);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let after = loop {
            assert!(
                std::time::Instant::now() < deadline,
                "fresh RAW preview did not arrive"
            );
            std::thread::sleep(std::time::Duration::from_millis(4));
            let state = frame(&mut app, &ctx, &mut time, vec![]);
            if let Some(rect) = state
                .images
                .iter()
                .find(|r| r.width() > 11. && r.height() > 9.)
            {
                break *rect;
            }
        };
        // The displayed baseline is edited 2x2; the picker must show freshly developed 12x10.
        let focus = [0.75, 0.25];
        let point = after.min + after.size() * egui::vec2(focus[0], focus[1]);
        let expected = raw.white_balance_at(focus, 2).unwrap();
        click(&mut app, &ctx, &mut time, point);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while !app.raw_dialog.as_ref().unwrap().ready() {
            assert!(
                std::time::Instant::now() < deadline,
                "picked preview did not arrive"
            );
            std::thread::sleep(std::time::Duration::from_millis(4));
            frame(&mut app, &ctx, &mut time, vec![]);
        }
        let panel = app.raw_dialog.as_ref().unwrap();
        assert_eq!(panel.settings.white_balance, WhiteBalance::Custom);
        assert_eq!(panel.settings.gains_fourth, expected.gains_fourth);
        for (actual, wanted) in panel.settings.gains.iter().zip(expected.gains) {
            assert!(
                (*actual - wanted).abs() < 1e-5,
                "focus{focus:?}, rectangle{after:?}: {:?} != {expected:?}",
                panel.settings.gains
            );
        }
        assert!(Arc::ptr_eq(
            app.tabs[0].editor.document.layers[0]
                .precision
                .as_ref()
                .unwrap(),
            &before
        ));
        let state = frame(&mut app, &ctx, &mut time, vec![]);
        let label = if apply { "적용" } else { "취소" };
        click(
            &mut app,
            &ctx,
            &mut time,
            state
                .labels
                .iter()
                .find(|(s, _)| s == label)
                .unwrap()
                .1
                .center(),
        );
        assert!(app.raw_dialog.is_none());
        let editor = &mut app.tabs[0].editor;
        if !apply {
            assert!(Arc::ptr_eq(
                editor.document.layers[0].precision.as_ref().unwrap(),
                &before
            ));
            assert!(!editor.can_undo());
            continue;
        }
        let layer = &editor.document.layers[0];
        assert_eq!(layer.precision.as_ref().unwrap().dimensions(), (12, 10));
        assert_eq!(layer.offset, [3, 5]);
        assert_eq!(layer.transform, transform);
        for (a, b) in crate::masks::world(layer).iter().zip(mask_world) {
            assert!((*a - b).abs() < 1e-10);
        }
        assert!(crate::masks::valid(layer));
        assert!(Arc::ptr_eq(layer.mask.as_ref().unwrap(), &mask));
        let applied_raw = layer.raw_source.clone().unwrap();
        assert!(Arc::ptr_eq(&applied_raw.bytes, &bytes));
        assert_eq!(
            applied_raw.record.settings.white_balance,
            WhiteBalance::Custom
        );
        let rendered = precision::composite(&editor.document);
        let mut dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/raw-white-balance");
        if fixture != "raw/linear-32-o1.dng" {
            dir = dir.join(std::path::Path::new(fixture).file_name().unwrap());
        }
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("picked.rphoto");
        files::save_project(&editor.document, &path).unwrap();
        let restored = files::load_project(&path).unwrap();
        assert_eq!(precision::composite(&restored), rendered);
        assert_eq!(
            *restored.layers[0].raw_source.as_ref().unwrap().bytes,
            *bytes
        );
        assert_eq!(
            restored.layers[0]
                .raw_source
                .as_ref()
                .unwrap()
                .record
                .settings,
            applied_raw.record.settings
        );
        for ext in ["psd", "psb", "exr"] {
            let path = dir.join(format!("picked.{ext}"));
            if ext == "exr" {
                files::export(
                    &restored,
                    &path,
                    image::ImageFormat::OpenExr,
                    &Default::default(),
                )
                .unwrap();
            } else {
                files::save_photoshop(&restored, &path).unwrap();
            }
            let actual = precision::composite(&files::open(&path).unwrap().document);
            for (a, b) in actual.as_raw().iter().zip(rendered.as_raw()) {
                assert!((*a - *b).abs() < 3e-5);
            }
        }
        editor.undo();
        assert!(Arc::ptr_eq(
            editor.document.layers[0].precision.as_ref().unwrap(),
            &before
        ));
        assert!(Arc::ptr_eq(
            editor.document.layers[0].raw_source.as_ref().unwrap(),
            &raw
        ));
        assert!(!editor.can_undo());
        editor.redo();
        assert_eq!(precision::composite(&editor.document), rendered);
    }
}
