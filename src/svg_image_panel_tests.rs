use super::*;
fn root() -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/svg-assets/panel");
    std::fs::create_dir_all(&root).unwrap();
    root
}
fn source() -> String {
    "<svg xmlns='http://www.w3.org/2000/svg' width='8' height='6'><image width='8' height='6'/></svg>".into()
}
fn wait(panel: &mut Panel, ctx: &egui::Context, source: &mut String) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while panel.busy() {
        panel.poll(ctx, source, Some(1));
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
#[test]
fn svg_image_worker_preserves_source_on_cancel_stale_selection_edits_errors_disconnect_and_retry() {
    let ctx = egui::Context::default();
    let original = source();
    let mut current = original.clone();
    let mut panel = Panel::default();
    for select in [Some(2), None] {
        let (sender, receiver) = mpsc::channel();
        panel.pending = Some(Pending {
            source: original.clone(),
            index: 1,
            receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
        });
        panel.poll(&ctx, &mut current, select);
        assert!(!panel.busy());
        assert_eq!(current, original);
        assert!(sender.send(Ok(None)).is_err());
    }
    let (sender, receiver) = mpsc::channel();
    panel.pending = Some(Pending {
        source: original.clone(),
        index: 1,
        receiver,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    current = current.replace("width='8'", "width='9'");
    let modified = current.clone();
    panel.poll(&ctx, &mut current, Some(1));
    assert!(!panel.busy());
    assert_eq!(current, modified);
    assert!(sender.send(Ok(None)).is_err());
    current = original.clone();
    let (sender, receiver) = mpsc::channel();
    panel.pending = Some(Pending {
        source: original.clone(),
        index: 1,
        receiver,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    panel.close();
    assert!(sender.send(Ok(None)).is_err());
    assert_eq!(current, original);
    let (sender, receiver) = mpsc::channel();
    panel.pending = Some(Pending {
        source: original.clone(),
        index: 1,
        receiver,
        cancelled: Arc::new(AtomicBool::new(false)),
    });
    drop(sender);
    panel.poll(&ctx, &mut current, Some(1));
    assert!(!panel.busy());
    assert!(panel.error.is_some());
    assert_eq!(current, original);
    panel.start(&ctx, &current, 1, Some(root().join("missing.png")));
    wait(&mut panel, &ctx, &mut current);
    assert!(panel.error.is_some());
    assert_eq!(current, original);
    let path = root().join("owned.png");
    image::RgbaImage::from_pixel(8, 6, image::Rgba([31, 180, 59, 255]))
        .save(&path)
        .unwrap();
    panel.start(&ctx, &current, 1, Some(path));
    wait(&mut panel, &ctx, &mut current);
    assert!(panel.error.is_none());
    assert!(panel.preview.is_some());
    assert_ne!(current, original);
    assert_eq!(
        crate::vector::render_svg(&current)
            .unwrap()
            .get_pixel(3, 3)
            .0,
        [31, 180, 59, 255]
    );
}
#[test]
fn svg_image_replacement_headless_button_starts_worker_and_displays_preview_without_dialog() {
    use egui::{Event, PointerButton, Pos2, Rect, Vec2};
    let ctx = egui::Context::default();
    let root = root();
    let path = root.join("button.png");
    image::RgbaImage::from_pixel(8, 6, image::Rgba([21, 64, 196, 255]))
        .save(&path)
        .unwrap();
    let mut panel = Panel {
        path: path.to_string_lossy().into_owned(),
        ..Default::default()
    };
    let mut current = source();
    let mut time = 0.;
    let mut frame = |panel: &mut Panel, current: &mut String, events| {
        time += 0.02;
        let mut output = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(500., 450.))),
                events,
                ..Default::default()
            },
            |ui| {
                panel.poll(&ctx, current, Some(1));
                panel.ui(ui, current, 1);
            },
        );
        output.textures_delta.clear();
        output
    };
    frame(&mut panel, &mut current, vec![]);
    let output = frame(&mut panel, &mut current, vec![]);
    let position = output
        .shapes
        .iter()
        .find_map(|shape| {
            if let egui::Shape::Text(text) = &shape.shape {
                if text.galley.job.text == "이미지 교체" {
                    Some(text.galley.rect.translate(text.pos.to_vec2()).center())
                } else {
                    None
                }
            } else {
                None
            }
        })
        .unwrap();
    frame(
        &mut panel,
        &mut current,
        vec![Event::PointerMoved(position)],
    );
    frame(
        &mut panel,
        &mut current,
        vec![Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Default::default(),
        }],
    );
    frame(
        &mut panel,
        &mut current,
        vec![Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }],
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while panel.busy() {
        frame(&mut panel, &mut current, vec![]);
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(panel.preview.is_some());
    assert_eq!(
        crate::vector::render_svg(&current)
            .unwrap()
            .get_pixel(3, 3)
            .0,
        [21, 64, 196, 255]
    );
}

#[test]
fn svg_image_save_worker_reports_success_and_errors_without_editing_source_and_cancel_flags_stop_late_work()
 {
    let root = root();
    let path = root.join("save.png");
    image::RgbaImage::from_pixel(8, 6, image::Rgba([21, 64, 196, 255]))
        .save(&path)
        .unwrap();
    let source = format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='8' height='6'><image width='8' height='6' href='data:image/png;base64,{}' data-rphoto-image-name='save.png'/></svg>",
        base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            std::fs::read(&path).unwrap()
        )
    );
    let original = source.clone();
    let mut source = source;
    let ctx = egui::Context::default();
    let mut panel = Panel::default();
    let target = root.join("saved.png");
    panel.save(
        &ctx,
        &source,
        1,
        crate::svg_assets::ImageFile::Original,
        Some(target.clone()),
    );
    wait(&mut panel, &ctx, &mut source);
    assert!(panel.saved.is_some());
    assert!(panel.error.is_none());
    assert_eq!(source, original);
    assert_eq!(std::fs::read(target).unwrap(), std::fs::read(path).unwrap());
    panel.save(
        &ctx,
        &source,
        1,
        crate::svg_assets::ImageFile::Display,
        Some(root.join("missing-directory/out.png")),
    );
    wait(&mut panel, &ctx, &mut source);
    assert!(panel.error.is_some());
    assert!(panel.saved.is_none());
    assert_eq!(source, original);
    let (sender, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    panel.pending = Some(Pending {
        source: source.clone(),
        index: 1,
        receiver,
        cancelled: cancelled.clone(),
    });
    panel.close();
    assert!(cancelled.load(Ordering::Acquire));
    assert!(sender.send(Ok(None)).is_err());
    assert_eq!(source, original);
    let (sender, receiver) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    panel.pending = Some(Pending {
        source: source.clone(),
        index: 1,
        receiver,
        cancelled: cancelled.clone(),
    });
    panel.poll(&ctx, &mut source, Some(2));
    assert!(cancelled.load(Ordering::Acquire));
    assert!(sender.send(Ok(None)).is_err());
    assert_eq!(source, original);
}
