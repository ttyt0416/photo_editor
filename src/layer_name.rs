use eframe::egui::{self, Key};

pub struct Session {
    pub tab: u64,
    pub layer: u64,
    pub name: String,
    focus: bool,
    ime_active: bool,
}
pub enum Outcome {
    Accept,
    Cancel,
}
impl Session {
    pub fn new(tab: u64, layer: u64, name: String) -> Self {
        Self {
            tab,
            layer,
            name,
            focus: true,
            ime_active: false,
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<Outcome> {
        let ime_event = ui.input(|i| {
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
        let (accept, cancel) = if self.ime_active || ime_event {
            ui.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, Key::Enter);
                i.consume_key(egui::Modifiers::NONE, Key::Escape);
            });
            (false, false)
        } else {
            ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, Key::Enter),
                    i.consume_key(egui::Modifiers::NONE, Key::Escape),
                )
            })
        };
        let mut output = egui::TextEdit::singleline(&mut self.name)
            .id(egui::Id::new(("layer-name", self.tab, self.layer)))
            .desired_width(ui.available_width().max(80.0))
            .hint_text("레이어 이름")
            .show(ui);
        if self.focus {
            output.response.request_focus();
            output
                .state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::select_all(&output.galley)));
            output.state.store(ui.ctx(), output.response.id);
            output.response.scroll_to_me(Some(egui::Align::Center));
            self.focus = false;
        }
        egui::Response::clone(&output.response)
            .on_hover_text("Enter 적용 · Esc 취소 · 중복 이름 허용");
        if cancel {
            output.response.surrender_focus();
            return Some(Outcome::Cancel);
        }
        let empty = self.name.trim().is_empty();
        if empty {
            ui.label(egui::RichText::new("이름을 입력하세요.").color(egui::Color32::YELLOW));
        }
        if accept {
            if empty {
                output.response.request_focus();
            } else {
                output.response.surrender_focus();
                return Some(Outcome::Accept);
            }
        }
        let outside_press=ui.input(|i|i.raw.events.iter().any(|event|matches!(event,
            egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,..} if !output.response.rect.contains(*pos)
        )));
        if (output.response.lost_focus() || outside_press) && !self.ime_active && !ime_event {
            output.response.surrender_focus();
            return Some(if empty {
                Outcome::Cancel
            } else {
                Outcome::Accept
            });
        }
        None
    }
}
