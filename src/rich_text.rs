use crate::vector::{Content, color, escape, fonts};
use eframe::egui::{self, Color32, FontFamily, FontId};
use serde::{Deserialize, Serialize};
use std::ops::Range;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Style {
    pub font: String,
    pub size: f32,
    pub color: [u8; 4],
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub style: Style,
}

pub fn base(content: &Content) -> Style {
    if let Content::Text {
        font, size, color, ..
    } = content
    {
        Style {
            font: font.clone(),
            size: *size,
            color: *color,
            bold: false,
            italic: false,
            underline: false,
        }
    } else {
        unreachable!("Text content required")
    }
}
pub fn validate(content: &Content) -> Result<(), String> {
    if let Content::Text { text, spans, .. } = content {
        let count = text.chars().count();
        if count > 100_000 || spans.len() > 4096 {
            return Err("텍스트·스타일이 편집 한도를 초과합니다.".into());
        }
        let mut end = 0;
        for span in spans {
            if span.start < end
                || span.start >= span.end
                || span.end > count
                || !span.style.size.is_finite()
                || !(1.0..=2000.0).contains(&span.style.size)
                || span.style.font.len() > 256
            {
                return Err("글자별 스타일의 범위·크기를 확인하세요.".into());
            }
            end = span.end;
        }
    }
    Ok(())
}
pub fn style_at(content: &Content, index: usize) -> Style {
    if let Content::Text { spans, .. } = content {
        spans
            .get(spans.partition_point(|s| s.end <= index))
            .filter(|s| index >= s.start)
            .map(|s| s.style.clone())
            .unwrap_or_else(|| base(content))
    } else {
        unreachable!()
    }
}
fn compress(styles: &[Style], default: &Style) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (i, style) in styles.iter().enumerate() {
        if style == default {
            continue;
        }
        if let Some(last) = spans.last_mut()
            && last.end == i
            && last.style == *style
        {
            last.end += 1;
        } else {
            spans.push(Span {
                start: i,
                end: i + 1,
                style: style.clone(),
            });
        }
    }
    spans
}
pub fn apply_style(content: &mut Content, range: Range<usize>, style: Style) {
    let default = base(content);
    let count = if let Content::Text { text, .. } = content {
        text.chars().count()
    } else {
        return;
    };
    let mut styles: Vec<_> = (0..count).map(|i| style_at(content, i)).collect();
    for s in styles
        .iter_mut()
        .take(range.end.min(count))
        .skip(range.start.min(count))
    {
        *s = style.clone();
    }
    if let Content::Text { spans, .. } = content {
        *spans = compress(&styles, &default);
    }
}
// Prefix/suffix matching keeps character ranges stable through Unicode typing and IME replacement.
pub fn replace_text(content: &mut Content, new: String) {
    let default = base(content);
    let Content::Text { text, .. } = content else {
        return;
    };
    if *text == new {
        return;
    }
    let old: Vec<_> = text.chars().collect();
    let chars: Vec<_> = new.chars().collect();
    let prefix = old.iter().zip(&chars).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(chars[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let inherited = if old.is_empty() {
        default.clone()
    } else {
        style_at(content, prefix.saturating_sub(1).min(old.len() - 1))
    };
    let mut styles: Vec<_> = (0..prefix).map(|i| style_at(content, i)).collect();
    styles.extend(std::iter::repeat_n(
        inherited,
        chars.len() - prefix - suffix,
    ));
    styles.extend((old.len() - suffix..old.len()).map(|i| style_at(content, i)));
    if let Content::Text { text, spans, .. } = content {
        *text = new;
        *spans = compress(&styles, &default);
    }
}
fn face_id(style: &Style) -> Option<resvg::usvg::fontdb::ID> {
    crate::font_picker::face_id(&style.font, style.bold, style.italic)
}
fn measure(text: &str, style: &Style, spacing: f32) -> f32 {
    face_id(style)
        .and_then(|id| {
            fonts().with_face_data(id, |data, index| {
                rustybuzz::Face::from_slice(data, index).map(|face| {
                    let mut buffer = rustybuzz::UnicodeBuffer::new();
                    buffer.push_str(text);
                    buffer.guess_segment_properties();
                    rustybuzz::shape(&face, &[], buffer)
                        .glyph_positions()
                        .iter()
                        .map(|p| p.x_advance as f32)
                        .sum::<f32>()
                        * style.size
                        / face.units_per_em() as f32
                })
            })
        })
        .flatten()
        .unwrap_or(text.chars().count() as f32 * style.size)
        + spacing * text.chars().count().saturating_sub(1) as f32
}
fn runs(content: &Content, chars: &[char], range: Range<usize>) -> Vec<(String, Style)> {
    let mut result: Vec<(String, Style)> = vec![];
    for (offset, c) in chars[range.clone()].iter().enumerate() {
        let style = style_at(content, range.start + offset);
        if let Some((s, previous)) = result.last_mut()
            && *previous == style
        {
            s.push(*c);
        } else {
            result.push((c.to_string(), style));
        }
    }
    result
}
fn width(content: &Content, chars: &[char], range: Range<usize>, spacing: f32) -> f32 {
    let runs = runs(content, chars, range);
    runs.iter()
        .map(|(s, st)| measure(s, st, spacing))
        .sum::<f32>()
        + spacing * runs.len().saturating_sub(1) as f32
}
fn lines(content: &Content, chars: &[char]) -> Vec<Range<usize>> {
    let Content::Text {
        width: w,
        spacing,
        wrap,
        ..
    } = content
    else {
        return vec![];
    };
    let mut result = vec![];
    let mut start = 0;
    loop {
        let end = chars[start..]
            .iter()
            .position(|&c| c == '\n')
            .map_or(chars.len(), |n| start + n);
        if !*wrap || start == end {
            result.push(start..end);
        } else {
            while start < end {
                // Exponential search bounds shaping work for short lines in long paragraphs.
                let fits =
                    |n| width(content, chars, start..n, *spacing) <= (*w as f32 - 8.0).max(1.0);
                let mut low = (start + 1).min(end);
                let mut high = low;
                while high < end && fits(high) {
                    low = high;
                    high = (start + (high - start) * 2).min(end);
                }
                if fits(high) {
                    low = high;
                } else {
                    while low + 1 < high {
                        let middle = low + (high - low) / 2;
                        if fits(middle) {
                            low = middle;
                        } else {
                            high = middle;
                        }
                    }
                }
                let mut boundary = low;
                if boundary < end
                    && !chars[boundary].is_whitespace()
                    && let Some(space) = chars[start..boundary]
                        .iter()
                        .rposition(|c| c.is_whitespace())
                    && space > 0
                {
                    boundary = start + space;
                }
                result.push(start..boundary);
                start = boundary;
                while start < end && chars[start].is_whitespace() {
                    start += 1;
                }
            }
        }
        if end == chars.len() {
            break;
        }
        start = end + 1;
    }
    result
}
pub fn svg(content: &Content) -> String {
    let Content::Text {
        text,
        font,
        size,
        width: w,
        height,
        color: c,
        line_height,
        spacing,
        align,
        wrap,
        spans,
    } = content
    else {
        return String::new();
    };
    let x = match align {
        1 => *w as f32 / 2.0,
        2 => *w as f32 - 4.0,
        _ => 4.0,
    };
    let anchor = match align {
        1 => "middle",
        2 => "end",
        _ => "start",
    };
    let body = if spans.is_empty() {
        crate::vector::text_lines(
            text,
            font,
            *size,
            *spacing,
            (*w as f32 - 8.0).max(1.0),
            *wrap,
        )
        .iter()
        .enumerate()
        .map(|(i, line)| {
            format!(
                "<tspan x='{x}' y='{}'>{}</tspan>",
                *size + i as f32 * size * line_height,
                escape(line)
            )
        })
        .collect::<String>()
    } else {
        let mut y = 0.0;
        let mut out = String::new();
        let chars: Vec<_> = text.chars().collect();
        for range in lines(content, &chars) {
            let runs = runs(content, &chars, range);
            let largest = if runs.is_empty() {
                *size
            } else {
                runs.iter().map(|(_, s)| s.size).fold(0.0, f32::max)
            };
            y += largest;
            let body=runs.iter().map(|(text,s)|format!("<tspan font-family='{},sans-serif' font-size='{}' fill='{}' font-weight='{}' font-style='{}' text-decoration='{}'>{}</tspan>",escape(&s.font),s.size,color(s.color),if s.bold {700}else{400},if s.italic {"italic"}else{"normal"},if s.underline {"underline"}else{"none"},escape(text))).collect::<String>();
            out.push_str(&format!("<tspan x='{x}' y='{y}'>{body}</tspan>"));
            y += largest * (line_height - 1.0);
        }
        out
    };
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{w}' height='{height}'><text xml:space='preserve' font-family='{},sans-serif' font-size='{size}' fill='{}' text-anchor='{anchor}' letter-spacing='{spacing}'>{body}</text></svg>",
        escape(font),
        color(*c)
    )
}
fn font_key(style: &Style) -> String {
    format!("text-face-{}-{}-{}", style.font, style.bold, style.italic)
}
pub fn install_fonts(ctx: &egui::Context, content: &Content) {
    let mut defs = ctx.fonts(|f| f.definitions().clone());
    let mut changed = false;
    let mut styles = vec![base(content)];
    if let Content::Text { spans, .. } = content {
        styles.extend(spans.iter().map(|s| s.style.clone()));
    }
    for style in styles {
        let key = font_key(&style);
        if defs
            .families
            .contains_key(&FontFamily::Name(key.clone().into()))
        {
            continue;
        }
        if let Some((data, index)) = face_id(&style)
            .and_then(|id| fonts().with_face_data(id, |data, index| (data.to_vec(), index)))
        {
            let mut font = egui::FontData::from_owned(data);
            font.index = index;
            defs.font_data
                .insert(key.clone(), std::sync::Arc::new(font));
            let mut fallback = defs
                .families
                .get(&FontFamily::Proportional)
                .cloned()
                .unwrap_or_default();
            fallback.insert(0, key.clone());
            defs.families.insert(FontFamily::Name(key.into()), fallback);
            changed = true;
        }
    }
    if changed {
        ctx.set_fonts(defs);
        ctx.request_repaint();
    }
}
pub fn layout_job(
    ctx: &egui::Context,
    content: &Content,
    text: &str,
    zoom: f32,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::default();
    let Content::Text {
        spacing,
        line_height,
        align,
        wrap,
        ..
    } = content
    else {
        return job;
    };
    job.wrap.max_width = if *wrap { wrap_width } else { f32::INFINITY };
    job.halign = match align {
        1 => egui::Align::Center,
        2 => egui::Align::Max,
        _ => egui::Align::Min,
    };
    let chars: Vec<_> = text.chars().collect();
    for (run, style) in runs(content, &chars, 0..chars.len()) {
        let key = font_key(&style);
        let family = ctx.fonts(|f| {
            if f.definitions()
                .families
                .contains_key(&FontFamily::Name(key.clone().into()))
            {
                FontFamily::Name(key.clone().into())
            } else {
                FontFamily::Proportional
            }
        });
        let rgba = style.color;
        job.append(
            &run,
            0.0,
            egui::TextFormat {
                font_id: FontId::new((style.size * zoom).clamp(2.0, 600.0), family),
                extra_letter_spacing: spacing * zoom,
                line_height: Some((style.size * zoom * line_height).max(2.0)),
                color: Color32::from_rgba_unmultiplied(rgba[0], rgba[1], rgba[2], rgba[3]),
                italics: crate::font_picker::synthetic_italic(
                    &style.font,
                    style.bold,
                    style.italic,
                ),
                underline: if style.underline {
                    egui::Stroke::new(1.0, Color32::from_rgb(rgba[0], rgba[1], rgba[2]))
                } else {
                    egui::Stroke::NONE
                },
                ..Default::default()
            },
        );
    }
    job
}
pub fn controls(
    ui: &mut egui::Ui,
    content: &mut Content,
    selection: &mut Range<usize>,
    style: &mut Style,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label("선택 문자");
        ui.add(
            egui::DragValue::new(&mut selection.start)
                .range(0..=100_000)
                .prefix("시작 "),
        );
        ui.label("–");
        ui.add(
            egui::DragValue::new(&mut selection.end)
                .range(0..=100_000)
                .prefix("끝 "),
        );
        crate::font_picker::ui(
            ui,
            "selected-font",
            &mut style.font,
            style.bold,
            style.italic,
        );
        ui.add(
            egui::DragValue::new(&mut style.size)
                .range(1.0..=2000.0)
                .prefix("크기 "),
        );
        ui.checkbox(&mut style.bold, "굵게");
        ui.checkbox(&mut style.italic, "기울임");
        ui.checkbox(&mut style.underline, "밑줄");
        ui.color_edit_button_srgba_unmultiplied(&mut style.color);
        if ui
            .add_enabled(
                selection.start < selection.end,
                egui::Button::new("선택에 스타일 적용"),
            )
            .clicked()
        {
            apply_style(content, selection.clone(), style.clone());
        }
        if ui.button("선택 스타일 초기화").clicked() {
            apply_style(content, selection.clone(), base(content));
        }
    });
}

