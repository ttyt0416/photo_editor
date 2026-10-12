use super::*;
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}
fn out(name: &str) -> PathBuf {
    let dir = root().join("qa-output/lens-library").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
fn wait(ctx: &egui::Context) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        poll(ctx);
        if !status(ctx).0 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "catalog worker timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
fn db(ctx: &egui::Context) -> Option<Arc<Database>> {
    ctx.data(|d| d.get_temp(egui::Id::new("lens-profile-library")))
}
#[test]
fn lens_library_background_import_restart_retry_failure_clear_and_unicode_paths() {
    let dir = out("restart");
    let path = dir.join("settings.json");
    Settings {
        version: 1,
        watch: true,
        paths: vec![],
    }
    .write(&path)
    .unwrap();
    let unicode = dir.join("한글 교정.xml");
    std::fs::copy(
        root().join("tests/fixtures/lens-profiles/synthetic.xml"),
        &unicode,
    )
    .unwrap();
    let paths = vec![unicode, root().join("tests/fixtures/lens-profiles/tca.xml")];
    let ctx = egui::Context::default();
    initialize(&ctx, path.clone());
    import(&ctx, None, paths.clone());
    assert!(status(&ctx).0);
    assert!(db(&ctx).is_none());
    wait(&ctx);
    assert!(status(&ctx).2.is_none());
    let original = db(&ctx).unwrap();
    assert_eq!(original.profiles.len(), 13);
    assert_eq!(Settings::read(&path).unwrap().paths, paths);
    let restarted = egui::Context::default();
    initialize(&restarted, path.clone());
    wait(&restarted);
    assert_eq!(db(&restarted).unwrap().profiles, original.profiles);
    let before = std::fs::read(&path).unwrap();
    std::fs::write(dir.join("persisted-before-clear.json"), &before).unwrap();
    let missing_settings = dir.join("missing-file-settings.json");
    Settings {
        version: 1,
        watch: true,
        paths: vec![dir.join("missing.xml")],
    }
    .write(&missing_settings)
    .unwrap();
    let saved_missing = std::fs::read(&missing_settings).unwrap();
    let missing_ctx = egui::Context::default();
    initialize(&missing_ctx, missing_settings.clone());
    wait(&missing_ctx);
    assert!(db(&missing_ctx).is_none());
    assert!(status(&missing_ctx).2.is_some());
    assert!(status(&missing_ctx).1);
    assert_eq!(std::fs::read(&missing_settings).unwrap(), saved_missing);
    import(&ctx, Some(original.clone()), vec![dir.join("missing.xml")]);
    wait(&ctx);
    assert!(status(&ctx).2.is_some());
    assert!(Arc::ptr_eq(&db(&ctx).unwrap(), &original));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    retry(&ctx);
    wait(&ctx);
    assert_eq!(db(&ctx).unwrap().profiles, original.profiles);
    clear(&ctx);
    assert!(db(&ctx).is_none());
    assert!(!status(&ctx).1);
    assert!(Settings::read(&path).unwrap().paths.is_empty());
    let restarted = egui::Context::default();
    initialize(&restarted, path);
    assert!(!status(&restarted).0);
    assert!(db(&restarted).is_none());
}
#[test]
fn lens_library_invalid_settings_preserved_and_failed_save_reports_loaded_session() {
    let dir = out("errors");
    let path = dir.join("corrupt.json");
    let source = br#"{"version":99,"paths":[]}"#;
    std::fs::write(&path, source).unwrap();
    let ctx = egui::Context::default();
    initialize(&ctx, path.clone());
    assert!(status(&ctx).2.is_some());
    import(
        &ctx,
        None,
        vec![root().join("tests/fixtures/lens-profiles/tca.xml")],
    );
    wait(&ctx);
    assert!(db(&ctx).is_some());
    assert!(status(&ctx).2.unwrap().contains("실행 중"));
    assert_eq!(std::fs::read(&path).unwrap(), source);
    clear(&ctx);
    assert_eq!(std::fs::read(&path).unwrap(), source);
    for settings in [
        Settings {
            version: 1,
            watch: true,
            paths: vec![PathBuf::from("relative.xml")],
        },
        Settings {
            version: 1,
            watch: true,
            paths: vec![root().join("x"); 257],
        },
    ] {
        assert!(settings.write(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), source);
    }
    let too_large = dir.join("large.json");
    std::fs::write(&too_large, vec![b' '; MAX_SETTINGS + 1]).unwrap();
    assert!(Settings::read(&too_large).is_err());
    let directory = dir.join("cannot-replace-directory");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("keep.txt"), b"preserved").unwrap();
    let mut state = Library {
        settings: Some(directory.clone()),
        ..Default::default()
    };
    let (send, receive) = mpsc::channel();
    state.pending = Some(Pending {
        cancelled: Arc::new(AtomicBool::new(false)),

        paths: vec![root().join("tests/fixtures/lens-profiles/tca.xml")],
        receive,
        replace: false,
        automatic: false,
    });
    send.send(Ok(Imported {
        database: Arc::unwrap_or_clone(empty()),
        snapshot: Default::default(),
    }))
    .unwrap();
    assert!(state.finish().is_some());
    assert!(state.error.is_some());
    assert_eq!(
        std::fs::read(directory.join("keep.txt")).unwrap(),
        b"preserved"
    );
}
#[test]
fn lens_library_cancel_clear_late_results_disconnect_and_failure_keep_committed_paths() {
    let ctx = egui::Context::default();
    let value = library(&ctx);
    let paths = vec![root().join("old.xml")];
    value.lock().unwrap().paths = paths.clone();
    let (send, receive) = mpsc::channel();
    value.lock().unwrap().pending = Some(Pending {
        cancelled: Arc::new(AtomicBool::new(false)),

        paths: vec![root().join("new.xml")],
        receive,
        replace: false,
        automatic: false,
    });
    cancel(&ctx);
    assert!(
        send.send(Ok(Imported {
            database: Arc::unwrap_or_clone(empty()),
            snapshot: Default::default()
        }))
        .is_err()
    );
    poll(&ctx);
    assert_eq!(value.lock().unwrap().paths, paths);
    assert!(db(&ctx).is_none());
    value
        .lock()
        .unwrap()
        .worker_running
        .store(true, Ordering::Release);
    assert!(
        value
            .lock()
            .unwrap()
            .begin(empty(), vec![root().join("new.xml")], false, &ctx)
            .is_err()
    );
    value
        .lock()
        .unwrap()
        .worker_running
        .store(false, Ordering::Release);
    assert!(
        empty()
            .import_cancellable(&[root().join("missing.xml")], &AtomicBool::new(true))
            .unwrap_err()
            .contains("취소")
    );

    let (send, receive) = mpsc::channel();
    value.lock().unwrap().pending = Some(Pending {
        cancelled: Arc::new(AtomicBool::new(false)),

        paths: vec![root().join("new.xml")],
        receive,
        replace: false,
        automatic: false,
    });
    clear(&ctx);
    assert!(
        send.send(Ok(Imported {
            database: Arc::unwrap_or_clone(empty()),
            snapshot: Default::default()
        }))
        .is_err()
    );
    poll(&ctx);
    assert!(value.lock().unwrap().paths.is_empty());
    value.lock().unwrap().paths = paths.clone();
    let (send, receive) = mpsc::channel();
    value.lock().unwrap().pending = Some(Pending {
        cancelled: Arc::new(AtomicBool::new(false)),

        paths: vec![],
        receive,
        replace: false,
        automatic: false,
    });
    drop(send);
    poll(&ctx);
    assert_eq!(value.lock().unwrap().paths, paths);
    assert!(status(&ctx).2.unwrap().contains("중단"));
    let (send, receive) = mpsc::channel();
    value.lock().unwrap().pending = Some(Pending {
        cancelled: Arc::new(AtomicBool::new(false)),

        paths: vec![],
        receive,
        replace: false,
        automatic: false,
    });
    send.send(Err("invalid XML".into())).unwrap();
    poll(&ctx);
    assert_eq!(value.lock().unwrap().paths, paths);
    assert_eq!(status(&ctx).2.as_deref(), Some("invalid XML"));
}

