use eframe::egui::{self, Color32, Pos2, Rect, Response, Stroke, Ui, Vec2};

#[derive(Clone, Copy)]
pub enum Icon {
    Move,
    Selection,
    Brush,
    Eraser,
    Eyedropper,
    Hand,
    Zoom,
    Crop,
    Heal,
    Stamp,
    Shapes,
    Text,
    DefaultColors,
    Ellipse,
    Lasso,
    Wand,
    Blur,
    Gradient,
    Fill,
    Pen,
}

pub fn tool_button(
    ui: &mut Ui,
    icon: Icon,
    label: &str,
    selected: bool,
    enabled: bool,
) -> Response {
    ui.push_id(label, |ui| {
        let response = ui.add_enabled(
            enabled,
            egui::Button::new("")
                .min_size(Vec2::new(42.0, 36.0))
                .corner_radius(5.0)
                .selected(selected),
        );
        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Button,
                response.enabled(),
                selected,
                label,
            )
        });
        if ui.is_rect_visible(response.rect) {
            let color = if !response.enabled() {
                Color32::from_rgb(87, 94, 107)
            } else if selected {
                Color32::from_rgb(150, 197, 255)
            } else if response.hovered() {
                Color32::WHITE
            } else {
                Color32::from_rgb(207, 214, 227)
            };
            draw_icon(ui.painter(), icon, response.rect.center(), color);
        }
        response
    })
    .inner
}

