use super::Database;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
};

#[path = "lens_watch.rs"]
mod watch;
fn watch_default() -> bool {
    true
}

const MAX_SETTINGS: usize = 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    version: u8,
    paths: Vec<PathBuf>,
    #[serde(default = "watch_default")]
    watch: bool,
}
impl Settings {
    fn valid(&self) -> bool {
        self.version == 1
            && self.paths.len() <= 256
            && self.paths.iter().all(|p| {
                p.is_absolute() && p.to_str().is_some_and(|s| !s.is_empty() && s.len() <= 4096)
            })
    }
    fn read(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self {
                version: 1,
                paths: vec![],
                watch: true,
            });
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .map_err(|e| e.to_string())?
            .take(MAX_SETTINGS as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_SETTINGS {
            return Err("렌즈 목록 설정이 현재 읽기 예산을 넘습니다.".into());
        }
        let settings: Self = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if !settings.valid() {
            return Err("지원하지 않는 렌즈 목록 설정입니다. 기존 파일을 유지합니다.".into());
        }
        Ok(settings)
    }
    fn write(&self, path: &Path) -> Result<(), String> {
        if !self.valid() {
            return Err("렌즈 목록 설정이 잘못되었습니다.".into());
        }
        let bytes = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        if bytes.len() > MAX_SETTINGS {
            return Err("렌즈 목록 설정이 현재 저장 예산을 넘습니다.".into());
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        crate::files::atomic_write(path, |file| {
            file.write_all(&bytes).map_err(|e| e.to_string())
        })
    }
}
struct Imported {
    database: Database,
    snapshot: watch::Snapshot,
}
struct Pending {
    cancelled: Arc<AtomicBool>,
    paths: Vec<PathBuf>,
    receive: mpsc::Receiver<Result<Imported, String>>,
    replace: bool,
    automatic: bool,
}
struct Library {
    paths: Vec<PathBuf>,
    settings: Option<PathBuf>,
    pending: Option<Pending>,
    error: Option<String>,
    preserve_bad_settings: bool,
    worker_running: Arc<AtomicBool>,
    watch: bool,
    watcher: watch::Watcher,
}
impl Default for Library {
    fn default() -> Self {
        Self {
            paths: vec![],
            settings: None,
            pending: None,
            error: None,
            preserve_bad_settings: false,
            worker_running: Arc::new(AtomicBool::new(false)),
            watch: true,
            watcher: Default::default(),
        }
    }
}
impl Library {
    fn save(&self) -> Result<(), String> {
        if self.preserve_bad_settings {
            return Err(
                "기존 렌즈 설정 파일이 잘못되어 이번 목록은 실행 중에만 유지합니다.".into(),
            );
        }
        if let Some(path) = &self.settings {
            Settings {
                version: 1,
                paths: self.paths.clone(),
                watch: self.watch,
            }
            .write(path)?;
        }
        Ok(())
    }
    fn begin(
        &mut self,
        base: Arc<Database>,
        paths: Vec<PathBuf>,
        replace: bool,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        if self.pending.is_some() || self.worker_running.load(Ordering::Acquire) {
            return Err("렌즈 목록을 이미 불러오는 중입니다.".into());
        }
        let mut saved = if replace {
            Vec::new()
        } else {
            self.paths.clone()
        };
        for path in &paths {
            if !saved.contains(path) {
                saved.push(path.clone());
            }
        }
        if !(Settings {
            version: 1,
            paths: saved.clone(),
            watch: self.watch,
        })
        .valid()
        {
            return Err("렌즈 파일 경로나 목록 개수가 현재 설정 예산을 넘습니다.".into());
        }
        self.watcher.invalidate();
        let (send, receive) = mpsc::channel();
        let ctx = ctx.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancelled.clone();
        let running = self.worker_running.clone();
        running.store(true, Ordering::Release);
        if let Err(error) = std::thread::Builder::new()
            .name("lens-library-import".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let before=watch::scan(&paths,&worker_cancel)?;
                    let database=base.import_cancellable(&paths, &worker_cancel)?;
                    let snapshot=watch::scan(&paths,&worker_cancel)?;
                    if before!=snapshot {return Err("읽는 중 렌즈 XML이 바뀌었습니다. 기존 목록을 유지하며 변경이 끝난 뒤 다시 불러옵니다.".into());}
                    Ok(Imported {database,snapshot})
                }))
                .unwrap_or_else(|_| {
                    Err("렌즈 목록 작업을 완료하지 못했습니다. 기존 목록을 유지합니다.".into())
                });
                let _ = send.send(result);
                running.store(false, Ordering::Release);
                ctx.request_repaint();
            })
        {
            self.worker_running.store(false, Ordering::Release);
            return Err(error.to_string());
        }
        self.pending = Some(Pending {
            paths: saved,
            receive,
            cancelled,
            replace,
            automatic: false,
        });
        self.error = None;
        Ok(())
    }
    fn finish(&mut self) -> Option<Arc<Database>> {
        let result = match self.pending.as_ref()?.receive.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("렌즈 목록 작업이 중단되었습니다. 기존 목록을 유지합니다.".into())
            }
        };
        let pending = self.pending.take().unwrap();
        match result {
            Ok(imported) => {
                self.watcher.record(imported.snapshot, pending.replace);
                self.paths = pending.paths;
                self.error = self.save().err();
                Some(Arc::new(imported.database))
            }
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }
    fn cancel(&mut self) {
        self.watcher.invalidate();
        if let Some(pending) = self.pending.take() {
            pending.cancelled.store(true, Ordering::Release);
        }
    }
    fn clear(&mut self) {
        // Dropping the receiver prevents a late worker from replacing a cleared catalog.
        self.cancel();
        self.paths.clear();
        self.watcher.clear();
        self.error = self.save().err();
    }
}
type Shared = Arc<Mutex<Library>>;
fn id() -> egui::Id {
    egui::Id::new("lens-profile-library-service")
}
fn library(ctx: &egui::Context) -> Shared {
    if let Some(value) = ctx.data(|d| d.get_temp::<Shared>(id())) {
        return value;
    }
    let value = Arc::new(Mutex::new(Library::default()));
    ctx.data_mut(|d| d.insert_temp(id(), value.clone()));
    value
}
fn empty() -> Arc<Database> {
    Arc::new(Database {
        profiles: vec![],
        bodies: vec![],
        notes: vec![],
    })
}