fn ui_frame(
    ctx: &egui::Context,
    profile: &mut Option<super::super::Profile>,
    clock: &mut f64,
    events: Vec<egui::Event>,
    target: &str,
) -> Option<egui::Pos2> {
    *clock += 0.3;
    let mut output = ctx.run_ui(
        egui::RawInput {
            time: Some(*clock),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000., 900.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            super::super::ui(profile, None, ui);
        },
    );
    let result = output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text == target => {
            Some(text.pos + egui::Vec2::splat(5.))
        }
        _ => None,
    });
    output.textures_delta.clear();
    result
}
fn ui_click(
    ctx: &egui::Context,
    profile: &mut Option<super::super::Profile>,
    clock: &mut f64,
    target: &str,
) {
    ui_frame(ctx, profile, clock, vec![], target);
    let point =
        ui_frame(ctx, profile, clock, vec![], target).unwrap_or_else(|| panic!("missing {target}"));
    for pressed in [true, false] {
        ui_frame(
            ctx,
            profile,
            clock,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            target,
        );
    }
}
#[test]
fn lens_library_ui_loading_cancel_and_retry_do_not_change_photo_profile() {
    let ctx = egui::Context::default();
    let mut clock = 0.;
    let value = library(&ctx);
    let paths = vec![root().join("tests/fixtures/lens-profiles/synthetic.xml")];
    let loaded = Arc::new(super::super::load(&paths[0]).unwrap());
    let mut profile = Some(loaded.profiles[0].clone());
    let before = profile.clone();
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("lens-profile-library"), loaded));
    let (send, receive) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut state = value.lock().unwrap();
        state.paths = paths.clone();
        state.pending = Some(Pending {
            cancelled: cancelled.clone(),
            paths,
            receive,
            replace: false,
            automatic: false,
        });
    }
    ui_click(&ctx, &mut profile, &mut clock, "렌즈 프로파일");
    assert!(
        ui_frame(
            &ctx,
            &mut profile,
            &mut clock,
            vec![],
            "렌즈 목록을 불러오는 중…"
        )
        .is_some()
    );
    ui_click(&ctx, &mut profile, &mut clock, "렌즈 목록 불러오기 취소");
    assert!(cancelled.load(Ordering::Acquire));
    assert!(
        send.send(Ok(Imported {
            database: Arc::unwrap_or_clone(empty()),
            snapshot: Default::default()
        }))
        .is_err()
    );
    assert_eq!(profile, before);
    ui_click(&ctx, &mut profile, &mut clock, "렌즈 목록 다시 불러오기");
    wait(&ctx);
    assert_eq!(profile, before);
    assert_eq!(db(&ctx).unwrap().profiles.len(), 5);
    ui_click(&ctx, &mut profile, &mut clock, "불러온 프로파일 비우기");
    assert!(db(&ctx).is_none());
    assert_eq!(profile, before);
}

