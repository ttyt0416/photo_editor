#[test]
fn lens_profile_ui_auto_find_toggle_remove_cancel_apply_and_undo_keep_source() {
    let (mut app, ctx, mut clock) = input_app(Tool::Brush);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let db = Arc::new(
        crate::lens_profile::load(&root.join("tests/fixtures/lens-profiles/synthetic.xml"))
            .unwrap(),
    );
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("lens-profile-library"), db));
    let doc = files::open(&root.join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let original = doc.layers[0].precision.clone().unwrap();
    let raw = doc.layers[0].raw_source.clone().unwrap();
    app.tabs[0].editor = Editor::new(doc, false);
    app.open_adjustments();
    adjustment_click(&mut app, &ctx, &mut clock, "렌즈 프로파일");
    assert!(
        adjustment_frame(&mut app, &ctx, &mut clock, vec![], "프로파일 XML 불러오기…").is_some()
    );
    adjustment_click(&mut app, &ctx, &mut clock, "교정값 선택");
    adjustment_click(
        &mut app,
        &ctx,
        &mut clock,
        "Test Optics PTLens Test · 35 mm · Ptlens",
    );
    assert_eq!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .model,
        crate::lens_profile::Model::Ptlens
    );
    adjustment_click(&mut app, &ctx, &mut clock, "촬영 정보로 프로파일 찾기");
    let profile = app.tabs[0].editor.document.layers[0]
        .adjustments
        .lens_profile
        .clone()
        .unwrap();
    assert_eq!(profile.name, "Known Prime 35mm");
    assert_eq!(profile.input_crop, 1.5);
    adjustment_click(&mut app, &ctx, &mut clock, "프로파일 왜곡 보정");
    assert!(
        !app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .enabled
    );
    adjustment_click(&mut app, &ctx, &mut clock, "프로파일 색수차 보정");
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .tca
            .as_ref()
            .unwrap()
            .enabled
    );
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .effect_enabled()
    );
    assert!(
        !app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .enabled
    );
    adjustment_click(&mut app, &ctx, &mut clock, "프로파일 색수차 보정");
    assert!(
        !app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .tca
            .as_ref()
            .unwrap()
            .enabled
    );
    adjustment_click(&mut app, &ctx, &mut clock, "프로파일 왜곡 보정");
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .enabled
    );
    adjustment_click(&mut app, &ctx, &mut clock, "렌즈 프로파일 제거");
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .is_none()
    );
    adjustment_click(&mut app, &ctx, &mut clock, "촬영 정보로 프로파일 찾기");
    adjustment_click(&mut app, &ctx, &mut clock, "취소");
    assert!(app.adjust_dialog.is_none());
    assert!(!app.tabs[0].editor.can_undo());
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .is_none()
    );
    app.open_adjustments();
    adjustment_click(&mut app, &ctx, &mut clock, "촬영 정보로 프로파일 찾기");
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

