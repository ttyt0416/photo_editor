use crate::{
    document::Document,
    files::{self, ExportOptions},
};
use eframe::egui::{self, Color32, Pos2, Rect, Sense, Vec2};
use image::RgbaImage;
use std::{
    path::{Path, PathBuf},
    sync::mpsc,
    time::Duration,
};
const EDGE: u32 = 512;
const DETAIL: u32 = 160;
const DEBOUNCE: f64 = 0.25;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Key {
    tab: u64,
    revision: u64,
    format: usize,
    options: ExportOptions,
    focus: [u32; 2],
}
struct Output {
    size: [u32; 2],
    bytes: u64,
    images: [RgbaImage; 4],
}
struct TemporaryPreview {
    directory: PathBuf,
    path: PathBuf,
}
impl Drop for TemporaryPreview {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}
fn thumbnail(image: &RgbaImage) -> RgbaImage {
    let (w, h) = image.dimensions();
    if w <= EDGE && h <= EDGE {
        return image.clone();
    }
    let scale = (f64::from(EDGE) / f64::from(w)).min(f64::from(EDGE) / f64::from(h));
    let (tw, th) = (
        (f64::from(w) * scale).floor().max(1.) as u32,
        (f64::from(h) * scale).floor().max(1.) as u32,
    );
    let sx = f64::from(w) / f64::from(tw);
    let sy = f64::from(h) / f64::from(th);
    image::RgbaImage::from_fn(tw, th, |x, y| {
        let (left, right, top, bottom) = (
            f64::from(x) * sx,
            f64::from(x + 1) * sx,
            f64::from(y) * sy,
            f64::from(y + 1) * sy,
        );
        let mut sum = [0f64; 4];
        let mut weight = 0.;
        for py in top.floor() as u32..(bottom.ceil() as u32).min(h) {
            for px in left.floor() as u32..(right.ceil() as u32).min(w) {
                let area = (right.min(f64::from(px + 1)) - left.max(f64::from(px)))
                    * (bottom.min(f64::from(py + 1)) - top.max(f64::from(py)));
                let p = image.get_pixel(px, py);
                let alpha = f64::from(p[3]) / 255.;
                for c in 0..3 {
                    sum[c] += f64::from(p[c]) * alpha * area;
                }
                sum[3] += alpha * area;
                weight += area;
            }
        }
        image::Rgba([
            if sum[3] > 0. {
                (sum[0] / sum[3]).round() as u8
            } else {
                0
            },
            if sum[3] > 0. {
                (sum[1] / sum[3]).round() as u8
            } else {
                0
            },
            if sum[3] > 0. {
                (sum[2] / sum[3]).round() as u8
            } else {
                0
            },
            (sum[3] / weight * 255.).round() as u8,
        ])
    })
}
fn detail(image: &RgbaImage, focus: [u32; 2]) -> RgbaImage {
    let width = image.width().min(DETAIL);
    let height = image.height().min(DETAIL);
    let x = focus[0]
        .saturating_sub(width / 2)
        .min(image.width() - width);
    let y = focus[1]
        .saturating_sub(height / 2)
        .min(image.height() - height);
    image::imageops::crop_imm(image, x, y, width, height).to_image()
}
fn generate(doc: &Document, key: &Key, root: &Path) -> Result<Output, String> {
    let extension = files::export_extension(key.format)?;
    let directory = root.join(format!(
        "photo-editor-preview-{}",
        crate::document_id::new()
    ));
    std::fs::create_dir(&directory).map_err(|e| e.to_string())?;
    let temporary = TemporaryPreview {
        path: directory.join(format!("image.{extension}")),
        directory,
    };
    files::export_by_index(doc, &temporary.path, key.format, &key.options)?;
    let bytes = std::fs::metadata(&temporary.path)
        .map_err(|e| e.to_string())?
        .len();
    let restored = files::open(&temporary.path)?.document.composite();
    let original = doc.composite();
    Ok(Output {
        size: [restored.width(), restored.height()],
        bytes,
        images: [
            thumbnail(&original),
            thumbnail(&restored),
            detail(&original, key.focus),
            detail(&restored, key.focus),
        ],
    })
}
struct Pending {
    token: u64,
    receiver: mpsc::Receiver<Result<Output, String>>,
}
struct State {
    key: Option<Key>,
    changed: f64,
    token: u64,
    completed: Option<u64>,
    receiver: Option<Pending>,
    output: Option<Result<Output, String>>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            key: None,
            changed: 0.,
            token: 0,
            completed: None,
            receiver: None,
            output: None,
        }
    }
}
impl State {
    fn request(&mut self, key: Key, now: f64) {
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.changed = now;
            self.token += 1;
            self.completed = None;
            self.output = None;
        }
    }
    fn close(&mut self) {
        if self.key.take().is_some() {
            self.token += 1;
        }
        self.completed = None;
        self.output = None;
        self.poll();
    }
    fn accept(&mut self, token: u64, result: Result<Output, String>) {
        if token == self.token && self.key.is_some() {
            self.completed = Some(token);
            self.output = Some(result);
        }
    }
    fn poll(&mut self) {
        if let Some(pending) = &self.receiver {
            let token = pending.token;
            match pending.receiver.try_recv() {
                Ok(result) => {
                    self.receiver = None;
                    self.accept(token, result);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.receiver = None;
                    self.accept(token, Err("미리보기 작업이 종료되었습니다.".into()));
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
    }
    fn ready(&self, now: f64) -> bool {
        self.key.is_some()
            && self.receiver.is_none()
            && self.completed != Some(self.token)
            && now - self.changed >= DEBOUNCE
    }
}
pub struct Panel {
    pub enabled: bool,
    state: State,
    focus: [f32; 2],
    textures: Option<(u64, [egui::TextureHandle; 4])>,
}
impl Default for Panel {
    fn default() -> Self {
        Self {
            enabled: true,
            state: State::default(),
            focus: [0.5; 2],
            textures: None,
        }
    }
}
impl Panel {
    pub fn close(&mut self) {
        self.state.close();
        self.textures = None;
    }
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        document: Option<(&Document, u64, u64)>,
        format: usize,
        options: &ExportOptions,
    ) {
        ui.set_min_width(360.);
        ui.set_max_width(360.);
        ui.heading("출력 미리보기");
        ui.checkbox(&mut self.enabled, "실시간 미리보기");
        let Some((doc, tab, revision)) = document else {
            self.close();
            ui.label("문서를 먼저 여세요.");
            return;
        };
        if !self.enabled {
            self.close();
            ui.label("미리보기를 켜면 화질·변환·파일 크기를 확인할 수 있습니다.");
            return;
        }
        if self.state.key.as_ref().is_some_and(|k| k.tab != tab) {
            self.focus = [0.5; 2];
        }
        let now = ctx.input(|i| i.time);
        let focus = [
            (self.focus[0] * doc.width.saturating_sub(1) as f32).round() as u32,
            (self.focus[1] * doc.height.saturating_sub(1) as f32).round() as u32,
        ];
        self.state.request(
            Key {
                tab,
                revision,
                format,
                options: options.clone(),
                focus,
            },
            now,
        );
        self.state.poll();
        if self.state.ready(now) {
            let key = self.state.key.clone().unwrap();
            let token = self.state.token;
            let document = doc.clone();
            let ctx = ctx.clone();
            let (sender, receiver) = mpsc::channel();
            self.state.receiver = Some(Pending { token, receiver });
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    generate(&document, &key, &std::env::temp_dir())
                }))
                .unwrap_or_else(|_| Err("미리보기 생성 중 오류가 발생했습니다.".into()));
                let _ = sender.send(result);
                ctx.request_repaint();
            });
        }
        match self.state.output.as_ref() {
            Some(Ok(output)) => {
                if self
                    .textures
                    .as_ref()
                    .is_none_or(|(token, _)| *token != self.state.token)
                {
                    let textures = std::array::from_fn(|i| {
                        let image = &output.images[i];
                        ctx.load_texture(
                            format!("export-preview-{}-{i}", self.state.token),
                            egui::ColorImage::from_rgba_unmultiplied(
                                [image.width() as usize, image.height() as usize],
                                image.as_raw(),
                            ),
                            egui::TextureOptions::NEAREST,
                        )
                    });
                    self.textures = Some((self.state.token, textures));
                }
                let textures = &self.textures.as_ref().unwrap().1;
                ui.label(format!(
                    "{} × {} · {} ({}바이트)",
                    output.size[0],
                    output.size[1],
                    size_label(output.bytes),
                    output.bytes
                ));
                ui.columns(2, |columns| {
                    for i in 0..2 {
                        columns[i].label(if i == 0 { "원본" } else { "저장 결과" });
                        if let Some(p) = picture(
                            &mut columns[i],
                            &textures[i],
                            &output.images[i],
                            false,
                            ctx.pixels_per_point(),
                        ) {
                            self.focus = p;
                        }
                    }
                });
                ui.label("전체 보기에서 클릭하면 해당 위치를 100%로 비교합니다.");
                ui.columns(2, |columns| {
                    for i in 0..2 {
                        columns[i].label(if i == 0 {
                            "원본 100%"
                        } else {
                            "저장 결과 100%"
                        });
                        picture(
                            &mut columns[i],
                            &textures[i + 2],
                            &output.images[i + 2],
                            true,
                            ctx.pixels_per_point(),
                        );
                    }
                });
                ui.label(format!("부분 보기 중심: {}, {}", focus[0], focus[1]));
                ui.label("파일을 다시 읽은 결과입니다. 화면 표시는 현재 8비트입니다.");
            }
            Some(Err(error)) => {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            None => {
                ui.spinner();
                ui.label("미리보기 갱신 중…");
                ctx.request_repaint_after(Duration::from_millis(50));
            }
        }
    }
}
fn size_label(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.2} MB", bytes as f64 / 1048576.)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{bytes} B")
    }
}
fn picture(
    ui: &mut egui::Ui,
    texture: &egui::TextureHandle,
    image: &RgbaImage,
    one_to_one: bool,
    dpi: f32,
) -> Option<[f32; 2]> {
    let pixels = Vec2::new(image.width() as f32, image.height() as f32);
    let scale = if one_to_one {
        1. / dpi
    } else {
        (ui.available_width() / pixels.x)
            .min(160. / pixels.y)
            .min(1.)
    };
    let size = pixels * scale;
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    for y in 0..(size.y / 8.).ceil() as u32 {
        for x in 0..(size.x / 8.).ceil() as u32 {
            painter.rect_filled(
                Rect::from_min_size(
                    rect.min + Vec2::new(x as f32 * 8., y as f32 * 8.),
                    Vec2::splat(8.),
                ),
                0.,
                Color32::from_gray(if (x + y) % 2 == 0 { 60 } else { 90 }),
            );
        }
    }
    painter.image(
        texture.id(),
        rect,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
        Color32::WHITE,
    );
    if !one_to_one && response.clicked() {
        response
            .interact_pointer_pos()
            .map(|p| [(p.x - rect.min.x) / size.x, (p.y - rect.min.y) / size.y])
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root(name: &str) -> PathBuf {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("qa-output/export-preview")
            .join(name);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn key(format: usize) -> Key {
        Key {
            tab: 1,
            revision: 0,
            format,
            options: Default::default(),
            focus: [26, 20],
        }
    }
    fn source() -> Document {
        Document::from_image(
            "preview".into(),
            image::RgbaImage::from_fn(53, 41, |x, y| {
                image::Rgba([
                    (x * 4) as u8,
                    (y * 6) as u8,
                    ((x + y) * 2) as u8,
                    [0, 64, 128, 255][(x % 4) as usize],
                ])
            }),
        )
    }
    fn no_temporary(root: &Path) {
        assert!(!std::fs::read_dir(root).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("photo-editor-preview-")
        }));
    }
    #[test]
    fn jpeg2000_multitile_previews_match_saved_final_pixels_bytes_and_keep_source() {
        let root = root("jpeg2000-tiled");
        let doc = crate::precision::document(
            "tiled preview".into(),
            image::DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(259, 131, |x, y| {
                image::Rgba([
                    (x * 197 + y * 31) as u16,
                    (x * 37 + y * 173) as u16,
                    (x * 13 + y * 59) as u16,
                    if x % 3 == 0 { 32768 } else { 65535 },
                ])
            })),
        )
        .unwrap();
        let before = doc.layers[0].precision.clone().unwrap();
        for format in [
            files::EXPORT_FORMATS.len() + 3,
            files::EXPORT_FORMATS.len() + 4,
        ] {
            for gray in [false, true] {
                let mut key = key(format);
                key.options.jp2_tile = 128;
                key.options.jp2_resolution_enabled = true;
                key.options.jp2_resolution = 300;
                key.options.jp2_grayscale = gray;
                key.options.jp2_lossless = false;
                key.options.jp2_layers = 4;
                let preview = generate(&doc, &key, &root).unwrap();
                let path = root.join(format!(
                    "tiled-{format}-{gray}.{}",
                    files::export_extension(format).unwrap()
                ));
                files::export_by_index(&doc, &path, format, &key.options).unwrap();
                let restored = files::open(&path).unwrap().document.composite();
                assert_eq!(preview.bytes, std::fs::metadata(&path).unwrap().len());
                assert_eq!(preview.images[1], restored);
                assert_eq!(preview.images[3], detail(&restored, key.focus));
                assert_eq!(preview.size, [259, 131]);
                no_temporary(&root);
            }
        }
        assert!(std::sync::Arc::ptr_eq(
            &before,
            doc.layers[0].precision.as_ref().unwrap()
        ));
    }
    #[test]
    fn jpeg2000_gray_previews_follow_actual_options_pixels_bytes_and_keep_source() {
        let root = root("jpeg2000-gray");
        let doc = source();
        let before = doc.layers[0].pixels.clone();
        let source_pixels = doc.composite();
        for format in [
            files::EXPORT_FORMATS.len() + 3,
            files::EXPORT_FORMATS.len() + 4,
        ] {
            for depth in [8, 16] {
                for lossless in [false, true] {
                    let mut key = key(format);
                    key.options.jp2_grayscale = true;
                    key.options.jp2_depth = depth;
                    key.options.jp2_lossless = lossless;
                    let preview = generate(&doc, &key, &root).unwrap();
                    let path = root.join(format!(
                        "gray-{format}-{depth}-{lossless}.{}",
                        files::export_extension(format).unwrap()
                    ));
                    files::export_by_index(&doc, &path, format, &key.options).unwrap();
                    let restored = files::open(&path).unwrap().document.composite();
                    assert_eq!(preview.bytes, std::fs::metadata(&path).unwrap().len());
                    assert_eq!(preview.images[0], source_pixels);
                    assert_eq!(preview.images[1], restored);
                    assert_eq!(preview.images[3], detail(&restored, key.focus));
                    assert!(restored.pixels().all(|p| p[0] == p[1] && p[1] == p[2]));
                    assert_ne!(restored, source_pixels);
                    no_temporary(&root);
                }
            }
        }
        assert!(std::sync::Arc::ptr_eq(&before, &doc.layers[0].pixels));
    }
    #[test]
    fn all_export_previews_match_actual_file_bytes_and_decoded_pixels_without_source_changes() {
        let root = root("formats");
        let doc = source();
        let original = doc.layers[0].pixels.clone();
        doc.composite().save(root.join("source.png")).unwrap();
        let mut manifest = Vec::new();
        for format in 0..files::export_count() {
            let key = key(format);
            let preview = generate(&doc, &key, &root).unwrap();
            let path = root.join(format!(
                "format{format}.{}",
                files::export_extension(format).unwrap()
            ));
            files::export_by_index(&doc, &path, format, &key.options).unwrap();
            assert_eq!(
                preview.bytes,
                std::fs::metadata(&path).unwrap().len(),
                "format{format}"
            );
            let restored = files::open(&path).unwrap().document.composite();
            assert_eq!(preview.images[0], doc.composite());
            assert_eq!(preview.images[1], restored);
            assert_eq!(preview.images[2], detail(&doc.composite(), key.focus));
            assert_eq!(preview.images[3], detail(&restored, key.focus));
            assert_eq!(preview.size, [53, 41]);
            preview.images[1]
                .save(root.join(format!("format{format}-preview.png")))
                .unwrap();
            manifest.push(serde_json::json!({"file":path.file_name().unwrap().to_string_lossy(),"bytes":preview.bytes,"size":preview.size,"preview":format!("format{format}-preview.png")}));
            no_temporary(&root);
        }
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert!(std::sync::Arc::ptr_eq(&original, &doc.layers[0].pixels));
        let jpeg = files::EXPORT_FORMATS
            .iter()
            .position(|f| f.0 == image::ImageFormat::Jpeg)
            .unwrap();
        let mut low = key(jpeg);
        low.options.quality = 10;
        let mut high = low.clone();
        high.options.quality = 95;
        let a = generate(&doc, &low, &root).unwrap();
        let b = generate(&doc, &high, &root).unwrap();
        assert!(a.bytes < b.bytes);
        assert_ne!(a.images[1], b.images[1]);
        for (name, p) in [("jpeg-low", a), ("jpeg-high", b)] {
            p.images[1].save(root.join(format!("{name}.png"))).unwrap();
        }
        let hdr = crate::precision::document(
            "HDR preview".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
                17,
                9,
                image::Rgba([2., -0.25, 0.5, 0.75]),
            )),
        )
        .unwrap();
        let before = hdr.layers[0].precision.clone().unwrap();
        for format in [
            image::ImageFormat::Png,
            image::ImageFormat::Tiff,
            image::ImageFormat::OpenExr,
            image::ImageFormat::Hdr,
        ] {
            let index = files::EXPORT_FORMATS
                .iter()
                .position(|f| f.0 == format)
                .unwrap();
            assert_eq!(generate(&hdr, &key(index), &root).unwrap().size, [17, 9]);
        }
        assert!(std::sync::Arc::ptr_eq(
            &before,
            hdr.layers[0].precision.as_ref().unwrap()
        ));
        no_temporary(&root);
    }
    #[test]
    fn preview_failure_cleanup_alpha_thumbnail_and_full_resolution_detail_bounds() {
        let root = root("cleanup");
        let doc = source();
        assert!(generate(&doc, &key(usize::MAX), &root).is_err());
        let mut invalid = key(0);
        invalid.options.png_level = 10;
        assert!(generate(&doc, &invalid, &root).is_err());
        let ico = files::EXPORT_FORMATS
            .iter()
            .position(|v| v.0 == image::ImageFormat::Ico)
            .unwrap();
        assert!(generate(&Document::new(512, 8, true).unwrap(), &key(ico), &root).is_err());
        no_temporary(&root);
        let transparent = image::RgbaImage::from_fn(1024, 2, |x, _| {
            image::Rgba(if x % 2 == 0 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 0]
            })
        });
        let thumb = thumbnail(&transparent);
        assert_eq!(thumb.dimensions(), (512, 1));
        assert!(thumb.pixels().all(|p| p.0 == [255, 0, 0, 128]));
        let large = image::RgbaImage::from_fn(1023, 711, |x, y| {
            image::Rgba([(x % 251) as u8, (y % 241) as u8, 13, 255])
        });
        let thumb = thumbnail(&large);
        assert!(thumb.width() <= 512 && thumb.height() <= 512);
        let last = detail(&large, [u32::MAX; 2]);
        assert_eq!(last.dimensions(), (160, 160));
        assert_eq!(last.get_pixel(0, 0), large.get_pixel(863, 551));
        let first = detail(&large, [0; 2]);
        assert_eq!(first.get_pixel(159, 159), large.get_pixel(159, 159));
        thumb.save(root.join("bounded-thumbnail.png")).unwrap();
        last.save(root.join("edge-detail.png")).unwrap();
    }
    #[test]
    fn debounce_single_worker_latest_options_close_reopen_and_old_disconnect_are_safe() {
        let mut state = State::default();
        let mut current = key(0);
        state.request(current.clone(), 0.);
        assert!(!state.ready(0.24));
        assert!(state.ready(0.25));
        let first = state.token;
        let (sender, receiver) = mpsc::channel();
        state.receiver = Some(Pending {
            token: first,
            receiver,
        });
        for quality in [10, 20, 30] {
            current.options.quality = quality;
            state.request(current.clone(), 0.3);
        }
        assert!(!state.ready(10.));
        sender.send(Err("stale".into())).unwrap();
        state.poll();
        assert!(state.output.is_none());
        assert!(state.ready(10.));
        let (sender, receiver) = mpsc::channel();
        let latest = state.token;
        state.receiver = Some(Pending {
            token: latest,
            receiver,
        });
        sender.send(Err("latest".into())).unwrap();
        state.poll();
        assert!(matches!(state.output.as_ref(),Some(Err(e)) if e=="latest"));
        assert!(!state.ready(10.));
        state.request(current.clone(), 11.);
        assert_eq!(state.token, latest);
        current.revision += 1;
        state.request(current.clone(), 12.);
        let closed = state.token;
        let (sender, receiver) = mpsc::channel();
        state.receiver = Some(Pending {
            token: closed,
            receiver,
        });
        state.close();
        state.request(current.clone(), 13.);
        drop(sender);
        state.poll();
        assert!(state.output.is_none());
        assert!(state.ready(14.));
        state.close();
        state.accept(closed, Err("closed".into()));
        assert!(state.output.is_none());
        assert!(!state.ready(20.));
        current.tab += 1;
        state.request(current, 21.);
        assert!(!state.ready(21.));
        assert!(state.ready(22.));
    }
    #[test]
    fn preview_panel_displays_bounded_textures_and_physical_pixel_detail_at_one_and_two_dpi() {
        let doc = source();
        let options = ExportOptions::default();
        for dpi in [1., 2.] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(dpi);
            for i in 0..2 {
                let mut warm = ctx.run_ui(
                    egui::RawInput {
                        time: Some(i as f64 * 0.1),
                        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(880., 620.))),
                        ..Default::default()
                    },
                    |_| {},
                );
                warm.textures_delta.clear();
            }
            let mut panel = Panel::default();
            panel.state.request(key(0), 0.);
            panel.state.accept(
                panel.state.token,
                Ok(Output {
                    size: [53, 41],
                    bytes: 1234,
                    images: [
                        doc.composite(),
                        doc.composite(),
                        doc.composite(),
                        doc.composite(),
                    ],
                }),
            );
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(1.),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(880., 620.))),
                    ..Default::default()
                },
                |ctx| {
                    ctx.set_pixels_per_point(dpi);
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let context = ui.ctx().clone();
                        panel.ui(ui, &context, Some((&doc, 1, 0)), 0, &options)
                    });
                },
            );
            let meshes: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Mesh(m) = &s.shape {
                        if panel
                            .textures
                            .as_ref()
                            .unwrap()
                            .1
                            .iter()
                            .any(|t| t.id() == m.texture_id)
                        {
                            Some(m.calc_bounds())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();
            output.textures_delta.clear();
            assert_eq!(meshes.len(), 4);
            assert!(
                (ctx.pixels_per_point() - dpi).abs() < 0.01,
                "dpi {}",
                ctx.pixels_per_point()
            );
            assert!((meshes[2].width() - 53. / dpi).abs() < 0.01);
            assert!((meshes[2].height() - 41. / dpi).abs() < 0.01);
            assert!(output.shapes.iter().any(|s|matches!(&s.shape,egui::Shape::Text(t) if t.galley.job.text.contains("1234바이트"))));
            for rect in &meshes {
                assert!(rect.min.x >= 0. && rect.max.x <= 880. && rect.max.y <= 620.);
            }
            output.textures_delta.clear();
            panel.enabled = false;
            panel.close();
            assert!(panel.textures.is_none());
            assert!(panel.state.output.is_none());
        }
    }
    #[test]
    fn background_preview_finishes_and_clicking_overview_requests_a_new_detail_without_stale_images()
     {
        let doc = source();
        let ctx = egui::Context::default();
        let mut panel = Panel::default();
        let format = files::EXPORT_FORMATS
            .iter()
            .position(|f| f.0 == image::ImageFormat::Jpeg)
            .unwrap();
        let render = |panel: &mut Panel, time, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    time: Some(time),
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(880., 620.))),
                    events,
                    ..Default::default()
                },
                |context| {
                    egui::CentralPanel::default().show(context, |ui| {
                        let context = ui.ctx().clone();
                        panel.ui(
                            ui,
                            &context,
                            Some((&doc, 1, 0)),
                            format,
                            &ExportOptions::default(),
                        );
                    });
                },
            );
            output.textures_delta.clear();
            output
        };
        render(&mut panel, 0., vec![]);
        assert!(panel.state.receiver.is_none());
        render(&mut panel, 0.3, vec![]);
        assert!(panel.state.receiver.is_some());
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while panel.state.output.is_none() && std::time::Instant::now() < deadline {
            panel.state.poll();
            std::thread::sleep(Duration::from_millis(2));
        }
        assert!(matches!(panel.state.output.as_ref(),Some(Ok(p)) if p.bytes>0 && p.size==[53,41]));
        let output = render(&mut panel, 0.4, vec![]);
        let texture = panel.textures.as_ref().unwrap().1[0].id();
        let rect = output
            .shapes
            .iter()
            .find_map(|s| {
                if let egui::Shape::Mesh(m) = &s.shape {
                    if m.texture_id == texture {
                        Some(m.calc_bounds())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .unwrap();
        let point = rect.min + rect.size() * Vec2::new(0.8, 0.2);
        render(&mut panel, 0.41, vec![egui::Event::PointerMoved(point)]);
        for (i, pressed) in [true, false].into_iter().enumerate() {
            render(
                &mut panel,
                0.42 + i as f64 * 0.01,
                vec![egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
        assert!(
            (panel.focus[0] - 0.8).abs() < 0.02 && (panel.focus[1] - 0.2).abs() < 0.02,
            "focus {:?}",
            panel.focus
        );
        render(&mut panel, 0.44, vec![]);
        assert!(panel.state.output.is_none());
        assert!(panel.state.receiver.is_none());
        assert_eq!(panel.state.key.as_ref().unwrap().focus, [42, 8]);
        panel.close();
        assert!(panel.state.key.is_none() && panel.textures.is_none());
    }
}
