use eframe::egui::{self, emath::Numeric};
use std::ops::RangeInclusive;

type Formatter<'a> = Box<dyn Fn(f64, RangeInclusive<usize>) -> String + 'a>;
#[derive(Clone, Copy, Default)]
struct Scrub {
    point: f32,
    value: f64,
}
pub struct Slider<'a, N: Numeric> {
    value: &'a mut N,
    range: RangeInclusive<N>,
    text: egui::WidgetText,
    logarithmic: bool,
    formatter: Option<Formatter<'a>>,
}
impl<'a, N: Numeric> Slider<'a, N> {
    pub fn new(value: &'a mut N, range: RangeInclusive<N>) -> Self {
        Self {
            value,
            range,
            text: "".into(),
            logarithmic: false,
            formatter: None,
        }
    }
    pub fn text(mut self, text: impl Into<egui::WidgetText>) -> Self {
        self.text = text.into();
        self
    }
    pub fn logarithmic(mut self, enabled: bool) -> Self {
        self.logarithmic = enabled;
        self
    }
    pub fn custom_formatter(
        mut self,
        formatter: impl Fn(f64, RangeInclusive<usize>) -> String + 'a,
    ) -> Self {
        self.formatter = Some(Box::new(formatter));
        self
    }
}
fn scrub_label<N: Numeric>(
    ui: &mut egui::Ui,
    value: &mut N,
    range: RangeInclusive<N>,
    text: egui::WidgetText,
    logarithmic: bool,
    speed: f64,
) -> egui::Response {
    let before = value.to_f64();
    let mut label = ui.add(egui::Label::new(text)
        .wrap_mode(egui::TextWrapMode::Extend)
        .selectable(false)
        .sense(egui::Sense::click_and_drag()))
        .on_hover_cursor(egui::CursorIcon::ResizeHorizontal)
        .on_hover_text("이름을 좌우로 드래그하면 값을 조절합니다. Shift는 10배, Ctrl은 100배 더 정밀하게 조절합니다. 클릭 후 방향키로도 조절할 수 있습니다.");
    let id = label.id.with("scrub-start");
    if label.is_pointer_button_down_on()
        && ui.data(|d| d.get_temp::<Scrub>(id)).is_none()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        ui.data_mut(|d| {
            d.insert_temp(
                id,
                Scrub {
                    point: origin.x,
                    value: value.to_f64(),
                },
            )
        });
    }
    if label.dragged()
        && ui.is_enabled()
        && let Some(now) = ui.input(|i| i.pointer.interact_pos())
        && let Some(mut scrub) = ui.data(|d| d.get_temp::<Scrub>(id))
    {
        let precision = ui.input(|i| {
            if i.modifiers.ctrl {
                0.01
            } else if i.modifiers.shift {
                0.1
            } else {
                1.
            }
        });
        let delta = f64::from(now.x - scrub.point) * precision;
        let lo = range.start().to_f64();
        let hi = range.end().to_f64();
        scrub.value = if logarithmic && lo > 0. && hi > lo {
            (scrub.value.clamp(lo, hi).ln() + delta / 200. * (hi.ln() - lo.ln()))
                .exp()
                .clamp(lo, hi)
        } else {
            (scrub.value + delta * speed).clamp(lo.min(hi), lo.max(hi))
        };
        scrub.point = now.x;
        *value = N::from_f64(if N::INTEGRAL {
            scrub.value.round()
        } else {
            scrub.value
        });
        ui.data_mut(|d| d.insert_temp(id, scrub));
    }
    if !label.is_pointer_button_down_on() {
        ui.data_mut(|d| d.remove_temp::<Scrub>(id));
    }
    if value.to_f64() != before {
        label.mark_changed();
    }
    label
}
impl<N: Numeric> egui::Widget for Slider<'_, N> {
    fn ui(mut self, ui: &mut egui::Ui) -> egui::Response {
        let before = self.value.to_f64();
        let inner = ui.horizontal(|ui| {
            let mut slider = egui::Slider::new(&mut *self.value, self.range.clone())
                .logarithmic(self.logarithmic);
            if let Some(formatter) = self.formatter.take() {
                slider = slider.custom_formatter(formatter);
            }
            let mut response = ui.add(slider);
            if !self.text.is_empty() {
                let speed = (self.range.end().to_f64() - self.range.start().to_f64()) / 200.;
                let label = scrub_label(
                    ui,
                    self.value,
                    self.range,
                    self.text,
                    self.logarithmic,
                    speed,
                );
                response = response.labelled_by(label.id);
                if label.clicked() {
                    response.request_focus();
                }
                response = response.union(label);
            }
            if self.value.to_f64() != before {
                response.mark_changed();
            }
            response
        });
        inner.inner | inner.response
    }
}
pub fn labeled_drag<N: Numeric>(
    ui: &mut egui::Ui,
    text: impl Into<egui::WidgetText>,
    value: &mut N,
    range: RangeInclusive<N>,
    speed: f64,
    suffix: &str,
) -> egui::Response {
    let inner = ui.horizontal(|ui| {
        let label = scrub_label(ui, value, range.clone(), text.into(), false, speed);
        let response = ui
            .add(
                egui::DragValue::new(value)
                    .range(range)
                    .speed(speed)
                    .suffix(suffix),
            )
            .labelled_by(label.id);
        if label.clicked() {
            response.request_focus();
        }
        response.union(label)
    });
    inner.inner | inner.response
}
#[cfg(test)]
#[path = "numeric_slider_tests.rs"]
mod tests;
