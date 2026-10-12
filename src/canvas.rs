use eframe::egui::{Pos2, Rect, Vec2};

pub struct View {
    pub zoom: f32,
    pub pan: Vec2,
    pub fit: bool,
}

impl Default for View {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit: true,
        }
    }
}

impl View {
    pub fn image_rect(&self, area: Rect, size: [u32; 2], pixels_per_point: f32) -> Rect {
        Rect::from_center_size(
            area.center() + self.pan,
            Vec2::new(size[0] as f32, size[1] as f32) * self.zoom / pixels_per_point,
        )
    }

    pub fn document_pos(
        &self,
        area: Rect,
        size: [u32; 2],
        pixels_per_point: f32,
        screen: Pos2,
    ) -> [f32; 2] {
        let point = (screen - self.image_rect(area, size, pixels_per_point).min) * pixels_per_point
            / self.zoom;
        [point.x, point.y]
    }

    pub fn center_on(&mut self, size: [u32; 2], pixels_per_point: f32, point: [f32; 2]) {
        self.pan = Vec2::new(
            size[0] as f32 / 2. - point[0],
            size[1] as f32 / 2. - point[1],
        ) * self.zoom
            / pixels_per_point;
        self.fit = false;
    }
    pub fn visible_document(&self, area: Rect, size: [u32; 2], pixels_per_point: f32) -> Rect {
        let min = self.document_pos(area, size, pixels_per_point, area.min);
        let max = self.document_pos(area, size, pixels_per_point, area.max);
        Rect::from_min_max(Pos2::new(min[0], min[1]), Pos2::new(max[0], max[1])).intersect(
            Rect::from_min_size(Pos2::ZERO, Vec2::new(size[0] as f32, size[1] as f32)),
        )
    }
    pub fn fit_to(&mut self, area: Rect, size: [u32; 2], pixels_per_point: f32) {
        self.zoom = ((area.width() - 64.0).max(1.0) / size[0] as f32)
            .min((area.height() - 64.0).max(1.0) / size[1] as f32)
            * pixels_per_point;
        self.zoom = self.zoom.clamp(0.01, 32.0);
        self.pan = Vec2::ZERO;
        self.fit = false;
    }

    pub fn zoom_at(&mut self, area: Rect, pointer: Pos2, factor: f32) {
        let old = self.zoom;
        self.zoom = (self.zoom * factor).clamp(0.01, 32.0);
        self.pan =
            pointer - area.center() - (pointer - area.center() - self.pan) * (self.zoom / old);
        self.fit = false;
    }
}

pub fn handle_cursor(
    handle: crate::transform::Handle,
    corners: [Pos2; 4],
    down: bool,
) -> eframe::egui::CursorIcon {
    use crate::transform::Handle;
    use eframe::egui::CursorIcon;
    match handle {
        Handle::Rotate => {
            if down {
                CursorIcon::Grabbing
            } else {
                CursorIcon::Grab
            }
        }
        Handle::Move => CursorIcon::Move,
        Handle::Resize([x, y]) => {
            let x_axis = (corners[1] - corners[0]).normalized();
            let y_axis = (corners[3] - corners[0]).normalized();
            let direction = x_axis * (x as f32 - 1.) + y_axis * (y as f32 - 1.);
            if !direction.is_finite() || direction.length_sq() < 1e-6 {
                return CursorIcon::Crosshair;
            }
            match ((direction.angle() / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(4) {
                0 => CursorIcon::ResizeHorizontal,
                1 => CursorIcon::ResizeNwSe,
                2 => CursorIcon::ResizeVertical,
                _ => CursorIcon::ResizeNeSw,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resize_cursor_tracks_rotated_and_flipped_geometry() {
        use crate::transform::Handle;
        use eframe::egui::CursorIcon;
        let corners = [
            Pos2::new(0., 0.),
            Pos2::new(200., 0.),
            Pos2::new(200., 30.),
            Pos2::new(0., 30.),
        ];
        assert_eq!(
            handle_cursor(Handle::Resize([2, 1]), corners, false),
            CursorIcon::ResizeHorizontal
        );
        assert_eq!(
            handle_cursor(Handle::Resize([2, 2]), corners, false),
            CursorIcon::ResizeNwSe
        );
        let rotated = corners.map(|p| Pos2::new(-p.y, p.x));
        assert_eq!(
            handle_cursor(Handle::Resize([2, 1]), rotated, false),
            CursorIcon::ResizeVertical
        );
        let flipped = corners.map(|p| Pos2::new(-p.x, p.y));
        assert_eq!(
            handle_cursor(Handle::Resize([2, 2]), flipped, false),
            CursorIcon::ResizeNeSw
        );
        assert_eq!(
            handle_cursor(Handle::Rotate, corners, true),
            CursorIcon::Grabbing
        );
    }
    #[test]
    fn cursor_zoom_keeps_document_point_at_multiple_dpi_scales() {
        let area = Rect::from_min_size(Pos2::new(50.0, 70.0), Vec2::new(800.0, 600.0));
        for dpi in [1.0, 1.5, 2.0] {
            let mut view = View {
                zoom: 0.7,
                pan: Vec2::new(31.0, -22.0),
                fit: false,
            };
            let pointer = Pos2::new(230.0, 170.0);
            let before = view.document_pos(area, [1920, 1080], dpi, pointer);
            view.zoom_at(area, pointer, 1.6);
            let after = view.document_pos(area, [1920, 1080], dpi, pointer);
            assert!((before[0] - after[0]).abs() < 0.001);
            assert!((before[1] - after[1]).abs() < 0.001);
        }
    }
}
