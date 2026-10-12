#[test]
fn icc_output_native_checkbox_formats_toggle_preview_key_cancel_and_history() {
    fn frame(
        app: &mut PhotoEditor,
        ctx: &egui::Context,
        clock: &mut f64,
        events: Vec<egui::Event>,
        target: &str,
    ) -> Option<Pos2> {
        *clock += 1. / 60.;
        let mut out = ctx.run_ui(
            egui::RawInput {
                time: Some(*clock),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1320., 860.))),
                events,
                ..Default::default()
            },
            |_| app.dialogs(ctx),
        );
        let p = out.shapes.iter().find_map(|s| match &s.shape {
            egui::Shape::Text(t) if t.galley.job.text == target => Some(t.pos + Vec2::splat(3.)),
            _ => None,
        });
        out.textures_delta.clear();
        p
    }
    fn click(app: &mut PhotoEditor, ctx: &egui::Context, clock: &mut f64, target: &str) {
        for _ in 0..5 {
            frame(app, ctx, clock, vec![], target);
        }
        let p =
            frame(app, ctx, clock, vec![], target).unwrap_or_else(|| panic!("missing {target}"));
        frame(app, ctx, clock, vec![egui::Event::PointerMoved(p)], target);
        for pressed in [true, false] {
            frame(
                app,
                ctx,
                clock,
                vec![egui::Event::PointerButton {
                    pos: p,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                }],
                target,
            );
        }
    }
    for format in [
        image::ImageFormat::Png,
        image::ImageFormat::Jpeg,
        image::ImageFormat::Tiff,
        image::ImageFormat::WebP,
    ] {
        let (mut app, ctx, mut clock) = input_app(Tool::Move);
        app.export_dialog = true;
        app.export_format = files::EXPORT_FORMATS
            .iter()
            .position(|f| f.0 == format)
            .unwrap();
        let before = app.tab().unwrap().editor.document.layers[0].pixels.clone();
        let original = app.export_options.clone();
        assert!(original.include_icc);
        click(&mut app, &ctx, &mut clock, "작업 sRGB 프로파일 포함");
        assert!(!app.export_options.include_icc);
        assert_ne!(app.export_options, original);
        if format != image::ImageFormat::Tiff {
            assert!(app.export_options.include_exif);
            click(&mut app, &ctx, &mut clock, "촬영 EXIF 메타데이터 포함");
            assert!(!app.export_options.include_exif);
        } else {
            assert!(
                frame(
                    &mut app,
                    &ctx,
                    &mut clock,
                    vec![],
                    "촬영 EXIF 메타데이터 포함"
                )
                .is_none()
            );
        }
        assert!(app.export_options.include_xmp);
        click(&mut app, &ctx, &mut clock, "촬영 XMP 메타데이터 포함");
        assert!(!app.export_options.include_xmp);
        click(&mut app, &ctx, &mut clock, "취소");
        assert!(!app.export_dialog);
        assert_eq!(app.tab().unwrap().editor.history().count(), 0);
        assert!(std::sync::Arc::ptr_eq(
            &before,
            &app.tab().unwrap().editor.document.layers[0].pixels
        ));
    }
    let (mut app, ctx, mut clock) = input_app(Tool::Move);
    app.export_dialog = true;
    app.export_format = files::EXPORT_FORMATS.len() + 3;
    let before = app.tab().unwrap().editor.document.layers[0].pixels.clone();
    let original = app.export_options.clone();
    assert!(!original.jp2_icc);
    click(&mut app, &ctx, &mut clock, "작업 색상 프로파일 포함");
    assert!(app.export_options.jp2_icc);
    assert_ne!(app.export_options, original);
    app.export_format = files::EXPORT_FORMATS.len() + 4;
    for _ in 0..5 {
        frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![],
            "작업 색상 프로파일 포함",
        );
    }
    assert!(
        frame(
            &mut app,
            &ctx,
            &mut clock,
            vec![],
            "작업 색상 프로파일 포함"
        )
        .is_none()
    );
    click(&mut app, &ctx, &mut clock, "취소");
    assert!(!app.export_dialog);
    assert!(std::sync::Arc::ptr_eq(
        &before,
        &app.tab().unwrap().editor.document.layers[0].pixels
    ));
    assert_eq!(app.tab().unwrap().editor.history().count(), 0);
}
