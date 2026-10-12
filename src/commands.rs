use crate::shortcuts::Action;
use eframe::egui::{self, Key};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Action(Action),
    Recovery,
    LegacyPhotoshop,
    RawDevelop,
    Psd,
    Psb,
    Import,
    Export,
    Fill,
    Gradient,
    Pattern,
    CopyLayers,
    PasteLayers,
    MergeSelected,
    MergeDown,
    MergeVisible,
    Rasterize,
    NewLayer,
    AdjustmentLayer,
    Duplicate,
    Delete,
    Group,
    Ungroup,
    Rename,
    AlphaSelection,
    SelectAll,
    MaskSelection,
    Refine,
    PixelGrid,
    Histogram,
    Fit,
    ActualSize,
    DisplaySampling(crate::display_sampling::Mode),
    Rulers,
    Guides,
    Grid,
    LayoutSettings,
    Snap,
    SnapGuides,
    SnapGrid,
    SnapLayers,
    SnapDocument,
    GuideEdit,
    VerticalGuide,
    HorizontalGuide,
    ClearGuides,
    Resize,
    CanvasSize,
    Crop,
    Adjust,
    Trim,
    FlipH,
    FlipV,
    Filter,
    Tools,
    GroupedTools,
    Layers,
    Properties,
    History,
    Navigator,
    Colors,
    MaskView,
    MaskOverlay,
    OverlaySettings,
    LeftPanels,
    ResetWorkspace,
    Shortcuts,
    ColorWindow,
    About,
    Licenses,
}

// Persisted shortcut IDs are array indices. Append new commands; never reorder or reuse IDs.
pub const SHORTCUT_COMMANDS: &[Command] = &[
    Command::Recovery,
    Command::LegacyPhotoshop,
    Command::RawDevelop,
    Command::Psd,
    Command::Psb,
    Command::Import,
    Command::Export,
    Command::Fill,
    Command::Gradient,
    Command::Pattern,
    Command::CopyLayers,
    Command::PasteLayers,
    Command::MergeSelected,
    Command::MergeDown,
    Command::MergeVisible,
    Command::Rasterize,
    Command::NewLayer,
    Command::AdjustmentLayer,
    Command::Duplicate,
    Command::Delete,
    Command::Group,
    Command::Ungroup,
    Command::Rename,
    Command::AlphaSelection,
    Command::SelectAll,
    Command::MaskSelection,
    Command::Refine,
    Command::PixelGrid,
    Command::Histogram,
    Command::Fit,
    Command::ActualSize,
    Command::DisplaySampling(crate::display_sampling::Mode::Nearest),
    Command::DisplaySampling(crate::display_sampling::Mode::Smooth),
    Command::DisplaySampling(crate::display_sampling::Mode::HighQuality),
    Command::Rulers,
    Command::Guides,
    Command::Grid,
    Command::LayoutSettings,
    Command::Snap,
    Command::SnapGuides,
    Command::SnapGrid,
    Command::SnapLayers,
    Command::SnapDocument,
    Command::GuideEdit,
    Command::VerticalGuide,
    Command::HorizontalGuide,
    Command::ClearGuides,
    Command::Resize,
    Command::CanvasSize,
    Command::Crop,
    Command::Adjust,
    Command::Trim,
    Command::FlipH,
    Command::FlipV,
    Command::Filter,
    Command::Tools,
    Command::GroupedTools,
    Command::Layers,
    Command::Properties,
    Command::History,
    Command::Navigator,
    Command::Colors,
    Command::MaskView,
    Command::MaskOverlay,
    Command::OverlaySettings,
    Command::LeftPanels,
    Command::ResetWorkspace,
    Command::Shortcuts,
    Command::ColorWindow,
    Command::About,
    Command::Licenses,
];
pub fn shortcut_label(id: u16) -> &'static str {
    static LABELS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    LABELS
        .get_or_init(|| {
            let catalog = catalog();
            SHORTCUT_COMMANDS
                .iter()
                .map(|command| {
                    catalog
                        .iter()
                        .find(|entry| entry.command == *command)
                        .map_or("알 수 없는 명령", |entry| entry.label)
                })
                .collect()
        })
        .get(usize::from(id))
        .copied()
        .unwrap_or("알 수 없는 명령")
}
impl Command {
    pub fn shortcut_action(self) -> Option<Action> {
        if let Self::Action(action) = self {
            Some(action)
        } else {
            SHORTCUT_COMMANDS
                .iter()
                .position(|c| *c == self)
                .map(|i| Action::Menu(i as u16))
        }
    }
    pub fn from_shortcut_id(id: u16) -> Option<Self> {
        SHORTCUT_COMMANDS.get(usize::from(id)).copied()
    }
}
#[derive(Clone)]
pub struct Entry {
    pub command: Command,
    pub category: &'static str,
    pub label: &'static str,
    pub keywords: &'static str,
    pub shortcut: String,
    pub disabled: Option<&'static str>,
}
pub const MENU_CATEGORIES: &[&str] = &[
    "파일",
    "편집",
    "이미지",
    "레이어",
    "선택",
    "필터",
    "보기",
    "창",
    "도움말",
];

