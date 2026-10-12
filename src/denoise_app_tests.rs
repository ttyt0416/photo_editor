#[test]
fn denoise_ui_band_controls_reset_cancel_apply_and_undo_preserve_raw_source() {
    let (mut app, ctx, mut clock) = input_app(Tool::Brush);
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/raw/linear-32-o1.dng");
    let doc = files::open(&path).unwrap().document;
    let original = doc.layers[0].precision.clone().unwrap();
    let raw = doc.layers[0].raw_source.clone().unwrap();
    app.tabs[0].editor = Editor::new(doc, false);
    app.open_adjustments();
    adjustment_click(&mut app, &ctx, &mut clock, "노이즈 감소");
    for label in [
        "밝기 노이즈 감소",
        "색상 노이즈 감소",
        "노이즈 감소 반경 px",
        "노이즈 허용 차이",
    ] {
        assert!(adjustment_frame(&mut app, &ctx, &mut clock, vec![], label).is_some());
    }
    app.tabs[0].editor.document.layers[0].adjustments.denoise = crate::denoise::Denoise {
        luminance: 0.8,
        color: 0.6,
        radius: 4,
        threshold: 0.3,
    };
    adjustment_click(&mut app, &ctx, &mut clock, "노이즈 감소 초기화");
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .denoise
            .is_default()
    );
    app.tabs[0].editor.document.layers[0]
        .adjustments
        .denoise
        .luminance = 0.8;
    adjustment_click(&mut app, &ctx, &mut clock, "취소");
    assert!(app.adjust_dialog.is_none());
    assert!(!app.tabs[0].editor.can_undo());
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .denoise
            .is_default()
    );
    app.open_adjustments();
    app.tabs[0].editor.document.layers[0]
        .adjustments
        .denoise
        .luminance = 0.7;
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
