fn options_frame(
    app: &mut PhotoEditor,
    ctx: &egui::Context,
    time: &mut f64,
    size: Vec2,
    events: Vec<egui::Event>,
) -> Vec<(String, Rect)> {
    *time += 0.1;
    let mut out = ctx.run_ui(
        egui::RawInput {
            time: Some(*time),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |_| app.command_ui(ctx),
    );
    let labels = out
        .shapes
        .iter()
        .filter_map(|s| {
            if let egui::Shape::Text(t) = &s.shape {
                let rect = Rect::from_min_size(t.pos, t.galley.size());
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
fn options_click(
    app: &mut PhotoEditor,
    ctx: &egui::Context,
    time: &mut f64,
    size: Vec2,
    label: &str,
) {
    options_frame(app, ctx, time, size, vec![]);
    let labels = options_frame(app, ctx, time, size, vec![]);
    let at = labels
        .iter()
        .find(|(s, _)| s == label)
        .unwrap_or_else(|| panic!("missing {label}: {labels:?}"))
        .1
        .center();
    for pressed in [true, false] {
        options_frame(
            app,
            ctx,
            time,
            size,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
    }
}
fn options_drag(
    app: &mut PhotoEditor,
    ctx: &egui::Context,
    time: &mut f64,
    size: Vec2,
    prefix: &str,
    steps: &[f32],
) {
    options_frame(app, ctx, time, size, vec![]);
    let labels = options_frame(app, ctx, time, size, vec![]);
    let at = labels
        .iter()
        .find(|(s, _)| s.starts_with(prefix))
        .unwrap_or_else(|| panic!("missing {prefix}: {labels:?}"))
        .1
        .center();
    options_frame(
        app,
        ctx,
        time,
        size,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    for dx in steps {
        options_frame(
            app,
            ctx,
            time,
            size,
            vec![egui::Event::PointerMoved(at + Vec2::new(*dx, 0.))],
        );
    }
    let end = at + Vec2::new(*steps.last().unwrap(), 0.);
    options_frame(
        app,
        ctx,
        time,
        size,
        vec![egui::Event::PointerButton {
            pos: end,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
    );
}
#[test]
fn option_dialog_layout_preview_cancel_apply_one_undo_noop_target_change_and_close() {
    use crate::commands::Command;
    for dpi in [1., 2.] {
        for size in [Vec2::new(1320., 860.), Vec2::new(360., 480.)] {
            let (mut app, ctx, mut time) = input_app(Tool::Move);
            ctx.set_pixels_per_point(dpi);
            let original = app.tab().unwrap().editor.document.layout.clone();
            let source = app
                .tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .pixels
                .clone();
            let revision = app.tab().unwrap().editor.revision;
            app.execute_command(Command::LayoutSettings, &ctx);
            for _ in 0..5 {
                options_frame(&mut app, &ctx, &mut time, size, vec![]);
            }
            let labels = options_frame(&mut app, &ctx, &mut time, size, vec![]);
            for text in ["적용", "취소"] {
                let rect = labels
                    .iter()
                    .find(|(s, _)| s == text)
                    .unwrap_or_else(|| {
                        panic!(
                            "missing footer {text} at {size:?}/{dpi}, actual {:?}: {labels:?}",
                            ctx.content_rect()
                        )
                    })
                    .1;
                assert!(
                    rect.min.x >= 0.
                        && rect.max.x <= size.x
                        && rect.min.y >= 0.
                        && rect.max.y <= size.y,
                    "{text} outside {size:?}: {rect:?}"
                );
            }
            options_drag(
                &mut app,
                &ctx,
                &mut time,
                size,
                "가로 간격",
                &[5., 10., 15.],
            );
            assert!(app.tab().unwrap().editor.document.layout.spacing[0] > original.spacing[0]);
            assert_eq!(app.tab().unwrap().editor.revision, revision);
            assert!(!app.tab().unwrap().editor.can_undo());
            assert!(app.tab().unwrap().editor.is_editing());
            options_click(&mut app, &ctx, &mut time, size, "취소");
            assert_eq!(app.tab().unwrap().editor.document.layout, original);
            assert!(!app.tab().unwrap().editor.is_editing());
            app.execute_command(Command::LayoutSettings, &ctx);
            options_drag(
                &mut app,
                &ctx,
                &mut time,
                size,
                "가로 간격",
                &[5., 10., 15.],
            );
            let edited = app.tab().unwrap().editor.document.layout.clone();
            options_click(&mut app, &ctx, &mut time, size, "적용");
            assert_eq!(app.tab().unwrap().editor.document.layout, edited);
            assert_eq!(app.tab().unwrap().editor.history().count(), 1);
            app.tab_mut().unwrap().editor.undo();
            assert_eq!(app.tab().unwrap().editor.document.layout, original);
            assert!(Arc::ptr_eq(
                &source,
                &app.tab()
                    .unwrap()
                    .editor
                    .document
                    .active_layer()
                    .unwrap()
                    .pixels
            ));
            app.execute_command(Command::LayoutSettings, &ctx);
            options_click(&mut app, &ctx, &mut time, size, "눈금자");
            options_click(&mut app, &ctx, &mut time, size, "눈금자");
            options_click(&mut app, &ctx, &mut time, size, "적용");
            assert!(!app.tab().unwrap().editor.can_undo());
            assert!(app.tab().unwrap().editor.can_redo());
            app.execute_command(Command::GuideEdit, &ctx);
            options_click(&mut app, &ctx, &mut time, size, "적용");
            assert!(!app.tab().unwrap().editor.can_undo());
            app.execute_command(Command::GuideEdit, &ctx);
            options_click(&mut app, &ctx, &mut time, size, "세로 가이드 추가");
            assert_eq!(app.tab().unwrap().editor.document.layout.guides.len(), 1);
            options_frame(
                &mut app,
                &ctx,
                &mut time,
                size,
                vec![key_event(Key::Escape)],
            );
            assert_eq!(app.tab().unwrap().editor.document.layout, original);
            app.execute_command(Command::GuideEdit, &ctx);
            options_click(&mut app, &ctx, &mut time, size, "세로 가이드 추가");
            let first = app.tab().unwrap().id;
            app.add_tab(files::Opened {
                source_stamp: None,
                document: Document::new(16, 16, false).unwrap(),
                project_path: None,
                title: "다른 문서".into(),
                notes: vec![],
            });
            options_frame(&mut app, &ctx, &mut time, size, vec![]);
            assert!(app.command_options.is_none());
            let first = app.tabs.iter().find(|t| t.id == first).unwrap();
            assert_eq!(first.editor.document.layout, original);
            assert!(!first.editor.is_editing());
            app.active = 0;
            app.execute_command(Command::GuideEdit, &ctx);
            options_click(&mut app, &ctx, &mut time, size, "세로 가이드 추가");
            app.request_close(CloseTarget::Tab(app.tab().unwrap().id), &ctx);
            assert!(app.command_options.is_none());
            assert!(app.layout_options.is_none());
        }
    }
}
#[test]
fn option_dialog_guide_limit_virtual_rows_project_and_one_history_preserve_source() {
    use crate::commands::Command;
    let (mut app, ctx, mut time) = input_app(Tool::Move);
    let source = app
        .tab()
        .unwrap()
        .editor
        .document
        .active_layer()
        .unwrap()
        .pixels
        .clone();
    app.tab_mut().unwrap().editor.document.layout.guides = (0..4095)
        .map(|i| crate::guides::Guide {
            axis: i % 2,
            position: (i % 256) as f32,
        })
        .collect();
    let before = app.tab().unwrap().editor.document.layout.clone();
    let size = Vec2::new(360., 480.);
    app.execute_command(Command::GuideEdit, &ctx);
    options_click(&mut app, &ctx, &mut time, size, "세로 가이드 추가");
    assert_eq!(app.tab().unwrap().editor.document.layout.guides.len(), 4096);
    options_click(&mut app, &ctx, &mut time, size, "가로 가이드 추가");
    assert_eq!(app.tab().unwrap().editor.document.layout.guides.len(), 4096);
    let labels = options_frame(&mut app, &ctx, &mut time, size, vec![]);
    assert!(labels.iter().filter(|(s, _)| s == "삭제").count() < 30);
    options_click(&mut app, &ctx, &mut time, size, "적용");
    assert_eq!(app.tab().unwrap().editor.history().count(), 1);
    assert!(app.tab().unwrap().editor.document.layout.valid());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/options-dialogs");
    std::fs::create_dir_all(&root).unwrap();
    let doc = &app.tab().unwrap().editor.document;
    files::save_document(doc, &root.join("guides.rphoto")).unwrap();
    let reopened = files::open(&root.join("guides.rphoto")).unwrap().document;
    assert_eq!(reopened.layout, doc.layout);
    assert_eq!(
        reopened.active_layer().unwrap().pixels.as_raw(),
        source.as_raw()
    );
    app.tab_mut().unwrap().editor.undo();
    assert_eq!(app.tab().unwrap().editor.document.layout, before);
    assert!(Arc::ptr_eq(
        &source,
        &app.tab()
            .unwrap()
            .editor
            .document
            .active_layer()
            .unwrap()
            .pixels
    ));
    app.tab_mut().unwrap().editor.redo();
    app.execute_command(Command::GuideEdit, &ctx);
    options_click(&mut app, &ctx, &mut time, size, "가이드 모두 삭제");
    assert!(app.tab().unwrap().editor.document.layout.guides.is_empty());
    options_click(&mut app, &ctx, &mut time, size, "취소");
    assert_eq!(app.tab().unwrap().editor.document.layout.guides.len(), 4096);
}
#[test]
fn option_dialog_refine_and_overlay_compact_apply_cancel_without_pixel_history() {
    use crate::commands::Command;
    for dpi in [1., 2.] {
        let (mut app, ctx, mut time) = input_app(Tool::Move);
        ctx.set_pixels_per_point(dpi);
        let size = Vec2::new(360., 480.);
        let source = app
            .tab()
            .unwrap()
            .editor
            .document
            .active_layer()
            .unwrap()
            .pixels
            .clone();
        app.tab_mut().unwrap().selection = Some(Selection::rectangle([80, 80], [120, 120]));
        app.execute_command(Command::Refine, &ctx);
        options_drag(&mut app, &ctx, &mut time, size, "확대·축소 px", &[10., 20.]);
        options_drag(&mut app, &ctx, &mut time, size, "페더 px", &[10.]);
        assert_eq!(app.selection_expand, 22, "dpi={dpi}");
        assert_eq!(app.selection_feather, 7., "dpi={dpi}");
        options_click(&mut app, &ctx, &mut time, size, "취소");
        assert_eq!(
            app.tab()
                .unwrap()
                .selection
                .as_ref()
                .unwrap()
                .coverage(70, 100),
            0.
        );
        app.execute_command(Command::Refine, &ctx);
        options_click(&mut app, &ctx, &mut time, size, "선택 경계에 적용");
        let selection = app.tab().unwrap().selection.as_ref().unwrap();
        assert!(selection.coverage(70, 100) > 0.5);
        assert_eq!(selection.coverage(10, 10), 0.);
        assert_eq!(selection.coverage(100, 100), 1.);
        app.execute_command(Command::OverlaySettings, &ctx);
        options_click(&mut app, &ctx, &mut time, size, "마스크 오버레이");
        options_drag(
            &mut app,
            &ctx,
            &mut time,
            size,
            "표시 불투명도",
            &[10., 20.],
        );
        assert!(app.workspace.layout.mask_overlay.enabled);
        assert!((app.workspace.layout.mask_overlay.opacity - 0.5).abs() < 1e-5);
        options_click(&mut app, &ctx, &mut time, size, "닫기");
        assert!(app.command_options.is_none());
        assert!(!app.tab().unwrap().editor.can_undo());
        assert!(Arc::ptr_eq(
            &source,
            &app.tab()
                .unwrap()
                .editor
                .document
                .active_layer()
                .unwrap()
                .pixels
        ));
    }
}