fn lens_adjustment_scroll(app: &mut PhotoEditor, ctx: &egui::Context, clock: &mut f64, delta: f32) {
    let point =
        adjustment_frame(app, ctx, clock, vec![], "사진 보정·필터").unwrap() + Vec2::new(80., 150.);
    adjustment_frame(
        app,
        ctx,
        clock,
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: Vec2::new(0., delta),
                modifiers: Modifiers::NONE,
            },
        ],
        "",
    );
    for _ in 0..3 {
        adjustment_frame(app, ctx, clock, vec![], "");
    }
}
#[test]
fn lens_vignetting_ui_measured_aperture_selection_toggle_cancel_apply_undo_keeps_raw() {
    let (mut app, ctx, mut clock) = input_app(Tool::Brush);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let db = Arc::new(
        crate::lens_profile::load(&root.join("tests/fixtures/lens-profiles/vignetting.xml"))
            .unwrap(),
    );
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("lens-profile-library"), db));
    let doc = files::open(&root.join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let raw = doc.layers[0].raw_source.clone().unwrap();
    let source = doc.layers[0].precision.clone().unwrap();
    app.tabs[0].editor = Editor::new(doc, false);
    app.open_adjustments();
    adjustment_click(&mut app, &ctx, &mut clock, "렌즈 프로파일");
    adjustment_click(&mut app, &ctx, &mut clock, "촬영 정보로 프로파일 찾기");
    assert_eq!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .vignetting_selected,
        Some(0)
    );
    lens_adjustment_scroll(&mut app, &ctx, &mut clock, -180.);
    adjustment_click(&mut app, &ctx, &mut clock, "f/2.8 · 10 m · PA");
    adjustment_click(&mut app, &ctx, &mut clock, "f/4 · 1000 m · PA");
    assert_eq!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .vignetting_selected,
        Some(2)
    );
    adjustment_click(&mut app, &ctx, &mut clock, "프로파일 주변부 밝기 보정");
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .vignetting_enabled
    );
    adjustment_click(&mut app, &ctx, &mut clock, "프로파일 주변부 밝기 보정");
    assert!(
        !app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .vignetting_enabled
    );
    adjustment_click(&mut app, &ctx, &mut clock, "취소");
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .is_none()
    );
    assert!(!app.tabs[0].editor.can_undo());
    app.open_adjustments();
    lens_adjustment_scroll(&mut app, &ctx, &mut clock, 180.);
    adjustment_click(&mut app, &ctx, &mut clock, "촬영 정보로 프로파일 찾기");
    lens_adjustment_scroll(&mut app, &ctx, &mut clock, -180.);
    adjustment_click(&mut app, &ctx, &mut clock, "프로파일 주변부 밝기 보정");
    adjustment_click(&mut app, &ctx, &mut clock, "적용");
    assert!(app.adjust_dialog.is_none());
    assert_eq!(app.tabs[0].editor.history().count(), 1);
    let changed = crate::precision::composite(&app.tabs[0].editor.document);
    assert_ne!(changed, *source);
    assert!(Arc::ptr_eq(
        app.tabs[0].editor.document.layers[0]
            .raw_source
            .as_ref()
            .unwrap(),
        &raw
    ));
    assert!(Arc::ptr_eq(
        app.tabs[0].editor.document.layers[0]
            .precision
            .as_ref()
            .unwrap(),
        &source
    ));
    app.tabs[0].editor.undo();
    assert_eq!(
        crate::precision::composite(&app.tabs[0].editor.document),
        *source
    );
    app.tabs[0].editor.redo();
    assert_eq!(
        crate::precision::composite(&app.tabs[0].editor.document),
        changed
    );
}

