use eframe::egui;
#[path = "svg_image_panel.rs"]
mod image_panel;

#[derive(Clone)]
pub struct Element {
    pub index: usize,
    pub label: String,
    pub attributes: Vec<(String, String)>,
    pub fragment: String,
    image_name: Option<String>,
}
#[cfg(test)]
pub fn elements(source: &str) -> Result<Vec<Element>, String> {
    elements_impl(source, false)
}
fn elements_impl(source: &str, compact: bool) -> Result<Vec<Element>, String> {
    let doc = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let mut result = vec![];
    for (index, node) in doc
        .descendants()
        .filter(|n| n.is_element())
        .enumerate()
        .skip(1)
    {
        if result.len() >= 4096 {
            return Err("SVG 요소가 편집 한도를 초과합니다.".into());
        }
        let depth = node.ancestors().count().saturating_sub(3);
        let id = node.attribute("id").unwrap_or("");
        let attributes = node
            .attributes()
            .filter(|a| {
                !compact
                    || (!a.name().starts_with("data-rphoto-image-")
                        && !(a.name() == "href" && a.value().trim().starts_with("data:")))
            })
            .map(|a| {
                let prefix = if a.namespace() == Some("http://www.w3.org/1999/xlink") {
                    "xlink:"
                } else {
                    ""
                };
                (format!("{prefix}{}", a.name()), a.value().to_string())
            })
            .collect();
        result.push(Element {
            index,
            label: format!(
                "{}{} {}",
                "  ".repeat(depth.min(12)),
                node.tag_name().name(),
                id
            ),
            attributes,
            image_name: node.attribute("data-rphoto-image-name").map(str::to_owned),
            fragment: if compact {
                String::new()
            } else {
                source[node.range()].to_string()
            },
        });
    }
    Ok(result)
}
fn checked(source: String) -> Result<String, String> {
    roxmltree::Document::parse(&source).map_err(|e| e.to_string())?;
    crate::vector::render_svg(&source)?;
    Ok(source)
}
pub fn attribute(source: &str, index: usize, name: &str, value: &str) -> Result<String, String> {
    checked(attribute_text(source, index, name, value)?)
}
pub(crate) fn attribute_text(
    source: &str,
    index: usize,
    name: &str,
    value: &str,
) -> Result<String, String> {
    if name.is_empty()
        || name.len() > 128
        || !name.chars().enumerate().all(|(i, c)| {
            c.is_ascii_alphabetic()
                || c == '_'
                || ((c.is_ascii_digit() || matches!(c, '-' | '.' | ':')) && i > 0)
        })
    {
        return Err("SVG 속성 이름을 확인하세요.".into());
    }
    let doc = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let node = doc
        .descendants()
        .filter(|n| n.is_element())
        .nth(index)
        .ok_or("SVG 요소가 없습니다.")?;
    let escaped = crate::vector::escape(value);
    let mut output = source.to_string();
    let local = name.strip_prefix("xlink:").unwrap_or(name);
    if let Some(attr) = node
        .attributes()
        .find(|a| a.name() == local && (name.starts_with("xlink:") == a.namespace().is_some()))
    {
        output.replace_range(attr.range_value(), &escaped);
    } else {
        let start = node.range().start;
        let mut quote = None;
        let mut end = None;
        for (i, c) in source[start..].char_indices() {
            if let Some(q) = quote {
                if c == q {
                    quote = None;
                }
            } else if matches!(c, '\'' | '"') {
                quote = Some(c);
            } else if c == '>' {
                end = Some(start + i);
                break;
            }
        }
        let mut end = end.ok_or("SVG 태그가 잘못되었습니다.")?;
        if source[..end].ends_with('/') {
            end -= 1;
        }
        output.insert_str(end, &format!(" {name}=\"{escaped}\""));
    }
    Ok(output)
}
pub fn fragment(source: &str, index: usize, new: &str) -> Result<String, String> {
    let doc = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let node = doc
        .descendants()
        .filter(|n| n.is_element())
        .nth(index)
        .ok_or("SVG 요소가 없습니다.")?;
    if index == 0 {
        return Err("루트는 전체 SVG 원본에서 편집하세요.".into());
    }
    let mut output = source.to_string();
    output.replace_range(node.range(), new);
    checked(output)
}
#[derive(Default)]
pub struct Panel {
    selected: Option<usize>,
    source_key: String,
    name: String,
    value: String,
    fragment: String,
    error: Option<String>,
    image: image_panel::Panel,
}
impl Panel {
    pub fn close(&mut self) {
        self.image.close();
    }
    pub fn busy(&self) -> bool {
        self.image.busy()
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, source: &mut String) {
        let replaced = image_panel::poll(&mut self.image, ui.ctx(), source, self.selected);
        let list = match elements_impl(source, true) {
            Ok(list) => list,
            Err(error) => {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
                ui.add(
                    egui::TextEdit::multiline(source)
                        .id(egui::Id::new("svg-source-editor"))
                        .code_editor()
                        .desired_rows(10)
                        .desired_width(440.0),
                );
                return;
            }
        };
        ui.label("요소 선택 후 위치·크기·색·변형·경로 등의 속성을 편집하세요.");
        egui::ScrollArea::vertical()
            .id_salt("svg-elements")
            .max_height(150.0)
            .show(ui, |ui| {
                for element in &list {
                    if ui
                        .selectable_label(self.selected == Some(element.index), &element.label)
                        .clicked()
                    {
                        self.selected = Some(element.index);
                        self.source_key.clear();
                    }
                }
            });
        if let Some(element) = list.iter().find(|e| Some(e.index) == self.selected) {
            if self.source_key != *source {
                if !replaced {
                    self.image.clear_preview();
                }
                self.fragment = element.fragment.clone();
                self.source_key = source.clone();
            }
            if element
                .label
                .split_whitespace()
                .next()
                .is_some_and(|name| matches!(name, "image" | "feImage"))
            {
                if let Some(name) = &element.image_name {
                    ui.label(format!("원본 이미지: {name}"));
                }
                self.image.ui(ui, source, element.index);
            }
            ui.horizontal_wrapped(|ui| {
                for (name, value) in &element.attributes {
                    if ui.small_button(name).clicked() {
                        self.name = name.clone();
                        self.value = value.clone();
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("속성");
                ui.add(egui::TextEdit::singleline(&mut self.name).desired_width(100.0));
                ui.label("값");
                ui.add(egui::TextEdit::singleline(&mut self.value).desired_width(180.0));
            });
            ui.horizontal_wrapped(|ui| {
                for name in [
                    "x",
                    "y",
                    "width",
                    "height",
                    "fill",
                    "stroke",
                    "stroke-width",
                    "opacity",
                    "transform",
                    "d",
                ] {
                    if ui.small_button(name).clicked() {
                        self.name = name.into();
                        self.value = element
                            .attributes
                            .iter()
                            .find(|(n, _)| n == name)
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default();
                    }
                }
            });
            if ui.button("속성 적용").clicked() {
                match attribute(source, element.index, &self.name, &self.value) {
                    Ok(updated) => {
                        *source = updated;
                        self.error = None;
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            ui.collapsing("선택 요소 원본", |ui| {
                if self.fragment.is_empty()
                    && let Ok(doc) = roxmltree::Document::parse(source)
                    && let Some(node) = doc
                        .descendants()
                        .filter(|n| n.is_element())
                        .nth(element.index)
                {
                    self.fragment = source[node.range()].to_owned();
                }
                ui.add(
                    egui::TextEdit::multiline(&mut self.fragment)
                        .code_editor()
                        .desired_rows(4)
                        .desired_width(440.0),
                );
                if ui.button("선택 요소 적용").clicked() {
                    match fragment(source, element.index, &self.fragment) {
                        Ok(updated) => {
                            *source = updated;
                            self.error = None;
                            self.selected = None;
                        }
                        Err(e) => self.error = Some(e),
                    }
                }
            });
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        ui.collapsing("전체 SVG 원본", |ui| {
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(source)
                            .id(egui::Id::new("svg-source-editor"))
                            .code_editor()
                            .desired_width(440.0),
                    );
                });
        });
    }
}

// resvg's gradient shader interpolates sRGB. Densify only the temporary preview,
// preserving the original SVG's linearRGB attribute and editable stop structure.
pub fn linear_preview(source: &str) -> Result<String, String> {
    if !source.contains("linearRGB") {
        return Ok(source.to_string());
    }
    let doc = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let mut edits = vec![];
    let mut sheet = simplecss::StyleSheet::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("style")) {
        for child in node.children().filter(|n| n.is_text()) {
            sheet.parse_more(child.text().unwrap_or_default());
        }
    }
    let property = |node: roxmltree::Node<'_, '_>, name: &str| -> Option<String> {
        let mut value = node.attribute(name).map(str::to_string);
        let mut important = false;
        for rule in &sheet.rules {
            if rule.selector.matches(&CssNode(node)) {
                for d in rule.declarations.iter().filter(|d| d.name == name) {
                    if !important || d.important {
                        value = Some(d.value.to_string());
                        important = d.important;
                    }
                }
            }
        }
        if let Some(inline) = node.attribute("style") {
            for d in simplecss::DeclarationTokenizer::from(inline).filter(|d| d.name == name) {
                if !important || d.important {
                    value = Some(d.value.to_string());
                    important = d.important;
                }
            }
        }
        value.filter(|v| !matches!(v.as_str(), "inherit" | "unset"))
    };
    let number = |value: &str| -> Option<f32> {
        if let Some(v) = value.trim().strip_suffix('%') {
            v.parse::<f32>().ok().map(|v| v / 100.0)
        } else {
            value.trim().parse().ok()
        }
    };
    for gradient in doc
        .descendants()
        .filter(|n| n.has_tag_name("linearGradient") || n.has_tag_name("radialGradient"))
    {
        let mode = gradient
            .ancestors()
            .find_map(|n| property(n, "color-interpolation"));
        if mode.as_deref() != Some("linearRGB") {
            continue;
        }
        let mut target = gradient;
        let mut visited = std::collections::HashSet::new();
        while !target.children().any(|n| n.has_tag_name("stop")) {
            let Some(id) = target
                .attribute("href")
                .or_else(|| target.attribute(("http://www.w3.org/1999/xlink", "href")))
                .and_then(|v| v.strip_prefix('#'))
            else {
                break;
            };
            if !visited.insert(id) {
                return Err("순환 SVG 그라디언트 참조입니다.".into());
            }
            let Some(next) = doc.descendants().find(|n| n.attribute("id") == Some(id)) else {
                break;
            };
            target = next;
        }
        let nodes: Vec<_> = target
            .children()
            .filter(|n| n.has_tag_name("stop"))
            .collect();
        if nodes.len() < 2 {
            continue;
        }
        if nodes.len() > 4096 {
            return Err("SVG 그라디언트 색상점이 너무 많습니다.".into());
        }
        let mut stops = vec![];
        let mut last = 0.0;
        for node in &nodes {
            let position = property(*node, "offset")
                .and_then(|v| number(&v))
                .unwrap_or(0.0)
                .clamp(last, 1.0);
            last = position;
            let mut color = node
                .ancestors()
                .find_map(|n| property(n, "stop-color"))
                .unwrap_or_else(|| "black".into());
            if color == "currentColor" {
                color = node
                    .ancestors()
                    .find_map(|n| property(n, "color"))
                    .unwrap_or_else(|| "black".into());
            }
            let rgba = color
                .parse::<svgtypes::Color>()
                .map_err(|e| e.to_string())?;
            let alpha = property(*node, "stop-opacity")
                .and_then(|v| number(&v))
                .unwrap_or(1.0)
                .clamp(0.0, 1.0)
                * rgba.alpha as f32
                / 255.0;
            stops.push((
                position,
                [
                    rgba.red as f32 / 255.0,
                    rgba.green as f32 / 255.0,
                    rgba.blue as f32 / 255.0,
                    alpha,
                ],
            ));
        }
        let expand = |v: f32| {
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        let compress = |v: f32| {
            if v <= 0.0031308 {
                v * 12.92
            } else {
                1.055 * v.powf(1.0 / 2.4) - 0.055
            }
        };
        let mut body = String::new();
        for (i, pair) in stops.windows(2).enumerate() {
            let (a, b) = (pair[0], pair[1]);
            let sample = |t: f32| {
                std::array::from_fn::<_, 4, _>(|c| {
                    if c == 3 {
                        a.1[c] + (b.1[c] - a.1[c]) * t
                    } else {
                        compress(expand(a.1[c]) + (expand(b.1[c]) - expand(a.1[c])) * t)
                    }
                })
            };
            let mut samples = vec![];
            if i == 0 {
                samples.push((0.0, a.1));
            }
            // Quarter-channel tolerance bounds preview error without 256 stops per segment.
            refine(&sample, 0.0, a.1, 1.0, b.1, 0, &mut samples);
            for (t, rgba) in samples {
                let position = a.0 + (b.0 - a.0) * t;
                body.push_str(&format!("<stop offset='{position}' style='stop-color:rgb({:.5}%,{:.5}%,{:.5}%)!important;stop-opacity:{}!important'/>",rgba[0]*100.0,rgba[1]*100.0,rgba[2]*100.0,rgba[3]));
            }
            if body.len() > 8_000_000 {
                return Err("SVG 그라디언트 미리보기가 메모리 예산을 초과합니다.".into());
            }
        }
        // The consumer gradient owns temporary stops, even when the original inherits them.
        let range = gradient.range();
        let mut original = source[range.clone()].to_string();
        if target == gradient {
            for node in nodes.iter().rev() {
                let r = node.range();
                original.replace_range(r.start - range.start..r.end - range.start, "");
            }
        }
        if let Some(end) = original.rfind("</") {
            original.insert_str(end, &body);
        } else if let Some(end) = original.rfind("/>") {
            original.replace_range(
                end..end + 2,
                &format!(">{body}</{}>", gradient.tag_name().name()),
            );
        }
        edits.push((range, original));
    }
    edits.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut output = source.to_string();
    for (range, new) in edits {
        output.replace_range(range, &new);
    }
    Ok(output)
}

struct CssNode<'a, 'input>(roxmltree::Node<'a, 'input>);
impl simplecss::Element for CssNode<'_, '_> {
    fn parent_element(&self) -> Option<Self> {
        self.0.parent_element().map(Self)
    }
    fn prev_sibling_element(&self) -> Option<Self> {
        self.0
            .prev_siblings()
            .skip(1)
            .find(|n| n.is_element())
            .map(Self)
    }
    fn has_local_name(&self, name: &str) -> bool {
        self.0.has_tag_name(name)
    }
    fn attribute_matches(&self, name: &str, op: simplecss::AttributeOperator<'_>) -> bool {
        self.0.attribute(name).is_some_and(|v| op.matches(v))
    }
    fn pseudo_class_matches(&self, class: simplecss::PseudoClass<'_>) -> bool {
        match class {
            simplecss::PseudoClass::FirstChild => {
                self.0.prev_siblings().skip(1).all(|n| !n.is_element())
            }
            simplecss::PseudoClass::Link => {
                self.0.has_tag_name("a") && self.0.attribute("href").is_some()
            }
            _ => false,
        }
    }
}
fn refine(
    sample: &impl Fn(f32) -> [f32; 4],
    a: f32,
    ca: [f32; 4],
    b: f32,
    cb: [f32; 4],
    depth: u8,
    out: &mut Vec<(f32, [f32; 4])>,
) {
    let mid = (a + b) * 0.5;
    let cm = sample(mid);
    if depth < 12 && (0..3).any(|c| (cm[c] - (ca[c] + cb[c]) * 0.5).abs() > 0.25 / 255.0) {
        refine(sample, a, ca, mid, cm, depth + 1, out);
        refine(sample, mid, cm, b, cb, depth + 1, out);
    } else {
        out.push((b, cb));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const SOURCE: &str = r#"<svg xmlns='http://www.w3.org/2000/svg' width='64' height='32'><style>#shape {fill: red}</style><defs><mask id='mask'><rect width='64' height='32' fill='white'/></mask><path id='shape' d='M0 0H20V20H0Z'/></defs><g transform='translate(4 3)' mask='url(#mask)'><use href='#shape'/></g></svg>"#;
    #[test]
    fn element_changes_preserve_unselected_defs_css_references_and_project_undo() {
        let list = elements(SOURCE).unwrap();
        let id = list
            .iter()
            .find(|e| e.attributes.iter().any(|(n, v)| n == "id" && v == "shape"))
            .unwrap()
            .index;
        let updated = attribute(SOURCE, id, "d", "M0 0H40V20H0Z").unwrap();
        assert!(updated.contains("<style>#shape {fill: red}</style>"));
        assert!(updated.contains("href='#shape'") && updated.contains("mask='url(#mask)'"));
        assert_eq!(
            crate::vector::render_svg(SOURCE).unwrap().get_pixel(35, 10)[3],
            0
        );
        assert_eq!(
            crate::vector::render_svg(&updated)
                .unwrap()
                .get_pixel(35, 10)[0],
            255
        );
        let mut editor = crate::document::Editor::new(
            crate::document::Document::new(64, 32, true).unwrap(),
            true,
        );
        let old = crate::vector::Content::Svg {
            source: SOURCE.into(),
        };
        editor
            .document
            .add_layer("SVG".into(), old.render().unwrap())
            .unwrap();
        editor.document.active_layer_mut().unwrap().content = Some(old.clone());
        let content = crate::vector::Content::Svg { source: updated };
        let pixels = content.render().unwrap();
        editor.edit("SVG 요소 편집", |doc| {
            let l = doc.active_layer_mut().unwrap();
            l.content = Some(content.clone());
            l.pixels = std::sync::Arc::new(pixels);
        });
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/text-vector");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("SVG-요소.rphoto");
        crate::files::save_project(&editor.document, &path).unwrap();
        let restored = crate::files::load_project(&path).unwrap();
        assert_eq!(
            restored.active_layer().unwrap().content,
            Some(content.clone())
        );
        editor.undo();
        assert_eq!(editor.document.active_layer().unwrap().content, Some(old));
        editor.redo();
        assert_eq!(
            editor.document.active_layer().unwrap().content,
            Some(content)
        );
        assert_eq!(editor.history().count(), 1);
    }
    #[test]
    fn element_xml_replacement_validation_and_escaping_are_atomic() {
        let id = elements(SOURCE)
            .unwrap()
            .iter()
            .find(|e| e.label.trim() == "use")
            .unwrap()
            .index;
        let updated =
            attribute(SOURCE, id, "data-note", "한글 <tag> & 'quote' \"double\"").unwrap();
        let doc = roxmltree::Document::parse(&updated).unwrap();
        assert_eq!(
            doc.descendants()
                .find(|n| n.has_tag_name("use"))
                .unwrap()
                .attribute("data-note"),
            Some("한글 <tag> & 'quote' \"double\"")
        );
        assert!(attribute(SOURCE, id, "bad name", "x").is_err());
        assert!(fragment(SOURCE, id, "<path>").is_err());
        let replaced = fragment(SOURCE, id, "<rect width='12' height='12' fill='blue'/>").unwrap();
        assert!(replaced.contains("id='shape'"));
        assert_eq!(
            crate::vector::render_svg(&replaced)
                .unwrap()
                .get_pixel(6, 6)
                .0,
            [0, 0, 255, 255]
        );
        assert_eq!(SOURCE.matches("<use").count(), 1);
    }
    #[test]
    fn linear_preview_handles_css_inherited_stops_radial_and_cycles() {
        let source = r#"<svg xmlns='http://www.w3.org/2000/svg' width='256' height='16'><style>.linear {color-interpolation:linearRGB} .dark {stop-color:black} .light {stop-color:white;stop-opacity:50%}</style><defs><linearGradient id='base'><stop class='dark' offset='0'/><stop class='light' offset='1'/></linearGradient><linearGradient id='g' class='linear' href='#base'/></defs><rect width='256' height='16' fill='url(#g)'/></svg>"#;
        let prepared = linear_preview(source).unwrap();
        let xml = roxmltree::Document::parse(&prepared).unwrap();
        assert!(xml.descendants().filter(|n| n.has_tag_name("stop")).count() < 100);
        let pixel = crate::vector::render_svg(source)
            .unwrap()
            .get_pixel(127, 8)
            .0;
        assert!(pixel[0].abs_diff(187) <= 3, "{pixel:?}");
        assert!(pixel[3].abs_diff(191) <= 2, "{pixel:?}");
        let radial = source.replace("linearGradient", "radialGradient");
        assert!(
            crate::vector::render_svg(&radial)
                .unwrap()
                .pixels()
                .any(|p| p[0] > 180)
        );
        let cycle = "<svg width='20' height='20'><linearGradient id='a' color-interpolation='linearRGB' href='#b'/><linearGradient id='b' href='#a'/></svg>";
        assert!(linear_preview(cycle).is_err());
        assert_eq!(source.matches("<stop").count(), 2);
    }
}

#[cfg(test)]
mod image_summary_tests {
    #[test]
    fn svg_image_summary_does_not_clone_large_encoded_assets_or_fragments() {
        let payload = "A".repeat(2 * 1024 * 1024);
        let source = format!(
            "<svg><image id='target' href='data:image/png;base64,{payload}' data-rphoto-image-original='data:application/octet-stream;base64,{payload}' width='12' height='8'/></svg>"
        );
        let list = super::elements_impl(&source, true).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].fragment.is_empty());
        assert!(
            list[0]
                .attributes
                .iter()
                .map(|(n, v)| n.len() + v.len())
                .sum::<usize>()
                < 100
        );
        assert!(source.contains(&payload));
    }
}
