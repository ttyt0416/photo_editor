use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Grade {
    pub hue: f32,
    pub saturation: f32,
    pub luminance: f32,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ColorGrading {
    pub tones: [Grade; 4],
    pub blending: f32,
    pub balance: f32,
}
impl Default for ColorGrading {
    fn default() -> Self {
        Self {
            tones: std::array::from_fn(|_| Grade::default()),
            blending: 50.,
            balance: 0.,
        }
    }
}
pub const NAMES: [&str; 4] = ["그림자", "중간톤", "밝은 영역", "전체 톤"];
fn lum(c: [f64; 3]) -> f64 {
    c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
}
fn basis(h: f64) -> [f64; 3] {
    let h = h.rem_euclid(360.) / 60.;
    let x = 1. - (h % 2. - 1.).abs();
    match h as u8 {
        0 => [1., x, 0.],
        1 => [x, 1., 0.],
        2 => [0., 1., x],
        3 => [0., x, 1.],
        4 => [x, 0., 1.],
        _ => [1., 0., x],
    }
}
impl ColorGrading {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    pub fn enabled(&self) -> bool {
        self.tones
            .iter()
            .any(|g| g.saturation != 0. || g.luminance != 0.)
    }
    pub fn valid(&self) -> bool {
        self.blending.is_finite()
            && (0.0..=100.).contains(&self.blending)
            && self.balance.is_finite()
            && (-100.0..=100.).contains(&self.balance)
            && self.tones.iter().all(|g| {
                g.hue.is_finite()
                    && (0.0..=360.).contains(&g.hue)
                    && g.saturation.is_finite()
                    && (0.0..=1.).contains(&g.saturation)
                    && g.luminance.is_finite()
                    && (-1.0..=1.).contains(&g.luminance)
            })
    }
    pub fn weights(&self, y: f64) -> [f64; 3] {
        let t = (y + f64::from(self.balance) * 0.0025).clamp(0., 1.);
        let p = 4. - f64::from(self.blending) * 0.03;
        let w = [
            (1. - t).powf(p),
            (1. - (2. * t - 1.).abs()).max(0.).powf(p),
            t.powf(p),
        ];
        let total = w.iter().sum::<f64>();
        w.map(|v| v / total)
    }
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        if !self.enabled() {
            return rgb;
        }
        let c = rgb.map(f64::from);
        let y = lum(c);
        let w = self.weights(y);
        let mut centered = c.map(|v| v - y);
        let mut brightness = 0.;
        for (i, g) in self.tones.iter().enumerate() {
            let weight = if i == 3 { 1. } else { w[i] };
            let color = basis(f64::from(g.hue));
            let mean = lum(color);
            let strength = y.abs() * 0.5 * f64::from(g.saturation) * weight;
            for k in 0..3 {
                centered[k] += (color[k] - mean) * strength;
            }
            if i < 3 {
                brightness += f64::from(g.luminance) * weight;
            }
        }
        // Work in the current numerical RGB space. Preserve luminosity for tinting,
        // and keep extended HDR/negative values without a display-range clamp.
        if c.iter().all(|v| (0.0..=1.).contains(v)) {
            let mut scale = 1f64;
            for v in centered {
                if v > 0. {
                    scale = scale.min((1. - y) / v);
                } else if v < 0. {
                    scale = scale.min(-y / v);
                }
            }
            centered = centered.map(|v| v * scale.clamp(0., 1.));
        }
        let gain = (1. + brightness) * (1. + f64::from(self.tones[3].luminance));
        centered.map(|v| ((y + v) * gain).clamp(-f64::from(f32::MAX), f64::from(f32::MAX)) as f32)
    }
    pub fn ui(&mut self, ui: &mut eframe::egui::Ui) {
        ui.collapsing("컬러 그레이딩",|ui| {
   let id=ui.id().with("grading-tone");let mut index=ui.data(|d|d.get_temp::<usize>(id)).unwrap_or(0).min(3);
   ui.horizontal_wrapped(|ui| {for (i,name) in NAMES.iter().enumerate() {if ui.selectable_label(index==i,*name).clicked(){index=i;}}});ui.data_mut(|d|d.insert_temp(id,index));
   ui.horizontal_wrapped(|ui| {wheel(ui,&mut self.tones[index]);ui.vertical(|ui| {ui.add(crate::numeric_slider::Slider::new(&mut self.tones[index].hue,0.0..=360.).text("톤 색조"));ui.add(crate::numeric_slider::Slider::new(&mut self.tones[index].saturation,0.0..=1.).text("물들이기 강도"));ui.add(crate::numeric_slider::Slider::new(&mut self.tones[index].luminance,-1.0..=1.).text("톤 밝기"));});});
   ui.add(crate::numeric_slider::Slider::new(&mut self.blending,0.0..=100.).text("톤 혼합"));ui.add(crate::numeric_slider::Slider::new(&mut self.balance,-100.0..=100.).text("톤 균형"));
   ui.label("색상환의 방향은 색조, 중심에서 거리는 강도입니다. 혼합은 톤 사이 겹침, 양의 균형은 밝은 영역의 영향을 늘립니다.");
   if ui.button("이 톤 초기화").clicked(){self.tones[index]=Grade::default();}
   if ui.button("그레이딩 전체 초기화").clicked(){*self=Self::default();}
  });
    }
}
fn wheel(ui: &mut eframe::egui::Ui, g: &mut Grade) -> eframe::egui::Response {
    use eframe::egui::{self, Color32, Vec2};
    let (rect, mut response) =
        ui.allocate_exact_size(Vec2::splat(96.), egui::Sense::click_and_drag());
    let center = rect.center();
    let radius = 44.;
    if (response.clicked() || response.dragged())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let v = pos - center;
        g.saturation = (v.length() / radius).clamp(0., 1.);
        if v.length() > 0.01 {
            g.hue = v.y.atan2(v.x).to_degrees().rem_euclid(360.);
        }
        response.mark_changed();
    }
    let mut mesh = egui::Mesh::default();
    for i in 0..128 {
        let h = i as f32 * 360. / 128.;
        let next = (i + 1) as f32 * 360. / 128.;
        let base = mesh.vertices.len() as u32;
        for (p, color) in [
            (center, Color32::WHITE),
            (
                center + Vec2::angled(h.to_radians()) * radius,
                Color32::from_rgb(
                    (basis(h as f64)[0] * 255.) as u8,
                    (basis(h as f64)[1] * 255.) as u8,
                    (basis(h as f64)[2] * 255.) as u8,
                ),
            ),
            (
                center + Vec2::angled(next.to_radians()) * radius,
                Color32::from_rgb(
                    (basis(next as f64)[0] * 255.) as u8,
                    (basis(next as f64)[1] * 255.) as u8,
                    (basis(next as f64)[2] * 255.) as u8,
                ),
            ),
        ] {
            mesh.colored_vertex(p, color);
        }
        mesh.add_triangle(base, base + 1, base + 2);
    }
    ui.painter().add(egui::Shape::mesh(mesh));
    ui.painter().circle_stroke(
        center,
        radius,
        egui::Stroke::new(1., ui.visuals().text_color()),
    );
    let marker = center + Vec2::angled(g.hue.to_radians()) * radius * g.saturation;
    ui.painter()
        .circle_stroke(marker, 4., egui::Stroke::new(3., Color32::BLACK));
    ui.painter()
        .circle_stroke(marker, 4., egui::Stroke::new(1., Color32::WHITE));
    response.on_hover_text("색상환을 클릭하거나 드래그해 색조와 강도를 조절합니다.")
}
#[cfg(test)]
#[path = "color_grading_tests.rs"]
mod tests;