pub fn edit_ui(
    ui: &mut egui::Ui,
    content: &mut Content,
    selection: &mut Range<usize>,
    current: &mut Option<Style>,
) {
    let mut text = if let Content::Text { text, .. } = content {
        text.clone()
    } else {
        return;
    };
    let output = egui::TextEdit::multiline(&mut text)
        .desired_rows(5)
        .desired_width(440.0)
        .show(ui);
    if output.response.changed() {
        replace_text(content, text);
    }
    if output.response.has_focus()
        && let Some(range) = output.cursor_range
    {
        let a: usize = range.primary.index.min(range.secondary.index).into();
        let b: usize = range.primary.index.max(range.secondary.index).into();
        if a < b && *selection != (a..b) {
            *selection = a..b;
            *current = Some(style_at(content, a));
        }
    }
    if let Content::Text {
        font,
        size,
        width,
        height,
        color,
        line_height,
        spacing,
        align,
        wrap,
        ..
    } = content
    {
        ui.label("글꼴");
        crate::font_picker::ui(ui, "paragraph-font", font, false, false);
        ui.add(crate::numeric_slider::Slider::new(size, 4.0..=300.0).text("글자 크기"));
        ui.add(crate::numeric_slider::Slider::new(line_height, 0.5..=3.0).text("행간"));
        ui.add(crate::numeric_slider::Slider::new(spacing, -10.0..=30.0).text("자간"));
        ui.horizontal(|ui| {
            ui.selectable_value(align, 0, "왼쪽");
            ui.selectable_value(align, 1, "중앙");
            ui.selectable_value(align, 2, "오른쪽");
        });
        ui.checkbox(wrap, "문단 자동 줄바꿈");
        ui.color_edit_button_srgba_unmultiplied(color);
        ui.horizontal(|ui| {
            ui.add(
                egui::DragValue::new(width)
                    .range(1..=10000)
                    .prefix("상자 폭 "),
            );
            ui.add(
                egui::DragValue::new(height)
                    .range(1..=10000)
                    .prefix("높이 "),
            );
        });
    }
    let style = current.get_or_insert_with(|| base(content));
    controls(ui, content, selection, style);
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(value: &str) -> Content {
        let mut session = crate::text_canvas::Session::new(1, [0.0; 2], [20, 30, 40, 255]);
        replace_text(&mut session.content, value.into());
        session.content
    }
    #[test]
    fn unicode_replacement_preserves_styles_outside_ime_range() {
        let mut content = text("가나다🙂끝");
        let mut emphasis = base(&content);
        emphasis.bold = true;
        emphasis.size = 48.0;
        apply_style(&mut content, 1..3, emphasis.clone());
        let mut tail = base(&content);
        tail.underline = true;
        apply_style(&mut content, 4..5, tail.clone());
        replace_text(&mut content, "가나둘다🙂끝".into());
        assert_eq!(style_at(&content, 2), emphasis);
        assert_eq!(style_at(&content, 4), base(&content));
        assert_eq!(style_at(&content, 5), tail);
        replace_text(&mut content, "가🙂끝".into());
        assert_eq!(style_at(&content, 2), tail);
        validate(&content).unwrap();
    }
    #[test]
    fn legacy_defaults_and_invalid_character_ranges() {
        let mut content: Content = serde_json::from_str(r#"{"Text":{"text":"한글","font":"Malgun Gothic","size":20.0,"width":90,"height":60,"color":[0,0,0,255],"line_height":1.2,"spacing":0.0,"align":0}}"#).unwrap();
        assert!(matches!(&content, Content::Text { spans, wrap:false, .. } if spans.is_empty()));
        let mut style = base(&content);
        style.italic = true;
        if let Content::Text { spans, .. } = &mut content {
            spans.push(Span {
                start: 1,
                end: 3,
                style,
            });
        }
        assert!(content.validate().is_err());
    }
    #[test]
    fn styled_paragraph_wrap_uses_run_sizes_and_keeps_unicode_and_blank_lines() {
        let mut content = text("한글 긴문단을 자동으로 줄바꿈합니다\n\n끝\n");
        let mut style = base(&content);
        style.size = 52.0;
        style.bold = true;
        apply_style(&mut content, 0..2, style);
        if let Content::Text { width, .. } = &mut content {
            *width = 150;
        }
        let chars: Vec<_> = if let Content::Text { text, .. } = &content {
            text.chars().collect()
        } else {
            unreachable!()
        };
        let lines = lines(&content, &chars);
        assert!(lines.len() > 4);
        assert!(lines.iter().any(|r| r.is_empty()));
        assert_eq!(lines.last().unwrap().len(), 0);
        let joined = lines
            .iter()
            .map(|r| chars[r.clone()].iter().collect::<String>())
            .collect::<String>();
        assert_eq!(
            joined.replace(' ', ""),
            "한글긴문단을자동으로줄바꿈합니다끝"
        );
        for range in lines {
            if range.len() > 1 {
                assert!(width(&content, &chars, range, 0.0) <= 142.1);
            }
        }
        let svg = content.svg();
        assert!(svg.contains("font-weight='700'"));
        assert!(svg.contains("font-size='52'"));
        assert!(content.render().unwrap().pixels().any(|p| p[3] > 0));
    }
    #[test]
    fn editable_layout_keeps_paragraph_options_and_style_runs() {
        let mut content = text("AV 한글");
        if let Content::Text {
            spacing,
            line_height,
            align,
            wrap,
            ..
        } = &mut content
        {
            *spacing = 3.0;
            *line_height = 1.7;
            *align = 2;
            *wrap = false;
        }
        let mut style = base(&content);
        style.underline = true;
        style.color = [200, 10, 30, 128];
        apply_style(&mut content, 3..5, style);
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(Default::default(), |_ui| {
            let job = layout_job(&ctx, &content, "AV 한글", 2.0, 250.0);
            assert_eq!(job.sections.len(), 2);
            assert!(job.wrap.max_width.is_infinite());
            assert_eq!(job.halign, egui::Align::Max);
            assert_eq!(job.sections[0].format.extra_letter_spacing, 6.0);
            assert!((job.sections[0].format.line_height.unwrap() - 108.8).abs() < 0.01);
            assert_eq!(job.sections[1].format.color.a(), 128);
            assert_ne!(job.sections[1].format.underline, egui::Stroke::NONE);
        });
        output.textures_delta.clear();
    }
}
