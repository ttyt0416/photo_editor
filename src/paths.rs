use crate::{
    document::{Editor, Layer, MAX_PIXELS, validate_size},
    vector::{Content, PathNode},
};
use eframe::egui::{self, Color32, Pos2, Stroke};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    Anchor,
    Incoming,
    Outgoing,
}

#[derive(Clone, Copy)]
struct Drag {
    layer: Option<u64>,
    index: usize,
    handle: Handle,
}

pub enum Action {
    Press([f32; 2], bool, bool), // Alt breaks handles, Ctrl inserts an anchor.
    Move([f32; 2], bool),
    Release,
    Finish,
    Cancel,
    Delete,
}

pub struct Pen {
    pub drawing: bool,
    pub draft: Vec<PathNode>,
    pub selected: Option<usize>,
    pub svg: crate::svg_paths::Pen,
    selected_layer: Option<u64>,
    drag: Option<Drag>,
}
impl Default for Pen {
    fn default() -> Self {
        Self {
            drawing: true,
            draft: vec![],
            selected: None,
            svg: Default::default(),
            selected_layer: None,
            drag: None,
        }
    }
}

pub fn point(a: &PathNode, b: &PathNode, t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    std::array::from_fn(|i| {
        u * u * u * a.anchor[i]
            + 3.0 * u * u * t * a.outgoing[i]
            + 3.0 * u * t * t * b.incoming[i]
            + t * t * t * b.anchor[i]
    })
}

pub fn split(nodes: &mut Vec<PathNode>, index: usize, t: f32, closed: bool) -> bool {
    if nodes.len() < if closed { 1 } else { 2 }
        || nodes.len() >= 16384
        || index >= nodes.len()
        || (!closed && index + 1 == nodes.len())
    {
        return false;
    }
    let next = (index + 1) % nodes.len();
    let (a, b) = (&nodes[index], &nodes[next]);
    let mix = |a: [f32; 2], b: [f32; 2]| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
    let q0 = mix(a.anchor, a.outgoing);
    let q1 = mix(a.outgoing, b.incoming);
    let q2 = mix(b.incoming, b.anchor);
    let r0 = mix(q0, q1);
    let r1 = mix(q1, q2);
    let anchor = mix(r0, r1);
    nodes[index].outgoing = q0;
    nodes[next].incoming = q2;
    nodes.insert(
        index + 1,
        PathNode {
            anchor,
            incoming: r0,
            outgoing: r1,
        },
    );
    true
}

fn local_point(layer: &Layer, point: [f32; 2]) -> [f32; 2] {
    layer.transform.to_local(
        point,
        layer.offset,
        [layer.pixels.width(), layer.pixels.height()],
    )
}

// Expanding the source must leave its transformed document-space pivot unchanged.
fn expand(layer: &mut Layer, content: &mut Content) -> Result<(), String> {
    let Content::Path {
        width,
        height,
        nodes,
        thickness,
        ..
    } = content
    else {
        return Ok(());
    };
    let old = [*width, *height];
    let pad = *thickness / 2.0 + 2.0;
    let mut min = [0.0f32; 2];
    let mut max = old.map(|v| v as f32);
    for node in nodes.iter() {
        for p in [node.anchor, node.incoming, node.outgoing] {
            for i in 0..2 {
                if !p[i].is_finite() || p[i].abs() > 1e9 {
                    return Err("경로 좌표가 유효하지 않습니다.".into());
                }
                min[i] = min[i].min((p[i] - pad).floor());
                max[i] = max[i].max((p[i] + pad).ceil());
            }
        }
    }
    let new = std::array::from_fn::<_, 2, _>(|i| (max[i] - min[i]) as u32);
    validate_size(new[0], new[1])?;
    if new == old {
        return Ok(());
    }
    for node in nodes {
        for p in [&mut node.anchor, &mut node.incoming, &mut node.outgoing] {
            for i in 0..2 {
                p[i] -= min[i];
            }
        }
    }
    layer.transform = layer
        .transform
        .rebased(layer.offset, old, new, min)
        .ok_or("경로 확장이 원근 변형의 투영 한계를 넘습니다.")?;
    if layer.mask_placement.is_none()
        && let Some(source) = &layer.mask
    {
        let mut mask = image::GrayImage::new(new[0], new[1]);
        image::imageops::replace(&mut mask, &**source, -min[0] as i64, -min[1] as i64);
        layer.mask = Some(Arc::new(mask));
    }
    crate::masks::rebase(layer, min);
    *width = new[0];
    *height = new[1];
    Ok(())
}