#[test]
fn lens_orientation_ui_manual_direction_preserves_xml_and_other_correction_settings() {
    let ctx = egui::Context::default();
    let mut clock = 0.;
    let db =
        super::super::load(&root().join("tests/fixtures/lens-profiles/synthetic.xml")).unwrap();
    let mut profile = Some(db.profiles[0].clone());
    let before = profile.clone().unwrap();
    ui_click(&ctx, &mut profile, &mut clock, "렌즈 프로파일");
    ui_click(&ctx, &mut profile, &mut clock, "기본 방향");
    ui_click(&ctx, &mut profile, &mut clock, "시계 방향 90°");
    assert_eq!(profile.as_ref().unwrap().orientation, 6);
    assert!(Arc::ptr_eq(&profile.as_ref().unwrap().xml, &before.xml));
    let mut expected = before.clone();
    expected.orientation = 6;
    assert_eq!(profile, Some(expected));
    ui_click(&ctx, &mut profile, &mut clock, "시계 방향 90°");
    ui_click(&ctx, &mut profile, &mut clock, "기본 방향");
    assert_eq!(profile, Some(before));
}

fn watch_once(ctx: &egui::Context) {
    library(ctx).lock().unwrap().watcher.due();
    poll(ctx);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        poll(ctx);
        let shared = library(ctx);
        let state = shared.lock().unwrap();
        let busy = state.watcher.busy()
            || state.pending.is_some()
            || state.worker_running.load(Ordering::Acquire);
        drop(state);
        if !busy {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
#[test]
fn lens_library_watch_changed_deleted_corrupt_repaired_same_metadata_and_current_profile() {
    let dir = out("watch");
    let file = dir.join("한글 렌즈.xml");
    let original =
        std::fs::read_to_string(root().join("tests/fixtures/lens-profiles/synthetic.xml")).unwrap();
    std::fs::write(&file, &original).unwrap();
    let second = root().join("tests/fixtures/lens-profiles/tca.xml");
    let settings = dir.join("settings.json");
    Settings {
        version: 1,
        paths: vec![],
        watch: true,
    }
    .write(&settings)
    .unwrap();
    let ctx = egui::Context::default();
    initialize(&ctx, settings.clone());
    import(&ctx, None, vec![file.clone(), second]);
    wait(&ctx);
    let before = db(&ctx).unwrap();
    let current = before.profiles[0].clone();
    let current_xml = current.xml.clone();
    let previous_settings = std::fs::read(&settings).unwrap();
    watch_once(&ctx);
    assert!(Arc::ptr_eq(&db(&ctx).unwrap(), &before));
    let modified = std::fs::metadata(&file).unwrap().modified().unwrap();
    let replacement = original.replacen("k1=\"0.02\"", "k1=\"0.08\"", 1);
    assert_eq!(replacement.len(), original.len());
    std::fs::write(&file, &replacement).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&file)
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(modified))
        .unwrap();
    watch_once(&ctx);
    let changed = db(&ctx).unwrap();
    assert_eq!(changed.profiles[0].coefficients[0], 0.08);
    assert_eq!(changed.profiles.len(), before.profiles.len());
    assert!(status(&ctx).2.is_none());
    assert_eq!(current.coefficients[0], 0.02);
    assert!(Arc::ptr_eq(&current.xml, &current_xml));
    assert_eq!(current.xml.as_str(), current_xml.as_str());
    std::fs::write(
        dir.join("before.json"),
        serde_json::to_vec_pretty(&before.profiles).unwrap(),
    )
    .unwrap();
    std::fs::write(
        dir.join("changed.json"),
        serde_json::to_vec_pretty(&changed.profiles).unwrap(),
    )
    .unwrap();
    // Remove only this test's own XML, leaving the last committed catalog and selected source intact.
    std::fs::remove_file(&file).unwrap();
    watch_once(&ctx);
    assert!(status(&ctx).2.is_some());
    assert!(Arc::ptr_eq(&db(&ctx).unwrap(), &changed));
    assert_eq!(std::fs::read(&settings).unwrap(), previous_settings);
    std::fs::write(&file, "<lensdatabase>").unwrap();
    watch_once(&ctx);
    assert!(status(&ctx).2.is_some());
    assert!(Arc::ptr_eq(&db(&ctx).unwrap(), &changed));
    let error = status(&ctx).2;
    watch_once(&ctx);
    assert_eq!(status(&ctx).2, error);
    assert!(Arc::ptr_eq(&db(&ctx).unwrap(), &changed));
    std::fs::write(&file, &original).unwrap();
    watch_once(&ctx);
    let repaired = db(&ctx).unwrap();
    assert_eq!(repaired.profiles, before.profiles);
    assert!(status(&ctx).2.is_none());
    assert_eq!(current.xml.as_str(), current_xml.as_str());
    assert_eq!(std::fs::read(&settings).unwrap(), previous_settings);
    clear(&ctx);
    watch_once(&ctx);
    assert!(db(&ctx).is_none());
    assert!(!status(&ctx).1);
}
#[test]
fn lens_library_watch_ui_preference_restart_disable_and_cancel_automatic_reload() {
    let dir = out("watch-ui");
    let settings = dir.join("settings.json");
    let file = dir.join("own.xml");
    let original =
        std::fs::read_to_string(root().join("tests/fixtures/lens-profiles/synthetic.xml")).unwrap();
    std::fs::write(&file, &original).unwrap();
    // Previous v1 settings have no watch field; the default is enabled.
    std::fs::write(
        &settings,
        serde_json::to_vec(&serde_json::json!({"version":1,"paths":[file]})).unwrap(),
    )
    .unwrap();
    let ctx = egui::Context::default();
    initialize(&ctx, settings.clone());
    wait(&ctx);
    assert!(Settings::read(&settings).unwrap().watch);
    let mut profile = Some(db(&ctx).unwrap().profiles[0].clone());
    let source = profile.clone();
    let mut clock = 0.;
    ui_click(&ctx, &mut profile, &mut clock, "렌즈 프로파일");
    ui_click(&ctx, &mut profile, &mut clock, "XML 파일 변경 자동 반영");
    assert!(!library(&ctx).lock().unwrap().watch);
    assert!(!Settings::read(&settings).unwrap().watch);
    assert_eq!(profile, source);
    let restarted = egui::Context::default();
    initialize(&restarted, settings.clone());
    wait(&restarted);
    assert!(!library(&restarted).lock().unwrap().watch);
    let before = db(&restarted).unwrap();
    std::fs::write(&file, original.replacen("k1=\"0.02\"", "k1=\"0.09\"", 1)).unwrap();
    watch_once(&restarted);
    assert!(Arc::ptr_eq(&db(&restarted).unwrap(), &before));
    ui_click(&restarted, &mut profile, &mut clock, "렌즈 프로파일");
    ui_click(
        &restarted,
        &mut profile,
        &mut clock,
        "XML 파일 변경 자동 반영",
    );
    watch_once(&restarted);
    assert_eq!(db(&restarted).unwrap().profiles[0].coefficients[0], 0.09);
    assert_eq!(profile, source);
    let (send, receive) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    library(&restarted).lock().unwrap().pending = Some(Pending {
        receive,
        cancelled: cancelled.clone(),
        paths: vec![file.clone()],
        replace: true,
        automatic: true,
    });
    ui_click(
        &restarted,
        &mut profile,
        &mut clock,
        "XML 파일 변경 자동 반영",
    );
    assert!(cancelled.load(Ordering::Acquire));
    assert!(
        send.send(Ok(Imported {
            database: Arc::unwrap_or_clone(empty()),
            snapshot: Default::default()
        }))
        .is_err()
    );
    assert!(!status(&restarted).0);
    clear(&restarted);
    assert!(db(&restarted).is_none());
    assert_eq!(profile, source);
}
