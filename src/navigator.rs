use crate::canvas::View;
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
pub fn ui(
    ui: &mut egui::Ui,
    view: &mut View,
    area: Option<Rect>,
    texture: Option<&egui::TextureHandle>,
    size: [u32; 2],
    dpi: f32,
) -> Rect {
    ui.heading("내비게이터");
    let width = ui.available_width().max(1.);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 160.), Sense::click_and_drag());
    let scale = (width / size[0] as f32).min(rect.height() / size[1] as f32);
    let image = Rect::from_center_size(
        rect.center(),
        Vec2::new(size[0] as f32, size[1] as f32) * scale,
    );
    let painter = ui.painter().with_clip_rect(image.intersect(ui.clip_rect()));
    painter.rect_filled(rect, 0., Color32::from_gray(20));
    for y in 0..(image.height() / 8.).ceil() as i32 {
        for x in 0..(image.width() / 8.).ceil() as i32 {
            painter.rect_filled(
                Rect::from_min_size(
                    image.min + Vec2::new(x as f32 * 8., y as f32 * 8.),
                    Vec2::splat(8.),
                ),
                0.,
                Color32::from_gray(if (x + y) % 2 == 0 { 65 } else { 90 }),
            );
        }
    }
    if let Some(texture) = texture {
        painter.image(
            texture.id(),
            image,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
            Color32::WHITE,
        );
    }
    if (response.clicked() || response.dragged() || response.drag_stopped())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let p = (pointer - image.min) / scale;
        view.center_on(
            size,
            dpi,
            [p.x.clamp(0., size[0] as f32), p.y.clamp(0., size[1] as f32)],
        );
    }
    if let Some(area) = area {
        let visible = view.visible_document(area, size, dpi);
        if visible.is_positive() {
            painter.rect_stroke(
                Rect::from_min_max(
                    image.min + visible.min.to_vec2() * scale,
                    image.min + visible.max.to_vec2() * scale,
                ),
                0.,
                Stroke::new(1.5, Color32::from_rgb(110, 185, 255)),
                egui::StrokeKind::Inside,
            );
        }
        let old = view.zoom;
        let mut zoom = old * 100.;
        let mut next = None;
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("−").clicked() {
                next = Some(old / 1.25);
            }
            if ui
                .add(
                    egui::DragValue::new(&mut zoom)
                        .range(1.0..=3200.)
                        .speed(1.)
                        .suffix("%"),
                )
                .changed()
            {
                next = Some(zoom / 100.);
            }
            if ui.small_button("+").clicked() {
                next = Some(old * 1.25);
            }
            if ui.small_button("100%").clicked() {
                next = Some(1.);
            }
            if ui.small_button("화면 맞춤").clicked() {
                view.fit_to(area, size, dpi);
            }
        });
        if let Some(zoom) = next {
            view.zoom_at(area, area.center(), zoom / old);
        }
    } else {
        ui.label("캔버스 준비 중");
    }
    response.on_hover_text("클릭·드래그: 보고 싶은 위치로 이동");
    image
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn navigator_pointer_recenters_and_numeric_controls_do_not_change_zoom_without_input() {
        let ctx = egui::Context::default();
        let mut view = View {
            zoom: 2.,
            pan: Vec2::ZERO,
            fit: false,
        };
        let area = Rect::from_min_size(Pos2::new(80., 50.), Vec2::new(600., 400.));
        let mut time = 0.;
        fn draw(
            ctx: &egui::Context,
            view: &mut View,
            area: Rect,
            time: &mut f64,
            events: Vec<egui::Event>,
        ) -> Rect {
            *time += 0.4;
            let mut image = Rect::NOTHING;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    time: Some(*time),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(512.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    image = super::ui(ui, view, Some(area), None, [2000, 1000], 1.);
                },
            );
            out.textures_delta.clear();
            image
        }
        let image = draw(&ctx, &mut view, area, &mut time, vec![]);
        assert_eq!(view.zoom, 2.);
        let at = image.min + image.size() * Vec2::new(0.25, 0.75);
        draw(
            &ctx,
            &mut view,
            area,
            &mut time,
            vec![egui::Event::PointerMoved(at)],
        );
        draw(
            &ctx,
            &mut view,
            area,
            &mut time,
            vec![
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        let p = view.document_pos(area, [2000, 1000], 1., area.center());
        assert!(
            (p[0] - 500.).abs() < 0.001 && (p[1] - 750.).abs() < 0.001,
            "{p:?}"
        );
        assert_eq!(view.zoom, 2.);
    }
    #[test]
    fn center_visible_and_zoom_keep_document_coordinates_at_multiple_dpi() {
        let size = [2000, 1000];
        let area = Rect::from_min_size(Pos2::new(80., 70.), Vec2::new(600., 400.));
        for dpi in [1., 1.5, 2.] {
            let mut view = View {
                zoom: 2.,
                pan: Vec2::ZERO,
                fit: false,
            };
            view.center_on(size, dpi, [500., 250.]);
            let p = view.document_pos(area, size, dpi, area.center());
            assert!((p[0] - 500.).abs() < 0.001 && (p[1] - 250.).abs() < 0.001);
            let rect = view.visible_document(area, size, dpi);
            assert!((rect.width() - 300. * dpi).abs() < 0.001);
            view.zoom_at(area, area.center(), 1.25);
            let after = view.document_pos(area, size, dpi, area.center());
            assert!((after[0] - p[0]).abs() < 0.001 && (after[1] - p[1]).abs() < 0.001);
        }
    }
}
