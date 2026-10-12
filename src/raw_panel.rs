use crate::raw_input::{Settings, Source, WhiteBalance};
use eframe::egui;
use image::Rgba32FImage;
use std::{
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};
struct Prepared {
    pixels: Arc<Rgba32FImage>,
    preview: image::RgbaImage,
    shown: Option<Arc<Rgba32FImage>>,
}
type Developed = Result<Prepared, String>;
pub enum Action {
    Apply(
        Arc<Rgba32FImage>,
        Settings,
        Box<crate::adjustments::Adjustments>,
    ),
    Cancel,
}
#[derive(Clone, PartialEq)]
struct PreviewKey {
    settings: Settings,
    adjustments: crate::adjustments::Adjustments,
    picking: bool,
}
fn prepare(pixels: Arc<Rgba32FImage>, key: &PreviewKey) -> Developed {
    if !key.adjustments.valid() {
        return Err("보정 값이 유효하지 않습니다.".into());
    }
    let shown = if key.picking || !key.adjustments.active() {
        None
    } else {
        let (mut image, pad) = key.adjustments.apply_expanded(&pixels)?;
        if pad != 0 {
            image = image::imageops::crop_imm(
                &image,
                pad as u32,
                pad as u32,
                pixels.width(),
                pixels.height(),
            )
            .to_image();
        }
        Some(Arc::new(image))
    };
    let preview = crate::raw_preview::thumbnail(shown.as_ref().unwrap_or(&pixels));
    Ok(Prepared {
        pixels,
        preview,
        shown,
    })
}
pub struct Panel {
    pub tab: u64,
    pub layer: u64,
    pub revision: u64,
    pub source: Arc<Source>,
    pub settings: Settings,
    pub adjustments: crate::adjustments::Adjustments,
    original_adjustments: crate::adjustments::Adjustments,
    original_pixels: Arc<Rgba32FImage>,
    advanced: bool,
    curve_channel: usize,
    curve_drag: Option<usize>,
    result: Option<(PreviewKey, Developed)>,
    job: Option<(PreviewKey, mpsc::Receiver<Developed>, bool)>,
    changed_at: Instant,
    displayed_key: PreviewKey,
    view: crate::raw_preview::View,
    picker: white_balance_picker::Picker,
    raw_preview_settings: Option<Settings>,
}
impl Panel {
    pub fn new(
        tab: u64,
        layer: u64,
        revision: u64,
        source: Arc<Source>,
        current: Arc<Rgba32FImage>,
        ctx: &egui::Context,
    ) -> Self {
        let settings = source.record.settings.clone();
        let key = PreviewKey {
            settings: settings.clone(),
            adjustments: Default::default(),
            picking: false,
        };
        let (sender, receiver) = mpsc::channel();
        let original = current.clone();
        std::thread::spawn(move || {
            let preview = crate::raw_preview::thumbnail(&original);
            let _ = sender.send(Ok(Prepared {
                pixels: original,
                preview,
                shown: None,
            }));
        });
        ctx.request_repaint();
        Self {
            settings: source.record.settings.clone(),
            tab,
            layer,
            revision,
            source,
            result: None,
            displayed_key: key.clone(),
            job: Some((key, receiver, true)),
            adjustments: Default::default(),
            original_adjustments: Default::default(),
            original_pixels: current.clone(),
            advanced: false,
            curve_channel: 0,
            curve_drag: None,
            changed_at: Instant::now(),
            view: crate::raw_preview::View::new(current),
            picker: white_balance_picker::Picker::new(),
            raw_preview_settings: None,
        }
    }
    pub fn for_layer(
        tab: u64,
        revision: u64,
        layer: &crate::document::Layer,
        ctx: &egui::Context,
    ) -> Option<Self> {
        let source = layer.raw_source.clone()?;
        let pixels = layer.precision.clone().unwrap_or_else(|| {
            Arc::new(image::DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f())
        });
        let mut panel = Self::new(tab, layer.id, revision, source, pixels, ctx);
        panel.adjustments = layer.adjustments.clone();
        panel.original_adjustments = layer.adjustments.clone();
        panel.displayed_key = panel.key();
        panel.advanced = true;
        Some(panel)
    }
    fn key(&self) -> PreviewKey {
        PreviewKey {
            settings: self.settings.clone(),
            adjustments: self.adjustments.clone(),
            picking: self.picker.enabled,
        }
    }
    pub fn ready(&self) -> bool {
        !self.picker.busy()
            && self.settings.valid()
            && self.adjustments.valid()
            && (self.settings != self.source.record.settings
                || self.adjustments != self.original_adjustments)
            && self
                .result
                .as_ref()
                .is_some_and(|(key, r)| key == &self.key() && r.is_ok())
    }
    fn invalidate_changed_settings(&mut self) {
        let key = self.key();
        if self.displayed_key == key {
            return;
        }
        self.displayed_key = key;
        self.result = None;
        self.raw_preview_settings = None;
        self.changed_at = Instant::now();
        if self.settings == self.source.record.settings && !self.adjustments.active() {
            self.view.restore_before();
        } else {
            self.view.clear_after();
        }
    }
    fn needs_pick_preview(&self) -> bool {
        self.picker.enabled
            && self.raw_preview_settings.as_ref() != Some(&self.settings)
            && !self
                .result
                .as_ref()
                .is_some_and(|(key, r)| key == &self.key() && r.is_err())
    }
    pub fn ui(&mut self, ctx: &egui::Context) -> Option<Action> {
        self.picker.poll(&self.source, &mut self.settings, ctx);
        self.invalidate_changed_settings();
        if let Some((settings, receiver, initial)) = &self.job {
            match receiver.try_recv() {
                Ok(result) => {
                    let settings = settings.clone();
                    let initial = *initial;
                    self.job = None;
                    if initial && let Ok(prepared) = &result {
                        self.view.before_ready(ctx, &prepared.preview);
                    }
                    if settings == self.key() {
                        if let Ok(prepared) = &result {
                            if !initial {
                                self.raw_preview_settings = Some(settings.settings.clone());
                            }
                            self.view.after_ready(
                                ctx,
                                prepared
                                    .shown
                                    .clone()
                                    .unwrap_or_else(|| prepared.pixels.clone()),
                                &prepared.preview,
                            );
                        }
                        self.result = Some((settings, result));
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    let settings = settings.clone();
                    self.job = None;
                    self.result = Some((settings, Err("RAW 현상 작업이 중단됐습니다.".into())));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let needs_pick_preview = self.needs_pick_preview();
        if self.job.is_none()
            && (needs_pick_preview
                || self
                    .result
                    .as_ref()
                    .is_none_or(|(key, _)| key != &self.key()))
            && self.changed_at.elapsed() >= Duration::from_millis(200)
            && self.settings.valid()
            && self.adjustments.valid()
        {
            let source = self.source.clone();
            let key = self.key();
            let (sender, receiver) = mpsc::channel();
            let current = key.clone();
            let original = self.original_pixels.clone();
            std::thread::spawn(move || {
                let pixels = if !current.picking && current.settings == source.record.settings {
                    Ok(original)
                } else {
                    source.develop(&current.settings).map(Arc::new)
                };
                let _ = sender.send(pixels.and_then(|pixels| prepare(pixels, &current)));
            });
            self.job = Some((key, receiver, false));
        }
        let mut action = None;
        let wide = self.advanced && ctx.content_rect().width() >= 1100.;
        let width = (ctx.content_rect().width() - 48.).clamp(160., if wide { 1120. } else { 460. });
        let height =
            (ctx.content_rect().height() - 48.).clamp(180., if wide { 760. } else { 560. });
        let body_height = if wide {
            (height - 90.).max(96.)
        } else {
            (ctx.content_rect().height() - 140.).max(96.)
        };
        egui::Modal::new(egui::Id::new("raw-develop")).show(ctx, |ui| {
            ui.set_width(width);
            ui.set_height(height);
            ui.heading("RAW 현상");
            if wide {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(360., body_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.set_max_width(360.);
                            egui::ScrollArea::vertical()
                                .id_salt("raw-sidebar")
                                .auto_shrink([false, false])
                                .max_height(body_height)
                                .show(ui, |ui| {
                                    self.basic_ui(ui);
                                    ui.separator();
                                    ui.heading("현상 보정");
                                    self.advanced_ui(ui);
                                    self.validation_ui(ui);
                                });
                        },
                    );
                    ui.separator();
                    ui.allocate_ui_with_layout(
                        egui::vec2((width - 392.).max(160.), body_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("raw-wide-preview")
                                .auto_shrink([false, false])
                                .max_height(body_height)
                                .show(ui, |ui| {
                                    self.preview_ui(ui, (body_height * 0.4).clamp(96., 300.))
                                });
                        },
                    );
                });
            } else {
                egui::ScrollArea::both()
                    .max_width(width)
                    .id_salt("raw-settings")
                    .auto_shrink([false, false])
                    .max_height(body_height)
                    .show(ui, |ui| {
                        self.basic_ui(ui);
                        self.preview_ui(ui, 180.);
                        if self.advanced {
                            ui.collapsing("고급 현상 보정", |ui| self.advanced_ui(ui));
                        }
                        self.validation_ui(ui);
                    });
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.ready(), egui::Button::new("적용"))
                    .clicked()
                    && let Some((_, Ok(pixels))) = &self.result
                {
                    action = Some(Action::Apply(
                        pixels.pixels.clone(),
                        self.settings.clone(),
                        Box::new(self.adjustments.clone()),
                    ));
                }
                if ui.button("취소").clicked() {
                    action = Some(Action::Cancel);
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                action = Some(Action::Cancel);
            }
        });
        if self.job.is_some()
            || self.picker.busy()
            || self.needs_pick_preview()
            || self
                .result
                .as_ref()
                .is_none_or(|(key, _)| key != &self.key())
        {
            ctx.request_repaint_after(Duration::from_millis(30));
        }
        action
    }
    fn basic_ui(&mut self, ui: &mut egui::Ui) {
        let info = &self.source.record.info;
        ui.label(format!(
            "{} {} · {}비트 · {} × {}",
            info.make, info.model, info.bits, info.sensor[0], info.sensor[1]
        ));
        ui.label(&self.source.record.filename);
        if info.camera_wb_fourth.is_some() {
            ui.label("4채널 카메라 센서 · 네 번째 채널은 투명도가 아닙니다.");
        }
        self.source.record.info.shooting.ui(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label("화이트 밸런스");
            egui::ComboBox::from_id_salt("raw-wb")
                .selected_text(match self.settings.white_balance {
                    WhiteBalance::Camera => "촬영값",
                    WhiteBalance::Neutral => "중립",
                    WhiteBalance::Custom => "사용자 지정",
                    WhiteBalance::Temperature => "색온도·틴트",
                })
                .show_ui(ui, |ui| {
                    for (mode, label) in [
                        (WhiteBalance::Camera, "촬영값"),
                        (WhiteBalance::Neutral, "중립"),
                        (WhiteBalance::Custom, "사용자 지정"),
                        (WhiteBalance::Temperature, "색온도·틴트"),
                    ] {
                        ui.selectable_value(&mut self.settings.white_balance, mode, label);
                    }
                });
        });
        if self.settings.white_balance == WhiteBalance::Camera {
            ui.label("촬영 화이트 밸런스를 사용합니다. 이중 광원 행렬이 있으면 촬영 광원에 맞춰 색을 변환합니다.");
        }
        if self.settings.white_balance == WhiteBalance::Custom {
            for i in 0..3 {
                ui.add(
                    crate::numeric_slider::Slider::new(&mut self.settings.gains[i], 0.01..=32.)
                        .logarithmic(true)
                        .text(format!("{} 배율", info.channel_label(i))),
                );
            }
        }
        if self.settings.white_balance == WhiteBalance::Custom
            && let Some(camera_fourth) = info.camera_wb_fourth
        {
            let mut gain = self.settings.gains_fourth.unwrap_or(camera_fourth);
            if ui
                .add(
                    crate::numeric_slider::Slider::new(&mut gain, 0.01..=32.)
                        .logarithmic(true)
                        .text(format!("{} 배율", info.channel_label(3))),
                )
                .changed()
            {
                self.settings.gains_fourth = Some(gain);
            }
        }
        if self.settings.white_balance == WhiteBalance::Custom {
            ui.label("이중 광원 프로파일은 현재 배율에 맞춰 색을 변환합니다. 광원을 계산할 수 없는 배율은 적용할 수 없습니다.");
        }
        if self.settings.white_balance == WhiteBalance::Temperature {
            ui.push_id("raw-temperature", |ui| {
                ui.add(
                    crate::numeric_slider::Slider::new(
                        &mut self.settings.temperature,
                        1000. ..=50000.,
                    )
                    .logarithmic(true)
                    .text("색온도 K"),
                );
            });
            ui.push_id("raw-tint", |ui| {
                ui.add(
                    crate::numeric_slider::Slider::new(&mut self.settings.tint, -200. ..=200.)
                        .text("틴트"),
                );
            });
            ui.label("틴트: − 초록 / + 자홍. 선택한 광원으로 색을 변환하며 이중 광원 행렬이 있으면 색온도에 따라 보간합니다.");
        }
        ui.add(
            crate::numeric_slider::Slider::new(&mut self.settings.exposure, -16.0..=16.0)
                .text("노출 EV"),
        );
        if ui.button("촬영값으로 초기화").clicked() {
            self.settings = Settings::default();
        }
    }
    fn preview_ui(&mut self, ui: &mut egui::Ui, fit_height: f32) {
        ui.label("화이트 밸런스·노출을 바꾸면 센서에서 다시 현상해 직접 그린 픽셀을 교체합니다. 보정만 바꾸면 현재 픽셀을 유지합니다. 마스크·이동·변형·효과를 유지하며 실행 취소로 돌아갈 수 있습니다.");
        ui.label("32비트의 밝은 값과 음수 색상을 유지합니다. 화면은 일반 밝기 미리보기입니다.");
        if let Some((settings, Err(error))) = &self.result
            && settings == &self.key()
        {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        self.invalidate_changed_settings();
        if self.picker.ui(ui) && self.picker.enabled {
            self.raw_preview_settings = None;
            self.result = None;
            self.view.clear_after();
        }
        if self.picker.enabled
            && self
                .result
                .as_ref()
                .is_some_and(|(s, r)| s == &self.key() && r.is_err())
            && ui.button("미리보기 다시 계산").clicked()
        {
            self.result = None;
            self.raw_preview_settings = None;
            self.changed_at = Instant::now();
        }
        let picking =
            self.picker.enabled && self.raw_preview_settings.as_ref() == Some(&self.settings);
        if let Some(focus) = self.view.ui_pick_sized(ui, picking, fit_height) {
            self.picker.queue(&self.settings, focus);
        }

        if self.job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("현상 중…");
            });
        }
    }
    fn validation_ui(&mut self, ui: &mut egui::Ui) {
        if !self.adjustments.valid() {
            ui.colored_label(
                egui::Color32::YELLOW,
                "보정 값이나 렌즈 경계가 유효하지 않습니다. 값을 조절하세요.",
            );
        }
        self.invalidate_changed_settings();
    }
    fn advanced_ui(&mut self, ui: &mut egui::Ui) {
        ui.label("현상 뒤의 보정은 원본 픽셀과 분리해 프로젝트에 유지합니다. 적용·취소와 실행 취소에 함께 포함됩니다.");
        if self.picker.enabled {
            ui.label("화이트 밸런스 샘플은 렌즈·색 보정 전 현상 이미지에서 선택합니다.");
        }
        let a = &mut self.adjustments;
        ui.collapsing("톤·곡선", |ui| {
            for (value, label, range) in [
                (&mut a.brightness, "밝기", -1. ..=1.),
                (&mut a.contrast, "대비", -1. ..=2.),
                (&mut a.saturation, "채도", -1. ..=2.),
                (&mut a.gamma, "감마", 0.1..=5.),
                (&mut a.black, "블랙 레벨", 0. ..=0.95),
                (&mut a.white, "화이트 레벨", 0.05..=1.),
            ] {
                ui.add(crate::numeric_slider::Slider::new(value, range).text(label));
            }
            ui.horizontal_wrapped(|ui| {
                for (i, label) in ["RGB", "R", "G", "B"].into_iter().enumerate() {
                    if ui
                        .selectable_value(&mut self.curve_channel, i, label)
                        .changed()
                    {
                        self.curve_drag = None;
                    }
                }
                if ui.button("커브 초기화").clicked() {
                    a.curves[self.curve_channel] = Default::default();
                    self.curve_drag = None;
                }
            });
            crate::app::curve_editor(ui, &mut a.curves[self.curve_channel], &mut self.curve_drag);
        });
        a.tonal_contrast.ui(ui);
        a.color_mixer.ui(ui);
        a.color_grading.ui(ui);
        a.denoise.ui(ui);
        a.detail_sharpen.ui(ui);
        crate::lens_profile::ui(&mut a.lens_profile, Some(&self.source.record.info), ui);
        a.lens.ui(ui);
        a.geometry.ui(ui);
        if ui.button("현상 보정 초기화").clicked() {
            let scope = a.scope.clone();
            *a = Default::default();
            a.scope = scope;
            self.curve_drag = None;
        }
        ui.label("마스크·이동·레이어 변형·효과는 유지됩니다. 미리보기는 이 레이어의 현상·보정 결과입니다.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn raw_preview_worker_latest_initial_reset_failure_and_cancel_keep_baseline_and_source() {
        let doc = crate::files::open(
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/raw/linear-32-o1.dng"),
        )
        .unwrap()
        .document;
        let source = doc.layers[0].raw_source.clone().unwrap();
        let original = doc.layers[0].precision.clone().unwrap();
        let ctx = egui::Context::default();
        let mut panel = Panel::new(1, doc.active, 0, source.clone(), original.clone(), &ctx);
        let mut time = 0.;
        fn frame(
            ctx: &egui::Context,
            panel: &mut Panel,
            time: &mut f64,
            events: Vec<egui::Event>,
        ) -> (Vec<String>, Option<Action>) {
            *time += 0.05;
            let mut action = None;
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::Vec2::new(880., 620.),
                    )),
                    time: Some(*time),
                    events,
                    ..Default::default()
                },
                |_| action = panel.ui(ctx),
            );
            let labels = out
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        Some(t.galley.job.text.clone())
                    } else {
                        None
                    }
                })
                .collect();
            out.textures_delta.clear();
            (labels, action)
        }
        fn queue(panel: &mut Panel, settings: Settings, initial: bool, result: Developed) {
            let (sender, receiver) = mpsc::channel();
            let mut key = panel.key();
            key.settings = settings;
            panel.job = Some((key, receiver, initial));
            sender.send(result).ok().unwrap();
        }
        queue(
            &mut panel,
            source.record.settings.clone(),
            true,
            Ok(Prepared {
                shown: None,
                pixels: original.clone(),
                preview: crate::raw_preview::thumbnail(&original),
            }),
        );
        frame(&ctx, &mut panel, &mut time, vec![]);
        assert!(!panel.ready());
        assert!(Arc::ptr_eq(
            &panel.result.as_ref().unwrap().1.as_ref().unwrap().pixels,
            &original
        ));
        panel.settings.exposure = 1.;
        queue(
            &mut panel,
            source.record.settings.clone(),
            false,
            Ok(Prepared {
                shown: None,
                pixels: original.clone(),
                preview: crate::raw_preview::thumbnail(&original),
            }),
        );
        let (labels, _) = frame(&ctx, &mut panel, &mut time, vec![]);
        assert!(panel.result.is_none());
        assert!(!labels.iter().any(|s| s == "현상 결과 100%"));
        panel.settings.exposure = 2.;
        let latest = Arc::new(source.develop(&panel.settings).unwrap());
        let queued_settings = panel.settings.clone();
        queue(
            &mut panel,
            queued_settings,
            false,
            Ok(Prepared {
                shown: None,
                pixels: latest.clone(),
                preview: crate::raw_preview::thumbnail(&latest),
            }),
        );
        frame(&ctx, &mut panel, &mut time, vec![]);
        assert!(panel.ready());
        assert!(Arc::ptr_eq(
            &panel.result.as_ref().unwrap().1.as_ref().unwrap().pixels,
            &latest
        ));
        panel.settings = source.record.settings.clone();
        let (labels, _) = frame(&ctx, &mut panel, &mut time, vec![]);
        assert!(!panel.ready());
        assert!(labels.iter().any(|s| s == "현상 결과 100%"));
        panel.settings.exposure = 3.;
        let queued_settings = panel.settings.clone();
        queue(
            &mut panel,
            queued_settings,
            false,
            Err("검사용 현상 실패".into()),
        );
        let (labels, _) = frame(&ctx, &mut panel, &mut time, vec![]);
        assert!(!panel.ready());
        assert!(labels.iter().any(|s| s == "검사용 현상 실패"));
        assert!(!labels.iter().any(|s| s == "현상 결과 100%"));
        let (_, action) = frame(
            &ctx,
            &mut panel,
            &mut time,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
        );
        assert!(matches!(action, Some(Action::Cancel)));
        assert!(Arc::ptr_eq(&panel.source, &source));
        assert!(Arc::ptr_eq(
            doc.layers[0].precision.as_ref().unwrap(),
            &original
        ));
    }
}

#[path = "raw_white_balance_picker.rs"]
mod white_balance_picker;

#[cfg(test)]
#[path = "raw_white_balance_panel_tests.rs"]
mod white_balance_panel_tests;

#[cfg(test)]
#[path = "raw_advanced_panel_tests.rs"]
mod advanced_tests;

#[cfg(test)]
#[path = "raw_sidebar_tests.rs"]
mod sidebar_tests;
