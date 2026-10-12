use super::*;
type SampleResult = Result<crate::raw_input::BalanceSample, String>;
struct Request {
    serial: u64,
    settings: Settings,
    focus: [f32; 2],
    radius: u32,
}
#[derive(Default)]
pub(super) struct Picker {
    pub enabled: bool,
    radius: u32,
    serial: u64,
    request: Option<Request>,
    job: Option<(u64, mpsc::Receiver<SampleResult>)>,
    error: Option<String>,
}
impl Picker {
    pub fn new() -> Self {
        Self {
            radius: 2,
            ..Default::default()
        }
    }
    pub fn busy(&self) -> bool {
        self.request.is_some()
    }
    pub fn queue(&mut self, settings: &Settings, focus: [f32; 2]) {
        if !self.enabled {
            return;
        }
        self.serial = self.serial.wrapping_add(1);
        self.request = Some(Request {
            serial: self.serial,
            settings: settings.clone(),
            focus,
            radius: self.radius,
        });
        self.error = None;
    }
    pub fn poll(&mut self, source: &Arc<Source>, settings: &mut Settings, ctx: &egui::Context) {
        if self
            .request
            .as_ref()
            .is_some_and(|r| !self.enabled || r.settings != *settings || r.radius != self.radius)
        {
            self.request = None;
        }
        if let Some((serial, receiver)) = &self.job {
            let result = match receiver.try_recv() {
                Ok(r) => Some(r),
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("화이트 밸런스 계산이 중단됐습니다.".into()))
                }
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                let serial = *serial;
                self.job = None;
                if self.request.as_ref().is_some_and(|r| r.serial == serial) {
                    self.request = None;
                    match result {
                        Ok(sample) => {
                            settings.white_balance = WhiteBalance::Custom;
                            settings.gains = sample.gains;
                            settings.gains_fourth = sample.gains_fourth;
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
        }
        if self.job.is_none()
            && let Some(request) = &self.request
        {
            let serial = request.serial;
            let focus = request.focus;
            let radius = request.radius;
            let source = source.clone();
            let ctx = ctx.clone();
            let (sender, receiver) = mpsc::channel();
            let spawned = std::thread::Builder::new()
                .name("raw-white-balance".into())
                .spawn(move || {
                    let _ = sender.send(source.white_balance_at(focus, radius));
                    ctx.request_repaint();
                });
            match spawned {
                Ok(_) => self.job = Some((serial, receiver)),
                Err(error) => {
                    self.request = None;
                    self.error = Some(format!("화이트 밸런스 계산을 시작할 수 없습니다: {error}"));
                }
            }
        }
    }
    pub fn ui(&mut self, ui: &mut egui::Ui) -> bool {
        let before = self.enabled;
        ui.horizontal_wrapped(|ui| {
            if ui
                .selectable_label(self.enabled, "화이트 밸런스 스포이드")
                .clicked()
            {
                self.enabled = !self.enabled;
                self.request = None;
                self.error = None;
            }
            ui.add(
                crate::numeric_slider::Slider::new(&mut self.radius, 0..=64).text("샘플 반경 px"),
            );
        });
        if self.enabled {
            ui.label(
                "현상 결과의 흰색·회색 부분을 클릭하세요. 원래 센서 값의 주변 평균을 사용합니다.",
            );
        }
        if self.busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("화이트 밸런스 계산 중…");
            });
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        before != self.enabled
    }
}

#[cfg(test)]
#[path = "raw_white_balance_picker_tests.rs"]
mod tests;