fn replace(
    editor: &mut Editor,
    id: u64,
    mut content: Content,
    single: bool,
) -> Result<bool, String> {
    let Some(index) = editor.document.layers.iter().position(|l| l.id == id) else {
        return Ok(false);
    };
    if editor.document.is_locked(id) {
        return Ok(false);
    }
    let mut layer = editor.document.layers[index].clone();
    expand(&mut layer, &mut content)?;
    let pixels = content.render()?;
    if editor.document.pixel_count()
        - u64::from(layer.pixels.width()) * u64::from(layer.pixels.height())
        + u64::from(pixels.width()) * u64::from(pixels.height())
        > MAX_PIXELS
    {
        return Err("경로 편집이 문서 메모리 예산을 초과합니다.".into());
    }
    layer.pixels = Arc::new(pixels);
    layer.precision = None;
    layer.bit_depth = 8;
    layer.content = Some(content);
    editor.begin("경로 편집");
    editor.document.layers[index] = layer;
    if single {
        editor.finish();
    }
    Ok(true)
}

impl Pen {
    pub fn reset(&mut self) {
        self.svg.reset();
        self.draft.clear();
        self.selected = None;
        self.selected_layer = None;
        self.drag = None;
    }

    fn finish(
        &mut self,
        editor: &mut Editor,
        fill: [u8; 4],
        stroke: [u8; 4],
        thickness: f32,
        closed: bool,
    ) -> Result<bool, String> {
        if self.draft.len() < if closed { 3 } else { 2 } {
            return Ok(false);
        }
        let pad = thickness / 2.0 + 2.0;
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        for node in &self.draft {
            for p in [node.anchor, node.incoming, node.outgoing] {
                for i in 0..2 {
                    min[i] = min[i].min((p[i] - pad).floor());
                    max[i] = max[i].max((p[i] + pad).ceil());
                }
            }
        }
        let size = std::array::from_fn::<_, 2, _>(|i| (max[i] - min[i]).max(1.0) as u32);
        validate_size(size[0], size[1])?;
        let mut nodes = self.draft.clone();
        for node in &mut nodes {
            for p in [&mut node.anchor, &mut node.incoming, &mut node.outgoing] {
                for i in 0..2 {
                    p[i] -= min[i];
                }
            }
        }
        let content = Content::Path {
            width: size[0],
            height: size[1],
            nodes,
            closed,
            fill,
            stroke,
            thickness,
        };
        let pixels = content.render()?;
        editor.begin("펜 경로 만들기");
        if let Err(error) = editor.document.add_layer("경로".into(), pixels) {
            editor.cancel();
            return Err(error);
        }
        let layer = editor.document.active_layer_mut().unwrap();
        layer.offset = min.map(|v| v as i32);
        layer.content = Some(content);
        editor.finish();
        self.reset();
        self.drawing = false;
        Ok(true)
    }

