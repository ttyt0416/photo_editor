fn main_menu_frame(
    app: &mut PhotoEditor,
    ctx: &egui::Context,
    clock: &mut f64,
    size: Vec2,
    events: Vec<egui::Event>,
) -> Vec<(String, Rect)> {
    *clock += 0.1;
    let mut out = ctx.run_ui(
        egui::RawInput {
            time: Some(*clock),
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| app.top_ui(ui, ctx),
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
fn main_menu_click(
    app: &mut PhotoEditor,
    ctx: &egui::Context,
    clock: &mut f64,
    size: Vec2,
    label: &str,
) {
    main_menu_frame(app, ctx, clock, size, vec![]);
    let labels = main_menu_frame(app, ctx, clock, size, vec![]);
    let at = labels
        .iter()
        .find(|(s, _)| s == label)
        .unwrap_or_else(|| panic!("missing {label}: {labels:?}"))
        .1
        .center();
    for pressed in [true, false] {
        main_menu_frame(
            app,
            ctx,
            clock,
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
#[test]
fn main_menu_all_commands_current_keys_checked_states_guards_and_manifest() {
    use crate::{commands::Command, shortcuts::Action};
    let (mut app, ctx, mut clock) = input_app(Tool::Move);
    let entries = app.menu_entries();
    assert_eq!(entries.len(), 89);
    for category in crate::commands::MENU_CATEGORIES {
        assert!(entries.iter().any(|e| e.category == *category));
    }
    for e in crate::commands::catalog()
        .into_iter()
        .filter(|e| e.category != "도구")
    {
        assert_eq!(
            entries
                .iter()
                .filter(|menu| menu.command == e.command)
                .count(),
            1
        );
        let actual = entries
            .iter()
            .find(|menu| menu.command == e.command)
            .unwrap();
        assert_eq!(actual.disabled, app.command_reason(e.command));
    }
    assert!(entries.iter().any(|e|e.command==Command::Action(Action::CommandPalette)&&e.shortcut=="Ctrl+F"));
    assert!(entries.iter().any(
        |e| e.command == Command::Action(Action::Redo) && e.shortcut == "Ctrl+Shift+Z / Ctrl+Y"
    ));
    for command in [
        Command::Tools,
        Command::GroupedTools,
        Command::Layers,
        Command::Properties,
        Command::History,
        Command::Navigator,
        Command::Colors,
        Command::MaskView,
        Command::MaskOverlay,
        Command::LeftPanels,
        Command::PixelGrid,
        Command::Histogram,
        Command::Rulers,
        Command::Guides,
        Command::Grid,
        Command::Snap,
        Command::SnapGuides,
        Command::SnapGrid,
        Command::SnapLayers,
        Command::SnapDocument,
    ] {
        let before = app.command_checked(command).unwrap();
        app.execute_command(command, &ctx);
        assert_eq!(app.command_checked(command), Some(!before), "{command:?}");
    }
    for mode in [
        crate::display_sampling::Mode::Nearest,
        crate::display_sampling::Mode::Smooth,
        crate::display_sampling::Mode::HighQuality,
    ] {
        app.execute_command(Command::DisplaySampling(mode), &ctx);
        assert_eq!(
            app.command_checked(Command::DisplaySampling(mode)),
            Some(true)
        );
    }
    app.workspace
        .layout
        .shortcuts
        .assign(
            Command::NewLayer.shortcut_action().unwrap(),
            Some((Key::F10, Modifiers::NONE)),
        )
        .unwrap();
    let size = Vec2::new(1320., 860.);
    main_menu_click(&mut app, &ctx, &mut clock, size, "레이어");
    let labels = main_menu_frame(&mut app, &ctx, &mut clock, size, vec![]);
    assert!(labels.iter().any(|(s, _)| s == "F10"));
    let before = app.tab().unwrap().editor.history().count();
    main_menu_click(&mut app, &ctx, &mut clock, size, "새 레이어");
    assert_eq!(app.tab().unwrap().editor.document.layers.len(), 2);
    assert_eq!(app.tab().unwrap().editor.history().count(), before + 1);
    app.execute_command(Command::Action(Action::Undo), &ctx);
    assert_eq!(app.tab().unwrap().editor.document.layers.len(), 1);
    app.execute_command(Command::Action(Action::Redo), &ctx);
    app.tab_mut()
        .unwrap()
        .editor
        .document
        .active_layer_mut()
        .unwrap()
        .locked = true;
    assert!(app.command_reason(Command::Delete).is_some());
    assert!(
        app.command_reason(Command::Action(Action::Transform))
            .is_some()
    );
    let revision = app.tab().unwrap().editor.revision;
    app.execute_command(Command::Delete, &ctx);
    assert_eq!(app.tab().unwrap().editor.revision, revision);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/menu-dispatch");
    std::fs::create_dir_all(&root).unwrap();
    let manifest:Vec<_>=app.menu_entries().into_iter().map(|e|serde_json::json!({"category":e.category,"label":e.label,"action":e.command.shortcut_action(),"shortcut":e.shortcut,"disabled":e.disabled,"checked":app.command_checked(e.command)})).collect();
    std::fs::write(
        root.join("catalog.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();
    app.tabs.clear();
    for command in [
        Command::NewLayer,
        Command::Psd,
        Command::Psb,
        Command::Resize,
        Command::Adjust,
    ] {
        assert!(app.command_reason(command).is_some());
    }
    for command in [
        Command::Tools,
        Command::About,
        Command::Licenses,
        Command::Action(Action::New),
    ] {
        assert!(app.command_reason(command).is_none());
    }
    app.about_panel.show(false);
    assert!(app.menu_entries().iter().all(|e| e.disabled.is_some()));
}
#[test]
fn main_menu_nine_categories_pointer_small_width_dpi_and_disabled_click() {
    use crate::commands::Command;
    for dpi in [1., 2.] {
        for size in [Vec2::new(1320., 1000.), Vec2::new(360., 480.)] {
            for category in crate::commands::MENU_CATEGORIES {
                let (mut app, ctx, mut clock) = input_app(Tool::Move);
                ctx.set_pixels_per_point(dpi);
                main_menu_frame(&mut app, &ctx, &mut clock, size, vec![]);
                main_menu_frame(&mut app, &ctx, &mut clock, size, vec![]);
                let labels = main_menu_frame(&mut app, &ctx, &mut clock, size, vec![]);
                for label in crate::commands::MENU_CATEGORIES {
                    let rect = labels
                        .iter()
                        .find(|(s, _)| s == *label)
                        .unwrap_or_else(|| {
                            panic!("missing header {label} at {size:?}/{dpi}: {labels:?}")
                        })
                        .1;
                    assert!(
                        rect.min.x >= 0. && rect.max.x <= size.x,
                        "{label} outside {size:?}: {rect:?}"
                    );
                }
                main_menu_click(&mut app, &ctx, &mut clock, size, category);
                let labels = main_menu_frame(&mut app, &ctx, &mut clock, size, vec![]);
                let expected: Vec<_> = app
                    .menu_entries()
                    .into_iter()
                    .filter(|e| e.category == *category)
                    .collect();
                assert!(
                    expected
                        .iter()
                        .any(|e| labels.iter().any(|(s, _)| s == e.label)),
                    "{category} did not open"
                );
                let mut seen: HashSet<_> = labels.iter().map(|(s, _)| s.clone()).collect();
                let mut labels = labels;
                for _ in 0..8 {
                    if expected.iter().all(|e| seen.contains(e.label)) {
                        break;
                    }
                    let at = labels
                        .iter()
                        .find(|(s, _)| expected.iter().any(|e| e.label == s))
                        .unwrap()
                        .1
                        .center();
                    main_menu_frame(
                        &mut app,
                        &ctx,
                        &mut clock,
                        size,
                        vec![
                            egui::Event::PointerMoved(at),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                phase: egui::TouchPhase::Move,
                                delta: Vec2::new(0., -300.),
                                modifiers: Modifiers::NONE,
                            },
                        ],
                    );
                    labels = main_menu_frame(&mut app, &ctx, &mut clock, size, vec![]);
                    seen.extend(labels.iter().map(|(s, _)| s.clone()));
                }
                for e in expected {
                    assert!(
                        seen.contains(e.label),
                        "missing {} in {category} at {size:?}/{dpi}, seen: {seen:?}",
                        e.label
                    );
                }
            }
        }
    }
    let (mut app, ctx, mut clock) = input_app(Tool::Move);
    app.tabs.clear();
    let size = Vec2::new(1320., 860.);
    main_menu_click(&mut app, &ctx, &mut clock, size, "파일");
    main_menu_click(&mut app, &ctx, &mut clock, size, "PSD로 저장…");
    assert!(app.psd_dialog.is_none());
    assert!(app.tabs.is_empty());
    assert!(app.command_reason(Command::Psd).is_some());
}