pub fn catalog() -> Vec<Entry> {
    let mut entries: Vec<_> = Action::core()
        .into_iter()
        .filter(|a| !matches!(a, Action::RedoAlternate | Action::CommandPalette))
        .map(|action| Entry {
            command: Command::Action(action),
            category: match action {
                Action::Tool(_) => "도구",
                Action::New | Action::Open | Action::Save | Action::SaveAs | Action::Close => {
                    "파일"
                }
                Action::Transform => "레이어",
                Action::Deselect | Action::InvertSelection => "선택",
                Action::CanvasOnly => "보기",
                _ => "편집",
            },
            label: action.label(),
            keywords: match action {
                Action::Tool(_) => "tool",
                Action::Save | Action::SaveAs => "save project",
                Action::Open => "open image",
                Action::New => "new document",
                Action::Undo => "undo",
                Action::Redo => "redo",
                _ => "",
            },
            shortcut: String::new(),
            disabled: None,
        })
        .collect();
    entries.extend(
        [
            (
                Command::Recovery,
                "파일",
                "자동 저장 복구…",
                "recovery restore",
            ),
            (Command::Psd, "파일", "PSD로 저장…", "photoshop"),
            (Command::Psb, "파일", "PSB로 저장…", "photoshop large"),
            (
                Command::LegacyPhotoshop,
                "파일",
                "이전 개발 PSD/PSB 열기…",
                "legacy photoshop 이전 개발 레이어 순서",
            ),
            (
                Command::RawDevelop,
                "레이어",
                "RAW 현상…",
                "raw develop camera sensor white balance exposure",
            ),
            (Command::Import, "파일", "레이어로 가져오기", "import"),
            (
                Command::Export,
                "파일",
                "이미지 내보내기…",
                "export png jpeg webp tiff bmp gif",
            ),
            (Command::Fill, "레이어", "색상 채우기…", "solid fill"),
            (
                Command::Gradient,
                "레이어",
                "그라디언트 채우기…",
                "gradient fill",
            ),
            (Command::Pattern, "레이어", "패턴 채우기…", "pattern"),
            (
                Command::CopyLayers,
                "레이어",
                "선택 레이어 복사",
                "레이어·그룹 복사 copy layers",
            ),
            (
                Command::PasteLayers,
                "레이어",
                "레이어 붙여넣기",
                "레이어·그룹 붙여넣기 paste layers",
            ),
            (
                Command::MergeSelected,
                "레이어",
                "선택 레이어·그룹 병합",
                "선택 레이어 병합 merge",
            ),
            (
                Command::MergeDown,
                "레이어",
                "아래 레이어와 병합",
                "merge down",
            ),
            (
                Command::MergeVisible,
                "레이어",
                "보이는 레이어 병합",
                "merge visible",
            ),
            (Command::Rasterize, "레이어", "래스터화", "rasterize"),
            (Command::NewLayer, "레이어", "새 레이어", "new layer"),
            (
                Command::AdjustmentLayer,
                "레이어",
                "조정 레이어…",
                "adjustment layer 비파괴 보정",
            ),
            (
                Command::Duplicate,
                "레이어",
                "레이어 복제",
                "선택 레이어 복제 duplicate",
            ),
            (
                Command::Delete,
                "레이어",
                "레이어 삭제",
                "선택 레이어 삭제 delete",
            ),
            (Command::Group, "레이어", "그룹으로 묶기", "group"),
            (Command::Ungroup, "레이어", "그룹에서 꺼내기", "ungroup"),
            (Command::Rename, "레이어", "레이어 이름 변경", "rename"),
            (
                Command::AlphaSelection,
                "선택",
                "레이어 알파에서 선택",
                "alpha select",
            ),
            (Command::SelectAll, "선택", "전체 선택", "select all"),
            (
                Command::MaskSelection,
                "선택",
                "레이어 마스크에서 선택",
                "mask select",
            ),
            (
                Command::Refine,
                "선택",
                "선택 경계에 적용",
                "페더 확대 축소 refine feather",
            ),
            (Command::PixelGrid, "보기", "픽셀 격자", "pixel grid"),
            (Command::Histogram, "보기", "히스토그램", "histogram"),
            (Command::Fit, "보기", "화면에 맞추기", "화면 맞춤 fit"),
            (Command::ActualSize, "보기", "100%", "actual size"),
            (Command::Rulers, "보기", "눈금자", "rulers"),
            (Command::Guides, "보기", "가이드 표시", "guides"),
            (Command::Grid, "보기", "레이아웃 격자", "grid"),
            (
                Command::LayoutSettings,
                "보기",
                "격자·스냅 설정",
                "spacing subdivision",
            ),
            (Command::Snap, "보기", "스냅 사용", "snap"),
            (Command::SnapGuides, "보기", "가이드에 스냅", "snap guides"),
            (Command::SnapGrid, "보기", "격자에 스냅", "snap grid"),
            (
                Command::SnapLayers,
                "보기",
                "다른 레이어 가장자리·중심",
                "snap layers",
            ),
            (
                Command::SnapDocument,
                "보기",
                "문서 가장자리·중심",
                "snap document",
            ),
            (Command::GuideEdit, "보기", "가이드 편집", "guide position"),
            (
                Command::VerticalGuide,
                "보기",
                "세로 가이드 추가",
                "vertical guide",
            ),
            (
                Command::HorizontalGuide,
                "보기",
                "가로 가이드 추가",
                "horizontal guide",
            ),
            (
                Command::ClearGuides,
                "보기",
                "가이드 모두 삭제",
                "clear guides",
            ),
            (Command::Resize, "이미지", "이미지 크기…", "resize resample"),
            (Command::CanvasSize, "이미지", "캔버스 크기…", "canvas size"),
            (Command::Crop, "이미지", "선택 영역으로 자르기", "crop"),
            (
                Command::Adjust,
                "이미지",
                "사진 보정…",
                "adjustments exposure curves levels color balance bloom lens",
            ),
            (
                Command::Trim,
                "이미지",
                "투명 가장자리 자르기",
                "trim transparent",
            ),
            (
                Command::FlipH,
                "이미지",
                "캔버스 좌우 뒤집기",
                "flip horizontal",
            ),
            (
                Command::FlipV,
                "이미지",
                "캔버스 상하 뒤집기",
                "flip vertical",
            ),
            (Command::Filter, "필터", "블러·선명화…", "blur sharpen"),
            (Command::Tools, "창", "도구 모음", "toolbar"),
            (
                Command::GroupedTools,
                "창",
                "도구 묶어서 표시",
                "grouped tools",
            ),
            (Command::Layers, "창", "레이어 패널", "layers panel"),
            (Command::Properties, "창", "속성 패널", "properties"),
            (Command::History, "창", "히스토리 패널", "history"),
            (Command::Navigator, "창", "내비게이터", "navigator"),
            (Command::Colors, "창", "색상·팔레트 패널", "colors palette"),
            (Command::MaskView, "창", "마스크 원본 보기", "mask viewer"),
            (
                Command::MaskOverlay,
                "창",
                "마스크 오버레이",
                "mask overlay",
            ),
            (
                Command::OverlaySettings,
                "창",
                "마스크 오버레이 설정",
                "opacity color",
            ),
            (
                Command::LeftPanels,
                "창",
                "패널을 왼쪽에 배치",
                "left panels",
            ),
            (
                Command::ResetWorkspace,
                "창",
                "작업 화면 초기화",
                "reset workspace",
            ),
            (Command::Shortcuts, "창", "단축키 설정…", "shortcuts"),
            (
                Command::ColorWindow,
                "창",
                "색상·팔레트…",
                "colors palette window",
            ),
            (
                Command::About,
                "도움말",
                "프로그램 정보…",
                "about version 버전",
            ),
            (
                Command::Licenses,
                "도움말",
                "오픈소스 라이선스…",
                "licenses notices dependency 저작권",
            ),
        ]
        .into_iter()
        .map(|(command, category, label, keywords)| Entry {
            command,
            category,
            label,
            keywords,
            shortcut: String::new(),
            disabled: None,
        }),
    );
    entries.extend(crate::display_sampling::Mode::ALL.map(|mode| {
        use crate::display_sampling::Mode;
        let (label, keywords) = match mode {
            Mode::Nearest => ("화면 보간: 최근접", "screen filter sampling nearest"),
            Mode::Smooth => ("화면 보간: 부드럽게", "screen filter sampling smooth"),
            Mode::HighQuality => (
                "화면 보간: 고품질 축소",
                "screen filter sampling mipmap high quality downsampling",
            ),
        };
        Entry {
            command: Command::DisplaySampling(mode),
            category: "보기",
            label,
            keywords,
            shortcut: String::new(),
            disabled: None,
        }
    }));
    entries
}
fn matches(entry: &Entry, query: &str) -> bool {
    let haystack = format!(
        "{} {} {} {}",
        entry.category, entry.label, entry.keywords, entry.shortcut
    )
    .to_lowercase();
    query
        .split_whitespace()
        .all(|word| haystack.contains(&word.to_lowercase()))
}
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    pub query: String,
    selected: usize,
    focus: bool,
    ime_active: bool,
}
impl Panel {
    pub fn show(&mut self) {
        *self = Self {
            open: true,
            focus: true,
            ..Default::default()
        };
    }
    pub fn ui(&mut self, ctx: &egui::Context, entries: &[Entry]) -> Option<Command> {
        if !self.open {
            return None;
        }
        let mut chosen = None;
        let ime_event = ctx.input(|i| {
            let mut any = false;
            for event in &i.raw.events {
                if let egui::Event::Ime(event) = event {
                    any = true;
                    match event {
                        egui::ImeEvent::Preedit { text, .. } => self.ime_active = !text.is_empty(),
                        egui::ImeEvent::Commit(_) => self.ime_active = false,
                        _ => {}
                    }
                }
            }
            any
        });
        // Leave IME candidate navigation/confirmation to the text editor.
        let (up, down, enter, escape) = if self.ime_active || ime_event {
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, Key::Enter);
                i.consume_key(egui::Modifiers::NONE, Key::Escape);
            });
            (false, false, false, false)
        } else {
            ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                    i.consume_key(egui::Modifiers::NONE, Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, Key::Escape),
                )
            })
        };
        if escape {
            self.open = false;
            return None;
        }
        egui::Modal::new(egui::Id::new("command-search")).show(ctx, |ui| {
            ui.set_width(460.0);
            ui.heading("명령 검색");
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .id_salt("command-query")
                    .hint_text("메뉴 또는 도구 이름 검색"),
            );
            if self.focus {
                response.request_focus();
                self.focus = false;
            }
            if response.changed() {
                self.selected = 0;
            }
            let filtered: Vec<_> = entries
                .iter()
                .filter(|entry| matches(entry, &self.query))
                .collect();
            self.selected = self.selected.min(filtered.len().saturating_sub(1));
            if up {
                self.selected = self.selected.saturating_sub(1);
            }
            if down && !filtered.is_empty() {
                self.selected = (self.selected + 1).min(filtered.len() - 1);
            }
            if enter
                && let Some(entry) = filtered.get(self.selected)
                && entry.disabled.is_none()
            {
                chosen = Some(entry.command);
            }
            ui.label(format!(
                "{}개 명령 · ↑↓ 선택 · Enter 실행 · Esc 닫기",
                filtered.len()
            ));
            egui::ScrollArea::vertical()
                .id_salt("command-results")
                .max_height(330.0)
                .show(ui, |ui| {
                    for (index, entry) in filtered.iter().enumerate() {
                        let response = ui.add_enabled(
                            entry.disabled.is_none(),
                            egui::Button::selectable(
                                self.selected == index,
                                format!(
                                    "{}  ›  {}  {}",
                                    entry.category, entry.label, entry.shortcut
                                ),
                            ),
                        );
                        if (up || down || response.changed()) && self.selected == index {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                        if let Some(reason) = entry.disabled {
                            response.clone().on_hover_text(reason);
                        }
                        if response.clicked() {
                            chosen = Some(entry.command);
                        }
                    }
                });
            if filtered.is_empty() {
                ui.label("일치하는 명령이 없습니다.");
            }
            if let Some(entry) = filtered.get(self.selected)
                && let Some(reason) = entry.disabled
            {
                ui.label(reason);
            }
            if ui.button("닫기").clicked() {
                self.open = false;
            }
        });
        if chosen.is_some() {
            self.open = false;
        }
        chosen
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_has_unique_commands_all_tools_and_menu_actions() {
        let entries = catalog();
        for (index, entry) in entries.iter().enumerate() {
            assert!(!entries[..index].iter().any(|e| e.command == entry.command));
        }
        assert_eq!(entries.iter().filter(|e| e.category == "도구").count(), 19);
        for label in [
            "PSD로 저장…",
            "PSB로 저장…",
            "격자·스냅 설정",
            "가이드 편집",
            "선택 경계에 적용",
            "작업 화면 초기화",
            "단축키 설정…",
            "블러·선명화…",
        ] {
            assert!(entries.iter().any(|e| e.label == label));
        }
        assert!(entries.len() > 90);
    }
    #[test]
    fn catalog_covers_literal_top_and_layout_menu_actions() {
        let source = include_str!("app.rs");
        let top = &source[source.find("    fn top_ui(").unwrap()
            ..source[source.find("    fn top_ui(").unwrap()..]
                .find("        if self.tabs.is_empty()")
                .map(|i| i + source.find("    fn top_ui(").unwrap())
                .unwrap_or(source.len())];
        let layout = &source[source.find("    fn layout_menu(").unwrap()
            ..source.find("    fn canvas_ui(").unwrap()];
        let entries = catalog();
        for menu in [top, layout] {
            for token in [".button(\"", "egui::Button::new(\"", "ui.checkbox("] {
                for tail in menu.split(token).skip(1) {
                    let label = if token == "ui.checkbox(" {
                        tail.split('"').nth(1).unwrap()
                    } else {
                        tail.split('"').next().unwrap()
                    };
                    // Guide rows expose dynamic position and delete controls in GuideEdit.
                    if label == "삭제" {
                        continue;
                    }
                    assert!(
                        entries
                            .iter()
                            .any(|e| e.label == label || e.keywords.contains(label)),
                        "missing menu action: {label}"
                    );
                }
            }
        }
    }
    #[test]
    fn search_combines_words_categories_aliases_case_and_shortcuts() {
        let entries = catalog();
        assert!(
            entries
                .iter()
                .filter(|e| matches(e, "파일 PSD"))
                .all(|e| matches!(e.command, Command::Psd | Command::LegacyPhotoshop))
        );
        assert_eq!(
            entries
                .iter()
                .filter(|e| matches(e, "GrAdIeNt FiLl"))
                .count(),
            1
        );
        assert_eq!(entries.iter().filter(|e| matches(e, "없는명령")).count(), 0);
        let mut entry = entries[0].clone();
        entry.shortcut = "Ctrl+N".into();
        assert!(matches(&entry, "ctrl+n"));
    }
}