    pub fn apply(
        &mut self,
        editor: &mut Editor,
        action: Action,
        tolerance: f32,
        fill: [u8; 4],
        stroke: [u8; 4],
        thickness: f32,
    ) -> Result<bool, String> {
        if !self.drawing
            && editor
                .document
                .active_layer()
                .is_some_and(|l| matches!(l.content, Some(Content::Svg { .. })))
        {
            return self.svg.apply(editor, action, tolerance);
        }
        match action {
            Action::Cancel => {
                self.reset();
                editor.cancel();
                return Ok(true);
            }
            Action::Finish => return self.finish(editor, fill, stroke, thickness, false),
            Action::Release => {
                self.drag = None;
                editor.finish();
                return Ok(false);
            }
            _ => {}
        }
        if self.drawing {
            match action {
                Action::Press(p, _, _) => {
                    if self.draft.len() >= 3 && distance(self.draft[0].anchor, p) <= tolerance {
                        return self.finish(editor, fill, stroke, thickness, true);
                    }
                    if self.draft.len() >= 16384 {
                        return Err("경로 점이 너무 많습니다.".into());
                    }
                    self.draft.push(PathNode::corner(p));
                    let index = self.draft.len() - 1;
                    self.selected = Some(index);
                    self.drag = Some(Drag {
                        layer: None,
                        index,
                        handle: Handle::Outgoing,
                    });
                }
                Action::Move(p, alt) => {
                    if let Some(drag) = self.drag {
                        let node = &mut self.draft[drag.index];
                        node.outgoing = p;
                        if !alt {
                            node.incoming = std::array::from_fn(|i| 2.0 * node.anchor[i] - p[i]);
                        }
                    }
                }
                Action::Delete => {
                    self.draft.pop();
                    self.selected = self.draft.len().checked_sub(1);
                    self.drag = None;
                }
                _ => {}
            }
            return Ok(false);
        }
        let Some(layer) = editor
            .document
            .active_layer()
            .filter(|l| !editor.document.is_locked(l.id))
        else {
            return Ok(false);
        };
        let Some(mut content @ Content::Path { .. }) = layer.content.clone() else {
            return Ok(false);
        };
        let id = layer.id;
        if self.selected_layer != Some(id) {
            self.selected = None;
            self.drag = None;
            self.selected_layer = Some(id);
        }
        let Content::Path { nodes, closed, .. } = &mut content else {
            unreachable!();
        };
        match action {
            Action::Press(p, alt, insert) => {
                if insert {
                    let nearest = nodes
                        .iter()
                        .enumerate()
                        .filter(|(i, _)| *closed || *i + 1 < nodes.len())
                        .flat_map(|(i, a)| {
                            let b = &nodes[(i + 1) % nodes.len()];
                            (1..24).map(move |n| {
                                let t = n as f32 / 24.0;
                                let q = layer.transform.to_document(
                                    point(a, b, t),
                                    layer.offset,
                                    [layer.pixels.width(), layer.pixels.height()],
                                );
                                (i, t, distance(q, p))
                            })
                        })
                        .min_by(|a, b| a.2.total_cmp(&b.2));
                    if let Some((index, t, d)) = nearest
                        && d <= tolerance
                        && split(nodes, index, t, *closed)
                    {
                        self.selected = Some(index + 1);
                        return replace(editor, id, content, true);
                    }
                }
                let hit = nodes
                    .iter()
                    .enumerate()
                    .flat_map(|(i, n)| {
                        [
                            (Handle::Anchor, n.anchor),
                            (Handle::Incoming, n.incoming),
                            (Handle::Outgoing, n.outgoing),
                        ]
                        .into_iter()
                        .map(move |(h, q)| {
                            let q = layer.transform.to_document(
                                q,
                                layer.offset,
                                [layer.pixels.width(), layer.pixels.height()],
                            );
                            (i, h, distance(q, p))
                        })
                    })
                    .filter(|(_, _, d)| *d <= tolerance)
                    .min_by(|a, b| {
                        a.2.total_cmp(&b.2).then_with(|| {
                            let priority = |h| match h {
                                Handle::Anchor => 0,
                                Handle::Incoming => 1,
                                Handle::Outgoing => 2,
                            };
                            priority(a.1).cmp(&priority(b.1))
                        })
                    });
                if let Some((index, handle, _)) = hit {
                    self.selected = Some(index);
                    if alt && handle == Handle::Anchor {
                        nodes[index] = PathNode::corner(nodes[index].anchor);
                        return replace(editor, id, content, true);
                    }
                    self.drag = Some(Drag {
                        layer: Some(id),
                        index,
                        handle,
                    });
                } else {
                    self.selected = None;
                }
            }
            Action::Move(p, alt) => {
                if let Some(drag) = self.drag.filter(|d| d.layer == Some(id))
                    && drag.index < nodes.len()
                {
                    let p = local_point(layer, p);
                    let node = &mut nodes[drag.index];
                    match drag.handle {
                        Handle::Anchor => node.move_anchor(p),
                        Handle::Incoming => {
                            node.incoming = p;
                            if !alt {
                                node.outgoing =
                                    std::array::from_fn(|i| 2.0 * node.anchor[i] - p[i]);
                            }
                        }
                        Handle::Outgoing => {
                            node.outgoing = p;
                            if !alt {
                                node.incoming =
                                    std::array::from_fn(|i| 2.0 * node.anchor[i] - p[i]);
                            }
                        }
                    }
                    return replace(editor, id, content, false);
                }
            }
            Action::Delete => {
                if let Some(index) = self.selected.filter(|i| *i < nodes.len()) {
                    if nodes.len() <= if *closed { 3 } else { 2 } {
                        return Err("열린 경로는 두 점, 닫힌 경로는 세 점 이상 필요합니다.".into());
                    }
                    nodes.remove(index);
                    self.selected = None;
                    return replace(editor, id, content, true);
                }
            }
            _ => {}
        }
        Ok(false)
    }

