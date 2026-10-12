use crate::{
    canvas::View,
    document::{Document, Editor, Layer},
    rich_text::{self, Style},
    transform::Transform,
    vector::Content,
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use std::{ops::Range, sync::Arc};

pub struct Session {
    pub tab: u64,
    pub layer: Option<u64>,
    pub content: Content,
    pub offset: [i32; 2],
    pub transform: Transform,
    pub selection: Range<usize>,
    pub style: Style,
    pub focus: bool,
    pub ime_active: bool,
    pub last_error: Option<String>,
}
impl Session {
    pub fn new(tab: u64, point: [f32; 2], color: [u8; 4]) -> Self {
        let content = Content::Text {
            text: String::new(),
            font: "Malgun Gothic".into(),
            size: 32.0,
            width: 300,
            height: 160,
            color,
            line_height: 1.3,
            spacing: 0.0,
            align: 0,
            wrap: true,
            spans: vec![],
        };
        Self {
            tab,
            layer: None,
            style: rich_text::base(&content),
            content,
            offset: point.map(|v| v.floor() as i32),
            transform: Transform {
                translation: point.map(|v| v - v.floor()),
                ..Default::default()
            },
            selection: 0..0,
            focus: true,
            ime_active: false,
            last_error: None,
        }
    }
    pub fn existing(tab: u64, layer: &Layer) -> Option<Self> {
        let content = layer.content.clone()?;
        if !matches!(content, Content::Text { .. }) {
            return None;
        }
        Some(Self {
            tab,
            layer: Some(layer.id),
            style: rich_text::base(&content),
            content,
            offset: layer.offset,
            transform: layer.transform,
            selection: 0..0,
            focus: true,
            ime_active: false,
            last_error: None,
        })
    }
    pub fn size(&self) -> [u32; 2] {
        if let Content::Text { width, height, .. } = self.content {
            [width, height]
        } else {
            unreachable!()
        }
    }
    pub fn resize(&mut self, size: [u32; 2]) {
        let size = size.map(|v| v.clamp(1, 10000));
        let Some(transform) = self
            .transform
            .rebased(self.offset, self.size(), size, [0.; 2])
        else {
            return;
        };
        if let Content::Text { width, height, .. } = &mut self.content {
            *width = size[0];
            *height = size[1];
        }
        self.transform = transform;
    }
    pub fn apply(&self, editor: &mut Editor) -> Result<bool, String> {
        if self.ime_active {
            return Err("한글 조합을 마친 뒤 적용하세요.".into());
        }
        self.content.validate()?;
        if let Some(id) = self.layer
            && editor.document.is_locked(id)
        {
            return Err("잠긴 텍스트는 편집할 수 없습니다.".into());
        }
        let pixels = self.content.render()?;
        if !self.transform.valid_for([pixels.width(), pixels.height()]) {
            return Err("텍스트 영역의 원근 변형이 유효하지 않습니다.".into());
        }
        let mut next = editor.document.clone();
        let id = if let Some(id) = self.layer {
            id
        } else {
            next.add_layer("텍스트".into(), pixels.clone())?;
            next.active
        };
        let layer = next
            .layers
            .iter_mut()
            .find(|l| l.id == id)
            .ok_or("텍스트 레이어가 없습니다.")?;
        if layer.content.as_ref() == Some(&self.content)
            && layer.offset == self.offset
            && layer.transform == self.transform
        {
            return Ok(false);
        }
        let old = [layer.pixels.width(), layer.pixels.height()];
        if layer.mask_placement.is_none()
            && let Some(mask) = &layer.mask
            && old != self.size()
        {
            // Reflow expands/crops the source box; existing mask coordinates stay fixed.
            layer.mask = Some(Arc::new(image::GrayImage::from_fn(
                self.size()[0],
                self.size()[1],
                |x, y| {
                    if x < old[0] && y < old[1] {
                        *mask.get_pixel(x, y)
                    } else {
                        image::Luma([255])
                    }
                },
            )));
        }
        layer.pixels = Arc::new(pixels);
        layer.content = Some(self.content.clone());
        layer.offset = self.offset;
        layer.transform = self.transform;
        layer.precision = None;
        layer.bit_depth = 8;
        if next.pixel_count() > crate::document::MAX_PIXELS {
            return Err("텍스트 상자가 문서 메모리 예산을 초과합니다.".into());
        }
        next.active = id;
        editor.begin("텍스트 편집");
        editor.document = next;
        editor.finish();
        Ok(true)
    }
    pub fn ui(
        &mut self,
        ctx: &egui::Context,
        area: Rect,
        view: &View,
        doc: &Document,
    ) -> Option<bool> {
        ctx.input(|input| {
            for event in &input.raw.events {
                match event {
                    egui::Event::Ime(egui::ImeEvent::Preedit { text, .. }) => {
                        self.ime_active = !text.is_empty()
                    }
                    egui::Event::Ime(egui::ImeEvent::Commit(_)) => self.ime_active = false,
                    _ => {}
                }
            }
        });
        if self.ime_active {
            ctx.input_mut(|i| {
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape);
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter);
            });
        }
        let mut decision = None;
        let mut requested_size = self.size();
        egui::Window::new("텍스트 속성")
            .id(egui::Id::new("canvas-text-controls"))
            .default_pos(Pos2::new(area.right() - 330.0, area.top() + 10.0))
            .default_width(300.0)
            .max_height((area.height() - 24.0).max(120.0))
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label("Enter: 줄바꿈 · Ctrl+Enter: 적용 · Esc: 취소");
                if let Content::Text {
                    font,
                    size,
                    color,
                    line_height,
                    spacing,
                    align,
                    wrap,
                    ..
                } = &mut self.content
                {
                    ui.label("기본 글꼴");
                    crate::font_picker::ui(ui, "canvas-paragraph-font", font, false, false);
                    ui.add(
                        egui::DragValue::new(size)
                            .range(1.0..=2000.0)
                            .prefix("글자 크기 "),
                    );
                    ui.color_edit_button_srgba_unmultiplied(color);
                    ui.add(crate::numeric_slider::Slider::new(line_height, 0.5..=3.0).text("행간"));
                    ui.add(crate::numeric_slider::Slider::new(spacing, -10.0..=30.0).text("자간"));
                    ui.horizontal(|ui| {
                        ui.selectable_value(align, 0, "왼쪽");
                        ui.selectable_value(align, 1, "중앙");
                        ui.selectable_value(align, 2, "오른쪽");
                    });
                    ui.checkbox(wrap, "자동 줄바꿈");
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut requested_size[0])
                            .range(1..=10000)
                            .prefix("상자 폭 "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut requested_size[1])
                            .range(1..=10000)
                            .prefix("높이 "),
                    );
                });
                ui.separator();
                rich_text::controls(ui, &mut self.content, &mut self.selection, &mut self.style);
                if let Some(error) = &self.last_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(!self.ime_active, egui::Button::new("적용"))
                        .clicked()
                    {
                        decision = Some(true);
                    }
                    if ui.button("취소").clicked() {
                        decision = Some(false);
                    }
                });
            });
        if requested_size != self.size() {
            self.resize(requested_size);
        }
        rich_text::install_fonts(ctx, &self.content);
        let zoom = view.zoom / ctx.pixels_per_point();
        let source_size = self.size();
        let image = view.image_rect(area, [doc.width, doc.height], ctx.pixels_per_point());
        let origin = self
            .transform
            .to_document([0.0, 0.0], self.offset, source_size);
        let position = image.min + Vec2::new(origin[0], origin[1]) * zoom;
        let editor_zoom = (zoom * self.transform.scale[0].abs()).clamp(0.03, 10.0);
        let width = (source_size[0] as f32 * editor_zoom).clamp(80.0, area.width().max(80.0));
        let height = (source_size[1] as f32 * zoom * self.transform.scale[1].abs())
            .clamp(50.0, area.height().max(50.0));
        egui::Area::new(egui::Id::new("canvas-text-input"))
            .order(egui::Order::Foreground)
            .fixed_pos(position)
            .movable(false)
            .show(ctx, |ui| {
                ui.set_clip_rect(area);
                let (_, header) = ui.allocate_space(Vec2::new(width, 22.0));
                let response = ui.interact(header, egui::Id::new("text-box-move"), Sense::drag());
                ui.painter()
                    .rect_filled(header, 0.0, Color32::from_rgb(45, 65, 90));
                ui.painter().text(
                    header.left_center(),
                    egui::Align2::LEFT_CENTER,
                    "텍스트 상자 이동",
                    egui::FontId::proportional(12.0),
                    Color32::WHITE,
                );
                if response.dragged() {
                    let delta = response.drag_delta() / zoom;
                    self.transform.translation[0] += delta.x;
                    self.transform.translation[1] += delta.y;
                }
                let mut text = if let Content::Text { text, .. } = &self.content {
                    text.clone()
                } else {
                    String::new()
                };
                let draft = self.content.clone();
                let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap_width: f32| {
                    let job = rich_text::layout_job(
                        ui.ctx(),
                        &draft,
                        text.as_str(),
                        editor_zoom,
                        wrap_width,
                    );
                    ui.fonts_mut(|f| f.layout_job(job))
                };
                let output = egui::Frame::new()
                    .fill({
                        let color = rich_text::base(&self.content).color;
                        if u32::from(color[0]) * 299
                            + u32::from(color[1]) * 587
                            + u32::from(color[2]) * 114
                            < 128_000
                        {
                            Color32::from_rgb(238, 240, 243)
                        } else {
                            Color32::from_rgb(30, 33, 38)
                        }
                    })
                    .stroke(Stroke::new(1.0, Color32::LIGHT_BLUE))
                    .show(ui, |ui| {
                        egui::TextEdit::multiline(&mut text)
                            .id(egui::Id::new("canvas-text-editor"))
                            .event_filter(egui::EventFilter {
                                escape: true,
                                horizontal_arrows: true,
                                vertical_arrows: true,
                                ..Default::default()
                            })
                            .desired_width(width)
                            .min_size(Vec2::new(width, height))
                            .char_limit(100_000)
                            .layouter(&mut layouter)
                            .show(ui)
                    })
                    .inner;
                if self.focus {
                    output.response.request_focus();
                    self.focus = false;
                }
                if output.response.changed() {
                    rich_text::replace_text(&mut self.content, text);
                }
                if output.response.has_focus()
                    && let Some(range) = output.cursor_range
                {
                    let (a, b) = if range.primary.index < range.secondary.index {
                        (range.primary.index, range.secondary.index)
                    } else {
                        (range.secondary.index, range.primary.index)
                    };
                    let a: usize = a.into();
                    let b: usize = b.into();
                    if a != b && self.selection != (a..b) {
                        self.selection = a..b;
                        self.style = rich_text::style_at(&self.content, a);
                    }
                }
                // A separate footer prevents TextEdit's selection drag from taking the handle.
                let (_, footer) = ui.allocate_space(Vec2::new(width, 14.0));
                let handle = Rect::from_min_size(
                    footer.right_top() - Vec2::new(12.0, 0.0),
                    Vec2::splat(12.0),
                );
                ui.painter().rect_filled(handle, 0.0, Color32::LIGHT_BLUE);
                let response = ui.interact(handle, egui::Id::new("text-box-resize"), Sense::drag());
                if response.dragged() {
                    let delta = response.drag_delta();
                    self.resize([
                        (source_size[0] as f32 + delta.x / editor_zoom)
                            .round()
                            .clamp(1.0, 10000.0) as u32,
                        (source_size[1] as f32 + delta.y / (zoom * self.transform.scale[1].abs()))
                            .round()
                            .clamp(1.0, 10000.0) as u32,
                    ]);
                }
            });
        let ime = self.ime_active
            || ctx.input(|i| {
                i.raw
                    .events
                    .iter()
                    .any(|e| matches!(e, egui::Event::Ime(_)))
            });
        if !ime && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            decision = Some(false);
        }
        if !ime && ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter)) {
            decision = Some(true);
        }
        decision
    }
}
pub fn hit(document: &Document, point: [f32; 2]) -> Option<u64> {
    document
        .layers
        .iter()
        .rev()
        .find(|l| {
            l.visible
                && !l.group
                && ancestors_visible(document, l)
                && !document.is_locked(l.id)
                && matches!(l.content, Some(Content::Text { .. }))
                && {
                    let p = l.transform.to_local(
                        point,
                        l.offset,
                        [l.pixels.width(), l.pixels.height()],
                    );
                    p[0] >= 0.0
                        && p[1] >= 0.0
                        && p[0] < l.pixels.width() as f32
                        && p[1] < l.pixels.height() as f32
                }
        })
        .map(|l| l.id)
}

