use crate::app::Tool;
use eframe::egui::{self, Key, Modifiers};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    New,
    Open,
    Save,
    SaveAs,
    Close,
    Undo,
    Redo,
    RedoAlternate,
    Copy,
    CopyMerged,
    Cut,
    Paste,
    Transform,
    Deselect,
    InvertSelection,
    SwapColors,
    DefaultColors,
    CanvasOnly,
    CommandPalette,
    Tool(Tool),
    Menu(u16),
}
impl Action {
    pub fn all() -> Vec<Self> {
        let mut actions = Self::core();
        actions.extend((0..crate::commands::SHORTCUT_COMMANDS.len()).map(|i| Self::Menu(i as u16)));
        actions
    }
    pub fn core() -> Vec<Self> {
        let mut actions = vec![
            Self::New,
            Self::Open,
            Self::Save,
            Self::SaveAs,
            Self::Close,
            Self::Undo,
            Self::Redo,
            Self::RedoAlternate,
            Self::Copy,
            Self::CopyMerged,
            Self::Cut,
            Self::Paste,
            Self::Transform,
            Self::Deselect,
            Self::InvertSelection,
            Self::SwapColors,
            Self::DefaultColors,
            Self::CanvasOnly,
            Self::CommandPalette,
        ];
        actions.extend(Tool::entries().into_iter().map(|(t, _, _)| Self::Tool(t)));
        actions
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::New => "새 문서",
            Self::Open => "열기",
            Self::Save => "프로젝트 저장",
            Self::SaveAs => "다른 이름으로 저장",
            Self::Close => "문서 닫기",
            Self::Undo => "실행 취소",
            Self::Redo => "다시 실행",
            Self::RedoAlternate => "다시 실행 (추가 키)",
            Self::Copy => "복사",
            Self::CopyMerged => "합성 복사",
            Self::Cut => "잘라내기",
            Self::Paste => "붙여넣기",
            Self::Transform => "캔버스에서 변형…",
            Self::Deselect => "선택 해제",
            Self::InvertSelection => "선택 반전",
            Self::SwapColors => "전경색·배경색 교환",
            Self::DefaultColors => "기본색",
            Self::CanvasOnly => "캔버스만 보기",
            Self::CommandPalette => "명령 검색…",
            Self::Tool(t) => t.name(),
            Self::Menu(id) => crate::commands::shortcut_label(id),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub action: Action,
    pub key: Option<String>,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}
impl Binding {
    pub fn new(action: Action, key: Key, modifiers: Modifiers) -> Self {
        Self {
            action,
            key: Some(key.name().into()),
            ctrl: modifiers.ctrl || modifiers.command,
            shift: modifiers.shift,
            alt: modifiers.alt,
        }
    }
    pub fn label(&self) -> String {
        let Some(key) = &self.key else {
            return "지정 안 함".into();
        };
        format!(
            "{}{}{}{key}",
            if self.ctrl { "Ctrl+" } else { "" },
            if self.alt { "Alt+" } else { "" },
            if self.shift { "Shift+" } else { "" }
        )
    }
    fn matches(&self, key: Key, modifiers: Modifiers) -> bool {
        self.key.as_deref().and_then(Key::from_name) == Some(key)
            && self.ctrl == (modifiers.ctrl || modifiers.command)
            && self.shift == modifiers.shift
            && self.alt == modifiers.alt
            && !modifiers.mac_cmd
    }
    fn same_keys(&self, other: &Self) -> bool {
        self.key.is_some()
            && self.key == other.key
            && self.ctrl == other.ctrl
            && self.shift == other.shift
            && self.alt == other.alt
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    version: u8,
    #[serde(deserialize_with = "deserialize_bindings")]
    pub bindings: Vec<Binding>,
}
fn deserialize_bindings<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Binding>, D::Error> {
    let mut bindings = Vec::<Binding>::deserialize(deserializer)?;
    let additions = [Action::CanvasOnly, Action::CommandPalette];
    let base: Vec<_> = Action::core()
        .into_iter()
        .filter(|a| !additions.contains(a))
        .collect();
    // Accept only complete known historical sets: core 36/37/38 or core + the original 69 menu IDs.
    let unique = bindings
        .iter()
        .enumerate()
        .all(|(i, b)| !bindings[..i].iter().any(|other| other.action == b.action));
    let legacy_core = base.iter().all(|a| bindings.iter().any(|b| b.action == *a))
        && bindings.iter().all(|b| Action::core().contains(&b.action))
        && (bindings.len() == base.len()
            || (bindings.len() == base.len() + 1
                && bindings.iter().any(|b| b.action == Action::CanvasOnly))
            || bindings.len() == Action::core().len());
    let mut previous = Action::core();
    previous.extend((0..69).map(Action::Menu));
    let legacy_menu = bindings.len() == previous.len()
        && previous
            .iter()
            .all(|a| bindings.iter().any(|b| b.action == *a));
    if unique && (legacy_core || legacy_menu) {
        let defaults = Settings::default();
        for action in Action::all() {
            if !bindings.iter().any(|b| b.action == action) {
                let mut binding = defaults
                    .bindings
                    .iter()
                    .find(|b| b.action == action)
                    .unwrap()
                    .clone();
                if bindings.iter().any(|b| b.same_keys(&binding)) {
                    binding.key = None;
                }
                bindings.push(binding);
            }
        }
    }
    Ok(bindings)
}
impl Default for Settings {
    fn default() -> Self {
        use Action::*;
        let bindings = Action::all()
            .into_iter()
            .map(|action| {
                let (key, ctrl, shift) = match action {
                    New => ("N", true, false),
                    Open => ("O", true, false),
                    Save => ("S", true, false),
                    SaveAs => ("S", true, true),
                    Close => ("W", true, false),
                    Undo => ("Z", true, false),
                    Redo => ("Z", true, true),
                    RedoAlternate => ("Y", true, false),
                    Copy => ("C", true, false),
                    CopyMerged => ("C", true, true),
                    Cut => ("X", true, false),
                    Paste => ("V", true, false),
                    Transform => ("T", true, false),
                    Deselect => ("D", true, false),
                    InvertSelection => ("I", true, true),
                    SwapColors => ("X", false, false),
                    DefaultColors => ("D", false, false),
                    CanvasOnly => ("F", false, false),
                    CommandPalette => ("F", true, false),
                    Menu(_) => ("", false, false),
                    Tool(t) => (
                        crate::app::Tool::entries()
                            .into_iter()
                            .find(|(tool, _, _)| *tool == t)
                            .unwrap()
                            .2,
                        false,
                        false,
                    ),
                };
                Binding {
                    action,
                    key: if key.is_empty() {
                        None
                    } else {
                        Some(key.into())
                    },
                    ctrl,
                    shift,
                    alt: false,
                }
            })
            .collect();
        Self {
            version: 1,
            bindings,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.bindings.len() != Action::all().len() {
            return Err("단축키 설정 버전 또는 항목 수가 올바르지 않습니다.".into());
        }
        for (index, binding) in self.bindings.iter().enumerate() {
            if !Action::all().contains(&binding.action)
                || self.bindings[..index]
                    .iter()
                    .any(|b| b.action == binding.action)
            {
                return Err("중복되거나 알 수 없는 단축키 명령입니다.".into());
            }
            if let Some(name) = &binding.key {
                let Some(key) = Key::from_name(name) else {
                    return Err("알 수 없는 키입니다.".into());
                };
                if name != key.name() {
                    return Err("키 이름이 올바르지 않습니다.".into());
                }
                if matches!(
                    key,
                    Key::Escape
                        | Key::Enter
                        | Key::Space
                        | Key::Tab
                        | Key::Delete
                        | Key::Backspace
                        | Key::ArrowUp
                        | Key::ArrowDown
                        | Key::ArrowLeft
                        | Key::ArrowRight
                        | Key::Copy
                        | Key::Cut
                        | Key::Paste
                ) {
                    return Err("이 키는 입력·적용·취소·경로 편집·임시 손 도구에서 사용합니다. 다른 키를 선택하세요.".into());
                }
                if let Some(old) = self.bindings[..index].iter().find(|b| b.same_keys(binding)) {
                    return Err(format!(
                        "{}: ‘{}’와 ‘{}’가 겹칩니다.",
                        binding.label(),
                        old.action.label(),
                        binding.action.label()
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn label(&self, action: Action) -> String {
        self.bindings
            .iter()
            .find(|b| b.action == action)
            .map_or_else(|| "지정 안 함".into(), Binding::label)
    }
    pub fn menu_label(&self, action: Action) -> String {
        let binding = self.label(action);
        if binding == "지정 안 함" {
            action.label().into()
        } else {
            format!("{}    {binding}", action.label())
        }
    }
    pub fn assign(
        &mut self,
        action: Action,
        binding: Option<(Key, Modifiers)>,
    ) -> Result<(), String> {
        let mut next = self.clone();
        let slot = next
            .bindings
            .iter_mut()
            .find(|b| b.action == action)
            .ok_or("알 수 없는 명령입니다.")?;
        *slot = binding.map_or(
            Binding {
                action,
                key: None,
                ctrl: false,
                shift: false,
                alt: false,
            },
            |(key, mods)| Binding::new(action, key, mods),
        );
        next.validate()?;
        *self = next;
        Ok(())
    }
    pub fn consume_action(&self, ctx: &egui::Context, action: Action) -> bool {
        ctx.input_mut(|input| {
            let index = input.events.iter().position(|event| {
                let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    repeat: false,
                    ..
                } = event
                else {
                    return false;
                };
                self.bindings
                    .iter()
                    .any(|binding| binding.action == action && binding.matches(*key, *modifiers))
            });
            if let Some(index) = index {
                input.events.remove(index);
                true
            } else {
                false
            }
        })
    }
    pub fn consume(&self, ctx: &egui::Context) -> Option<Action> {
        ctx.input_mut(|input| {
            let found = input.events.iter().enumerate().find_map(|(index, event)| {
                if let egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    repeat,
                    ..
                } = event
                {
                    self.bindings
                        .iter()
                        .find(|b| {
                            b.matches(*key, *modifiers)
                                && (!*repeat
                                    || !matches!(
                                        b.action,
                                        Action::CanvasOnly
                                            | Action::CommandPalette
                                            | Action::Menu(_)
                                    ))
                        })
                        .map(|b| (index, b.action))
                } else {
                    None
                }
            });
            found.map(|(index, action)| {
                input.events.remove(index);
                action
            })
        })
    }
}
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    pub listening: Option<Action>,
    error: Option<String>,
    query: String,
}
impl Panel {
    fn capture(&mut self, ctx: &egui::Context, settings: &mut Settings) {
        let Some(action) = self.listening else {
            return;
        };
        let event = ctx.input_mut(|input| {
            let index = input.events.iter().position(|e| {
                matches!(
                    e,
                    egui::Event::Key {
                        pressed: true,
                        repeat: false,
                        ..
                    }
                )
            })?;
            Some(input.events.remove(index))
        });
        if let Some(egui::Event::Key { key, modifiers, .. }) = event {
            if key == Key::Escape {
                self.listening = None;
                return;
            }
            match settings.assign(action, Some((key, modifiers))) {
                Ok(()) => {
                    self.listening = None;
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            }
        }
    }
    pub fn ui(&mut self, ctx: &egui::Context, settings: &mut Settings) {
        if !self.open {
            return;
        }
        self.capture(ctx, settings);
        egui::Modal::new(egui::Id::new("shortcut-settings")).show(ctx, |ui| {
            ui.set_width((ctx.content_rect().width() - 64.).clamp(180., 480.));
            ui.heading("단축키 설정");
            ui.label("변경을 누르고 원하는 키 조합을 입력하세요. 변경 즉시 적용됩니다.");
            ui.label("Enter·Esc·Space·Tab·삭제·방향키는 편집과 입력에 예약되어 있습니다.");
            if let Some(error) = &self.error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            if let Some(action) = self.listening {
                ui.label(format!(
                    "‘{}’의 새 키를 기다립니다. Esc로 입력 취소",
                    action.label()
                ));
            }
            ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("명령 이름으로 검색"));
            let query = self.query.trim().to_lowercase();
            egui::ScrollArea::both()
                .max_height((ctx.content_rect().height() - 230.).clamp(100., 330.))
                .max_width((ctx.content_rect().width() - 64.).clamp(180., 480.))
                .show(ui, |ui| {
                    egui::Grid::new("shortcuts-grid")
                        .striped(true)
                        .show(ui, |ui| {
                            for action in Action::all()
                                .into_iter()
                                .filter(|a| a.label().to_lowercase().contains(&query))
                            {
                                ui.label(action.label());
                                ui.label(settings.label(action));
                                if ui.button("변경").clicked() {
                                    self.listening = Some(action);
                                    self.error = None;
                                }
                                if ui.small_button("해제").clicked() {
                                    let _ = settings.assign(action, None);
                                    self.listening = None;
                                    self.error = None;
                                }
                                ui.end_row();
                            }
                        });
                });
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("기본 단축키 복원").clicked() {
                    *settings = Settings::default();
                    self.listening = None;
                    self.error = None;
                }
                if ui.button("닫기").clicked() {
                    self.open = false;
                    self.listening = None;
                }
            });
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn old_shortcuts_upgrade_without_stealing_custom_f_binding() {
        let mut settings = Settings::default();
        settings.bindings.retain(|b| {
            !matches!(
                b.action,
                Action::CanvasOnly | Action::CommandPalette | Action::Menu(_)
            )
        });
        let old = serde_json::to_vec(&settings).unwrap();
        let restored: Settings = serde_json::from_slice(&old).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.label(Action::CanvasOnly), "F");
        settings
            .bindings
            .iter_mut()
            .find(|b| b.action == Action::Tool(Tool::Brush))
            .unwrap()
            .key = Some("F".into());
        let old = serde_json::to_vec(&settings).unwrap();
        let restored: Settings = serde_json::from_slice(&old).unwrap();
        restored.validate().unwrap();
        assert_eq!(restored.label(Action::Tool(Tool::Brush)), "F");
        assert_eq!(restored.label(Action::CanvasOnly), "지정 안 함");
        settings.bindings.pop();
        let invalid: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert!(invalid.validate().is_err());
    }
    #[test]
    fn historical_36_and_37_bindings_preserve_custom_search_key() {
        for with_canvas in [false, true] {
            let mut settings = Settings::default();
            settings
                .bindings
                .retain(|b| !matches!(b.action, Action::Menu(_)));
            settings.bindings.retain(|b| {
                b.action != Action::CommandPalette
                    && (with_canvas || b.action != Action::CanvasOnly)
            });
            let brush = settings
                .bindings
                .iter_mut()
                .find(|b| b.action == Action::Tool(Tool::Brush))
                .unwrap();
            brush.key = Some("F".into());
            brush.ctrl = true;
            let restored: Settings =
                serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
            restored.validate().unwrap();
            assert_eq!(restored.label(Action::Tool(Tool::Brush)), "Ctrl+F");
            assert_eq!(restored.label(Action::CommandPalette), "지정 안 함");
            settings.bindings.pop();
            let invalid: Settings =
                serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
            assert!(invalid.validate().is_err());
        }
    }
    #[test]
    fn conflicts_reserved_keys_and_corrupt_settings_are_rejected_without_changes() {
        let mut settings = Settings::default();
        settings.validate().unwrap();
        let before = settings.clone();
        assert!(
            settings
                .assign(Action::Tool(Tool::Brush), Some((Key::E, Modifiers::NONE)))
                .is_err()
        );
        assert_eq!(settings, before);
        for key in [Key::Space, Key::Escape, Key::Enter, Key::Delete] {
            assert!(
                settings
                    .assign(Action::Save, Some((key, Modifiers::COMMAND)))
                    .is_err()
            );
            assert_eq!(settings, before);
        }
        settings
            .assign(Action::Tool(Tool::Brush), Some((Key::F6, Modifiers::ALT)))
            .unwrap();
        settings
            .assign(
                Action::Tool(Tool::Ellipse),
                Some((Key::M, Modifiers::SHIFT)),
            )
            .unwrap();
        let restored: Settings =
            serde_json::from_slice(&serde_json::to_vec(&settings).unwrap()).unwrap();
        assert_eq!(restored, settings);
        assert_eq!(settings.label(Action::Tool(Tool::Brush)), "Alt+F6");
        settings.bindings[0].key = Some("unknown".into());
        assert!(settings.validate().is_err());
    }
    #[test]
    fn capture_input_updates_binding_and_keeps_listening_after_conflict() {
        let ctx = egui::Context::default();
        let mut settings = Settings::default();
        let mut panel = Panel {
            open: true,
            listening: Some(Action::Tool(Tool::Brush)),
            error: None,
            ..Default::default()
        };
        for (key, expected) in [(Key::E, "B"), (Key::F8, "F8")] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events: vec![egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    }],
                    ..Default::default()
                },
                |_| panel.ui(&ctx, &mut settings),
            );
            output.textures_delta.clear();
            assert_eq!(settings.label(Action::Tool(Tool::Brush)), expected);
        }
        assert!(panel.listening.is_none());
        assert!(panel.error.is_none());
    }
}

#[cfg(test)]
#[path = "menu_shortcut_tests.rs"]
mod menu_tests;
