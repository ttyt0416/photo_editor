#[cfg(test)]
use eframe::egui;
use eframe::egui::{TextureFilter, TextureOptions};
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Mode {
    Nearest,
    #[default]
    Smooth,
    HighQuality,
}
impl Mode {
    pub const ALL: [Self; 3] = [Self::Nearest, Self::Smooth, Self::HighQuality];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Nearest => "최근접",
            Self::Smooth => "부드럽게",
            Self::HighQuality => "고품질 축소",
        }
    }
    pub fn options(self) -> TextureOptions {
        match self {
            Self::Nearest => TextureOptions::NEAREST,
            Self::Smooth => TextureOptions::LINEAR,
            Self::HighQuality => {
                TextureOptions::LINEAR.with_mipmap_mode(Some(TextureFilter::Linear))
            }
        }
    }
    #[cfg(test)]
    pub fn ui(self_mode: &mut Self, ui: &mut egui::Ui) {
        for mode in Self::ALL {
            ui.radio_value(self_mode, mode, mode.label());
        }
        ui.label("화면 표시에 적용하며 원본과 저장 파일은 유지합니다.");
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pointer_mode_selection_persists_and_old_unknown_settings_are_safe() {
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/display-sampling");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("workspace.json");
        std::fs::write(&path, b"{}").unwrap();
        let mut store = crate::workspace::Store::load(path.clone()).unwrap();
        assert_eq!(store.layout.display_sampling, Mode::Smooth);
        let ctx = egui::Context::default();
        let mut time = 0.;
        let frame = |mode: &mut Mode, time: f64, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::new(500., 400.),
                    )),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| Mode::ui(mode, ui));
                },
            );
            output.textures_delta.clear();
            output
        };
        for mode in Mode::ALL {
            time += 1.;
            let output = frame(&mut store.layout.display_sampling, time, vec![]);
            let point = output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.job.text == mode.label() => {
                        Some(t.pos + egui::Vec2::splat(3.))
                    }
                    _ => None,
                })
                .unwrap();
            time += 0.1;
            frame(
                &mut store.layout.display_sampling,
                time,
                vec![egui::Event::PointerMoved(point)],
            );
            for pressed in [true, false] {
                time += 0.1;
                frame(
                    &mut store.layout.display_sampling,
                    time,
                    vec![egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
            assert_eq!(store.layout.display_sampling, mode);
            store.observe(true).unwrap();
            assert_eq!(
                crate::workspace::Store::load(path.clone())
                    .unwrap()
                    .layout
                    .display_sampling,
                mode
            );
        }
        let bad = b"{\"display_sampling\":\"unknown\"}";
        std::fs::write(&path, bad).unwrap();
        assert!(crate::workspace::Store::load(path.clone()).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bad);
    }
}
