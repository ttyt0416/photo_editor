use eframe::egui;
use resvg::usvg::fontdb::{Family, ID, Query, Style, Weight};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::OnceLock,
};
struct Entry {
    name: String,
    aliases: Vec<String>,
    variants: Vec<String>,
    search: String,
}
fn catalog() -> &'static [Entry] {
    static CATALOG: OnceLock<Vec<Entry>> = OnceLock::new();
    CATALOG.get_or_init(|| {
        let mut families: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
        for face in crate::vector::fonts().faces() {
            let Some((name, _)) = face.families.first() else {
                continue;
            };
            let (aliases, variants) = families.entry(name.clone()).or_default();
            aliases.extend(face.families.iter().map(|v| v.0.clone()));
            variants.insert(format!(
                "{}{}",
                weight_label(face.weight.0),
                match face.style {
                    Style::Normal => "",
                    Style::Italic => " 기울임",
                    Style::Oblique => " 오블리크",
                }
            ));
        }
        families
            .into_iter()
            .map(|(name, (aliases, variants))| {
                let aliases: Vec<_> = aliases.into_iter().collect();
                let search = aliases.join(" ").to_lowercase();
                Entry {
                    name,
                    aliases,
                    variants: variants.into_iter().collect(),
                    search,
                }
            })
            .collect()
    })
}
fn weight_label(weight: u16) -> String {
    match weight {
        100 => "매우 얇게",
        200 => "얇게",
        300 => "약간 얇게",
        400 => "보통",
        500 => "중간",
        600 => "약간 굵게",
        700 => "굵게",
        800 => "아주 굵게",
        900 => "가장 굵게",
        _ => return format!("굵기 {weight}"),
    }
    .into()
}
fn family(name: &str) -> Family<'_> {
    match name.trim().to_ascii_lowercase().as_str() {
        "sans-serif" => Family::SansSerif,
        "serif" => Family::Serif,
        "monospace" => Family::Monospace,
        "cursive" => Family::Cursive,
        "fantasy" => Family::Fantasy,
        _ => Family::Name(name),
    }
}
fn query(name: &str, bold: bool, italic: bool, fallback: bool) -> Option<ID> {
    let families = [family(name), Family::SansSerif];
    crate::vector::fonts().query(&Query {
        families: if fallback { &families } else { &families[..1] },
        weight: Weight(if bold { 700 } else { 400 }),
        style: if italic { Style::Italic } else { Style::Normal },
        ..Default::default()
    })
}
pub fn face_id(name: &str, bold: bool, italic: bool) -> Option<ID> {
    query(name, bold, italic, true)
}
pub fn synthetic_italic(name: &str, bold: bool, italic: bool) -> bool {
    italic
        && face_id(name, bold, true)
            .and_then(|id| crate::vector::fonts().face(id))
            .is_none_or(|f| f.style == Style::Normal)
}
fn notices(name: &str, bold: bool, italic: bool) -> Vec<String> {
    let Some(id) = face_id(name, bold, italic) else {
        return vec!["표시할 글꼴을 찾지 못했습니다.".into()];
    };
    let face = crate::vector::fonts().face(id).unwrap();
    let actual = face.families.first().map_or("기본 글꼴", |v| v.0.as_str());
    let mut notes = Vec::new();
    if query(name, bold, italic, false).is_none() {
        notes.push(format!("‘{name}’이 없어 ‘{actual}’ 등 대체 글꼴로 표시합니다. 원래 이름은 프로젝트에 유지합니다."));
    }
    if face.weight.0 != if bold { 700 } else { 400 }
        || (italic && face.style == Style::Normal)
        || (!italic && face.style != Style::Normal)
    {
        notes.push("선택한 굵기·기울임은 설치된 가까운 스타일로 표시합니다.".into());
    }
    notes
}
pub fn ui(ui: &mut egui::Ui, salt: &str, font: &mut String, bold: bool, italic: bool) -> bool {
    let before = font.clone();
    ui.push_id(salt, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(font).desired_width(140.));
            let id = ui.make_persistent_id("font-search");
            let mut search = ui
                .ctx()
                .data(|d| d.get_temp::<String>(id))
                .unwrap_or_default();
            egui::ComboBox::from_id_salt("installed-fonts")
                .selected_text("설치된 글꼴")
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                .width(140.)
                .show_ui(ui, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut search)
                            .hint_text("이름 검색")
                            .desired_width(260.),
                    );
                    let query = search.to_lowercase();
                    let filtered: Vec<_> = catalog()
                        .iter()
                        .filter(|entry| {
                            query
                                .split_whitespace()
                                .all(|word| entry.search.contains(word))
                        })
                        .collect();
                    ui.label(format!("{}개 글꼴", filtered.len()));
                    egui::ScrollArea::vertical().max_height(240.).show_rows(
                        ui,
                        24.,
                        filtered.len(),
                        |ui, rows| {
                            for index in rows {
                                let entry = filtered[index];
                                if ui
                                    .selectable_label(*font == entry.name, &entry.name)
                                    .on_hover_text(format!(
                                        "{}\n설치된 스타일: {}",
                                        entry.aliases.join(" · "),
                                        entry.variants.join(" · ")
                                    ))
                                    .clicked()
                                {
                                    *font = entry.name.clone();
                                    ui.close();
                                }
                            }
                        },
                    );
                    if filtered.is_empty() {
                        ui.label("일치하는 글꼴이 없습니다.");
                    }
                });
            ui.ctx().data_mut(|d| d.insert_temp(id, search));
        });
        for note in notices(font, bold, italic) {
            ui.colored_label(egui::Color32::YELLOW, note);
        }
    });
    *font != before
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installed_families_aliases_styles_and_fallback_are_preserved() {
        let db = crate::vector::fonts();
        let entries = catalog();
        assert!(!entries.is_empty());
        for face in db.faces() {
            let Some((name, _)) = face.families.first() else {
                continue;
            };
            let entry = entries.iter().find(|entry| entry.name == *name).unwrap();
            assert!(!entry.variants.is_empty());
            for (alias, _) in &face.families {
                assert!(entry.aliases.contains(alias));
                assert!(entry.search.contains(&alias.to_lowercase()));
            }
        }
        assert!(entries.windows(2).all(|w| w[0].name < w[1].name));
        for name in ["sans-serif", "serif", "monospace"] {
            assert_eq!(
                face_id(name, false, false),
                db.query(&Query {
                    families: &[family(name), Family::SansSerif],
                    ..Default::default()
                })
            );
        }
        let missing = "PhotoEditor Missing Font 43";
        assert!(query(missing, false, false, false).is_none());
        assert!(
            notices(missing, false, false)
                .iter()
                .any(|n| n.contains(missing) && n.contains("프로젝트에 유지"))
        );
        let italic = db
            .faces()
            .find(|f| f.style == Style::Italic && f.weight == Weight(400))
            .unwrap();
        let name = &italic.families[0].0;
        assert!(!synthetic_italic(name, false, true));
        let upright_only = entries
            .iter()
            .find(|e| {
                db.faces()
                    .filter(|f| f.families[0].0 == e.name)
                    .all(|f| f.style == Style::Normal)
            })
            .unwrap();
        assert!(synthetic_italic(&upright_only.name, false, true));
        assert!(!synthetic_italic(&upright_only.name, false, false));
    }
    #[test]
    fn popup_search_selects_localized_family_and_preserves_missing_names() {
        let ctx = egui::Context::default();
        let mut font = "PhotoEditor Missing Font 43".to_string();
        let original = font.clone();
        let mut time = 0.;
        let frame = |font: &mut String, time: f64, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::new(800., 700.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        super::ui(ui, "test", font, false, false);
                    });
                },
            );
            output.textures_delta.clear();
            output
        };
        let position = |output: &egui::FullOutput, text: &str| {
            output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.job.text == text => {
                        Some(t.pos + egui::Vec2::splat(4.))
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("missing label: {text}"))
        };
        let click = |font: &mut String, time: &mut f64, point| {
            *time += 0.1;
            frame(font, *time, vec![egui::Event::PointerMoved(point)]);
            for pressed in [true, false] {
                *time += 0.1;
                frame(
                    font,
                    *time,
                    vec![egui::Event::PointerButton {
                        pos: point,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
            }
        };
        for _ in 0..3 {
            time += 0.1;
            frame(&mut font, time, vec![]);
        }
        assert_eq!(font, original);
        time += 0.1;
        let output = frame(&mut font, time, vec![]);
        click(&mut font, &mut time, position(&output, "설치된 글꼴"));
        for _ in 0..3 {
            time += 0.1;
            frame(&mut font, time, vec![]);
        }
        time += 0.1;
        let output = frame(&mut font, time, vec![]);
        click(&mut font, &mut time, position(&output, "이름 검색"));
        time += 0.1;
        frame(
            &mut font,
            time,
            vec![egui::Event::Text("없는검색결과43".into())],
        );
        time += 0.1;
        let output = frame(&mut font, time, vec![]);
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.job.text == "일치하는 글꼴이 없습니다.")));
        assert_eq!(font, original);
        time += 0.1;
        frame(
            &mut font,
            time,
            vec![egui::Event::Key {
                key: egui::Key::A,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers {
                    ctrl: true,
                    command: true,
                    ..Default::default()
                },
            }],
        );
        time += 0.1;
        frame(&mut font, time, vec![egui::Event::Text("맑은".into())]);
        for _ in 0..3 {
            time += 0.1;
            frame(&mut font, time, vec![]);
        }
        time += 0.1;
        let output = frame(&mut font, time, vec![]);
        click(&mut font, &mut time, position(&output, "Malgun Gothic"));
        assert_eq!(font, "Malgun Gothic");
    }
    #[test]
    fn paragraph_run_fonts_roundtrip_undo_and_italic_layout() {
        use crate::{
            document::{Document, Editor},
            rich_text,
            text_canvas::Session,
            vector::Content,
        };
        let mut editor = Editor::new(Document::new(320, 180, false).unwrap(), true);
        let mut session = Session::new(1, [0.; 2], [20, 30, 40, 255]);
        rich_text::replace_text(&mut session.content, "AB 한글".into());
        if let Content::Text { font, .. } = &mut session.content {
            *font = "PhotoEditor Missing Font 43".into();
        }
        let mut style = rich_text::base(&session.content);
        let db = crate::vector::fonts();
        style.font = db
            .faces()
            .find(|f| f.style == super::Style::Italic && f.weight == Weight(400))
            .unwrap()
            .families[0]
            .0
            .clone();
        style.italic = true;
        rich_text::apply_style(&mut session.content, 0..2, style.clone());
        let ctx = egui::Context::default();
        let mut job = None;
        let mut output = ctx.run_ui(Default::default(), |_ui| {
            rich_text::install_fonts(&ctx, &session.content);
            job = Some(rich_text::layout_job(
                &ctx,
                &session.content,
                "AB 한글",
                1.,
                300.,
            ));
        });
        output.textures_delta.clear();
        let job = job.unwrap();
        assert!(!job.sections[0].format.italics);
        let svg = session.content.svg();
        assert!(svg.contains(&style.font));
        assert!(svg.contains("PhotoEditor Missing Font 43"));
        assert!(svg.contains("font-style='italic'"));
        let before = editor.document.clone();
        assert!(session.apply(&mut editor).unwrap());
        let updated = editor.document.clone();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/font-picker");
        std::fs::create_dir_all(&dir).unwrap();
        crate::files::save_project(&updated, &dir.join("fonts.rphoto")).unwrap();
        let restored = crate::files::load_project(&dir.join("fonts.rphoto")).unwrap();
        assert_eq!(
            restored.active_layer().unwrap().content,
            Some(session.content.clone())
        );
        assert_eq!(restored.composite(), updated.composite());
        editor.undo();
        assert_eq!(editor.document.composite(), before.composite());
        editor.redo();
        assert_eq!(
            editor.document.active_layer().unwrap().content,
            Some(session.content)
        );
    }
}