fn lens_focal_drag(
    app: &mut PhotoEditor,
    ctx: &egui::Context,
    clock: &mut f64,
    label: &str,
    delta: f32,
) {
    let point = adjustment_frame(app, ctx, clock, vec![], label)
        .unwrap_or_else(|| panic!("missing focal value: {label}"));
    adjustment_frame(
        app,
        ctx,
        clock,
        vec![
            egui::Event::PointerMoved(point),
            egui::Event::PointerButton {
                pos: point,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
        label,
    );
    let end = point + Vec2::new(delta, 0.);
    adjustment_frame(app, ctx, clock, vec![egui::Event::PointerMoved(end)], label);
    adjustment_frame(
        app,
        ctx,
        clock,
        vec![egui::Event::PointerButton {
            pos: end,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }],
        label,
    );
}
#[test]
fn lens_interpolation_ui_focal_drag_compute_reject_outside_cancel_apply_and_undo_keep_raw() {
    let (mut app, ctx, mut clock) = input_app(Tool::Brush);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let db = Arc::new(
        crate::lens_profile::load(&root.join("tests/fixtures/lens-profiles/synthetic.xml"))
            .unwrap(),
    );
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("lens-profile-library"), db));
    let doc = files::open(&root.join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let raw = doc.layers[0].raw_source.clone().unwrap();
    let source = doc.layers[0].precision.clone().unwrap();
    app.tabs[0].editor = Editor::new(doc, false);
    app.open_adjustments();
    adjustment_click(&mut app, &ctx, &mut clock, "렌즈 프로파일");
    adjustment_click(&mut app, &ctx, &mut clock, "촬영 정보로 프로파일 찾기");
    lens_adjustment_scroll(&mut app, &ctx, &mut clock, -230.);
    lens_focal_drag(&mut app, &ctx, &mut clock, "35.0", 75.);
    assert_eq!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .as_ref()
            .unwrap()
            .focal,
        35.
    );
    adjustment_click(&mut app, &ctx, &mut clock, "초점거리 보정값 계산");
    let profile = app.tabs[0].editor.document.layers[0]
        .adjustments
        .lens_profile
        .clone()
        .unwrap();
    assert_eq!(profile.focal, 42.5);
    assert_eq!(profile.interpolated, Some([35., 50.]));
    lens_focal_drag(&mut app, &ctx, &mut clock, "42.5", 200.);
    adjustment_click(&mut app, &ctx, &mut clock, "초점거리 보정값 계산");
    assert_eq!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile,
        Some(profile.clone())
    );
    adjustment_click(&mut app, &ctx, &mut clock, "취소");
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .is_none()
    );
    assert!(!app.tabs[0].editor.can_undo());
    app.open_adjustments();
    app.tabs[0].editor.document.layers[0]
        .adjustments
        .lens_profile = Some(profile);
    adjustment_click(&mut app, &ctx, &mut clock, "적용");
    assert_eq!(app.tabs[0].editor.history().count(), 1);
    let changed = crate::precision::composite(&app.tabs[0].editor.document);
    assert_ne!(changed, *source);
    assert!(Arc::ptr_eq(
        app.tabs[0].editor.document.layers[0]
            .raw_source
            .as_ref()
            .unwrap(),
        &raw
    ));
    assert!(Arc::ptr_eq(
        app.tabs[0].editor.document.layers[0]
            .precision
            .as_ref()
            .unwrap(),
        &source
    ));
    app.tabs[0].editor.undo();
    assert_eq!(
        crate::precision::composite(&app.tabs[0].editor.document),
        *source
    );
    app.tabs[0].editor.redo();
    assert_eq!(
        crate::precision::composite(&app.tabs[0].editor.document),
        changed
    );
}