fn ancestors_visible(document: &Document, layer: &Layer) -> bool {
    let mut parent = layer.parent;
    for _ in 0..64 {
        let Some(id) = parent else {
            return true;
        };
        let Some(group) = document.layers.iter().find(|l| l.id == id) else {
            return false;
        };
        if !group.visible || group.opacity <= 0.0 {
            return false;
        }
        parent = group.parent;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn paragraph_reflow_preserves_transformed_origin_masks_clipping_and_history() {
        let mut editor = Editor::new(Document::new(640, 480, false).unwrap(), true);
        let mut session = Session::new(1, [51.25, 39.75], [250, 30, 40, 255]);
        rich_text::replace_text(
            &mut session.content,
            "편집 가능한 한글\n두 번째 문단".into(),
        );
        let mut style = rich_text::base(&session.content);
        style.bold = true;
        style.underline = true;
        style.size = 42.0;
        rich_text::apply_style(&mut session.content, 3..6, style);
        session.apply(&mut editor).unwrap();
        let layer = editor.document.active_layer_mut().unwrap();
        layer.transform = Transform {
            angle: 31.0,
            scale: [-1.25, 0.8],
            ..Default::default()
        };
        layer.mask = Some(Arc::new(image::GrayImage::from_fn(300, 160, |x, y| {
            image::Luma([if x + y < 40 { 0 } else { 220 }])
        })));
        layer.clipped = true;
        let before = editor.document.clone();
        let mut session = Session::existing(1, editor.document.active_layer().unwrap()).unwrap();
        let origin = session
            .transform
            .to_document([0.0, 0.0], session.offset, session.size());
        session.resize([180, 220]);
        let after = session
            .transform
            .to_document([0.0, 0.0], session.offset, session.size());
        for i in 0..2 {
            assert!((origin[i] - after[i]).abs() < 0.001);
        }
        rich_text::replace_text(
            &mut session.content,
            "편집 가능한 한글\n두 번째 문단 추가".into(),
        );
        session.apply(&mut editor).unwrap();
        let updated = editor.document.clone();
        let layer = updated.active_layer().unwrap();
        assert!(layer.clipped);
        assert_eq!(layer.mask.as_ref().unwrap().get_pixel(10, 10)[0], 0);
        assert_eq!(layer.mask.as_ref().unwrap().get_pixel(100, 100)[0], 220);
        assert_eq!(layer.mask.as_ref().unwrap().get_pixel(100, 200)[0], 255);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/text-vector");
        std::fs::create_dir_all(&dir).unwrap();
        crate::files::save_project(&updated, &dir.join("글자별-스타일.rphoto")).unwrap();
        let restored = crate::files::load_project(&dir.join("글자별-스타일.rphoto")).unwrap();
        assert_eq!(restored.active_layer().unwrap().content, layer.content);
        assert_eq!(restored.composite(), updated.composite());
        editor.undo();
        assert_eq!(
            editor.document.active_layer().unwrap().content,
            before.active_layer().unwrap().content
        );
        assert_eq!(editor.document.composite(), before.composite());
        editor.redo();
        assert_eq!(editor.document.composite(), updated.composite());
    }
    #[test]
    fn locked_invalid_and_unchanged_text_do_not_add_history() {
        let mut editor = Editor::new(Document::new(80, 60, true).unwrap(), true);
        let mut session = Session::new(1, [0.0; 2], [255; 4]);
        session.resize([80, 60]);
        rich_text::replace_text(&mut session.content, "한글".into());
        session.apply(&mut editor).unwrap();
        let mut existing = Session::existing(1, editor.document.active_layer().unwrap()).unwrap();
        let revision = editor.revision;
        assert!(!existing.apply(&mut editor).unwrap());
        assert_eq!(editor.revision, revision);
        editor.document.active_layer_mut().unwrap().locked = true;
        rich_text::replace_text(&mut existing.content, "수정".into());
        assert!(existing.apply(&mut editor).is_err());
        assert_eq!(editor.revision, revision);
        editor.document.active_layer_mut().unwrap().locked = false;
        if let Content::Text { spans, .. } = &mut existing.content {
            spans.push(rich_text::Span {
                start: 0,
                end: 10,
                style: existing.style.clone(),
            });
        }
        assert!(existing.apply(&mut editor).is_err());
        assert_eq!(editor.revision, revision);
    }
    #[test]
    fn hit_testing_respects_transforms_hidden_ancestors_and_locks() {
        let mut editor = Editor::new(Document::new(640, 480, false).unwrap(), true);
        let mut session = Session::new(1, [20.0, 30.0], [255; 4]);
        rich_text::replace_text(&mut session.content, "한국어".into());
        session.apply(&mut editor).unwrap();
        let id = editor.document.active;
        editor.document.active_layer_mut().unwrap().transform = Transform {
            angle: 45.0,
            scale: [1.0, -1.0],
            ..Default::default()
        };
        let layer = editor.document.active_layer().unwrap();
        let p = layer
            .transform
            .to_document([20.0, 20.0], layer.offset, [300, 160]);
        assert_eq!(hit(&editor.document, p), Some(id));
        editor.document.group_active().unwrap();
        editor.document.active_layer_mut().unwrap().visible = false;
        assert_eq!(hit(&editor.document, p), None);
        editor.document.active_layer_mut().unwrap().visible = true;
        editor.document.active_layer_mut().unwrap().locked = true;
        assert_eq!(hit(&editor.document, p), None);
    }
}
