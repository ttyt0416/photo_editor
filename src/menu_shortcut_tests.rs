use super::*;
#[test]
fn all_menu_shortcuts_cover_catalog_preserve_historical_settings_reject_corruption_and_roundtrip() {
    use crate::commands::Command;
    let entries = crate::commands::catalog();
    let defaults = Settings::default();
    assert_eq!(defaults.bindings.len(), 109);
    defaults.validate().unwrap();
    for e in entries {
        let action = e.command.shortcut_action().unwrap();
        assert_eq!(action.label(), e.label);
        if let Action::Menu(id) = action {
            assert_eq!(Command::from_shortcut_id(id), Some(e.command));
            assert_eq!(defaults.label(action), "지정 안 함");
            let mut edited = defaults.clone();
            edited
                .assign(action, Some((Key::F8, Modifiers::ALT)))
                .unwrap();
            let ctx = egui::Context::default();
            let mut found = None;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: Key::F8,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers::ALT,
                    }],
                    ..Default::default()
                },
                |_| found = edited.consume(&ctx),
            );
            out.textures_delta.clear();
            assert_eq!(found, Some(action));
            let mut out = ctx.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key: Key::F8,
                        physical_key: None,
                        pressed: true,
                        repeat: true,
                        modifiers: Modifiers::ALT,
                    }],
                    ..Default::default()
                },
                |_| found = edited.consume(&ctx),
            );
            out.textures_delta.clear();
            assert_eq!(found, None);
        }
    }
    assert_eq!(Command::RawDevelop.shortcut_action(), Some(Action::Menu(2)));
    assert_eq!(Command::NewLayer.shortcut_action(), Some(Action::Menu(16)));
    assert_eq!(
        Command::ColorWindow.shortcut_action(),
        Some(Action::Menu(68))
    );
    let mut old = defaults.clone();
    old.bindings.retain(|b| Action::core().contains(&b.action));
    // A historical file is validated only after migration, while retaining its exact custom keys.
    old.bindings
        .iter_mut()
        .find(|b| b.action == Action::CommandPalette)
        .unwrap()
        .key = Some("F9".into());
    old.bindings
        .iter_mut()
        .find(|b| b.action == Action::Tool(Tool::Brush))
        .unwrap()
        .key = Some("F6".into());
    let restored: Settings = serde_json::from_slice(&serde_json::to_vec(&old).unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.label(Action::CommandPalette), "Ctrl+F9");
    assert_eq!(restored.label(Action::Tool(Tool::Brush)), "F6");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/menu-shortcuts");
    std::fs::create_dir_all(&root).unwrap();
    let mut store = crate::workspace::Store::load(root.join("workspace109.json")).unwrap();
    store.layout.shortcuts = restored;
    store
        .layout
        .shortcuts
        .assign(
            Command::RawDevelop.shortcut_action().unwrap(),
            Some((Key::F8, Modifiers::ALT)),
        )
        .unwrap();
    store
        .layout
        .shortcuts
        .assign(
            Command::NewLayer.shortcut_action().unwrap(),
            Some((Key::F10, Modifiers::NONE)),
        )
        .unwrap();
    store.observe(true).unwrap();
    let reopened = crate::workspace::Store::load(root.join("workspace109.json")).unwrap();
    assert_eq!(reopened.layout.shortcuts, store.layout.shortcuts);
    let mut bad = defaults.clone();
    bad.bindings[0].action = Action::Menu(999);
    assert!(bad.validate().is_err());
    let mut bad = defaults.clone();
    bad.bindings.pop();
    let loaded: Settings = serde_json::from_slice(&serde_json::to_vec(&bad).unwrap()).unwrap();
    assert!(loaded.validate().is_err());
    let mut bad = defaults.clone();
    bad.bindings[0].action = bad.bindings[1].action;
    assert!(bad.validate().is_err());
}
#[test]
fn menu_shortcut_search_pointer_capture_conflict_escape_clear_and_small_window() {
    use crate::commands::Command;
    let action = Command::RawDevelop.shortcut_action().unwrap();
    let ctx = egui::Context::default();
    let mut settings = Settings::default();
    let mut panel = Panel {
        open: true,
        query: "RAW".into(),
        ..Default::default()
    };
    let mut time = 0.;
    fn frame(
        panel: &mut Panel,
        ctx: &egui::Context,
        settings: &mut Settings,
        time: &mut f64,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        *time += 0.05;
        let mut out = ctx.run_ui(
            egui::RawInput {
                time: Some(*time),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(540., 480.),
                )),
                events,
                ..Default::default()
            },
            |_| panel.ui(ctx, settings),
        );
        let labels = out
            .shapes
            .iter()
            .filter_map(|s| {
                if let egui::Shape::Text(t) = &s.shape {
                    Some((
                        t.galley.job.text.clone(),
                        egui::Rect::from_min_size(t.pos, t.galley.size()),
                    ))
                } else {
                    None
                }
            })
            .collect();
        out.textures_delta.clear();
        labels
    }
    fn click(
        panel: &mut Panel,
        ctx: &egui::Context,
        settings: &mut Settings,
        time: &mut f64,
        label: &str,
    ) {
        frame(panel, ctx, settings, time, vec![]);
        let labels = frame(panel, ctx, settings, time, vec![]);
        let at = labels.iter().find(|(s, _)| s == label).unwrap().1.center();
        for pressed in [true, false] {
            frame(
                panel,
                ctx,
                settings,
                time,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
    }
    frame(&mut panel, &ctx, &mut settings, &mut time, vec![]);
    let labels = frame(&mut panel, &ctx, &mut settings, &mut time, vec![]);
    assert!(labels.iter().any(|(s, _)| s == "RAW 현상…"));
    assert!(!labels.iter().any(|(s, _)| s == "프로젝트 저장"));
    click(&mut panel, &ctx, &mut settings, &mut time, "변경");
    assert_eq!(panel.listening, Some(action));
    for (key, mods) in [(Key::Z, Modifiers::COMMAND), (Key::F10, Modifiers::NONE)] {
        frame(
            &mut panel,
            &ctx,
            &mut settings,
            &mut time,
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: mods,
            }],
        );
        if key == Key::Z {
            assert!(panel.error.is_some());
            assert_eq!(settings.label(action), "지정 안 함");
        } else {
            assert_eq!(settings.label(action), "F10");
            assert!(panel.listening.is_none());
        }
    }
    click(&mut panel, &ctx, &mut settings, &mut time, "변경");
    frame(
        &mut panel,
        &ctx,
        &mut settings,
        &mut time,
        vec![egui::Event::Key {
            key: Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert!(panel.listening.is_none());
    assert_eq!(settings.label(action), "F10");
    click(&mut panel, &ctx, &mut settings, &mut time, "해제");
    assert_eq!(settings.label(action), "지정 안 함");
}

#[test]
fn program_info_shortcut_upgrade_preserves_complete_107_keys_and_rejects_partial_sets() {
    use crate::commands::Command;
    let mut previous = Settings::default();
    previous
        .assign(
            Command::RawDevelop.shortcut_action().unwrap(),
            Some((Key::F8, Modifiers::ALT)),
        )
        .unwrap();
    previous
        .assign(
            Command::ColorWindow.shortcut_action().unwrap(),
            Some((Key::F9, Modifiers::NONE)),
        )
        .unwrap();
    previous
        .bindings
        .retain(|b| !matches!(b.action, Action::Menu(69..)));
    assert_eq!(previous.bindings.len(), 107);
    let loaded: Settings = serde_json::from_slice(&serde_json::to_vec(&previous).unwrap()).unwrap();
    loaded.validate().unwrap();
    assert_eq!(&loaded.bindings[..107], &previous.bindings);
    assert_eq!(
        loaded.label(Command::About.shortcut_action().unwrap()),
        "지정 안 함"
    );
    assert_eq!(
        loaded.label(Command::Licenses.shortcut_action().unwrap()),
        "지정 안 함"
    );
    assert_eq!(Command::About.shortcut_action(), Some(Action::Menu(69)));
    assert_eq!(Command::Licenses.shortcut_action(), Some(Action::Menu(70)));
    let mut partial = previous.clone();
    partial.bindings.pop();
    let broken: Settings = serde_json::from_slice(&serde_json::to_vec(&partial).unwrap()).unwrap();
    assert!(broken.validate().is_err());
    let mut duplicate = previous.clone();
    duplicate.bindings[0].action = duplicate.bindings[1].action;
    let broken: Settings =
        serde_json::from_slice(&serde_json::to_vec(&duplicate).unwrap()).unwrap();
    assert!(broken.validate().is_err());
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/program-info");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("previous107.json"),
        serde_json::to_vec_pretty(&previous).unwrap(),
    )
    .unwrap();
    std::fs::write(
        root.join("current109.json"),
        serde_json::to_vec_pretty(&loaded).unwrap(),
    )
    .unwrap();
}
