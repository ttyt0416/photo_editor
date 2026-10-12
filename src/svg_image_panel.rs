use eframe::egui;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
};
enum Finished {
    Imported(crate::svg_assets::Replacement),
    Saved(PathBuf),
}
struct Pending {
    source: String,
    index: usize,
    receiver: Receiver<Result<Option<Finished>, String>>,
    cancelled: Arc<AtomicBool>,
}
#[derive(Default)]
pub(super) struct Panel {
    path: String,
    pending: Option<Pending>,
    error: Option<String>,
    notes: Vec<String>,
    preview: Option<egui::TextureHandle>,
    saved: Option<String>,
}
impl Panel {
    pub fn clear_preview(&mut self) {
        self.saved = None;
        self.preview = None;
        self.notes.clear();
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }
    pub fn close(&mut self) {
        if let Some(job) = self.pending.take() {
            job.cancelled.store(true, Ordering::Release);
        }
        self.saved = None;
        self.preview = None;
        self.error = None;
        self.notes.clear();
    }
    fn start(&mut self, ctx: &egui::Context, source: &str, index: usize, path: Option<PathBuf>) {
        if self.busy() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let source = source.to_owned();
        let input = source.clone();
        let context = ctx.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let spawned = std::thread::Builder::new()
            .name("svg-image-import".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(|| {
                    let path = path.or_else(|| {
                        rfd::FileDialog::new()
                            .set_title("SVG 이미지 바꾸기")
                            .add_filter("이미지", crate::files::IMAGE_EXTENSIONS)
                            .pick_file()
                    });
                    if stop.load(Ordering::Acquire) {
                        return Ok(None);
                    }
                    path.map(|path| {
                        crate::svg_assets::replace_image(&input, index, &path)
                            .map(Finished::Imported)
                    })
                    .transpose()
                })
                .unwrap_or_else(|_| Err("SVG 이미지 교체 중 파일 해석에 실패했습니다.".into()));
                let _ = sender.send(result);
                context.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.pending = Some(Pending {
                    source,
                    index,
                    receiver,
                    cancelled,
                });
                self.error = None;
                self.notes.clear();
                self.preview = None;
                self.saved = None;
            }
            Err(e) => self.error = Some(format!("이미지 가져오기를 시작할 수 없습니다: {e}")),
        }
    }
    fn save(
        &mut self,
        ctx: &egui::Context,
        source: &str,
        index: usize,
        kind: crate::svg_assets::ImageFile,
        path: Option<PathBuf>,
    ) {
        if self.busy() {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let source = source.to_owned();
        let input = source.clone();
        let context = ctx.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let spawned = std::thread::Builder::new()
            .name("svg-image-save".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(|| {
                    let resource = crate::svg_assets::image_file(&input, index, kind)?;
                    if stop.load(Ordering::Acquire) {
                        return Ok(None);
                    }
                    let title = if kind == crate::svg_assets::ImageFile::Original {
                        "SVG 원본 이미지 저장"
                    } else {
                        "SVG 표시용 이미지 저장"
                    };
                    let path = path.or_else(|| {
                        rfd::FileDialog::new()
                            .set_title(title)
                            .set_file_name(&resource.name)
                            .save_file()
                    });
                    if stop.load(Ordering::Acquire) {
                        return Ok(None);
                    }
                    path.map(|path| {
                        crate::svg_assets::save_image(&resource, &path, || {
                            !stop.load(Ordering::Acquire)
                        })
                        .map(|_| Finished::Saved(path))
                    })
                    .transpose()
                })
                .unwrap_or_else(|_| Err("SVG 이미지 파일 저장 중 처리에 실패했습니다.".into()));
                let _ = sender.send(result);
                context.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.pending = Some(Pending {
                    source,
                    index,
                    receiver,
                    cancelled,
                });
                self.error = None;
                self.saved = None;
            }
            Err(e) => self.error = Some(format!("이미지 저장을 시작할 수 없습니다: {e}")),
        }
    }
    fn poll(&mut self, ctx: &egui::Context, source: &mut String, index: Option<usize>) -> bool {
        let Some(job) = self.pending.as_ref() else {
            return false;
        };
        if job.source != *source || Some(job.index) != index {
            self.close();
            return false;
        }
        let result = match job.receiver.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                "이미지 가져오기가 끝나지 못했습니다. 다시 시도하세요.".into(),
            )),
        };
        if let Some(result) = result {
            self.pending = None;
            match result {
                Ok(Some(Finished::Imported(output))) => {
                    let pixels = image::imageops::thumbnail(&output.pixels, 240, 160);
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [pixels.width() as usize, pixels.height() as usize],
                        pixels.as_raw(),
                    );
                    self.preview = Some(ctx.load_texture(
                        "svg-image-preview",
                        image,
                        egui::TextureOptions::LINEAR,
                    ));
                    *source = output.source;
                    self.notes = output.notes;
                    self.error = None;
                    return true;
                }
                Ok(Some(Finished::Saved(path))) => {
                    self.saved = Some(format!(
                        "{} 저장했습니다.",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ));
                    self.error = None;
                }
                Ok(None) => {}
                Err(e) => self.error = Some(e),
            }
        }
        false
    }
    pub fn ui(&mut self, ui: &mut egui::Ui, source: &str, index: usize) {
        ui.label("이미지 파일을 바꾸면 위치·크기·변형·스타일을 유지합니다.");
        ui.horizontal(|ui| {
            ui.add_enabled(
                !self.busy(),
                egui::TextEdit::singleline(&mut self.path)
                    .id(egui::Id::new("svg-image-path"))
                    .hint_text("이미지 파일 경로")
                    .desired_width(220.),
            );
            if ui
                .add_enabled(!self.busy(), egui::Button::new("찾아보기…"))
                .clicked()
            {
                self.start(ui.ctx(), source, index, None);
            }
        });
        if ui
            .add_enabled(
                !self.busy() && !self.path.trim().is_empty(),
                egui::Button::new("이미지 교체"),
            )
            .clicked()
        {
            self.start(
                ui.ctx(),
                source,
                index,
                Some(PathBuf::from(self.path.trim())),
            );
        }
        ui.horizontal_wrapped(|ui| {
            for (label, kind) in [
                ("원본 파일 저장", crate::svg_assets::ImageFile::Original),
                ("표시용 이미지 저장", crate::svg_assets::ImageFile::Display),
            ] {
                if ui
                    .add_enabled(!self.busy(), egui::Button::new(label))
                    .clicked()
                {
                    self.save(ui.ctx(), source, index, kind, None);
                }
            }
        });
        ui.small(
            "원본은 처음 파일의 데이터로, 표시용 이미지는 SVG에서 사용하는 파일로 저장합니다. 레이어 결과는 이미지 내보내기에서 저장하세요.",
        );
        if let Some(saved) = &self.saved {
            ui.label(saved);
        }
        if self.busy() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("이미지 작업 중…");
                if ui.button("작업 취소").clicked() {
                    self.close();
                }
            });
        }
        if let Some(preview) = &self.preview {
            ui.add(egui::Image::new(preview).max_size(egui::vec2(240., 160.)));
        }
        for note in &self.notes {
            ui.label(note);
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    }
}
pub(super) fn poll(
    panel: &mut Panel,
    ctx: &egui::Context,
    source: &mut String,
    index: Option<usize>,
) -> bool {
    panel.poll(ctx, source, index)
}
#[cfg(test)]
#[path = "svg_image_panel_tests.rs"]
mod tests;
