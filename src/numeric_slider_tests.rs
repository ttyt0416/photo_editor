use super::*;
#[allow(clippy::too_many_arguments)]
fn frame<N: Numeric>(
    ctx: &egui::Context,
    value: &mut N,
    range: RangeInclusive<N>,
    log: bool,
    enabled: bool,
    time: &mut f64,
    mods: egui::Modifiers,
    mut events: Vec<egui::Event>,
) -> (egui::Rect, bool) {
    events.insert(0, egui::Event::ModifiersChanged(mods));
    *time += 0.05;
    let mut rect = egui::Rect::NOTHING;
    let mut changed = false;
    let mut out = ctx.run_ui(
        egui::RawInput {
            time: Some(*time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(700., 300.),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            changed = ui
                .add_enabled(
                    enabled,
                    Slider::new(value, range.clone())
                        .text("수치 조절")
                        .logarithmic(log),
                )
                .changed();
        },
    );
    for shape in &out.shapes {
        if let egui::Shape::Text(t) = &shape.shape
            && t.galley.job.text == "수치 조절"
        {
            rect = egui::Rect::from_min_size(t.pos, t.galley.size());
        }
    }
    out.textures_delta.clear();
    (rect, changed)
}
#[allow(clippy::too_many_arguments)]
fn drag<N: Numeric>(
    value: &mut N,
    range: RangeInclusive<N>,
    log: bool,
    enabled: bool,
    mods: egui::Modifiers,
    distance: f32,
    pieces: u32,
) -> bool {
    let ctx = egui::Context::default();
    let mut time = 0.;
    frame(
        &ctx,
        value,
        range.clone(),
        log,
        enabled,
        &mut time,
        mods,
        vec![],
    );
    let at = frame(
        &ctx,
        value,
        range.clone(),
        log,
        enabled,
        &mut time,
        mods,
        vec![],
    )
    .0
    .center();
    frame(
        &ctx,
        value,
        range.clone(),
        log,
        enabled,
        &mut time,
        mods,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: mods,
            },
        ],
    );
    let mut changed = false;
    for piece in 1..=pieces {
        changed |= frame(
            &ctx,
            value,
            range.clone(),
            log,
            enabled,
            &mut time,
            mods,
            vec![egui::Event::PointerMoved(
                at + egui::vec2(distance * piece as f32 / pieces as f32, 0.),
            )],
        )
        .1;
    }
    frame(
        &ctx,
        value,
        range,
        log,
        enabled,
        &mut time,
        mods,
        vec![egui::Event::PointerButton {
            pos: at + egui::vec2(distance, 0.),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: mods,
        }],
    );
    changed
}
#[test]
fn numeric_label_drag_float_integer_log_precision_clamp_disabled_and_accumulated_small_steps() {
    for (mods, expected) in [
        (egui::Modifiers::NONE, 0.6),
        (egui::Modifiers::SHIFT, 0.51),
        (egui::Modifiers::CTRL, 0.501),
    ] {
        let mut value = 0.5f32;
        assert!(drag(&mut value, 0. ..=1., false, true, mods, 20., 1));
        assert!((value - expected).abs() < 1e-5, "{mods:?}: {value}");
    }
    let mut integer = 10u32;
    assert!(drag(
        &mut integer,
        0..=20,
        false,
        true,
        egui::Modifiers::NONE,
        10.,
        20
    ));
    assert_eq!(integer, 11);
    let mut logarithmic = 1f64;
    assert!(drag(
        &mut logarithmic,
        0.01..=100.,
        true,
        true,
        egui::Modifiers::NONE,
        20.,
        10
    ));
    assert!((logarithmic - 10f64.powf(0.4)).abs() < 1e-8);
    let mut value = 0.5f32;
    assert!(drag(
        &mut value,
        0. ..=1.,
        false,
        true,
        egui::Modifiers::NONE,
        1000.,
        4
    ));
    assert_eq!(value, 1.);
    assert!(drag(
        &mut value,
        0. ..=1.,
        false,
        true,
        egui::Modifiers::NONE,
        -1000.,
        4
    ));
    assert_eq!(value, 0.);
    let mut disabled = 0.5f32;
    assert!(!drag(
        &mut disabled,
        0. ..=1.,
        false,
        false,
        egui::Modifiers::NONE,
        100.,
        5
    ));
    assert_eq!(disabled, 0.5);
}
#[test]
fn numeric_label_click_keyboard_step_retains_native_format_and_track_controls() {
    let ctx = egui::Context::default();
    let mut time = 0.;
    let mut value = 0.5f32;
    frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::NONE,
        vec![],
    );
    let at = frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::NONE,
        vec![],
    )
    .0
    .center();
    for pressed in [true, false] {
        frame(
            &ctx,
            &mut value,
            0. ..=1.,
            false,
            true,
            &mut time,
            egui::Modifiers::NONE,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    assert_eq!(value, 0.5);
    let changed = frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::NONE,
        vec![egui::Event::Key {
            key: egui::Key::ArrowRight,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }],
    )
    .1;
    assert!(changed && value > 0.5);
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.add(
            Slider::new(&mut value, 0. ..=1.)
                .text("퍼센트")
                .custom_formatter(|v, _| format!("{:.0}%", v * 100.)),
        );
    });
    assert!(
        out.shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text.contains('%')))
    );
    out.textures_delta.clear();
}

