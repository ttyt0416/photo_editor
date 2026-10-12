#[test]
fn raw_advanced_apply_cancel_reopen_project_source_mask_transform_and_one_undo() {
    raw_advanced_apply_case(false);
}
#[test]
fn raw_geometry_apply_cancel_reopen_project_source_mask_transform_and_one_undo() {
    raw_advanced_apply_case(true);
}
fn raw_advanced_apply_case(geometry: bool) {
    let (mut app, ctx, mut time) = input_app(Tool::Brush);
    let mut doc = files::open(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/raw/linear-32-o1.dng"),
    )
    .unwrap()
    .document;
    let layer = &mut doc.layers[0];
    layer.offset = [2, 3];
    layer.transform.angle = 7.;
    layer.mask = Some(Arc::new(image::GrayImage::from_pixel(
        layer.pixels.width(),
        layer.pixels.height(),
        image::Luma([190]),
    )));
    layer.adjustments.contrast = 0.2;
    let original = layer.precision.clone().unwrap();
    let source = layer.raw_source.clone().unwrap();
    let before = doc.clone();
    app.tabs[0].editor = Editor::new(doc, false);
    fn frame(
        app: &mut PhotoEditor,
        ctx: &egui::Context,
        time: &mut f64,
        events: Vec<egui::Event>,
    ) -> Vec<(String, Rect)> {
        *time += 0.05;
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(*time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(880., 620.))),
                events,
                ..Default::default()
            },
            |_| app.dialogs(ctx),
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
    app.open_raw(&ctx);
    assert_eq!(
        app.raw_dialog.as_ref().unwrap().adjustments,
        before.layers[0].adjustments
    );
    app.raw_dialog.as_mut().unwrap().adjustments.gamma = 1.2;
    if geometry {
        app.raw_dialog.as_mut().unwrap().adjustments.geometry.angle = -15.;
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
            modifiers: Default::default(),
        }],
    );
    assert!(app.raw_dialog.is_none());
    assert_eq!(
        app.tabs[0].editor.document.layers[0].adjustments,
        before.layers[0].adjustments
    );
    app.open_raw(&ctx);
    let panel = app.raw_dialog.as_mut().unwrap();
    panel.adjustments.curves[0].points = vec![[0., 0.], [0.5, 0.7], [1., 1.]];
    panel.adjustments.color_mixer.hue[2] = 18.;
    panel.adjustments.color_grading.tones[0].saturation = 0.2;
    panel.adjustments.denoise.luminance = 0.1;
    panel.adjustments.detail_sharpen.amount = 0.2;
    panel.adjustments.lens.zoom = 1.02;
    if geometry {
        panel.adjustments.geometry = crate::photo_geometry::Geometry {
            angle: 12.,
            perspective: [0.15, -0.2],
            zoom: 1.2,
            aspect: 0.9,
            shift: [0.05, -0.03],
            sampling: crate::sampling::Sampling::Bicubic,
        };
    }
    let expected = panel.adjustments.clone();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !app.raw_dialog.as_ref().unwrap().ready() && Instant::now() < deadline {
        frame(&mut app, &ctx, &mut time, vec![]);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(app.raw_dialog.as_ref().unwrap().ready());
    frame(&mut app, &ctx, &mut time, vec![]);
    let at = frame(&mut app, &ctx, &mut time, vec![])
        .iter()
        .find(|(s, _)| s == "적용")
        .unwrap()
        .1
        .center();
    for pressed in [true, false] {
        frame(
            &mut app,
            &ctx,
            &mut time,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
    }
    assert!(app.raw_dialog.is_none());
    let editor = &mut app.tabs[0].editor;
    let layer = &editor.document.layers[0];
    assert_eq!(layer.adjustments, expected);
    assert_eq!(layer.precision.as_deref(), Some(original.as_ref()));
    assert_eq!(layer.raw_source.as_ref().unwrap().bytes, source.bytes);
    assert_eq!(
        layer.raw_source.as_ref().unwrap().record.settings,
        source.record.settings
    );
    assert_eq!(layer.mask, before.layers[0].mask);
    assert_eq!(layer.offset, before.layers[0].offset);
    assert_eq!(layer.transform, before.layers[0].transform);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(if geometry {
        "qa-output/raw-geometry"
    } else {
        "qa-output/raw-advanced"
    });
    std::fs::create_dir_all(&root).unwrap();
    let project = root.join("recipe.rphoto");
    files::save_project(&editor.document, &project).unwrap();
    let loaded = files::load_project(&project).unwrap();
    assert_eq!(loaded.layers[0].adjustments, expected);
    assert_eq!(
        loaded.layers[0].precision.as_deref(),
        Some(original.as_ref())
    );
    assert_eq!(
        loaded.layers[0].raw_source.as_ref().unwrap().bytes,
        source.bytes
    );
    assert_eq!(
        crate::precision::composite(&loaded),
        crate::precision::composite(&editor.document)
    );
    assert!(editor.can_undo());
    editor.undo();
    assert_eq!(
        editor.document.layers[0].adjustments,
        before.layers[0].adjustments
    );
    assert!(Arc::ptr_eq(
        editor.document.layers[0].precision.as_ref().unwrap(),
        &original
    ));
    assert!(!editor.can_undo());
    assert!(editor.can_redo());
    editor.redo();
    app.open_raw(&ctx);
    assert_eq!(app.raw_dialog.as_ref().unwrap().adjustments, expected);
}
