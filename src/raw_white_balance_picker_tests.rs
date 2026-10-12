use super::*;
fn source() -> Arc<Source> {
    crate::files::open(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/raw/linear-32-o1.dng"),
    )
    .unwrap()
    .document
    .layers[0]
        .raw_source
        .clone()
        .unwrap()
}
fn queued(picker: &mut Picker, settings: &Settings) -> mpsc::Sender<SampleResult> {
    picker.enabled = true;
    picker.queue(settings, [0.5; 2]);
    let serial = picker.request.as_ref().unwrap().serial;
    let (sender, receiver) = mpsc::channel();
    picker.job = Some((serial, receiver));
    sender
}
#[test]
fn raw_white_balance_picker_latest_serial_real_worker_cancel_changed_settings_radius_and_error() {
    let source = source();
    let bytes = source.bytes.clone();
    let ctx = egui::Context::default();
    let mut settings = Settings {
        exposure: 1.5,
        gains_fourth: Some(8.),
        white_balance: WhiteBalance::Temperature,
        temperature: 3500.,
        tint: 50.,
        ..Default::default()
    };
    let before = settings.clone();
    let mut picker = Picker::new();
    let sender = queued(&mut picker, &settings);
    picker.queue(&settings, [0.9, 0.1]);
    let expected = source.white_balance_at([0.9, 0.1], 2).unwrap();
    sender
        .send(Ok(crate::raw_input::BalanceSample {
            gains: [4., 1., 3.],
            gains_fourth: Some(4.),
        }))
        .unwrap();
    picker.poll(&source, &mut settings, &ctx);
    assert_eq!(settings, before);
    assert!(picker.busy() && picker.job.is_some());
    let deadline = Instant::now() + Duration::from_secs(5);
    while picker.busy() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
        picker.poll(&source, &mut settings, &ctx);
    }
    assert_eq!(settings.gains, expected.gains);
    assert_eq!(settings.white_balance, WhiteBalance::Custom);
    assert_eq!(settings.exposure, 1.5);
    assert_eq!(settings.temperature, 3500.);
    assert_eq!(settings.tint, 50.);
    assert!(settings.gains_fourth.is_none());
    assert!(picker.job.is_none());
    for change in 0..3 {
        let sender = queued(&mut picker, &settings);
        let mut current = settings.clone();
        match change {
            0 => picker.enabled = false,
            1 => current.exposure = 2.,
            _ => picker.radius = 3,
        }
        let unchanged = current.clone();
        sender
            .send(Ok(crate::raw_input::BalanceSample {
                gains: [8., 1., 9.],
                gains_fourth: Some(3.),
            }))
            .unwrap();
        picker.poll(&source, &mut current, &ctx);
        assert_eq!(current, unchanged);
        assert!(!picker.busy() && picker.job.is_none());
    }
    let sender = queued(&mut picker, &settings);
    sender.send(Err("명시적 샘플 오류".into())).unwrap();
    let unchanged = settings.clone();
    picker.poll(&source, &mut settings, &ctx);
    assert_eq!(settings, unchanged);
    assert_eq!(picker.error.as_deref(), Some("명시적 샘플 오류"));
    assert!(!picker.busy());
    let mut found = false;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(560., 320.),
            )),
            ..Default::default()
        },
        |ui| {
            picker.ui(ui);
        },
    );
    for shape in &output.shapes {
        if let egui::Shape::Text(text) = &shape.shape
            && text.galley.job.text == "명시적 샘플 오류"
        {
            found = true;
        }
    }
    output.textures_delta.clear();
    assert!(found);
    let sender = queued(&mut picker, &settings);
    drop(sender);
    picker.poll(&source, &mut settings, &ctx);
    assert!(picker.error.as_ref().unwrap().contains("중단"));
    assert_eq!(settings, unchanged);
    assert!(Arc::ptr_eq(&source.bytes, &bytes));
    assert_eq!(source.record.settings, Settings::default());
}
