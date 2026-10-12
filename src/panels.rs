use eframe::egui;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Kind {
    Colors,
    Navigator,
    Layers,
    Properties,
    Histogram,
    History,
}
impl Kind {
    pub const ALL: [Self; 6] = [
        Self::Colors,
        Self::Navigator,
        Self::Layers,
        Self::Properties,
        Self::Histogram,
        Self::History,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Colors => "색상·팔레트",
            Self::Navigator => "내비게이터",
            Self::Layers => "레이어",
            Self::Properties => "속성",
            Self::Histogram => "히스토그램",
            Self::History => "히스토리",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct State {
    pub kind: Kind,
    pub floating: bool,
    pub collapsed: bool,
    pub position: Option<[f32; 2]>,
    pub size: [f32; 2],
}
impl Default for State {
    fn default() -> Self {
        Self {
            kind: Kind::Layers,
            floating: false,
            collapsed: false,
            position: None,
            size: [280., 300.],
        }
    }
}
pub fn defaults() -> Vec<State> {
    Kind::ALL
        .into_iter()
        .map(|kind| State {
            kind,
            ..Default::default()
        })
        .collect()
}
pub fn valid(states: &[State]) -> bool {
    states.len() == 6
        && Kind::ALL
            .iter()
            .all(|k| states.iter().filter(|s| s.kind == *k).count() == 1)
        && states.iter().all(|s| {
            s.position
                .is_none_or(|p| p.iter().all(|v| v.is_finite() && v.abs() <= 1_000_000.))
                && s.size
                    .iter()
                    .all(|v| v.is_finite() && (32. ..=10000.).contains(v))
        })
}
pub fn move_before(states: &mut Vec<State>, source: Kind, target: Kind) {
    if source == target {
        return;
    }
    let Some(from) = states.iter().position(|s| s.kind == source) else {
        return;
    };
    if !states.iter().any(|s| s.kind == target) {
        return;
    }
    let item = states.remove(from);
    let to = states.iter().position(|s| s.kind == target).unwrap();
    states.insert(to, item);
}
struct Drag(Kind);
#[derive(Clone, Copy)]
pub enum Action {
    Collapse,
    Float,
    Close,
    Up,
    Down,
    Before(Kind),
}
pub fn header(ui: &mut egui::Ui, state: &State, first: bool, last: bool) -> Option<Action> {
    let mut action = None;
    ui.horizontal(|ui| {
        let label = format!(
            "{} {}",
            if state.collapsed { "▸" } else { "▾" },
            state.kind.label()
        );
        let title = ui.add(
            egui::Label::new(egui::RichText::new(label).strong())
                .sense(egui::Sense::click_and_drag()),
        );
        if title.clicked() {
            action = Some(Action::Collapse);
        }
        if !state.floating {
            title.dnd_set_drag_payload(Drag(state.kind));
            if let Some(payload) = title.dnd_release_payload::<Drag>() {
                action = Some(Action::Before(payload.0));
            }
            title.on_hover_text("클릭: 접기/펼치기 · 다른 패널 제목 위로 끌기: 앞에 배치");
        }
        ui.menu_button("⋮", |ui| {
            if ui
                .button(if state.floating {
                    "패널 고정"
                } else {
                    "패널 분리"
                })
                .clicked()
            {
                action = Some(Action::Float);
                ui.close();
            }
            if ui
                .add_enabled(!first, egui::Button::new("위로 이동"))
                .clicked()
            {
                action = Some(Action::Up);
                ui.close();
            }
            if ui
                .add_enabled(!last, egui::Button::new("아래로 이동"))
                .clicked()
            {
                action = Some(Action::Down);
                ui.close();
            }
            if ui.button("패널 닫기").clicked() {
                action = Some(Action::Close);
                ui.close();
            }
        })
        .response
        .on_hover_text("패널 배치");
    });
    action
}

pub fn window_id(ctx: &egui::Context, kind: Kind) -> egui::Id {
    let generation = ctx
        .data(|d| d.get_temp::<u64>(egui::Id::new("panel-generation")))
        .unwrap_or_default();
    egui::Id::new(("floating-panel", kind, generation))
}
pub fn reset(ctx: &egui::Context) {
    let id = egui::Id::new("panel-generation");
    ctx.data_mut(|d| {
        let n = d.get_temp::<u64>(id).unwrap_or_default();
        d.insert_temp(id, n.wrapping_add(1));
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_settings_default_and_custom_order_floating_geometry_restore_or_reject_corruption() {
        let old: crate::workspace::Layout = serde_json::from_str("{}").unwrap();
        assert_eq!(old.panels, defaults());
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/panels");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("workspace.json");
        std::fs::write(&path, b"{}").unwrap();
        let mut store = crate::workspace::Store::load(path.clone()).unwrap();
        move_before(&mut store.layout.panels, Kind::History, Kind::Colors);
        store.layout.panels[0].floating = true;
        store.layout.panels[0].collapsed = true;
        store.layout.panels[0].position = Some([-90., 190.]);
        store.layout.panels[0].size = [310., 250.];
        store.observe(true).unwrap();
        assert_eq!(
            crate::workspace::Store::load(path.clone()).unwrap().layout,
            store.layout
        );
        let valid = std::fs::read(&path).unwrap();
        for mode in 0..4 {
            let mut bad = store.layout.clone();
            match mode {
                0 => bad.panels[1].kind = Kind::History,
                1 => {
                    bad.panels.pop();
                }
                2 => bad.panels[0].position = Some([1_000_001., 0.]),
                _ => bad.panels[0].size = [0., 100.],
            }
            assert!(!bad.valid());
            let bytes = serde_json::to_vec(&bad).unwrap();
            std::fs::write(&path, &bytes).unwrap();
            assert!(crate::workspace::Store::load(path.clone()).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
        std::fs::write(&path, valid).unwrap();
    }
    #[test]
    fn pointer_header_collapse_drag_reorder_and_menu_float() {
        let ctx = egui::Context::default();
        let mut states = defaults();
        let mut time = 0.;
        let frame = |states: &mut Vec<State>, time: f64, events| {
            let mut actions = vec![];
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::new(500., 500.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        for (i, state) in states.iter().enumerate() {
                            ui.push_id(state.kind, |ui| {
                                if let Some(action) =
                                    header(ui, state, i == 0, i + 1 == states.len())
                                {
                                    actions.push((state.kind, action));
                                }
                            });
                        }
                    });
                },
            );
            output.textures_delta.clear();
            for (kind, action) in actions {
                let index = states.iter().position(|s| s.kind == kind).unwrap();
                match action {
                    Action::Collapse => states[index].collapsed ^= true,
                    Action::Float => states[index].floating ^= true,
                    Action::Before(source) => move_before(states, source, kind),
                    _ => {}
                }
            }
            output
        };
        let position = |out: &egui::FullOutput, text: &str| {
            out.shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.job.text == text => {
                        Some(t.pos + egui::Vec2::splat(4.))
                    }
                    _ => None,
                })
                .unwrap()
        };
        let click = |states: &mut Vec<State>, time: &mut f64, p: egui::Pos2| {
            *time += 0.1;
            frame(states, *time, vec![egui::Event::PointerMoved(p)]);
            for pressed in [true, false] {
                *time += 0.1;
                frame(
                    states,
                    *time,
                    vec![egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
        };
        for _ in 0..3 {
            time += 0.1;
            frame(&mut states, time, vec![]);
        }
        time += 0.1;
        let out = frame(&mut states, time, vec![]);
        click(&mut states, &mut time, position(&out, "▾ 색상·팔레트"));
        assert!(states[0].collapsed);
        time += 0.1;
        let out = frame(&mut states, time, vec![]);
        let from = position(&out, "▾ 히스토리");
        let to = position(&out, "▸ 색상·팔레트");
        time += 0.1;
        frame(&mut states, time, vec![egui::Event::PointerMoved(from)]);
        time += 0.1;
        frame(
            &mut states,
            time,
            vec![egui::Event::PointerButton {
                pos: from,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        time += 0.1;
        frame(
            &mut states,
            time,
            vec![egui::Event::PointerMoved(from + egui::vec2(10., 0.))],
        );
        time += 0.1;
        frame(&mut states, time, vec![egui::Event::PointerMoved(to)]);
        time += 0.1;
        frame(
            &mut states,
            time,
            vec![egui::Event::PointerButton {
                pos: to,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(states[0].kind, Kind::History);
        time += 0.1;
        let out = frame(&mut states, time, vec![]);
        click(&mut states, &mut time, position(&out, "⋮"));
        for _ in 0..3 {
            time += 0.1;
            frame(&mut states, time, vec![]);
        }
        time += 0.1;
        let out = frame(&mut states, time, vec![]);
        click(&mut states, &mut time, position(&out, "패널 분리"));
        assert!(states[0].floating);
    }
}