    pub fn paint(
        &self,
        painter: &egui::Painter,
        layer: Option<&Layer>,
        screen: impl Fn([f32; 2]) -> Pos2,
    ) {
        if !self.drawing
            && let Some(layer) = layer
            && matches!(layer.content, Some(Content::Svg { .. }))
        {
            self.svg.paint(painter, layer, screen);
            return;
        }
        let (nodes, closed) = if self.drawing {
            (&self.draft, false)
        } else if let Some(Content::Path { nodes, closed, .. }) =
            layer.and_then(|l| l.content.as_ref())
        {
            (nodes, *closed)
        } else {
            return;
        };
        let screen = |p| {
            screen(if self.drawing {
                p
            } else {
                let l = layer.unwrap();
                l.transform
                    .to_document(p, l.offset, [l.pixels.width(), l.pixels.height()])
            })
        };
        let color = Color32::from_rgb(90, 153, 245);
        for (i, a) in nodes
            .iter()
            .enumerate()
            .filter(|(i, _)| closed || *i + 1 < nodes.len())
        {
            let b = &nodes[(i + 1) % nodes.len()];
            painter.add(egui::Shape::line(
                (0..=24)
                    .map(|n| screen(point(a, b, n as f32 / 24.0)))
                    .collect(),
                Stroke::new(1.3, color),
            ));
        }
        for (i, node) in nodes.iter().enumerate() {
            let selected = self.selected == Some(i)
                && (self.drawing || layer.is_some_and(|l| self.selected_layer == Some(l.id)));
            if selected {
                for p in [node.incoming, node.outgoing] {
                    painter.line_segment([screen(node.anchor), screen(p)], Stroke::new(1.0, color));
                    painter.circle_filled(screen(p), 3.5, Color32::WHITE);
                    painter.circle_stroke(screen(p), 3.5, Stroke::new(1.0, color));
                }
            }
            let rect = egui::Rect::from_center_size(screen(node.anchor), egui::Vec2::splat(7.0));
            painter.rect_filled(rect, 0.0, if selected { color } else { Color32::WHITE });
            painter.rect_stroke(rect, 0.0, Stroke::new(1.0, color), egui::StrokeKind::Inside);
        }
    }
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_anchor_does_not_follow_active_layer_changes() {
        let mut editor = Editor::new(crate::document::Document::new(80, 80, true).unwrap(), true);
        let content = Content::Path {
            width: 40,
            height: 40,
            nodes: vec![
                PathNode::corner([5.0, 5.0]),
                PathNode::corner([35.0, 5.0]),
                PathNode::corner([20.0, 35.0]),
            ],
            closed: false,
            fill: [0; 4],
            stroke: [0, 0, 0, 255],
            thickness: 2.0,
        };
        for _ in 0..2 {
            editor
                .document
                .add_layer("경로".into(), content.render().unwrap())
                .unwrap();
            editor.document.active_layer_mut().unwrap().content = Some(content.clone());
        }
        let first = editor.document.layers[1].id;
        let second = editor.document.layers[2].id;
        editor.document.active = first;
        let mut pen = Pen {
            drawing: false,
            ..Default::default()
        };
        pen.apply(
            &mut editor,
            Action::Press([5.0, 5.0], false, false),
            3.0,
            [0; 4],
            [0; 4],
            2.0,
        )
        .unwrap();
        assert_eq!(pen.selected, Some(0));
        pen.apply(&mut editor, Action::Release, 3.0, [0; 4], [0; 4], 2.0)
            .unwrap();
        editor.document.active = second;
        assert!(
            !pen.apply(&mut editor, Action::Delete, 3.0, [0; 4], [0; 4], 2.0)
                .unwrap()
        );
        assert_eq!(
            editor.document.active_layer().unwrap().content.as_ref(),
            Some(&content)
        );
        assert!(!editor.can_undo());
    }
    #[test]
    fn splitting_curve_preserves_geometry_for_open_and_closed_paths() {
        for closed in [false, true] {
            let mut nodes = vec![
                PathNode {
                    anchor: [2.0, 10.0],
                    incoming: [0.0, 2.0],
                    outgoing: [10.0, 0.0],
                },
                PathNode {
                    anchor: [22.0, 10.0],
                    incoming: [14.0, 20.0],
                    outgoing: [30.0, 8.0],
                },
            ];
            let index = usize::from(closed);
            let a = nodes[index].clone();
            let b = nodes[(index + 1) % nodes.len()].clone();
            assert!(split(&mut nodes, index, 0.4, closed));
            for step in 0..=100 {
                let t = step as f32 / 100.0;
                let expected = point(&a, &b, t);
                let actual = if t <= 0.4 {
                    point(&nodes[index], &nodes[index + 1], t / 0.4)
                } else {
                    point(
                        &nodes[index + 1],
                        &nodes[(index + 2) % nodes.len()],
                        (t - 0.4) / 0.6,
                    )
                };
                assert!(distance(expected, actual) < 0.0001);
            }
        }
    }
    #[test]
    fn path_creation_drag_expansion_cancel_and_one_undo_keep_rotated_pivot() {
        let mut editor = Editor::new(
            crate::document::Document::new(200, 200, true).unwrap(),
            true,
        );
        let mut pen = Pen::default();
        let apply = |pen: &mut Pen, editor: &mut Editor, a| {
            pen.apply(editor, a, 4.0, [200, 0, 0, 255], [0, 0, 200, 255], 3.0)
                .unwrap()
        };
        apply(
            &mut pen,
            &mut editor,
            Action::Press([30.0, 30.0], false, false),
        );
        apply(&mut pen, &mut editor, Action::Move([45.0, 20.0], false));
        apply(&mut pen, &mut editor, Action::Release);
        apply(
            &mut pen,
            &mut editor,
            Action::Press([100.0, 80.0], false, false),
        );
        apply(&mut pen, &mut editor, Action::Release);
        assert!(!editor.can_undo());
        assert!(apply(&mut pen, &mut editor, Action::Finish));
        assert_eq!(editor.history().count(), 1);
        let layer = editor.document.active_layer_mut().unwrap();
        layer.transform.angle = 30.0;
        layer.transform.scale = [-1.2, 0.8];
        layer.mask = Some(Arc::new(image::GrayImage::from_fn(
            layer.pixels.width(),
            layer.pixels.height(),
            |x, y| image::Luma([(x + y) as u8]),
        )));
        let layer = layer.clone();
        let Content::Path { nodes, .. } = layer.content.as_ref().unwrap() else {
            panic!();
        };
        let size = [layer.pixels.width(), layer.pixels.height()];
        let first = layer
            .transform
            .to_document(nodes[0].anchor, layer.offset, size);
        let second = layer
            .transform
            .to_document(nodes[1].anchor, layer.offset, size);
        apply(&mut pen, &mut editor, Action::Press(first, false, false));
        let target = [first[0] - 80.0, first[1] + 30.0];
        apply(&mut pen, &mut editor, Action::Move(target, false));
        apply(
            &mut pen,
            &mut editor,
            Action::Move([target[0] - 10.0, target[1]], false),
        );
        apply(&mut pen, &mut editor, Action::Release);
        assert_eq!(editor.history().count(), 2);
        let new = editor.document.active_layer().unwrap();
        let old_mask_point = layer.transform.to_document([2.0, 2.0], layer.offset, size);
        let new_mask_point = local_point(new, old_mask_point).map(|v| v.round() as u32);
        assert_eq!(
            new.mask
                .as_ref()
                .unwrap()
                .get_pixel(new_mask_point[0], new_mask_point[1]),
            layer.mask.as_ref().unwrap().get_pixel(2, 2)
        );
        let Content::Path { nodes, .. } = new.content.as_ref().unwrap() else {
            panic!();
        };
        assert!(
            distance(
                second,
                new.transform.to_document(
                    nodes[1].anchor,
                    new.offset,
                    [new.pixels.width(), new.pixels.height()]
                )
            ) < 0.0001
        );
        editor.undo();
        assert_eq!(
            editor.document.active_layer().unwrap().content,
            layer.content
        );
        pen.drawing = true;
        apply(
            &mut pen,
            &mut editor,
            Action::Press([20.0, 20.0], false, false),
        );
        apply(&mut pen, &mut editor, Action::Cancel);
        assert!(pen.draft.is_empty());
        assert_eq!(editor.history().count(), 1);
    }
}
