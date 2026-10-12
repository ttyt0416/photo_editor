#[test]
fn mixer_ui_band_controls_reset_cancel_apply_and_undo_preserve_raw_source() {
    let (mut app, ctx, mut clock) = input_app(Tool::Brush);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/raw/linear-32-o1.dng");
    let doc = files::open(&path).unwrap().document;
    let original = doc.layers[0].precision.clone().unwrap();
    let raw = doc.layers[0].raw_source.clone().unwrap();
    app.tabs[0].editor = Editor::new(doc, false);
    app.open_adjustments();
    adjustment_click(&mut app, &ctx, &mut clock, "색상 믹서");
    for name in crate::color_mixer::NAMES {
        assert!(adjustment_frame(&mut app, &ctx, &mut clock, vec![], name).is_some());
    }
    adjustment_click(&mut app, &ctx, &mut clock, "주황");
    for label in ["색조 이동", "색상별 채도", "색상별 밝기"] {
        assert!(adjustment_frame(&mut app, &ctx, &mut clock, vec![], label).is_some());
    }
    let m = &mut app.tabs[0].editor.document.layers[0]
        .adjustments
        .color_mixer;
    m.hue[0] = 30.;
    m.hue[1] = 50.;
    m.saturation[1] = -0.8;
    adjustment_click(&mut app, &ctx, &mut clock, "이 색상 초기화");
    let m = &app.tabs[0].editor.document.layers[0]
        .adjustments
        .color_mixer;
    assert_eq!(m.hue[0], 30.);
    assert_eq!(m.hue[1], 0.);
    assert_eq!(m.saturation[1], 0.);
    adjustment_click(&mut app, &ctx, &mut clock, "색상 믹서 전체 초기화");
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .color_mixer
            .disabled()
    );
    app.tabs[0].editor.document.layers[0]
        .adjustments
        .color_mixer
        .hue[0] = 40.;
    adjustment_click(&mut app, &ctx, &mut clock, "취소");
    assert!(app.adjust_dialog.is_none());
    assert!(!app.tabs[0].editor.can_undo());
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .color_mixer
            .disabled()
    );
    app.open_adjustments();
    app.tabs[0].editor.document.layers[0]
        .adjustments
        .color_mixer
        .hue = [35.; 8];
    adjustment_click(&mut app, &ctx, &mut clock, "적용");
    assert!(app.adjust_dialog.is_none());
    assert_eq!(app.tabs[0].editor.history().count(), 1);
    let changed = crate::precision::composite(&app.tabs[0].editor.document);
    assert_ne!(changed, *original);
    assert!(Arc::ptr_eq(
        app.tabs[0].editor.document.layers[0]
            .precision
            .as_ref()
            .unwrap(),
        &original
    ));
    assert!(Arc::ptr_eq(
        app.tabs[0].editor.document.layers[0]
            .raw_source
            .as_ref()
            .unwrap(),
        &raw
    ));
    app.tabs[0].editor.undo();
    assert_eq!(
        crate::precision::composite(&app.tabs[0].editor.document),
        *original
    );
    app.tabs[0].editor.redo();
    assert_eq!(
        crate::precision::composite(&app.tabs[0].editor.document),
        changed
    );
}
