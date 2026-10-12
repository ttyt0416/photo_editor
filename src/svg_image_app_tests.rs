#[test]
fn svg_image_app_modal_replacement_cancel_apply_masks_and_single_undo_in_small_window() {
    fn frame(
        app: &mut PhotoEditor,
        ctx: &egui::Context,
        time: &mut f64,
        events: Vec<egui::Event>,
        label: &str,
    ) -> Option<Pos2> {
        *time += 0.02;
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(*time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(880., 620.))),
                events,
                ..Default::default()
            },
            |_| {
                app.shortcuts(ctx);
                app.dialogs(ctx);
            },
        );
        let found = output.shapes.iter().find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                if text.galley.job.text.trim() == label {
                    let bounds = Rect::from_min_size(text.pos, text.galley.size());
                    assert!(
                        ctx.content_rect().contains_rect(bounds),
                        "{label}: {bounds:?}"
                    );
                    Some(bounds.center())
                } else {
                    None
                }
            } else {
                None
            }
        });
        output.textures_delta.clear();
        found
    }
    fn click(app: &mut PhotoEditor, ctx: &egui::Context, time: &mut f64, label: &str) {
        for _ in 0..5 {
            frame(app, ctx, time, vec![], "");
        }
        let visible = frame(app, ctx, time, vec![], label).unwrap_or_else(|| panic!("{label}"));
        let position = if label == "이미지 파일 경로" {
            ctx.read_response(egui::Id::new("svg-image-path"))
                .unwrap()
                .rect
                .center()
        } else {
            visible
        };
        frame(
            app,
            ctx,
            time,
            vec![egui::Event::PointerMoved(position)],
            "",
        );
        for pressed in [true, false] {
            frame(
                app,
                ctx,
                time,
                vec![egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                }],
                "",
            );
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/svg-assets/app");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("선택 이미지.bmp");
    image::RgbaImage::from_pixel(23, 19, image::Rgba([21, 64, 196, 255]))
        .save(&path)
        .unwrap();
    let source="<svg xmlns='http://www.w3.org/2000/svg' width='12' height='8'><rect width='12' height='8' fill='red'/><image id='target' width='12' height='8'/></svg>".to_owned();
    let content = Content::Svg { source };
    let mut doc = Document::from_image("SVG".into(), content.render().unwrap());
    let id = doc.active;
    let l = doc.active_layer_mut().unwrap();
    l.content = Some(content.clone());
    l.mask = Some(std::sync::Arc::new(image::GrayImage::from_pixel(
        12,
        8,
        image::Luma([199]),
    )));
    l.transform.angle = 17.;
    l.offset = [2, -1];
    let mask = l.mask.as_ref().unwrap().as_raw().clone();
    let baseline = doc.composite();
    let (mut app, ctx, mut time) = input_app(Tool::Brush);
    let tabid = app.tab().unwrap().id;
    app.tab_mut().unwrap().editor = crate::document::Editor::new(doc, true);
    for apply in [false, true] {
        ctx.set_pixels_per_point(if apply { 2. } else { 1. });
        app.svg_panel = Default::default();
        app.content_dialog = Some((tabid, Some(id), [2, -1], content.clone()));
        click(&mut app, &ctx, &mut time, "image target");
        click(&mut app, &ctx, &mut time, "이미지 파일 경로");
        assert_eq!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::new("svg-image-path"))
        );
        frame(
            &mut app,
            &ctx,
            &mut time,
            vec![egui::Event::Text(path.to_string_lossy().into_owned())],
            "",
        );
        click(&mut app, &ctx, &mut time, "이미지 교체");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            frame(&mut app, &ctx, &mut time, vec![], "");
            let (_, _, _, draft) = app.content_dialog.as_ref().unwrap();
            let changed = draft.svg().contains("data-rphoto-image-name");
            if changed && !app.svg_panel.busy() {
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert_eq!(app.tab().unwrap().editor.document.composite(), baseline);
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        click(
            &mut app,
            &ctx,
            &mut time,
            if apply { "적용" } else { "취소" },
        );
        assert!(app.content_dialog.is_none());
        if !apply {
            assert_eq!(app.tab().unwrap().editor.document.composite(), baseline);
            assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        }
    }
    let tab = app.tab_mut().unwrap();
    assert_eq!(tab.editor.history().count(), 1);
    assert_eq!(
        tab.editor
            .document
            .active_layer()
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .as_raw(),
        &mask
    );
    assert_eq!(tab.editor.document.active_layer().unwrap().offset, [2, -1]);
    assert_eq!(
        tab.editor.document.active_layer().unwrap().transform.angle,
        17.
    );
    let changed = tab.editor.document.composite();
    assert_ne!(changed, baseline);
    tab.editor.undo();
    assert_eq!(tab.editor.document.composite(), baseline);
    tab.editor.redo();
    assert_eq!(tab.editor.document.composite(), changed);
    crate::files::save_project(&tab.editor.document, &root.join("replaced.rphoto")).unwrap();
    assert_eq!(
        crate::files::load_project(&root.join("replaced.rphoto"))
            .unwrap()
            .composite(),
        changed
    );
}