// Geometry uses a 24-point grid, so icons stay sharp at every display scale.
fn draw_icon(painter: &egui::Painter, icon: Icon, center: Pos2, color: Color32) {
    let origin = center - Vec2::splat(12.0);
    let point = |x: f32, y: f32| origin + Vec2::new(x, y);
    let stroke = Stroke::new(1.7, color);
    let line = |a: [f32; 2], b: [f32; 2]| {
        painter.line_segment([point(a[0], a[1]), point(b[0], b[1])], stroke);
    };
    let path = |points: &[[f32; 2]]| {
        painter.add(egui::Shape::line(
            points.iter().map(|p| point(p[0], p[1])).collect(),
            stroke,
        ));
    };
    let rect = |min: [f32; 2], max: [f32; 2]| {
        painter.rect_stroke(
            Rect::from_min_max(point(min[0], min[1]), point(max[0], max[1])),
            1.0,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    match icon {
        Icon::Pen => {
            path(&[
                [4.0, 20.0],
                [7.0, 9.0],
                [17.0, 3.0],
                [21.0, 7.0],
                [15.0, 17.0],
                [4.0, 20.0],
            ]);
            line([4.0, 20.0], [12.0, 12.0]);
            painter.circle_stroke(point(13.0, 11.0), 2.0, stroke);
        }
        Icon::Ellipse => {
            let points = (0..=24)
                .map(|i| {
                    let angle = i as f32 * std::f32::consts::TAU / 24.0;
                    point(12.0 + angle.cos() * 9.0, 12.0 + angle.sin() * 7.0)
                })
                .collect();
            painter.add(egui::Shape::line(points, stroke));
        }
        Icon::Lasso => {
            path(&[
                [5.0, 7.0],
                [10.0, 3.0],
                [18.0, 4.0],
                [21.0, 10.0],
                [19.0, 16.0],
                [12.0, 18.0],
                [5.0, 15.0],
                [3.0, 10.0],
                [5.0, 7.0],
            ]);
            path(&[[7.0, 15.0], [7.0, 21.0], [11.0, 22.0]]);
        }
        Icon::Wand => {
            line([4.0, 20.0], [18.0, 6.0]);
            line([12.0, 6.0], [20.0, 6.0]);
            line([16.0, 2.0], [16.0, 10.0]);
            line([4.0, 4.0], [8.0, 4.0]);
            line([6.0, 2.0], [6.0, 6.0]);
        }
        Icon::Blur => {
            path(&[
                [12.0, 3.0],
                [5.0, 13.0],
                [4.0, 17.0],
                [7.0, 21.0],
                [17.0, 21.0],
                [20.0, 17.0],
                [19.0, 13.0],
                [12.0, 3.0],
            ]);
        }
        Icon::Gradient => {
            rect([3.0, 4.0], [21.0, 20.0]);
            for x in [6.0, 9.0, 12.0, 15.0, 18.0] {
                painter.line_segment(
                    [point(x, 6.0), point(x, 18.0)],
                    Stroke::new(1.7, color.gamma_multiply((x - 3.0) / 18.0)),
                );
            }
        }
        Icon::Fill => {
            path(&[
                [4.0, 12.0],
                [12.0, 4.0],
                [20.0, 12.0],
                [12.0, 20.0],
                [4.0, 12.0],
            ]);
            line([5.0, 12.0], [19.0, 12.0]);
            line([8.0, 2.0], [14.0, 8.0]);
            painter.circle_filled(point(21.0, 20.0), 2.0, color);
        }
        Icon::Move => {
            line([12.0, 3.0], [12.0, 21.0]);
            line([3.0, 12.0], [21.0, 12.0]);
            for points in [
                [[9.0, 6.0], [12.0, 3.0], [15.0, 6.0]],
                [[9.0, 18.0], [12.0, 21.0], [15.0, 18.0]],
                [[6.0, 9.0], [3.0, 12.0], [6.0, 15.0]],
                [[18.0, 9.0], [21.0, 12.0], [18.0, 15.0]],
            ] {
                path(&points);
            }
        }
        Icon::Selection => {
            for start in [4.0, 10.0, 16.0] {
                for edge in [4.0, 20.0] {
                    line([start, edge], [start + 4.0, edge]);
                    line([edge, start], [edge, start + 4.0]);
                }
            }
        }
        Icon::Brush => {
            path(&[
                [9.0, 13.0],
                [18.0, 3.0],
                [21.0, 6.0],
                [12.0, 16.0],
                [9.0, 13.0],
            ]);
            path(&[
                [9.0, 14.0],
                [6.0, 15.0],
                [5.0, 19.0],
                [3.0, 21.0],
                [8.0, 21.0],
                [11.0, 19.0],
                [11.0, 16.0],
            ]);
        }
        Icon::Eraser => {
            path(&[
                [3.0, 15.0],
                [14.0, 4.0],
                [21.0, 11.0],
                [12.0, 20.0],
                [8.0, 20.0],
                [3.0, 15.0],
            ]);
            line([8.0, 10.0], [15.0, 17.0]);
            line([13.0, 20.0], [22.0, 20.0]);
        }
        Icon::Eyedropper => {
            path(&[
                [5.0, 16.0],
                [15.0, 6.0],
                [18.0, 9.0],
                [8.0, 19.0],
                [4.0, 20.0],
                [5.0, 16.0],
            ]);
            line([13.0, 5.0], [19.0, 11.0]);
            path(&[[16.0, 6.0], [19.0, 3.0], [22.0, 6.0], [19.0, 9.0]]);
        }
        Icon::Hand => {
            path(&[
                [6.0, 12.0],
                [6.0, 5.0],
                [7.5, 4.0],
                [9.0, 5.0],
                [9.0, 11.0],
                [9.0, 3.0],
                [10.5, 2.0],
                [12.0, 3.0],
                [12.0, 11.0],
                [12.0, 4.0],
                [13.5, 3.0],
                [15.0, 4.0],
                [15.0, 12.0],
                [15.0, 7.0],
                [16.5, 6.0],
                [18.0, 7.0],
                [18.0, 16.0],
                [16.0, 21.0],
                [9.0, 21.0],
                [4.0, 15.0],
                [3.0, 12.0],
                [4.5, 11.0],
                [6.0, 12.0],
            ]);
        }
        Icon::Zoom => {
            painter.circle_stroke(point(10.0, 10.0), 6.5, stroke);
            line([15.0, 15.0], [21.0, 21.0]);
            line([7.0, 10.0], [13.0, 10.0]);
            line([10.0, 7.0], [10.0, 13.0]);
        }
        Icon::Crop => {
            path(&[[7.0, 2.0], [7.0, 17.0], [22.0, 17.0]]);
            path(&[[2.0, 7.0], [17.0, 7.0], [17.0, 22.0]]);
            line([10.0, 14.0], [21.0, 3.0]);
        }
        Icon::Heal => {
            path(&[
                [4.0, 13.0],
                [13.0, 4.0],
                [17.0, 3.0],
                [21.0, 7.0],
                [20.0, 11.0],
                [11.0, 20.0],
                [7.0, 21.0],
                [3.0, 17.0],
                [4.0, 13.0],
            ]);
            line([8.0, 9.0], [15.0, 16.0]);
            line([9.0, 8.0], [16.0, 15.0]);
            for (x, y) in [
                (12.0, 8.0),
                (16.0, 8.0),
                (16.0, 12.0),
                (8.0, 12.0),
                (8.0, 16.0),
                (12.0, 16.0),
            ] {
                painter.circle_filled(point(x, y), 0.75, color);
            }
        }
        Icon::Stamp => {
            path(&[
                [5.0, 17.0],
                [5.0, 14.0],
                [9.0, 12.0],
                [9.0, 9.0],
                [8.0, 6.0],
                [9.0, 3.0],
                [15.0, 3.0],
                [16.0, 6.0],
                [15.0, 9.0],
                [15.0, 12.0],
                [19.0, 14.0],
                [19.0, 17.0],
                [5.0, 17.0],
            ]);
            rect([3.0, 17.0], [21.0, 21.0]);
        }
        Icon::Shapes => {
            rect([3.0, 3.0], [14.0, 14.0]);
            painter.circle_stroke(point(16.0, 16.0), 5.0, stroke);
        }
        Icon::Text => {
            path(&[[4.0, 7.0], [4.0, 4.0], [20.0, 4.0], [20.0, 7.0]]);
            line([12.0, 4.0], [12.0, 20.0]);
            line([8.0, 20.0], [16.0, 20.0]);
        }
        Icon::DefaultColors => {
            painter.rect_filled(
                Rect::from_min_max(point(10.0, 10.0), point(20.0, 20.0)),
                1.0,
                Color32::WHITE,
            );
            painter.rect_filled(
                Rect::from_min_max(point(4.0, 4.0), point(14.0, 14.0)),
                1.0,
                Color32::BLACK,
            );
            rect([4.0, 4.0], [14.0, 14.0]);
        }
    }
}
