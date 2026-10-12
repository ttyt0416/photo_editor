use crate::{
    document::{Editor, Layer},
    matrix as m,
    paths::{Action, Handle},
    vector::{Content, PathNode},
};
use eframe::egui::{self, Color32, Pos2, Stroke};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Clone)]
pub struct Subpath {
    pub nodes: Vec<PathNode>,
    pub closed: bool,
}
#[derive(Clone)]
pub struct Element {
    pub index: usize,
    pub label: String,
    pub mapping: m::Matrix,
    pub bounds: [f32; 4],
    pub paths: Vec<Subpath>,
}

fn parse(data: &str) -> Result<Vec<Subpath>, String> {
    use svgtypes::SimplePathSegment as S;
    let mut paths = Vec::<Subpath>::new();
    for segment in svgtypes::SimplifyingPathParser::from(data) {
        match segment.map_err(|e| e.to_string())? {
            S::MoveTo { x, y } => paths.push(Subpath {
                nodes: vec![PathNode::corner([x as f32, y as f32])],
                closed: false,
            }),
            S::ClosePath => {
                let path = paths.last_mut().ok_or("SVG 경로의 시작점이 없습니다.")?;
                path.closed = true;
                if path.nodes.len() > 1 && path.nodes.last().unwrap().anchor == path.nodes[0].anchor
                {
                    let last = path.nodes.pop().unwrap();
                    path.nodes[0].incoming = last.incoming;
                }
            }
            segment => {
                let path = paths.last_mut().ok_or("SVG 경로의 시작점이 없습니다.")?;
                let prev = path.nodes.last_mut().unwrap();
                let node = match segment {
                    S::LineTo { x, y } => PathNode::corner([x as f32, y as f32]),
                    S::CurveTo {
                        x1,
                        y1,
                        x2,
                        y2,
                        x,
                        y,
                    } => {
                        prev.outgoing = [x1 as f32, y1 as f32];
                        PathNode {
                            anchor: [x as f32, y as f32],
                            incoming: [x2 as f32, y2 as f32],
                            outgoing: [x as f32, y as f32],
                        }
                    }
                    S::Quadratic { x1, y1, x, y } => {
                        let control = [x1 as f32, y1 as f32];
                        let end = [x as f32, y as f32];
                        prev.outgoing = std::array::from_fn(|i| {
                            prev.anchor[i] + (control[i] - prev.anchor[i]) * 2. / 3.
                        });
                        PathNode {
                            anchor: end,
                            incoming: std::array::from_fn(|i| {
                                end[i] + (control[i] - end[i]) * 2. / 3.
                            }),
                            outgoing: end,
                        }
                    }
                    _ => unreachable!(),
                };
                path.nodes.push(node);
            }
        }
        if paths.iter().map(|p| p.nodes.len()).sum::<usize>() > 16384 {
            return Err("SVG 경로 점이 너무 많습니다.".into());
        }
    }
    if paths
        .iter()
        .flat_map(|p| &p.nodes)
        .flat_map(|n| [n.anchor, n.incoming, n.outgoing])
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 1e9)
    {
        return Err("SVG 경로 좌표가 유효하지 않습니다.".into());
    }
    Ok(paths)
}
fn data(paths: &[Subpath]) -> String {
    let mut result = String::new();
    for path in paths {
        if let Some(first) = path.nodes.first() {
            result += &format!("M {} {} ", first.anchor[0], first.anchor[1]);
            for i in 0..path.nodes.len() {
                if !path.closed && i + 1 == path.nodes.len() {
                    break;
                }
                let a = &path.nodes[i];
                let b = &path.nodes[(i + 1) % path.nodes.len()];
                if a.outgoing == a.anchor && b.incoming == b.anchor {
                    if i + 1 < path.nodes.len() {
                        result += &format!("L {} {} ", b.anchor[0], b.anchor[1]);
                    }
                } else {
                    result += &format!(
                        "C {} {} {} {} {} {} ",
                        a.outgoing[0],
                        a.outgoing[1],
                        b.incoming[0],
                        b.incoming[1],
                        b.anchor[0],
                        b.anchor[1]
                    );
                }
            }
            if path.closed {
                result += "Z ";
            }
        }
    }
    result
}