#[test]
fn numeric_label_modifier_switch_no_jump_new_drag_has_fresh_baseline() {
    let ctx = egui::Context::default();
    let mut time = 0.;
    let mut value = 0.5f32;
    let at = frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::NONE,
        vec![],
    )
    .0
    .center();
    frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::NONE,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    let moved = at + egui::vec2(20., 0.);
    frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::NONE,
        vec![egui::Event::PointerMoved(moved)],
    );
    assert!((value - 0.6).abs() < 1e-5);
    frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::SHIFT,
        vec![],
    );
    assert!((value - 0.6).abs() < 1e-5);
    let end = moved + egui::vec2(20., 0.);
    frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::SHIFT,
        vec![egui::Event::PointerMoved(end)],
    );
    assert!((value - 0.61).abs() < 1e-5);
    frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::NONE,
        vec![egui::Event::PointerButton {
            pos: end,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }],
    );
    value = 0.8;
    let at = frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::NONE,
        vec![],
    )
    .0
    .center();
    frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::NONE,
        vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    frame(
        &ctx,
        &mut value,
        0. ..=1.,
        false,
        true,
        &mut time,
        egui::Modifiers::NONE,
        vec![egui::Event::PointerMoved(at + egui::vec2(20., 0.))],
    );
    assert!((value - 0.9).abs() < 1e-5);
}

#[test]
fn numeric_label_separate_value_drag_native_speed_suffix_focus_keyboard_and_disabled() {
    fn field_frame(
        ctx: &egui::Context,
        value: &mut u32,
        time: &mut f64,
        enabled: bool,
        events: Vec<egui::Event>,
    ) -> (egui::Rect, bool) {
        let check_suffix = *time == 0.;
        *time += 0.05;
        let mut changed = false;
        let mut out = ctx.run_ui(
            egui::RawInput {
                time: Some(*time),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600., 300.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    changed = labeled_drag(ui, "해상도", value, 1..=65535, 1., " ppi").changed()
                });
            },
        );
        let rect = out
            .shapes
            .iter()
            .find_map(|s| {
                if let egui::Shape::Text(t) = &s.shape
                    && t.galley.job.text == "해상도"
                {
                    Some(egui::Rect::from_min_size(t.pos, t.galley.size()))
                } else {
                    None
                }
            })
            .unwrap();
        let has_suffix = out
            .shapes
            .iter()
            .any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text.contains("ppi")));
        out.textures_delta.clear();
        if check_suffix {
            assert!(has_suffix);
        }
        (rect, changed)
    }
    for enabled in [true, false] {
        let ctx = egui::Context::default();
        let mut value = 300;
        let mut time = 0.;
        let at = field_frame(&ctx, &mut value, &mut time, enabled, vec![])
            .0
            .center();
        field_frame(
            &ctx,
            &mut value,
            &mut time,
            enabled,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let end = at + egui::vec2(20., 0.);
        let changed = field_frame(
            &ctx,
            &mut value,
            &mut time,
            enabled,
            vec![egui::Event::PointerMoved(end)],
        )
        .1;
        field_frame(
            &ctx,
            &mut value,
            &mut time,
            enabled,
            vec![egui::Event::PointerButton {
                pos: end,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(changed, enabled);
        assert_eq!(value, if enabled { 320 } else { 300 });
        if enabled {
            let at = field_frame(&ctx, &mut value, &mut time, true, vec![])
                .0
                .center();
            for pressed in [true, false] {
                field_frame(
                    &ctx,
                    &mut value,
                    &mut time,
                    true,
                    vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            field_frame(
                &ctx,
                &mut value,
                &mut time,
                true,
                vec![egui::Event::Key {
                    key: egui::Key::ArrowUp,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
            assert!(value > 320);
        }
    }
}