pub fn initialize(ctx: &egui::Context, path: PathBuf) {
    let mut state = Library {
        settings: Some(path.clone()),
        ..Default::default()
    };
    match Settings::read(&path) {
        Ok(settings) => {
            state.paths = settings.paths;
            state.watch = settings.watch;
            if !state.paths.is_empty()
                && let Err(error) = state.begin(empty(), state.paths.clone(), true, ctx)
            {
                state.error = Some(error);
            }
        }
        Err(error) => {
            state.error = Some(error);
            state.preserve_bad_settings = true;
        }
    }
    ctx.data_mut(|d| d.insert_temp(id(), Arc::new(Mutex::new(state))));
}
pub fn poll(ctx: &egui::Context) {
    let value = library(ctx);
    if let Ok(mut state) = value.lock() {
        if let Some(db) = state.finish() {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("lens-profile-library"), db));
        }
        if state.watch
            && !state.paths.is_empty()
            && state.pending.is_none()
            && !state.worker_running.load(Ordering::Acquire)
        {
            let paths = state.paths.clone();
            match state.watcher.poll(&paths, ctx) {
                Some(Ok(())) => {
                    if let Err(error) = state.begin(empty(), paths, true, ctx) {
                        state.error = Some(error);
                    } else if let Some(pending) = &mut state.pending {
                        pending.automatic = true;
                    }
                }
                Some(Err(error)) => state.error = Some(error),
                None => {}
            }
        }
        if state.pending.is_some() || state.worker_running.load(Ordering::Acquire) {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}
pub(super) fn import(ctx: &egui::Context, base: Option<Arc<Database>>, paths: Vec<PathBuf>) {
    let value = library(ctx);
    if let Ok(mut state) = value.lock()
        && let Err(error) = state.begin(base.unwrap_or_else(empty), paths, false, ctx)
    {
        state.error = Some(error);
    }
}
pub(super) fn retry(ctx: &egui::Context) {
    let value = library(ctx);
    if let Ok(mut state) = value.lock() {
        let paths = state.paths.clone();
        if let Err(error) = state.begin(empty(), paths, true, ctx) {
            state.error = Some(error);
        }
    }
}
pub(super) fn cancel(ctx: &egui::Context) {
    let value = library(ctx);
    if let Ok(mut state) = value.lock() {
        state.cancel();
        state.error = None;
    }
}
pub(super) fn clear(ctx: &egui::Context) {
    let value = library(ctx);
    if let Ok(mut state) = value.lock() {
        state.clear();
    }
    ctx.data_mut(|d| d.remove::<Arc<Database>>(egui::Id::new("lens-profile-library")));
}
pub(super) fn status(ctx: &egui::Context) -> (bool, bool, Option<String>) {
    let value = library(ctx);
    value
        .lock()
        .map(|state| {
            (
                state.pending.is_some() || state.worker_running.load(Ordering::Acquire),
                !state.paths.is_empty(),
                state.error.clone(),
            )
        })
        .unwrap_or((
            false,
            false,
            Some("렌즈 목록 상태를 읽지 못했습니다.".into()),
        ))
}

#[cfg(test)]
#[path = "lens_library_tests.rs"]
mod tests;

pub(super) fn watch_ui(ui: &mut egui::Ui) {
    let shared = library(ui.ctx());
    if let Ok(mut state) = shared.lock()
        && ui
            .checkbox(&mut state.watch, "XML 파일 변경 자동 반영")
            .changed()
    {
        state.watcher.invalidate();
        if !state.watch && state.pending.as_ref().is_some_and(|p| p.automatic) {
            state.cancel();
        }
        state.error = state.save().err();
    }
}