#[test]
fn lens_vignette_interpolation_ui_aperture_distance_drag_compute_reject_toggle_cancel_apply_undo() {
    let (mut app, ctx, mut clock) = input_app(Tool::Brush);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let db = Arc::new(
        crate::lens_profile::load(&root.join("tests/fixtures/lens-profiles/vignetting.xml"))
            .unwrap(),
    );
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("lens-profile-library"), db));
    let doc = files::open(&root.join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let raw = doc.layers[0].raw_source.clone().unwrap();
    let source = doc.layers[0].precision.clone().unwrap();
    app.tabs[0].editor = Editor::new(doc, false);
    app.open_adjustments();
    adjustment_click(&mut app, &ctx, &mut clock, "렌즈 프로파일");
    adjustment_click(&mut app, &ctx, &mut clock, "촬영 정보로 프로파일 찾기");
    lens_adjustment_scroll(&mut app, &ctx, &mut clock, -360.);
    lens_focal_drag(&mut app, &ctx, &mut clock, "2.8", 7.);
    lens_focal_drag(&mut app, &ctx, &mut clock, "10.0", 50.);
    adjustment_click(&mut app, &ctx, &mut clock, "주변부 밝기 보정값 계산");
    let calculated = app.tabs[0].editor.document.layers[0]
        .adjustments
        .lens_profile
        .clone()
        .unwrap();
    assert_eq!(calculated.vignetting_query, Some([35., 3.5, 15.]));
    assert!(!calculated.vignetting_enabled);
    lens_focal_drag(&mut app, &ctx, &mut clock, "3.5", -33.);
    adjustment_click(&mut app, &ctx, &mut clock, "주변부 밝기 보정값 계산");
    assert_eq!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile,
        Some(calculated)
    );
    lens_adjustment_scroll(&mut app, &ctx, &mut clock, 240.);
    adjustment_click(&mut app, &ctx, &mut clock, "프로파일 주변부 밝기 보정");
    let profile = app.tabs[0].editor.document.layers[0]
        .adjustments
        .lens_profile
        .clone()
        .unwrap();
    assert!(profile.vignetting_enabled);
    adjustment_click(&mut app, &ctx, &mut clock, "취소");
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .is_none()
    );
    assert!(!app.tabs[0].editor.can_undo());
    app.open_adjustments();
    app.tabs[0].editor.document.layers[0]
        .adjustments
        .lens_profile = Some(profile);
    adjustment_click(&mut app, &ctx, &mut clock, "적용");
    assert_eq!(app.tabs[0].editor.history().count(), 1);
    let changed = crate::precision::composite(&app.tabs[0].editor.document);
    assert_ne!(changed, *source);
    assert!(Arc::ptr_eq(
        app.tabs[0].editor.document.layers[0]
            .raw_source
            .as_ref()
            .unwrap(),
        &raw
    ));
    assert!(Arc::ptr_eq(
        app.tabs[0].editor.document.layers[0]
            .precision
            .as_ref()
            .unwrap(),
        &source
    ));
    app.tabs[0].editor.undo();
    assert_eq!(
        crate::precision::composite(&app.tabs[0].editor.document),
        *source
    );
    app.tabs[0].editor.redo();
    assert_eq!(
        crate::precision::composite(&app.tabs[0].editor.document),
        changed
    );
}

#[test]
fn lens_catalog_ui_search_clear_keeps_selected_profile_and_raw_source() {
    let (mut app, ctx, mut clock) = input_app(Tool::Brush);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let db = Arc::new(
        crate::lens_profile::load(&root.join("tests/fixtures/lens-profiles/synthetic.xml"))
            .unwrap(),
    );
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("lens-profile-library"), db));
    let doc = files::open(&root.join("tests/fixtures/raw-metadata/metadata-le.dng"))
        .unwrap()
        .document;
    let original = doc.layers[0].precision.clone().unwrap();
    let raw = doc.layers[0].raw_source.clone().unwrap();
    app.tabs[0].editor = Editor::new(doc, false);
    app.open_adjustments();
    adjustment_click(&mut app, &ctx, &mut clock, "렌즈 프로파일");
    adjustment_click(&mut app, &ctx, &mut clock, "렌즈 이름·제조사·초점거리 검색");
    adjustment_frame(
        &mut app,
        &ctx,
        &mut clock,
        vec![egui::Event::Text("pTlEnS".into())],
        "pTlEnS",
    );
    adjustment_click(&mut app, &ctx, &mut clock, "교정값 선택");
    assert!(
        adjustment_frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![],
            "Test Optics Known Prime 35mm · 35 mm · Poly3"
        )
        .is_none()
    );
    adjustment_click(
        &mut app,
        &ctx,
        &mut clock,
        "Test Optics PTLens Test · 35 mm · Ptlens",
    );
    let chosen = app.tabs[0].editor.document.layers[0]
        .adjustments
        .lens_profile
        .clone()
        .unwrap();
    adjustment_click(&mut app, &ctx, &mut clock, "불러온 프로파일 비우기");
    assert!(
        ctx.data(|d| d
            .get_temp::<Arc<crate::lens_profile::Database>>(egui::Id::new("lens-profile-library")))
            .is_none()
    );
    assert_eq!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile,
        Some(chosen)
    );
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
    adjustment_click(&mut app, &ctx, &mut clock, "취소");
    assert!(
        app.tabs[0].editor.document.layers[0]
            .adjustments
            .lens_profile
            .is_none()
    );
    assert!(!app.tabs[0].editor.can_undo());
}
