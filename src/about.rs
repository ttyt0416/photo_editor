use eframe::egui;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
struct Inventory {
    version: u8,
    target: String,
    packages: Vec<Package>,
}
#[derive(Deserialize)]
struct Package {
    name: String,
    version: String,
    license: String,
    repository: Option<String>,
    source: String,
    notices: Vec<Notice>,
}
#[derive(Deserialize)]
struct Notice {
    path: String,
    encoding: String,
    sha256: String,
    text: String,
}
fn inventory() -> &'static Inventory {
    static INVENTORY: OnceLock<Inventory> = OnceLock::new();
    INVENTORY.get_or_init(|| {
        let data: Inventory =
            serde_json::from_str(include_str!("../licenses/windows-dependencies.json"))
                .expect("validated bundled dependency inventory");
        assert_eq!(data.version, 1);
        assert_eq!(data.target, "x86_64-pc-windows-msvc");
        data
    })
}
fn matches(package: &Package, query: &str) -> bool {
    let text = format!("{} {} {}", package.name, package.version, package.license).to_lowercase();
    query
        .split_whitespace()
        .all(|word| text.contains(&word.to_lowercase()))
}
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    licenses: bool,
    query: String,
    selected: usize,
}
impl Panel {
    pub fn show(&mut self, licenses: bool) {
        self.open = true;
        self.licenses = licenses;
    }
    pub fn ui(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let response = egui::Modal::new(egui::Id::new("program-info")).show(ctx, |ui| {
            let width = (ctx.content_rect().width() - 64.).clamp(180., 760.);
            let height = ctx.content_rect().height();
            ui.set_width(width);
            ui.heading("Photo Editor");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.licenses, false, "프로그램 정보");
                ui.selectable_value(&mut self.licenses, true, "오픈소스 라이선스");
            });
            ui.separator();
            if !self.licenses {
                ui.label(format!("버전 {} · Windows", env!("CARGO_PKG_VERSION")));
                ui.label("로컬에서 사진을 편집하고 레이어가 있는 프로젝트를 저장합니다.");
                ui.label("이미지 형식과 편집 기능은 계속 확장 중입니다.");
                ui.label("라이선스 탭에서 사용한 라이브러리와 포함된 원문을 볼 수 있습니다.");
            } else {
                let data = inventory();
                ui.label(format!(
                    "{}개 의존성 · {}개 원문",
                    data.packages.len(),
                    data.packages.iter().map(|p| p.notices.len()).sum::<usize>()
                ));
                ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("이름·버전·라이선스로 검색"),
                );
                let found: Vec<_> = data
                    .packages
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| matches(p, &self.query))
                    .collect();
                if !found.iter().any(|(i, _)| *i == self.selected)
                    && let Some((i, _)) = found.first()
                {
                    self.selected = *i;
                }
                ui.label(format!("검색 결과 {}개", found.len()));
                egui::ScrollArea::vertical()
                    .id_salt("license-packages")
                    .max_height((height - 340.).clamp(48., 150.))
                    .show(ui, |ui| {
                        for (index, package) in &found {
                            if ui
                                .selectable_label(
                                    self.selected == *index,
                                    format!("{} {}", package.name, package.version),
                                )
                                .clicked()
                            {
                                self.selected = *index;
                            }
                        }
                    });
                ui.separator();
                egui::ScrollArea::both()
                    .id_salt(("license-text", self.selected))
                    .max_width(width)
                    .max_height((height - 350.).clamp(48., 280.))
                    .show(ui, |ui| {
                        ui.set_max_width(width);
                        if !found.is_empty() {
                            let package = &data.packages[self.selected];
                            ui.strong(format!("{} {}", package.name, package.version));
                            ui.label(format!("라이선스: {}", package.license));
                            ui.label(format!("출처: {}", package.source));
                            if let Some(repository) = &package.repository {
                                ui.add(egui::Label::new(repository).selectable(true).wrap());
                            }
                            if package.notices.is_empty() {
                                ui.label("패키지에 원문 파일이 없어 선언된 라이선스만 표시합니다.");
                            }
                            for notice in &package.notices {
                                ui.separator();
                                ui.strong(&notice.path);
                                ui.small(format!(
                                    "{} · SHA-256 {}",
                                    notice.encoding, notice.sha256
                                ));
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&notice.text).monospace())
                                        .selectable(true)
                                        .wrap(),
                                );
                            }
                        } else {
                            ui.label("일치하는 라이브러리가 없습니다.");
                        }
                    });
            }
            ui.separator();
            if ui.button("닫기").clicked() {
                self.open = false;
            }
        });
        if response.should_close() {
            self.open = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    #[test]
    fn program_info_inventory_exact_notices_and_windows_dependency_set() {
        let data = inventory();
        assert_eq!(data.target, "x86_64-pc-windows-msvc");
        assert_eq!(data.packages.len(), 308);
        assert_eq!(
            data.packages.iter().map(|p| p.notices.len()).sum::<usize>(),
            528
        );
        let mut seen = std::collections::HashSet::new();
        for p in &data.packages {
            assert!(seen.insert((&p.name, &p.version)));
            assert!(!p.license.is_empty());
            for notice in &p.notices {
                assert!(!std::path::Path::new(&notice.path).is_absolute());
                let raw: Vec<u8> = if notice.encoding == "utf-8" {
                    notice.text.as_bytes().to_vec()
                } else {
                    notice
                        .text
                        .chars()
                        .map(|c| u8::try_from(c as u32).unwrap())
                        .collect()
                };
                assert_eq!(format!("{:x}", Sha256::digest(raw)), notice.sha256);
            }
        }
        let rawler = data.packages.iter().find(|p| p.name == "rawler").unwrap();
        assert_eq!(rawler.license, "LGPL-2.1");
        assert!(
            rawler
                .notices
                .iter()
                .any(|n| n.text.contains("GNU LESSER GENERAL PUBLIC LICENSE"))
        );
        let fonts = data
            .packages
            .iter()
            .find(|p| p.name == "epaint_default_fonts")
            .unwrap();
        assert!(fonts.notices.iter().any(|n| n.path.ends_with("OFL.txt")));
        assert!(fonts.notices.iter().any(|n| n.path.ends_with("UFL.txt")));
        assert!(data.packages.iter().any(
            |p| p.name == "libwebp-sys" && p.notices.iter().any(|n| n.path == "vendor/COPYING")
        ));
        assert!(
            data.packages
                .iter()
                .any(|p| p.name == "egui" && p.notices.is_empty())
        );
        assert!(!data.packages.iter().any(|p| p.name == "photo_editor"));
    }
    #[test]
    fn program_info_search_compact_modal_pointer_and_escape() {
        let ctx = egui::Context::default();
        let mut panel = Panel::default();
        panel.show(false);
        let mut time = 0.;
        fn frame(
            ctx: &egui::Context,
            panel: &mut Panel,
            time: &mut f64,
            size: egui::Vec2,
            events: Vec<egui::Event>,
        ) -> Vec<(String, egui::Rect)> {
            *time += 0.05;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*time),
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                },
                |_| panel.ui(ctx),
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
        for size in [egui::vec2(1320., 860.), egui::vec2(360., 480.)] {
            frame(&ctx, &mut panel, &mut time, size, vec![]);
            let labels = frame(&ctx, &mut panel, &mut time, size, vec![]);
            assert!(
                labels
                    .iter()
                    .any(|(s, _)| s.contains(env!("CARGO_PKG_VERSION")))
            );
            let at = labels
                .iter()
                .find(|(s, _)| s == "오픈소스 라이선스")
                .unwrap()
                .1
                .center();
            for pressed in [true, false] {
                frame(
                    &ctx,
                    &mut panel,
                    &mut time,
                    size,
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
            assert!(panel.licenses);
            for query in ["RaWler LGPL", "epaint_default_fonts", "no-such-package-987"] {
                panel.query = query.into();
                frame(&ctx, &mut panel, &mut time, size, vec![]);
                let labels = frame(&ctx, &mut panel, &mut time, size, vec![]);
                assert!(labels.iter().any(|(s, _)| s
                    == if query.starts_with("no-") {
                        "검색 결과 0개"
                    } else {
                        "검색 결과 1개"
                    }));
                let close = labels.iter().find(|(s, _)| s == "닫기").unwrap().1;
                assert!(
                    close.min.x >= 0.
                        && close.max.x <= size.x
                        && close.min.y >= 0.
                        && close.max.y <= size.y,
                    "close outside: {close:?}"
                );
            }
            panel.query.clear();
            panel.licenses = false;
        }
        frame(
            &ctx,
            &mut panel,
            &mut time,
            egui::vec2(360., 480.),
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(!panel.open);
        panel.show(true);
        panel.query = "egui 0.36.2".into();
        frame(&ctx, &mut panel, &mut time, egui::vec2(1320., 860.), vec![]);
        let labels = frame(&ctx, &mut panel, &mut time, egui::vec2(1320., 860.), vec![]);
        assert!(labels.iter().any(|(s, _)| s.contains("선언된 라이선스만")));
    }
}
