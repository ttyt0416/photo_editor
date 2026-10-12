use crate::{
    document::{Editor, Layer},
    matrix::{self as m, Matrix},
    paths::Action,
    vector::Content,
};
use eframe::egui::{self, Color32, Pos2, Stroke};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
pub struct Element {
    pub index: usize,
    pub label: String,
    pub group: bool,
    pub bounds: [f32; 4],
    render_bounds: [f32; 4],
    mapping: Matrix,
    parent: Matrix,
    transform: Matrix,
}
fn matrix(t: resvg::usvg::Transform) -> Matrix {
    [
        t.sx as f64,
        t.kx as f64,
        t.tx as f64,
        t.ky as f64,
        t.sy as f64,
        t.ty as f64,
        0.,
        0.,
        1.,
    ]
}
fn transform(value: &str) -> Result<Matrix, String> {
    let t: svgtypes::Transform = value.parse().map_err(|e: svgtypes::Error| e.to_string())?;
    Ok([t.a, t.c, t.e, t.b, t.d, t.f, 0., 0., 1.])
}
fn encoded(t: Matrix) -> String {
    format!(
        "matrix({} {} {} {} {} {})",
        t[0], t[3], t[1], t[4], t[2], t[5]
    )
}
pub fn elements(source: &str) -> Result<Vec<Element>, String> {
    let xml = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let mut counts = HashMap::new();
    for id in xml.descendants().filter_map(|n| n.attribute("id")) {
        *counts.entry(id).or_insert(0usize) += 1;
    }
    let mut ids: HashSet<String> = counts.keys().map(|s| s.to_string()).collect();
    let mut candidates = Vec::new();
    let mut replacements = Vec::new();
    for (index, node) in xml
        .descendants()
        .filter(|n| n.is_element())
        .enumerate()
        .skip(1)
    {
        let tag = node.tag_name().name();
        if !matches!(
            tag,
            "rect"
                | "circle"
                | "ellipse"
                | "line"
                | "polyline"
                | "polygon"
                | "path"
                | "use"
                | "g"
                | "image"
        ) || node.attribute("data-photo-editor-viewport-content") == Some("1")
            || node.ancestors().skip(1).any(|n| {
                n.is_element()
                    && matches!(
                        n.tag_name().name(),
                        "defs" | "clipPath" | "mask" | "symbol" | "pattern" | "marker"
                    )
            })
        {
            continue;
        }
        if candidates.len() >= 4096 {
            return Err("SVG 요소가 편집 한도를 초과합니다.".into());
        }
        let id = if let Some(id) = node.attribute("id") {
            if counts.get(id) != Some(&1) {
                return Err("중복 SVG ID는 원본 속성에서 먼저 수정하세요.".into());
            }
            id.to_owned()
        } else {
            let mut id = format!("photo-editor-object-{index}");
            while !ids.insert(id.clone()) {
                id.push('_');
            }
            let start = node.range().start
                + 1
                + source[node.range().start + 1..]
                    .find(|c: char| c.is_whitespace() || matches!(c, '/' | '>'))
                    .ok_or("SVG 태그가 잘못되었습니다.")?;
            replacements.push((start, format!(" id=\"{id}\"")));
            id
        };
        let own = transform(node.attribute("transform").unwrap_or("matrix(1 0 0 1 0 0)"))?;
        candidates.push((
            index,
            id,
            format!("{tag} {}", node.attribute("id").unwrap_or("")),
            tag == "g",
            own,
        ));
    }
    let mut prepared = source.to_owned();
    for (start, value) in replacements.into_iter().rev() {
        prepared.insert_str(start, &value);
    }
    let prepared = crate::svg_namespace::preview_roots(&prepared)?;
    let tree = resvg::usvg::Tree::from_str(&prepared, &crate::svg_assets::options())
        .map_err(|e| e.to_string())?;
    let mut result = Vec::new();
    for (index, id, label, group, own) in candidates {
        if let Some(node) = tree.node_by_id(&id) {
            let mapping = matrix(node.abs_transform());
            let local = match node {
                resvg::usvg::Node::Group(g) => matrix(g.transform()),
                _ => own,
            };
            let Some(inverse) = m::inverse(local) else {
                continue;
            };
            let parent = m::multiply(mapping, inverse);
            if m::inverse(parent).is_none() {
                continue;
            }
            let b = node.abs_stroke_bounding_box();
            let bounds = [b.left(), b.top(), b.right(), b.bottom()];
            if bounds.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
                return Err("SVG 요소 좌표가 유효하지 않습니다.".into());
            }
            let layer_bounds = node.abs_layer_bounding_box();
            let render_bounds = [
                bounds[0].min(layer_bounds.map_or(bounds[0], |b| b.left())),
                bounds[1].min(layer_bounds.map_or(bounds[1], |b| b.top())),
                bounds[2].max(layer_bounds.map_or(bounds[2], |b| b.right())),
                bounds[3].max(layer_bounds.map_or(bounds[3], |b| b.bottom())),
            ];
            result.push(Element {
                render_bounds,
                index,
                label,
                group,
                bounds,
                mapping,
                parent,
                transform: own,
            });
        }
    }
    Ok(result)
}
fn points(b: [f32; 4]) -> [[f32; 2]; 9] {
    let [x, y, r, d] = b;
    let cx = (x + r) * 0.5;
    let cy = (y + d) * 0.5;
    [
        [x, y],
        [cx, y],
        [r, y],
        [r, cy],
        [r, d],
        [cx, d],
        [x, d],
        [x, cy],
        [cx, y - 20.],
    ]
}
fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
#[derive(Clone)]
struct Drag {
    element: Element,
    layer: Layer,
    source: String,
    start: [f32; 2],
    handle: Option<usize>,
}
#[derive(Default)]
pub struct Objects {
    layer: Option<u64>,
    pub selected: Option<usize>,
    pub proportions: bool,
    source: String,
    cache: Vec<Element>,
    drag: Option<Drag>,
}
impl Objects {
    fn sync(&mut self, layer: &Layer, source: &str) -> Result<(), String> {
        if self.layer != Some(layer.id) {
            *self = Self {
                layer: Some(layer.id),
                proportions: self.proportions,
                ..Self::default()
            };
        }
        if self.source != source {
            self.cache = elements(source)?;
            self.source = source.into();
        }
        if self
            .selected
            .is_some_and(|i| !self.cache.iter().any(|e| e.index == i))
        {
            self.selected = None;
        }
        Ok(())
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, layer: &Layer) {
        let Some(Content::Svg { source }) = &layer.content else {
            return;
        };
        if let Err(e) = self.sync(layer, source) {
            ui.colored_label(Color32::YELLOW, e);
            return;
        }
        egui::ComboBox::from_id_salt("svg-native-object")
            .selected_text(
                self.cache
                    .iter()
                    .find(|e| Some(e.index) == self.selected)
                    .map_or("SVG 요소 선택", |e| e.label.as_str()),
            )
            .show_ui(ui, |ui| {
                for e in &self.cache {
                    if ui
                        .selectable_value(&mut self.selected, Some(e.index), &e.label)
                        .changed()
                    {
                        self.drag = None;
                    }
                }
            });
        ui.checkbox(&mut self.proportions, "비율 유지");
        if self.cache.is_empty() {
            ui.label("표시되는 요소가 없습니다.");
        }
    }
    pub fn apply(
        &mut self,
        editor: &mut Editor,
        action: Action,
        tolerance: f32,
    ) -> Result<bool, String> {
        if matches!(action, Action::Cancel) {
            self.drag = None;
            editor.cancel();
            self.source.clear();
            return Ok(true);
        }
        if matches!(action, Action::Release | Action::Finish) {
            self.drag = None;
            editor.finish();
            return Ok(false);
        }
        let Some(layer) = editor
            .document
            .active_layer()
            .filter(|l| !editor.document.is_locked(l.id))
        else {
            return Ok(false);
        };
        let Some(Content::Svg { source }) = &layer.content else {
            return Ok(false);
        };
        self.sync(layer, source)?;
        let size = [layer.pixels.width(), layer.pixels.height()];
        match action {
            Action::Press(point, _, _) => {
                let local = layer.transform.to_local(point, layer.offset, size);
                let selected = self.cache.iter().find(|e| Some(e.index) == self.selected);
                let hit = selected.and_then(|e| {
                    points(e.bounds)
                        .into_iter()
                        .enumerate()
                        .map(|(i, p)| {
                            (
                                i,
                                distance(layer.transform.to_document(p, layer.offset, size), point),
                            )
                        })
                        .filter(|(_, d)| *d <= tolerance)
                        .min_by(|a, b| a.1.total_cmp(&b.1))
                        .map(|(i, _)| (e, i))
                });
                let contains = |e: &&Element| {
                    local[0] >= e.bounds[0]
                        && local[0] <= e.bounds[2]
                        && local[1] >= e.bounds[1]
                        && local[1] <= e.bounds[3]
                };
                let chosen = hit
                    .map(|(e, i)| (e, Some(i)))
                    .or_else(|| selected.filter(|e| contains(e)).map(|e| (e, None)))
                    .or_else(|| {
                        self.cache
                            .iter()
                            .rev()
                            .filter(|e| !e.group)
                            .find(contains)
                            .map(|e| (e, None))
                    });
                if let Some((e, handle)) = chosen {
                    self.selected = Some(e.index);
                    self.drag = Some(Drag {
                        element: e.clone(),
                        layer: layer.clone(),
                        source: source.clone(),
                        start: local,
                        handle,
                    });
                } else {
                    self.selected = None;
                    self.drag = None;
                }
            }
            Action::Move(point, alt) => {
                let Some(drag) = &self.drag else {
                    return Ok(false);
                };
                if drag.layer.id != layer.id {
                    self.drag = None;
                    return Ok(false);
                }
                let p = drag.layer.transform.to_local(
                    point,
                    drag.layer.offset,
                    [drag.layer.pixels.width(), drag.layer.pixels.height()],
                );
                if p.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
                    return Err("SVG 요소 좌표가 유효하지 않습니다.".into());
                }
                if distance(p, drag.start) < 1e-4 && layer.content == drag.layer.content {
                    return Ok(false);
                }
                let operation = operation(drag, p, alt, self.proportions)?;
                let e = &drag.element;
                let value = m::multiply(
                    m::multiply(
                        m::multiply(
                            m::inverse(e.parent).ok_or("SVG 부모 역변환이 없습니다.")?,
                            operation,
                        ),
                        e.parent,
                    ),
                    e.transform,
                );
                let source = crate::svg_edit::attribute_text(
                    &drag.source,
                    e.index,
                    "transform",
                    &encoded(value),
                )?;
                let changed = elements(&source)?
                    .into_iter()
                    .find(|n| n.index == e.index)
                    .ok_or("SVG 요소가 보이지 않게 되었습니다.")?;
                // CSS or unsupported use transforms must not silently defeat the requested operation.
                let expected = m::multiply(operation, e.mapping);
                for p in [[0., 0.], [1., 0.], [0., 1.]] {
                    if distance(m::point(changed.mapping, p), m::point(expected, p)) > 0.01 {
                        return Err(
                            "이 요소의 CSS/참조 변형은 SVG 원본 속성에서 편집하세요.".into()
                        );
                    }
                }
                let (changed, index) = crate::svg_paths::publish(
                    editor,
                    &drag.layer,
                    source,
                    changed.render_bounds,
                    e.index,
                    false,
                )?;
                self.selected = Some(index);
                return Ok(changed);
            }
            _ => {}
        }
        Ok(false)
    }
    pub fn paint(&self, painter: &egui::Painter, layer: &Layer, screen: impl Fn([f32; 2]) -> Pos2) {
        let Some(Content::Svg { source }) = &layer.content else {
            return;
        };
        let fallback;
        let list = if self.source == *source && self.layer == Some(layer.id) {
            &self.cache
        } else {
            fallback = elements(source).unwrap_or_default();
            &fallback
        };
        let Some(e) = list.iter().find(|e| Some(e.index) == self.selected) else {
            return;
        };
        let screen = |p| {
            screen(layer.transform.to_document(
                p,
                layer.offset,
                [layer.pixels.width(), layer.pixels.height()],
            ))
        };
        let p = points(e.bounds);
        let color = Color32::from_rgb(90, 153, 245);
        painter.add(egui::Shape::closed_line(
            [0, 2, 4, 6].map(|i| screen(p[i])).to_vec(),
            Stroke::new(1.5, color),
        ));
        painter.line_segment([screen(p[1]), screen(p[8])], Stroke::new(1., color));
        for (i, p) in p.into_iter().enumerate() {
            let p = screen(p);
            if i == 8 {
                painter.circle_filled(p, 4., color);
            } else {
                let rect = egui::Rect::from_center_size(p, egui::Vec2::splat(7.));
                painter.rect_filled(rect, 0., Color32::WHITE);
                painter.rect_stroke(rect, 0., Stroke::new(1., color), egui::StrokeKind::Inside);
            }
        }
    }
}
fn operation(drag: &Drag, p: [f32; 2], alt: bool, proportions: bool) -> Result<Matrix, String> {
    let b = drag.element.bounds;
    let center = [(b[0] + b[2]) * 0.5, (b[1] + b[3]) * 0.5];
    let Some(handle) = drag.handle else {
        return Ok(m::translate(
            (p[0] - drag.start[0]) as f64,
            (p[1] - drag.start[1]) as f64,
        ));
    };
    if handle == 8 {
        let a = (drag.start[1] - center[1]).atan2(drag.start[0] - center[0]);
        let a = (p[1] - center[1]).atan2(p[0] - center[0]) - a;
        let (sin, cos) = (a as f64).sin_cos();
        return Ok(m::multiply(
            m::multiply(
                m::translate(center[0] as f64, center[1] as f64),
                [cos, -sin, 0., sin, cos, 0., 0., 0., 1.],
            ),
            m::translate(-center[0] as f64, -center[1] as f64),
        ));
    }
    let h = points(b)[handle];
    let fixed = if alt {
        center
    } else {
        points(b)[(handle + 4) % 8]
    };
    let mut scale = [1.; 2];
    for i in 0..2 {
        if (h[i] - fixed[i]).abs() > 1e-5 {
            scale[i] = (h[i] + p[i] - drag.start[i] - fixed[i]) / (h[i] - fixed[i]);
        }
    }
    if proportions {
        let s = if (scale[0] - 1.).abs() > (scale[1] - 1.).abs() {
            scale[0]
        } else {
            scale[1]
        };
        scale = [s; 2];
    }
    if scale
        .iter()
        .any(|v| !v.is_finite() || *v < 0.001 || *v > 1e6)
    {
        return Err("SVG 요소 크기는 0보다 크게 유지하세요.".into());
    }
    Ok(m::multiply(
        m::multiply(
            m::translate(fixed[0] as f64, fixed[1] as f64),
            [scale[0] as f64, 0., 0., 0., scale[1] as f64, 0., 0., 0., 1.],
        ),
        m::translate(-fixed[0] as f64, -fixed[1] as f64),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{document::Document, precision};
    use std::sync::Arc;
    const SOURCE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="240" height="200" viewBox="0 0 240 200"><!--keep--><style>.paint{fill:#2468ac;stroke:#fff;stroke-width:2}</style><defs><path id="template" d="M0 0L12 0L12 12L0 12Z"/><symbol id="symbol" viewBox="0 0 12 12"><circle cx="6" cy="6" r="5"/></symbol></defs><g id="group" transform="translate(8 12) rotate(8)"><rect id="rect" class="paint" x="20" y="20" width="30" height="24" rx="3"/><circle id="circle" cx="80" cy="30" r="10"/><ellipse id="ellipse" cx="120" cy="30" rx="12" ry="8"/><line id="line" x1="10" y1="70" x2="40" y2="70" stroke="red" stroke-width="4"/><polyline id="polyline" points="60,60 80,70 60,80" fill="none" stroke="red"/><polygon id="polygon" points="100,60 120,70 100,80"/><use id="use" href="#template" x="20" y="100" transform="rotate(4)"/><use id="use2" href="#template" x="60" y="100"/><use id="sym" href="#symbol" x="100" y="100" width="20" height="24"/></g></svg>"##;
    fn editor(source: &str) -> Editor {
        let c = Content::Svg {
            source: source.into(),
        };
        let mut doc = Document::new(320, 280, true).unwrap();
        doc.add_layer("SVG".into(), c.render().unwrap()).unwrap();
        let l = doc.active_layer_mut().unwrap();
        l.content = Some(c);
        l.offset = [30, 25];
        l.transform.angle = 11.;
        l.transform.scale = [1.2, 0.85];
        l.transform.warp = Some([1., 0.02, 0., 0., 1., 0., 0.0001, 0., 1.]);
        Editor::new(doc, false)
    }
    fn source(e: &Editor) -> &str {
        let Some(Content::Svg { source }) = &e.document.active_layer().unwrap().content else {
            panic!()
        };
        source
    }
    fn world(l: &Layer, p: [f32; 2]) -> [f32; 2] {
        l.transform
            .to_document(p, l.offset, [l.pixels.width(), l.pixels.height()])
    }
    fn center(e: &Element) -> [f32; 2] {
        [
            (e.bounds[0] + e.bounds[2]) * 0.5,
            (e.bounds[1] + e.bounds[3]) * 0.5,
        ]
    }
    #[test]
    fn referenced_and_nested_svg_absolute_coordinates_apply_each_transform_once() {
        for (source, id, expected) in [
            (
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><rect id="r" width="8" height="6"/></defs><g transform="translate(5 7)"><use id="u" href="#r" x="10" y="12" transform="translate(3 4)"/></g></svg>"##,
                "u",
                [18., 23., 26., 29.],
            ),
            (
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><symbol id="s" viewBox="0 0 8 6" overflow="visible"><rect width="8" height="6"/></symbol></defs><g transform="translate(5 7)"><use id="u" href="#s" x="10" y="12" width="16" height="12" transform="translate(3 4)"/></g></svg>"##,
                "u",
                [18., 23., 34., 35.],
            ),
            (
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><defs><symbol id="s" viewBox="0 0 8 6"><rect width="8" height="6"/></symbol></defs><g transform="translate(5 7)"><use id="u" href="#s" x="10" y="12" width="16" height="12" transform="translate(3 4)"/></g></svg>"##,
                "u",
                [18., 23., 34., 35.],
            ),
            (
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><svg x="10" y="12" width="16" height="12" viewBox="0 0 8 6" transform="translate(3 4)" overflow="visible"><rect id="r" width="8" height="6"/></svg></svg>"##,
                "r",
                [13., 16., 29., 28.],
            ),
        ] {
            let prepared = crate::svg_namespace::preview_roots(source).unwrap();
            let tree =
                resvg::usvg::Tree::from_str(&prepared, &crate::svg_assets::options()).unwrap();
            let node = tree.node_by_id(id).unwrap();
            let b = node.abs_bounding_box();
            for (a, b) in [b.left(), b.top(), b.right(), b.bottom()]
                .into_iter()
                .zip(expected)
            {
                assert!((a - b).abs() < 0.001, "{id}: {a} != {b}");
            }
            let pixels = crate::vector::render_svg(source).unwrap();
            assert_eq!(
                pixels.get_pixel(expected[0] as u32 + 2, expected[1] as u32 + 2)[3],
                255
            );
            assert_eq!(
                pixels.get_pixel(expected[0] as u32 - 1, expected[1] as u32 + 2)[3],
                0
            );
        }
    }
    #[test]
    fn native_shapes_transformed_groups_use_and_symbol_instances_move_without_rewriting_geometry() {
        for name in [
            "rect", "circle", "ellipse", "line", "polyline", "polygon", "use", "use2", "sym",
            "group",
        ] {
            let mut editor = editor(SOURCE);
            let base = editor.document.active_layer().unwrap().clone();
            let e = elements(SOURCE)
                .unwrap()
                .into_iter()
                .find(|e| e.label.ends_with(&format!(" {name}")))
                .unwrap();
            let mut tool = Objects {
                selected: Some(e.index),
                layer: Some(base.id),
                ..Default::default()
            };
            let start = center(&e);
            let target = [start[0] + 3., start[1] + 4.];
            tool.apply(
                &mut editor,
                Action::Press(world(&base, start), false, false),
                0.001,
            )
            .unwrap();
            assert!(
                !tool
                    .apply(&mut editor, Action::Move(world(&base, start), false), 0.001)
                    .unwrap()
            );
            assert!(!editor.can_undo());
            assert!(
                tool.apply(
                    &mut editor,
                    Action::Move(world(&base, target), false),
                    0.001
                )
                .unwrap(),
                "{name}"
            );
            tool.apply(&mut editor, Action::Release, 0.001).unwrap();
            let changed = elements(source(&editor))
                .unwrap()
                .into_iter()
                .find(|n| n.label.ends_with(&format!(" {name}")))
                .unwrap();
            assert!(
                distance(center(&changed), target) < 0.03,
                "{name}: {:?} {:?}",
                center(&changed),
                target
            );
            let xml = roxmltree::Document::parse(source(&editor)).unwrap();
            let before = roxmltree::Document::parse(SOURCE).unwrap();
            let after = xml
                .descendants()
                .find(|n| n.attribute("id") == Some(name))
                .unwrap();
            let orig = before
                .descendants()
                .find(|n| n.attribute("id") == Some(name))
                .unwrap();
            assert_eq!(after.tag_name(), orig.tag_name());
            for attr in orig.attributes() {
                if attr.name() != "transform" {
                    assert_eq!(after.attribute(attr.name()), Some(attr.value()));
                }
            }
            assert_eq!(source(&editor).matches("<use ").count(), 3);
            assert!(source(&editor).contains("<!--keep-->"));
            assert!(source(&editor).contains(".paint{fill:#2468ac;stroke:#fff;stroke-width:2}"));
            let template = xml
                .descendants()
                .find(|n| n.attribute("id") == Some("template"))
                .unwrap();
            assert_eq!(template.attribute("d"), Some("M0 0L12 0L12 12L0 12Z"));
            assert_eq!(editor.history().count(), 1);
            let out = editor.document.composite();
            editor.undo();
            assert_eq!(source(&editor), SOURCE);
            editor.redo();
            assert_eq!(editor.document.composite(), out);
        }
    }
    #[test]
    fn eight_size_handles_center_ratio_rotation_and_cancel_preserve_opposite_geometry() {
        let src = r#"<svg xmlns="http://www.w3.org/2000/svg" width="120" height="100"><rect id="box" x="30" y="30" width="40" height="20"/></svg>"#;
        for handle in 0..8 {
            for alt in [false, true] {
                let mut editor = editor(src);
                let base = editor.document.active_layer().unwrap().clone();
                let e = elements(src).unwrap().remove(0);
                let p = points(e.bounds)[handle];
                let c = center(&e);
                let delta: [f32; 2] = std::array::from_fn(|i| {
                    if (p[i] - c[i]).abs() > 0.1 {
                        (p[i] - c[i]).signum() * 4.
                    } else {
                        0.
                    }
                });
                let target = [p[0] + delta[0], p[1] + delta[1]];
                let fixed = if alt {
                    c
                } else {
                    points(e.bounds)[(handle + 4) % 8]
                };
                let mut tool = Objects {
                    selected: Some(e.index),
                    layer: Some(base.id),
                    ..Default::default()
                };
                tool.apply(
                    &mut editor,
                    Action::Press(world(&base, p), false, false),
                    0.001,
                )
                .unwrap();
                tool.apply(&mut editor, Action::Move(world(&base, target), alt), 0.001)
                    .unwrap();
                let changed = elements(source(&editor)).unwrap().remove(0);
                let map = m::multiply(changed.mapping, m::inverse(e.mapping).unwrap());
                assert!(distance(m::point(map, fixed), fixed) < 0.001);
                tool.apply(&mut editor, Action::Cancel, 0.001).unwrap();
                assert_eq!(source(&editor), src);
                assert!(!editor.can_undo());
            }
        }
        let mut editor = editor(src);
        let base = editor.document.active_layer().unwrap().clone();
        let e = elements(src).unwrap().remove(0);
        let p = points(e.bounds)[4];
        let mut tool = Objects {
            selected: Some(e.index),
            layer: Some(base.id),
            proportions: true,
            ..Default::default()
        };
        tool.apply(
            &mut editor,
            Action::Press(world(&base, p), false, false),
            0.001,
        )
        .unwrap();
        tool.apply(
            &mut editor,
            Action::Move(world(&base, [p[0] + 20., p[1] + 3.]), false),
            0.001,
        )
        .unwrap();
        tool.apply(&mut editor, Action::Release, 0.001).unwrap();
        let resized = elements(source(&editor)).unwrap().remove(0);
        assert!(
            ((resized.bounds[2] - resized.bounds[0]) / (resized.bounds[3] - resized.bounds[1])
                - 2.)
                .abs()
                < 0.001
        );
        let l = editor.document.active_layer().unwrap().clone();
        let c = center(&resized);
        let p = points(resized.bounds)[8];
        let target = [c[0] + c[1] - p[1], c[1]];
        tool.apply(
            &mut editor,
            Action::Press(world(&l, p), false, false),
            0.001,
        )
        .unwrap();
        tool.apply(&mut editor, Action::Move(world(&l, target), false), 0.001)
            .unwrap();
        tool.apply(&mut editor, Action::Release, 0.001).unwrap();
        let rotated = elements(source(&editor)).unwrap().remove(0);
        assert!(
            ((rotated.bounds[2] - rotated.bounds[0]) / (rotated.bounds[3] - rotated.bounds[1])
                - 0.5)
                .abs()
                < 0.001
        );
        assert!(distance(center(&rotated), c) < 0.001);
        assert_eq!(editor.history().count(), 2);
    }
    #[test]
    fn viewport_growth_keeps_mask_scope_precision_native_reference_outputs_and_one_history() {
        let mut editor = editor(SOURCE);
        let layer = editor.document.active_layer_mut().unwrap();
        layer.mask = Some(Arc::new(image::GrayImage::from_pixel(
            240,
            200,
            image::Luma([220]),
        )));
        crate::masks::set_world(
            layer,
            crate::masks::world(layer),
            false,
            crate::sampling::Sampling::Bilinear,
        )
        .unwrap();
        let mask = crate::masks::world(layer);
        layer.bit_depth = 32;
        layer.precision = Some(Arc::new(
            image::DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f(),
        ));
        layer.adjustments.scope = Some(
            crate::adjustment_scope::Scope::capture(
                layer,
                &crate::document::Selection::rectangle([0, 0], [320, 280]),
                [320, 280],
            )
            .unwrap(),
        );
        let base = layer.clone();
        let e = elements(SOURCE)
            .unwrap()
            .into_iter()
            .find(|e| e.label == "use use")
            .unwrap();
        let c = center(&e);
        let mut tool = Objects {
            selected: Some(e.index),
            layer: Some(base.id),
            ..Default::default()
        };
        tool.apply(
            &mut editor,
            Action::Press(world(&base, c), false, false),
            0.001,
        )
        .unwrap();
        for target in [[-12., -10.], [-32., -18.], [-20., -15.]] {
            tool.apply(
                &mut editor,
                Action::Move(world(&base, target), false),
                0.001,
            )
            .unwrap();
            let changed = elements(source(&editor))
                .unwrap()
                .into_iter()
                .find(|e| e.label == "use use")
                .unwrap();
            let layer = editor.document.active_layer().unwrap();
            assert!(distance(world(layer, center(&changed)), world(&base, target)) < 0.005);
            assert!(crate::masks::valid(layer));
            assert_eq!(
                layer.precision.as_ref().unwrap().dimensions(),
                layer.pixels.dimensions()
            );
            assert_eq!(layer.bit_depth, 32);
            for p in [[0., 0.], [240., 200.]] {
                assert!(
                    distance(m::point(crate::masks::world(layer), p), m::point(mask, p)) < 0.001
                );
            }
            assert_eq!(
                source(&editor)
                    .matches("data-photo-editor-expanded-viewport=\"1\"")
                    .count(),
                1
            );
        }
        tool.apply(&mut editor, Action::Release, 0.001).unwrap();
        assert_eq!(editor.history().count(), 1);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/svg-objects");
        std::fs::create_dir_all(&dir).unwrap();
        crate::files::save_project(&editor.document, &dir.join("native.rphoto")).unwrap();
        let restored = crate::files::open(&dir.join("native.rphoto"))
            .unwrap()
            .document;
        assert_eq!(
            restored.active_layer().unwrap().content,
            editor.document.active_layer().unwrap().content
        );
        assert_eq!(
            precision::composite(&restored),
            precision::composite(&editor.document)
        );
        crate::svg_export::save(&editor.document, &dir.join("precision-flattened.svg")).unwrap();
        let layer = editor.document.active_layer().unwrap();
        let mut native = Document::new(320, 280, true).unwrap();
        native
            .add_layer("native SVG".into(), (*layer.pixels).clone())
            .unwrap();
        native.active_layer_mut().unwrap().content = layer.content.clone();
        crate::files::save_project(&native, &dir.join("native8.rphoto")).unwrap();
        crate::svg_export::save(&native, &dir.join("native.svg")).unwrap();
        let output = std::fs::read_to_string(dir.join("native.svg")).unwrap();
        assert_eq!(output.matches("<use ").count(), 3);
        assert!(
            output.contains("<rect ") && output.contains("<circle ") && output.contains("<symbol ")
        );
        assert_eq!(
            crate::files::open(&dir.join("native.svg"))
                .unwrap()
                .document
                .composite(),
            native.composite()
        );
        for ext in ["psd", "psb"] {
            crate::files::save_photoshop(&editor.document, &dir.join(format!("native.{ext}")))
                .unwrap();
            let restored = crate::files::open(&dir.join(format!("native.{ext}")))
                .unwrap()
                .document;
            let a = precision::composite(&restored);
            let b = precision::composite(&editor.document);
            for (a, b) in a.pixels().zip(b.pixels()) {
                for c in 0..3 {
                    assert!((a[c] * a[3] - b[c] * b[3]).abs() < 0.009);
                }
                assert!((a[3] - b[3]).abs() < 0.009);
            }
        }
    }
    #[test]
    fn locked_invalid_resize_nonfinite_budget_and_switched_layer_keep_original_document() {
        let mut editor = editor(SOURCE);
        let base = editor.document.active_layer().unwrap().clone();
        let e = elements(SOURCE)
            .unwrap()
            .into_iter()
            .find(|e| e.label == "rect rect")
            .unwrap();
        let p = points(e.bounds)[4];
        let mut tool = Objects {
            selected: Some(e.index),
            layer: Some(base.id),
            ..Default::default()
        };
        editor.document.active_layer_mut().unwrap().locked = true;
        assert!(
            !tool
                .apply(
                    &mut editor,
                    Action::Press(world(&base, p), false, false),
                    0.001
                )
                .unwrap()
        );
        editor.document.active_layer_mut().unwrap().locked = false;
        tool.apply(
            &mut editor,
            Action::Press(world(&base, p), false, false),
            0.001,
        )
        .unwrap();
        for target in [
            [e.bounds[0] - 5., e.bounds[1] - 5.],
            [100000., 100000.],
            [f32::NAN, 0.],
        ] {
            assert!(
                tool.apply(
                    &mut editor,
                    Action::Move(world(&base, target), false),
                    0.001
                )
                .is_err()
            );
            assert_eq!(source(&editor), SOURCE);
            assert!(Arc::ptr_eq(
                &base.pixels,
                &editor.document.active_layer().unwrap().pixels
            ));
            assert!(!editor.can_undo());
        }
        editor
            .document
            .add_layer("another".into(), image::RgbaImage::new(8, 8))
            .unwrap();
        assert!(
            !tool
                .apply(&mut editor, Action::Move([100., 100.], false), 0.001)
                .unwrap()
        );
        assert!(
            editor
                .document
                .layers
                .iter()
                .find(|l| l.id == base.id)
                .unwrap()
                .content
                == base.content
        );
    }
}