pub fn elements(source: &str) -> Result<Vec<Element>, String> {
    let xml = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let mut counts = HashMap::<&str, usize>::new();
    for id in xml.descendants().filter_map(|n| n.attribute("id")) {
        *counts.entry(id).or_default() += 1;
    }
    let mut ids: HashSet<String> = counts.keys().map(|id| id.to_string()).collect();
    let mut replacements = vec![];
    let mut candidates = vec![];
    for (index, node) in xml.descendants().filter(|n| n.is_element()).enumerate() {
        if node.tag_name().name() != "path"
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
            let mut id = format!("photo-editor-path-{index}");
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
        let paths = parse(node.attribute("d").unwrap_or(""))?;
        candidates.push((
            index,
            id,
            paths,
            node.attribute("id").unwrap_or("경로").to_owned(),
        ));
    }
    let mut prepared = source.to_owned();
    for (start, value) in replacements.into_iter().rev() {
        prepared.insert_str(start, &value);
    }
    let prepared = crate::svg_namespace::preview_roots(&prepared)?;
    let tree = resvg::usvg::Tree::from_str(&prepared, &crate::svg_assets::options())
        .map_err(|e| e.to_string())?;
    let mut result = vec![];
    for (index, id, paths, label) in candidates {
        if let Some(node) = tree.node_by_id(&id) {
            let t = node.abs_transform();
            let mapping = [
                t.sx as f64,
                t.kx as f64,
                t.tx as f64,
                t.ky as f64,
                t.sy as f64,
                t.ty as f64,
                0.,
                0.,
                1.,
            ];
            if m::inverse(mapping).is_some() && !paths.is_empty() {
                let stroke = node.abs_stroke_bounding_box();
                let layer = node.abs_layer_bounding_box();
                let bounds = [
                    stroke.left().min(layer.map_or(stroke.left(), |b| b.left())),
                    stroke.top().min(layer.map_or(stroke.top(), |b| b.top())),
                    stroke
                        .right()
                        .max(layer.map_or(stroke.right(), |b| b.right())),
                    stroke
                        .bottom()
                        .max(layer.map_or(stroke.bottom(), |b| b.bottom())),
                ];
                result.push(Element {
                    bounds,
                    index,
                    label,
                    mapping,
                    paths,
                });
            }
        }
    }
    Ok(result)
}
#[derive(Clone, Copy)]
struct Drag {
    path: usize,
    node: usize,
    handle: Handle,
}
#[derive(Default)]
pub struct Pen {
    layer: Option<u64>,
    pub element: Option<usize>,
    selected: Option<(usize, usize)>,
    drag: Option<Drag>,
    cache_source: String,
    cache: Vec<Element>,
    pub object_mode: bool,
    pub objects: crate::svg_objects::Objects,
}
impl Pen {
    pub fn reset(&mut self) {
        *self = Self::default();
    }
    fn sync(&mut self, layer: &Layer, source: &str) -> Result<(), String> {
        if self.layer != Some(layer.id) {
            self.reset();
            self.layer = Some(layer.id);
        }
        if self.cache_source != source {
            self.cache = elements(source)?;
            self.cache_source = source.to_owned();
            if self
                .element
                .is_some_and(|i| !self.cache.iter().any(|e| e.index == i))
            {
                self.element = None;
                self.selected = None;
                self.drag = None;
            }
        }
        Ok(())
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, layer: &Layer) {
        let Some(Content::Svg { source }) = &layer.content else {
            return;
        };
        ui.selectable_value(&mut self.object_mode, false, "경로 점");
        ui.selectable_value(&mut self.object_mode, true, "요소 변형");
        if self.object_mode {
            self.objects.ui(ui, layer);
            return;
        }
        if let Err(error) = self.sync(layer, source) {
            ui.colored_label(Color32::YELLOW, error);
            return;
        }
        egui::ComboBox::from_id_salt("svg-canvas-path")
            .selected_text(
                self.cache
                    .iter()
                    .find(|e| Some(e.index) == self.element)
                    .map_or("SVG 경로 선택", |e| e.label.as_str()),
            )
            .show_ui(ui, |ui| {
                for e in &self.cache {
                    if ui
                        .selectable_value(&mut self.element, Some(e.index), &e.label)
                        .changed()
                    {
                        self.selected = None;
                        self.drag = None;
                    }
                }
            });
        if self.cache.is_empty() {
            ui.label("표시되는 경로가 없습니다. SVG 요소/원본 편집에서 확인하세요.");
        }
    }
    pub fn apply(
        &mut self,
        editor: &mut Editor,
        action: Action,
        tolerance: f32,
    ) -> Result<bool, String> {
        if self.object_mode {
            return self.objects.apply(editor, action, tolerance);
        }
        if matches!(action, Action::Cancel) {
            self.reset();
            editor.cancel();
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
        let world = |e: &Element, p| {
            layer
                .transform
                .to_document(m::point(e.mapping, p), layer.offset, size)
        };
        match action {
            Action::Press(p, alt, insert) => {
                // A selected element exposes handles; other elements can be selected by their anchors/curve.
                let selected_element = self.element;
                let hit = self
                    .cache
                    .iter()
                    .flat_map(|e| {
                        e.paths.iter().enumerate().flat_map(move |(pi, path)| {
                            path.nodes.iter().enumerate().flat_map(move |(ni, n)| {
                                [
                                    (Handle::Anchor, n.anchor),
                                    (Handle::Incoming, n.incoming),
                                    (Handle::Outgoing, n.outgoing),
                                ]
                                .into_iter()
                                .filter(move |(h, _)| {
                                    *h == Handle::Anchor || selected_element == Some(e.index)
                                })
                                .map(move |(h, q)| (e.index, pi, ni, h, distance(world(e, q), p)))
                            })
                        })
                    })
                    .filter(|h| h.4 <= tolerance)
                    .min_by(|a, b| {
                        a.4.total_cmp(&b.4)
                            .then_with(|| priority(a.3).cmp(&priority(b.3)))
                    });
                let nearest = self
                    .cache
                    .iter()
                    .flat_map(|e| {
                        e.paths.iter().enumerate().flat_map(move |(pi, path)| {
                            path.nodes
                                .iter()
                                .enumerate()
                                .filter(move |(i, _)| path.closed || *i + 1 < path.nodes.len())
                                .flat_map(move |(ni, a)| {
                                    let b = &path.nodes[(ni + 1) % path.nodes.len()];
                                    (1..48).map(move |n| {
                                        let t = n as f32 / 48.;
                                        (
                                            e.index,
                                            pi,
                                            ni,
                                            t,
                                            distance(world(e, crate::paths::point(a, b, t)), p),
                                        )
                                    })
                                })
                        })
                    })
                    .filter(|h| h.4 <= tolerance)
                    .min_by(|a, b| a.4.total_cmp(&b.4));
                if insert {
                    if let Some((element, pi, ni, t, _)) = nearest {
                        let mut e = self
                            .cache
                            .iter()
                            .find(|e| e.index == element)
                            .unwrap()
                            .clone();
                        let path = &mut e.paths[pi];
                        if crate::paths::split(&mut path.nodes, ni, t, path.closed) {
                            self.element = Some(element);
                            self.selected = Some((pi, ni + 1));
                            return self.commit(editor, &e, true);
                        }
                    }
                } else if let Some((element, pi, ni, handle, _)) = hit {
                    self.element = Some(element);
                    self.selected = Some((pi, ni));
                    if alt && handle == Handle::Anchor {
                        let mut e = self
                            .cache
                            .iter()
                            .find(|e| e.index == element)
                            .unwrap()
                            .clone();
                        let n = &mut e.paths[pi].nodes[ni];
                        *n = PathNode::corner(n.anchor);
                        return self.commit(editor, &e, true);
                    }
                    self.drag = Some(Drag {
                        path: pi,
                        node: ni,
                        handle,
                    });
                } else if let Some((element, _, _, _, _)) = nearest {
                    self.element = Some(element);
                    self.selected = None;
                    self.drag = None;
                } else {
                    self.selected = None;
                    self.drag = None;
                }
            }
            Action::Move(p, alt) => {
                if let Some(drag) = self.drag
                    && let Some(mut e) = self
                        .cache
                        .iter()
                        .find(|e| Some(e.index) == self.element)
                        .cloned()
                {
                    if let Some(n) = e.paths.get(drag.path).and_then(|p| p.nodes.get(drag.node)) {
                        let point = match drag.handle {
                            Handle::Anchor => n.anchor,
                            Handle::Incoming => n.incoming,
                            Handle::Outgoing => n.outgoing,
                        };
                        if distance(world(&e, point), p) < 1e-4 {
                            return Ok(false);
                        }
                    }
                    let local = layer.transform.to_local(p, layer.offset, size);
                    let local = m::point(
                        m::inverse(e.mapping).ok_or("SVG 경로의 역변환이 없습니다.")?,
                        local,
                    );
                    if local.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
                        return Err("SVG 경로 좌표가 유효하지 않습니다.".into());
                    }
                    let Some(n) = e
                        .paths
                        .get_mut(drag.path)
                        .and_then(|p| p.nodes.get_mut(drag.node))
                    else {
                        self.drag = None;
                        return Ok(false);
                    };
                    match drag.handle {
                        Handle::Anchor => n.move_anchor(local),
                        Handle::Incoming => {
                            n.incoming = local;
                            if !alt {
                                n.outgoing = std::array::from_fn(|i| 2. * n.anchor[i] - local[i]);
                            }
                        }
                        Handle::Outgoing => {
                            n.outgoing = local;
                            if !alt {
                                n.incoming = std::array::from_fn(|i| 2. * n.anchor[i] - local[i]);
                            }
                        }
                    }
                    return self.commit(editor, &e, false);
                }
            }
            Action::Delete => {
                if let Some((pi, ni)) = self.selected
                    && let Some(mut e) = self
                        .cache
                        .iter()
                        .find(|e| Some(e.index) == self.element)
                        .cloned()
                    && let Some(path) = e.paths.get_mut(pi)
                {
                    if path.nodes.len() <= 2 {
                        return Err("경로에는 최소 두 점을 유지하세요. 하위 경로 삭제는 SVG 원본에서 편집하세요.".into());
                    }
                    if ni < path.nodes.len() {
                        path.nodes.remove(ni);
                        self.selected = None;
                        return self.commit(editor, &e, true);
                    }
                }
            }
            _ => {}
        }
        Ok(false)
    }
    fn commit(&mut self, editor: &mut Editor, e: &Element, single: bool) -> Result<bool, String> {
        let (changed, index) = replace(editor, e, single)?;
        self.element = Some(index);
        Ok(changed)
    }
    pub fn paint(&self, painter: &egui::Painter, layer: &Layer, screen: impl Fn([f32; 2]) -> Pos2) {
        if self.object_mode {
            self.objects.paint(painter, layer, screen);
            return;
        }
        let Some(Content::Svg { source }) = &layer.content else {
            return;
        };
        let fallback;
        let list = if self.cache_source == *source && self.layer == Some(layer.id) {
            &self.cache
        } else {
            fallback = elements(source).unwrap_or_default();
            &fallback
        };
        let color = Color32::from_rgb(90, 153, 245);
        for e in list {
            let selected = self.layer == Some(layer.id) && self.element == Some(e.index);
            let screen = |p| {
                screen(layer.transform.to_document(
                    m::point(e.mapping, p),
                    layer.offset,
                    [layer.pixels.width(), layer.pixels.height()],
                ))
            };
            for (pi, path) in e.paths.iter().enumerate() {
                for (i, a) in path
                    .nodes
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| path.closed || *i + 1 < path.nodes.len())
                {
                    let b = &path.nodes[(i + 1) % path.nodes.len()];
                    painter.add(egui::Shape::line(
                        (0..=32)
                            .map(|n| screen(crate::paths::point(a, b, n as f32 / 32.)))
                            .collect(),
                        Stroke::new(if selected { 1.5 } else { 0.8 }, color),
                    ));
                }
                if selected {
                    for (ni, n) in path.nodes.iter().enumerate() {
                        let active = self.selected == Some((pi, ni));
                        if active {
                            for p in [n.incoming, n.outgoing] {
                                painter.line_segment(
                                    [screen(n.anchor), screen(p)],
                                    Stroke::new(1., color),
                                );
                                painter.circle_filled(screen(p), 3.5, Color32::WHITE);
                                painter.circle_stroke(screen(p), 3.5, Stroke::new(1., color));
                            }
                        }
                        let rect =
                            egui::Rect::from_center_size(screen(n.anchor), egui::Vec2::splat(7.));
                        painter.rect_filled(rect, 0., if active { color } else { Color32::WHITE });
                        painter.rect_stroke(
                            rect,
                            0.,
                            Stroke::new(1., color),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            }
        }
    }
}
fn priority(h: Handle) -> u8 {
    match h {
        Handle::Anchor => 0,
        Handle::Incoming => 1,
        Handle::Outgoing => 2,
    }
}
fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}
// Wrap once, then adjust the outer viewport and translation without accumulating nested wrappers.
fn viewport(source: &str, min: [f32; 2], new: [u32; 2]) -> Result<(String, usize), String> {
    let xml = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let root = xml.root_element();
    if root.attribute("data-photo-editor-expanded-viewport") == Some("1") {
        let group = root
            .children()
            .find(|n| {
                n.is_element() && n.attribute("data-photo-editor-viewport-content") == Some("1")
            })
            .ok_or("SVG 확장 뷰포트의 내용 그룹이 없습니다.")?;
        let index = xml
            .descendants()
            .filter(|n| n.is_element())
            .position(|n| n == group)
            .unwrap();
        let t: svgtypes::Transform = group
            .attribute("transform")
            .unwrap_or("")
            .parse()
            .map_err(|e: svgtypes::Error| e.to_string())?;
        let matrix = format!(
            "matrix({} {} {} {} {} {})",
            t.a,
            t.b,
            t.c,
            t.d,
            t.e - min[0] as f64,
            t.f - min[1] as f64
        );
        let mut source = crate::svg_edit::attribute_text(source, index, "transform", &matrix)?;
        for (name, value) in [
            ("width", new[0].to_string()),
            ("height", new[1].to_string()),
            ("viewBox", format!("0 0 {} {}", new[0], new[1])),
        ] {
            source = crate::svg_edit::attribute_text(&source, 0, name, &value)?;
        }
        return Ok((source, 0));
    }
    let prepared = crate::svg_namespace::preview_roots(source)?;
    let tree = resvg::usvg::Tree::from_str(&prepared, &crate::svg_assets::options())
        .map_err(|e| e.to_string())?;
    let frame = tree.size();
    let mut inner = prepared;
    for (name, value) in [
        ("width", frame.width().to_string()),
        ("height", frame.height().to_string()),
        ("x", "0".to_owned()),
        ("y", "0".to_owned()),
        ("overflow", "visible".to_owned()),
    ] {
        inner = crate::svg_edit::attribute_text(&inner, 0, name, &value)?;
    }
    let xml = roxmltree::Document::parse(&inner).map_err(|e| e.to_string())?;
    let style = format!(
        "{};width:{}px!important;height:{}px!important;x:0px!important;y:0px!important;overflow:visible!important",
        xml.root_element().attribute("style").unwrap_or(""),
        frame.width(),
        frame.height()
    );
    inner = crate::svg_edit::attribute_text(&inner, 0, "style", &style)?;
    let xml = roxmltree::Document::parse(&inner).map_err(|e| e.to_string())?;
    let range = xml.root_element().range();
    let output = format!(
        "{}<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\" data-photo-editor-expanded-viewport=\"1\"><g data-photo-editor-viewport-content=\"1\" transform=\"translate({} {})\">{}</g></svg>{}",
        &inner[..range.start],
        new[0],
        new[1],
        new[0],
        new[1],
        -min[0],
        -min[1],
        &inner[range.clone()],
        &inner[range.end..]
    );
    Ok((output, 2))
}
fn replace(editor: &mut Editor, e: &Element, single: bool) -> Result<(bool, usize), String> {
    let layer = editor
        .document
        .active_layer()
        .ok_or("SVG 레이어가 없습니다.")?;
    let Some(Content::Svg { source }) = &layer.content else {
        return Ok((false, e.index));
    };
    let source = crate::svg_edit::attribute_text(source, e.index, "d", &data(&e.paths))?;
    let bounds = elements(&source)?
        .into_iter()
        .find(|n| n.index == e.index)
        .map_or(e.bounds, |e| e.bounds);
    publish(editor, &layer.clone(), source, bounds, e.index, single)
}
pub(crate) fn publish(
    editor: &mut Editor,
    layer: &Layer,
    mut source: String,
    bounds: [f32; 4],
    element_index: usize,
    single: bool,
) -> Result<(bool, usize), String> {
    let old = [layer.pixels.width(), layer.pixels.height()];
    let min = [
        (bounds[0] - 2.).floor().min(0.),
        (bounds[1] - 2.).floor().min(0.),
    ];
    let max = [
        (bounds[2] + 2.).ceil().max(old[0] as f32),
        (bounds[3] + 2.).ceil().max(old[1] as f32),
    ];
    if min
        .iter()
        .chain(max.iter())
        .any(|v| !v.is_finite() || v.abs() > 1e9)
    {
        return Err("SVG 외곽 좌표가 유효하지 않습니다.".into());
    }
    let new = std::array::from_fn::<_, 2, _>(|i| (max[i] - min[i]) as u32);
    crate::document::validate_size(new[0], new[1])?;
    let current = editor
        .document
        .active_layer()
        .ok_or("SVG 레이어가 없습니다.")?;
    let count = editor.document.pixel_count()
        - u64::from(current.pixels.width()) * u64::from(current.pixels.height())
        + u64::from(new[0]) * u64::from(new[1]);
    if count > crate::document::MAX_PIXELS {
        return Err("SVG 확장이 문서 메모리 예산을 초과합니다.".into());
    }
    let mut candidate = layer.clone();
    let mut index = element_index;
    let mask = candidate.mask.as_ref().map(|_| {
        (
            crate::masks::world(layer),
            crate::masks::linked(layer),
            crate::masks::sampling(layer),
        )
    });
    if new != old {
        let (expanded, added) = viewport(&source, min, new)?;
        source = expanded;
        index += added;
        candidate.transform = layer
            .transform
            .rebased(layer.offset, old, new, min)
            .ok_or("SVG 확장이 원근 투영의 한계를 넘습니다.")?;
        if let Some(scope) = &mut candidate.adjustments.scope {
            scope.rebase(m::translate(min[0] as f64, min[1] as f64));
        }
    }
    let pad = candidate.adjustments.padding() + candidate.effects.padding();
    let expanded = new.map(|v| v.saturating_add(pad * 2));
    crate::document::validate_size(expanded[0], expanded[1])?;
    if !candidate.transform.padded(pad as i32).valid_for(expanded) {
        return Err("SVG 필터 외곽이 원근 투영의 한계를 넘습니다.".into());
    }
    let content = Content::Svg { source };
    let pixels = content.render()?;
    if pixels.dimensions() != (new[0], new[1]) {
        return Err("SVG 확장 크기가 렌더링과 일치하지 않습니다.".into());
    }
    if editor
        .document
        .active_layer()
        .and_then(|l| l.content.as_ref())
        == Some(&content)
    {
        return Ok((false, index));
    }
    if candidate.precision.is_some() {
        candidate.precision = Some(Arc::new(
            image::DynamicImage::ImageRgba8(pixels.clone()).into_rgba32f(),
        ));
    }
    candidate.pixels = Arc::new(pixels);
    candidate.content = Some(content);
    candidate.psd_extra = Arc::new(Vec::new());
    if new != old
        && let Some((world, linked, sampling)) = mask
    {
        crate::masks::set_world(&mut candidate, world, linked, sampling)?;
    }
    editor.begin("SVG 경로 편집");
    *editor.document.active_layer_mut().unwrap() = candidate;
    if single {
        editor.finish();
    }
    Ok((true, index))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{Document, Selection},
        precision,
    };
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="80" viewBox="0 0 50 40"><!--keep comment--><style>.paint {fill:url(#grad);stroke:#334455;stroke-width:1}</style><defs><linearGradient id="grad"><stop stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient><path id="template" d="M0 0L5 0L2 5Z"/></defs><g transform="translate(3 4)"><path id="edit" class="paint" data-original="yes" d="M5 5 C 8 1 12 8 15 5 L 15 12 L 5 12 Z M 20 20 q 3 -6 6 0 t 6 0"/><path d="m 30 5 a 5 4 20 0 1 5 8 l-5 0z"/></g><use href="#template" x="40" y="30"/></svg>"##;
    fn editor() -> Editor {
        let content = Content::Svg { source: SVG.into() };
        let pixels = content.render().unwrap();
        let mut doc = Document::new(160, 130, true).unwrap();
        doc.add_layer("imported SVG".into(), pixels).unwrap();
        let l = doc.active_layer_mut().unwrap();
        l.content = Some(content);
        l.offset = [10, 12];
        l.transform.angle = 12.;
        l.transform.scale = [1.1, 0.9];
        l.transform.translation = [2.5, -1.5];
        l.transform.warp = Some([1., 0.05, 0., 0., 1., 0., 0.0005, 0., 1.]);
        Editor::new(doc, false)
    }
    fn source(editor: &Editor) -> &str {
        let Some(Content::Svg { source }) = &editor.document.active_layer().unwrap().content else {
            panic!()
        };
        source
    }
    fn world(editor: &Editor, e: &Element, p: [f32; 2]) -> [f32; 2] {
        let l = editor.document.active_layer().unwrap();
        l.transform.to_document(
            m::point(e.mapping, p),
            l.offset,
            [l.pixels.width(), l.pixels.height()],
        )
    }
    #[test]
    fn relative_quadratic_arc_subpaths_and_closing_curves_keep_geometry_and_structure() {
        let list = elements(SVG).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].paths.len(), 2);
        assert!(list[0].paths[0].closed);
        assert!(!list[0].paths[1].closed);
        assert!(list[1].paths[0].closed);
        let first = m::point(list[0].mapping, [5., 5.]);
        assert_eq!(first, [16., 18.]);
        for e in list {
            let out = crate::svg_edit::attribute(SVG, e.index, "d", &data(&e.paths)).unwrap();
            let before = crate::vector::render_svg(SVG).unwrap();
            let after = crate::vector::render_svg(&out).unwrap();
            assert!(
                before.pixels().zip(after.pixels()).all(|(a, b)| a
                    .0
                    .into_iter()
                    .zip(b.0)
                    .all(|(a, b)| a.abs_diff(b) <= 1))
            );
            assert!(out.contains("<!--keep comment-->"));
            assert!(out.contains("<use href=\"#template\""));
            assert!(out.contains(".paint {fill:url(#grad);stroke:#334455;stroke-width:1}"));
        }
        let paths = parse("M0 0C5 -5 10 5 0 0Z").unwrap();
        assert_eq!(paths[0].nodes.len(), 1);
        let out = data(&paths);
        assert!(out.contains('C'));
        assert!(out.contains('Z'));
        let namespaced = SVG.replace("<path ", "<s:path ").replace(
            "xmlns=\"http://www.w3.org/2000/svg\"",
            "xmlns=\"http://www.w3.org/2000/svg\" xmlns:s=\"http://www.w3.org/2000/svg\"",
        );
        assert_eq!(elements(&namespaced).unwrap().len(), 2);
    }
    #[test]
    fn projected_anchor_handles_single_history_cancel_and_source_mask_settings_survive() {
        let mut editor = editor();
        let l = editor.document.active_layer_mut().unwrap();
        l.mask = Some(Arc::new(image::GrayImage::from_pixel(
            100,
            80,
            image::Luma([200]),
        )));
        l.adjustments.exposure = 0.2;
        l.effects.overlay.enabled = true;
        l.effects.overlay.opacity = 0.1;
        let scope = crate::adjustment_scope::Scope::capture(
            l,
            &Selection::rectangle([20, 20], [100, 90]),
            [160, 130],
        )
        .unwrap();
        l.adjustments.scope = Some(scope);
        let original = l.clone();
        let e = elements(SVG).unwrap().remove(0);
        let p = world(&editor, &e, e.paths[0].nodes[0].anchor);
        let mut pen = crate::paths::Pen::default();
        pen.drawing = false;
        pen.apply(
            &mut editor,
            Action::Press(p, false, false),
            2.,
            [0; 4],
            [0; 4],
            1.,
        )
        .unwrap();
        assert!(
            !pen.apply(&mut editor, Action::Move(p, false), 2., [0; 4], [0; 4], 1.)
                .unwrap()
        );
        assert!(!editor.can_undo());
        let target = [p[0] + 4., p[1] - 3.];
        assert!(
            pen.apply(
                &mut editor,
                Action::Move(target, false),
                2.,
                [0; 4],
                [0; 4],
                1.
            )
            .unwrap()
        );
        pen.apply(&mut editor, Action::Release, 2., [0; 4], [0; 4], 1.)
            .unwrap();
        let changed = elements(source(&editor)).unwrap().remove(0);
        let actual = world(&editor, &changed, changed.paths[0].nodes[0].anchor);
        assert!(distance(actual, target) < 1e-4);
        let l = editor.document.active_layer().unwrap();
        assert_eq!(l.transform, original.transform);
        assert_eq!(l.offset, original.offset);
        assert!(Arc::ptr_eq(
            l.mask.as_ref().unwrap(),
            original.mask.as_ref().unwrap()
        ));
        assert_eq!(l.adjustments, original.adjustments);
        assert_eq!(l.effects, original.effects);
        assert_eq!(editor.history().count(), 1);
        assert!(source(&editor).contains("data-original=\"yes\""));
        let rendered = precision::composite(&editor.document);
        editor.undo();
        assert_eq!(source(&editor), SVG);
        editor.redo();
        assert_eq!(precision::composite(&editor.document), rendered);
        // Alt keeps the opposite handle, while a regular drag mirrors it.
        let e = elements(source(&editor)).unwrap().remove(0);
        let n = &e.paths[0].nodes[0];
        let outgoing = world(&editor, &e, n.outgoing);
        let incoming = n.incoming;
        pen.apply(
            &mut editor,
            Action::Press(outgoing, false, false),
            1.,
            [0; 4],
            [0; 4],
            1.,
        )
        .unwrap();
        pen.apply(
            &mut editor,
            Action::Move([outgoing[0] + 3., outgoing[1] + 2.], true),
            1.,
            [0; 4],
            [0; 4],
            1.,
        )
        .unwrap();
        assert_eq!(
            elements(source(&editor)).unwrap()[0].paths[0].nodes[0].incoming,
            incoming
        );
        pen.apply(&mut editor, Action::Cancel, 1., [0; 4], [0; 4], 1.)
            .unwrap();
        assert_eq!(precision::composite(&editor.document), rendered);
    }
    #[test]
    fn insert_split_and_delete_preserve_other_subpaths_and_project_photoshop_outputs() {
        let mut editor = editor();
        let e = elements(SVG).unwrap().remove(0);
        let a = &e.paths[0].nodes[0];
        let b = &e.paths[0].nodes[1];
        let p = world(&editor, &e, crate::paths::point(a, b, 0.5));
        let mut pen = Pen::default();
        assert!(
            pen.apply(&mut editor, Action::Press(p, false, true), 1.)
                .unwrap()
        );
        let split = elements(source(&editor)).unwrap().remove(0);
        assert_eq!(split.paths[0].nodes.len(), e.paths[0].nodes.len() + 1);
        assert_eq!(split.paths[1].nodes, e.paths[1].nodes);
        let before = crate::vector::render_svg(SVG).unwrap();
        let after = crate::vector::render_svg(source(&editor)).unwrap();
        let maximum = before
            .pixels()
            .zip(after.pixels())
            .flat_map(|(a, b)| a.0.into_iter().zip(b.0).map(|(a, b)| a.abs_diff(b)))
            .max()
            .unwrap();
        let different = before
            .pixels()
            .zip(after.pixels())
            .filter(|(a, b)| a != b)
            .count();
        // The rasterizer can flatten split cubics differently at a few antialiased stroke pixels.
        assert!(
            maximum <= 16 && different <= 4,
            "split raster delta {maximum}, count {different}"
        );
        for n in 0..=100 {
            let t = n as f32 / 100.;
            let original = crate::paths::point(a, b, t);
            let actual = if t <= 0.5 {
                crate::paths::point(&split.paths[0].nodes[0], &split.paths[0].nodes[1], t * 2.)
            } else {
                crate::paths::point(
                    &split.paths[0].nodes[1],
                    &split.paths[0].nodes[2],
                    (t - 0.5) * 2.,
                )
            };
            assert!(distance(original, actual) < 1e-5);
        }
        assert!(pen.apply(&mut editor, Action::Delete, 1.).unwrap());
        assert_eq!(
            elements(source(&editor)).unwrap()[0].paths[0].nodes.len(),
            e.paths[0].nodes.len()
        );
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/svg-paths");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("edited.rphoto");
        crate::files::save_project(&editor.document, &path).unwrap();
        let restored = crate::files::open(&path).unwrap().document;
        assert_eq!(
            restored.active_layer().unwrap().content,
            editor.document.active_layer().unwrap().content
        );
        assert_eq!(restored.composite(), editor.document.composite());
        for ext in ["psd", "psb"] {
            let path = dir.join(format!("edited.{ext}"));
            crate::files::save_photoshop(&editor.document, &path).unwrap();
            let restored = crate::files::open(&path).unwrap().document;
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
    fn viewport_growth_keeps_anchors_masks_scope_precision_history_and_single_wrapper() {
        for linked in [true, false] {
            let mut editor = editor();
            let layer = editor.document.active_layer_mut().unwrap();
            layer.mask = Some(Arc::new(image::GrayImage::from_pixel(
                100,
                80,
                image::Luma([255]),
            )));
            crate::masks::set_world(
                layer,
                crate::masks::world(layer),
                linked,
                crate::sampling::Sampling::Bilinear,
            )
            .unwrap();
            layer.adjustments.scope = Some(
                crate::adjustment_scope::Scope::capture(
                    layer,
                    &Selection::rectangle([0, 0], [160, 130]),
                    [160, 130],
                )
                .unwrap(),
            );
            layer.bit_depth = 16;
            layer.precision = Some(Arc::new(
                image::DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f(),
            ));
            let original = layer.clone();
            let original_source = source(&editor).to_owned();
            let e = elements(source(&editor)).unwrap().remove(0);
            let anchor = world(&editor, &e, e.paths[0].nodes[0].anchor);
            let fixed = world(&editor, &e, e.paths[0].nodes[2].anchor);
            let mask_world = crate::masks::world(editor.document.active_layer().unwrap());
            let mut pen = Pen::default();
            pen.apply(&mut editor, Action::Press(anchor, false, false), 1.)
                .unwrap();
            for (x, y) in [(-6., -4.), (-14., -9.), (-25., -12.)] {
                // Targets are expressed in the original SVG's coordinates, across repeated source growth.
                let target = original.transform.to_document(
                    m::point(e.mapping, [x, y]),
                    original.offset,
                    [100, 80],
                );
                assert!(
                    pen.apply(&mut editor, Action::Move(target, false), 1.)
                        .unwrap()
                );
                let list = elements(source(&editor)).unwrap();
                let changed = list.iter().find(|e| e.label == "edit").unwrap();
                assert!(
                    distance(
                        world(&editor, changed, changed.paths[0].nodes[0].anchor),
                        target
                    ) < 0.0002
                );
                assert!(
                    distance(
                        world(&editor, changed, changed.paths[0].nodes[2].anchor),
                        fixed
                    ) < 0.0002
                );
                let xml = roxmltree::Document::parse(source(&editor)).unwrap();
                assert_eq!(
                    xml.descendants()
                        .filter(|n| n.attribute("data-photo-editor-expanded-viewport") == Some("1"))
                        .count(),
                    1
                );
                assert_eq!(
                    xml.descendants()
                        .filter(|n| n.attribute("data-photo-editor-viewport-content") == Some("1"))
                        .count(),
                    1
                );
                let l = editor.document.active_layer().unwrap();
                assert!(l.pixels.width() > 100);
                assert!(l.pixels.height() > 80);
                assert!(crate::masks::valid(l));
                assert_eq!(crate::masks::linked(l), linked);
                assert_eq!(
                    l.precision.as_ref().unwrap().dimensions(),
                    l.pixels.dimensions()
                );
                assert_eq!(l.bit_depth, 16);
                // Homographies are equivalent up to a scalar; compare their actual mask coordinates.
                for point in [[0., 0.], [100., 0.], [100., 80.], [0., 80.], [50., 40.]] {
                    assert!(
                        distance(
                            m::point(crate::masks::world(l), point),
                            m::point(mask_world, point)
                        ) < 1e-4
                    );
                }
                let scope = l.adjustments.scope.as_ref().unwrap();
                assert!(
                    (scope.coverage(m::point(changed.mapping, changed.paths[0].nodes[2].anchor))
                        - original
                            .adjustments
                            .scope
                            .as_ref()
                            .unwrap()
                            .coverage(m::point(e.mapping, e.paths[0].nodes[2].anchor)))
                    .abs()
                        < 1e-6
                );
            }
            pen.apply(&mut editor, Action::Release, 1.).unwrap();
            assert_eq!(editor.history().count(), 1);
            let rendered = precision::composite(&editor.document);
            editor.undo();
            assert_eq!(source(&editor), original_source);
            assert!(Arc::ptr_eq(
                &original.pixels,
                &editor.document.active_layer().unwrap().pixels
            ));
            editor.redo();
            assert_eq!(precision::composite(&editor.document), rendered);
        }
    }
    #[test]
    fn expanded_root_style_prolog_native_outputs_and_cancellation_keep_unedited_elements() {
        let source_svg = r##"<?xml version="1.0"?><!--before--><svg xmlns="http://www.w3.org/2000/svg" width="40" height="30" viewBox="0 0 20 15"><style>:root{fill:red} .blue{fill:blue}</style><rect class="blue" x="12" y="8" width="4" height="4"/><path id="grow" d="M3 3L9 3L9 9L3 9Z"/></svg><!--after-->"##;
        let content = Content::Svg {
            source: source_svg.into(),
        };
        let mut doc = Document::new(100, 80, true).unwrap();
        doc.layers.clear();
        doc.add_layer("grow".into(), content.render().unwrap())
            .unwrap();
        doc.active_layer_mut().unwrap().content = Some(content);
        doc.active_layer_mut().unwrap().offset = [30, 20];
        let original = doc.composite();
        let mut editor = Editor::new(doc, false);
        let e = elements(source_svg).unwrap().remove(0);
        let anchor = world(&editor, &e, e.paths[0].nodes[0].anchor);
        let target = world(&editor, &e, [-5., -4.]);
        let mut pen = Pen::default();
        pen.apply(&mut editor, Action::Press(anchor, false, false), 1.)
            .unwrap();
        pen.apply(&mut editor, Action::Move(target, false), 1.)
            .unwrap();
        assert!(source(&editor).starts_with("<?xml version=\"1.0\"?><!--before-->"));
        assert!(source(&editor).ends_with("<!--after-->"));
        let rendered = editor.document.composite();
        assert_eq!(rendered.get_pixel(57, 39), original.get_pixel(57, 39));
        assert!(rendered.get_pixel(24, 16)[3] > 0);
        pen.apply(&mut editor, Action::Cancel, 1.).unwrap();
        assert_eq!(source(&editor), source_svg);
        assert_eq!(editor.document.composite(), original);
        assert!(!editor.can_undo());
        pen.apply(&mut editor, Action::Press(anchor, false, false), 1.)
            .unwrap();
        pen.apply(&mut editor, Action::Move(target, false), 1.)
            .unwrap();
        pen.apply(&mut editor, Action::Release, 1.).unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/svg-viewport");
        std::fs::create_dir_all(&dir).unwrap();
        crate::files::save_project(&editor.document, &dir.join("expanded.rphoto")).unwrap();
        let restored = crate::files::open(&dir.join("expanded.rphoto"))
            .unwrap()
            .document;
        assert_eq!(restored.composite(), editor.document.composite());
        crate::svg_export::save(&editor.document, &dir.join("expanded.svg")).unwrap();
        let svg = std::fs::read_to_string(dir.join("expanded.svg")).unwrap();
        let rendered = crate::vector::render_svg(&svg).unwrap();
        let expected = editor.document.composite();
        for (a, b) in rendered.pixels().zip(expected.pixels()) {
            assert!(a[3].abs_diff(b[3]) <= 2);
            for c in 0..3 {
                assert!((a[c] as f32 * a[3] as f32 - b[c] as f32 * b[3] as f32).abs() < 800.);
            }
        }
        for ext in ["psd", "psb"] {
            let path = dir.join(format!("expanded.{ext}"));
            crate::files::save_photoshop(&editor.document, &path).unwrap();
            let actual = crate::files::open(&path).unwrap().document.composite();
            for (a, b) in actual.pixels().zip(expected.pixels()) {
                assert!(a[3].abs_diff(b[3]) <= 2);
                for c in 0..3 {
                    assert!((a[c] as f32 * a[3] as f32 - b[c] as f32 * b[3] as f32).abs() < 800.);
                }
            }
        }
    }
    #[test]
    fn excessive_viewport_and_horizon_growth_fail_before_document_mutation() {
        let mut editor = editor();
        let original = editor.document.active_layer().unwrap().clone();
        let e = elements(SVG).unwrap().remove(0);
        let anchor = world(&editor, &e, e.paths[0].nodes[0].anchor);
        let mut pen = Pen::default();
        pen.apply(&mut editor, Action::Press(anchor, false, false), 1.)
            .unwrap();
        let target = original.transform.to_document(
            m::point(e.mapping, [-10_000., -10_000.]),
            original.offset,
            [100, 80],
        );
        assert!(
            pen.apply(&mut editor, Action::Move(target, false), 1.)
                .is_err()
        );
        assert!(Arc::ptr_eq(
            &original.pixels,
            &editor.document.active_layer().unwrap().pixels
        ));
        assert_eq!(source(&editor), SVG);
        assert!(!editor.can_undo());
        pen.reset();
        let l = editor.document.active_layer_mut().unwrap();
        l.transform.warp = Some([1., 0., 0., 0., 1., 0., 0.01, 0., 1.]);
        let original = l.clone();
        let e = elements(SVG).unwrap().remove(0);
        let anchor = world(&editor, &e, e.paths[0].nodes[0].anchor);
        pen.apply(&mut editor, Action::Press(anchor, false, false), 1.)
            .unwrap();
        let target = original
            .transform
            .to_document([-110., 20.], original.offset, [100, 80]);
        assert!(
            pen.apply(&mut editor, Action::Move(target, false), 1.)
                .is_err()
        );
        assert_eq!(source(&editor), SVG);
        assert!(Arc::ptr_eq(
            &original.pixels,
            &editor.document.active_layer().unwrap().pixels
        ));
    }
    #[test]
    fn locked_invalid_and_active_layer_changes_cannot_modify_previous_svg() {
        let mut editor = editor();
        let original = editor.document.active_layer().unwrap().pixels.clone();
        let id = editor.document.active;
        let e = elements(SVG).unwrap().remove(0);
        let p = world(&editor, &e, e.paths[0].nodes[0].anchor);
        let mut pen = Pen::default();
        editor.document.active_layer_mut().unwrap().locked = true;
        assert!(
            !pen.apply(&mut editor, Action::Press(p, false, false), 1.)
                .unwrap()
        );
        editor.document.active_layer_mut().unwrap().locked = false;
        pen.apply(&mut editor, Action::Press(p, false, false), 1.)
            .unwrap();
        assert!(
            pen.apply(&mut editor, Action::Move([f32::NAN, 0.], false), 1.)
                .is_err()
        );
        assert!(Arc::ptr_eq(
            &original,
            &editor.document.active_layer().unwrap().pixels
        ));
        assert!(!editor.can_undo());
        let content = Content::Svg { source: SVG.into() };
        editor
            .document
            .add_layer("other".into(), content.render().unwrap())
            .unwrap();
        editor.document.active_layer_mut().unwrap().content = Some(content);
        assert!(!pen.apply(&mut editor, Action::Delete, 1.).unwrap());
        assert_eq!(source(&editor), SVG);
        editor.document.active = id;
        assert!(Arc::ptr_eq(
            &original,
            &editor.document.active_layer().unwrap().pixels
        ));
        assert!(parse("M0 0 Q NaN 2 3 4").is_err());
        assert!(parse("M 1e100 0 L0 0").is_err());
    }
}
