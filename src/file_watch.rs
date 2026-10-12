use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime},
};
const METADATA_INTERVAL: Duration = Duration::from_secs(2);
const CONTENT_INTERVAL: Duration = Duration::from_secs(10);
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Snapshot {
    Present {
        length: u64,
        modified: SystemTime,
        hash: [u8; 32],
    },
    Missing,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct Metadata {
    length: u64,
    modified: SystemTime,
}
fn metadata(path: &Path) -> Result<Option<Metadata>, String> {
    match std::fs::metadata(path) {
        Ok(m) => {
            if !m.is_file() {
                return Err("열었던 파일 위치가 일반 파일이 아닙니다.".into());
            }
            Ok(Some(Metadata {
                length: m.len(),
                modified: m.modified().map_err(|e| e.to_string())?,
            }))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}
fn same_metadata(current: &Option<Metadata>, known: &Snapshot) -> bool {
    match (current, known) {
        (None, Snapshot::Missing) => true,
        (
            Some(m),
            Snapshot::Present {
                length, modified, ..
            },
        ) => m.length == *length && m.modified == *modified,
        _ => false,
    }
}
fn scan(
    path: &Path,
    known: Option<&Snapshot>,
    content: bool,
    cancelled: &AtomicBool,
) -> Result<Snapshot, String> {
    let before = metadata(path)?;
    let Some(before) = before else {
        return Ok(Snapshot::Missing);
    };
    if !content
        && let Some(known) = known
        && same_metadata(&Some(before.clone()), known)
    {
        return Ok(known.clone());
    }
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut length = 0u64;
    loop {
        if cancelled.load(Ordering::Acquire) {
            return Err("파일 변경 검사를 취소했습니다.".into());
        }
        let allowed = buffer.len().min(
            before
                .length
                .saturating_sub(length)
                .saturating_add(1)
                .min(65536) as usize,
        );
        let n = file
            .read(&mut buffer[..allowed])
            .map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        length = length
            .checked_add(n as u64)
            .ok_or("파일 길이를 처리할 수 없습니다.")?;
        if length > before.length {
            return Err("파일 변경 검사 중 원본이 커졌습니다. 다시 확인합니다.".into());
        }
        hasher.update(&buffer[..n]);
    }
    if length != before.length || metadata(path)? != Some(before.clone()) {
        return Err("파일 변경 검사 중 원본이 바뀌었습니다. 다시 확인합니다.".into());
    }
    Ok(Snapshot::Present {
        length,
        modified: before.modified,
        hash: hasher.finalize().into(),
    })
}
pub fn snapshot(path: &Path) -> Result<Snapshot, String> {
    scan(path, None, true, &AtomicBool::new(false))
}
struct Pending {
    receive: mpsc::Receiver<Result<Snapshot, String>>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Pending {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
#[derive(Default)]
pub struct Watch {
    pub baseline: Option<Snapshot>,
    pub notice: Option<Snapshot>,
    checked: Option<Snapshot>,
    last_notice: Option<Snapshot>,
    pending: Option<Pending>,
    deadline: Option<Instant>,
    content_deadline: Option<Instant>,
    pub warning: Option<String>,
}
impl Watch {
    pub fn new(baseline: Option<Snapshot>) -> Self {
        Self {
            checked: baseline.clone(),
            baseline,
            content_deadline: Some(Instant::now() + CONTENT_INTERVAL),
            ..Default::default()
        }
    }
    pub fn reset(&mut self, baseline: Snapshot) {
        *self = Self::new(Some(baseline));
    }
    #[cfg(test)]
    pub(crate) fn check_content_now(&mut self) {
        self.deadline = None;
        self.content_deadline = None;
    }
    fn observe(&mut self, result: Result<Snapshot, String>) {
        match result {
            Ok(current) => {
                self.warning = None;
                self.checked = Some(current.clone());
                if self.baseline.as_ref() == Some(&current) {
                    self.notice = None;
                    self.last_notice = None;
                } else if self.last_notice.as_ref() != Some(&current) {
                    self.notice = Some(current.clone());
                    self.last_notice = Some(current);
                }
            }
            Err(error) => self.warning = Some(error),
        }
    }
    pub fn poll(&mut self, path: &Path, ctx: &eframe::egui::Context) {
        if let Some(pending) = &self.pending {
            match pending.receive.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    self.observe(result);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.warning = Some("파일 변경 확인 작업이 종료됐습니다.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let now = Instant::now();
        if self.pending.is_none() && self.deadline.is_none_or(|deadline| now >= deadline) {
            let path: PathBuf = path.into();
            let ctx = ctx.clone();
            let known = self.checked.clone();
            let content = self.content_deadline.is_none_or(|deadline| now >= deadline);
            let cancelled = Arc::new(AtomicBool::new(false));
            let worker_cancelled = cancelled.clone();
            let (sender, receive) = mpsc::channel();
            self.pending = Some(Pending { receive, cancelled });
            std::thread::spawn(move || {
                let result = scan(&path, known.as_ref(), content, &worker_cancelled);
                if !worker_cancelled.load(Ordering::Acquire) {
                    let _ = sender.send(result);
                    ctx.request_repaint();
                }
            });
            self.deadline = Some(now + METADATA_INTERVAL);
            if content {
                self.content_deadline = Some(now + CONTENT_INTERVAL);
            }
        }
        ctx.request_repaint_after(METADATA_INTERVAL);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_deletion_reappearance_and_own_save_are_reported_once_without_writes() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/file-watch");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("source.bin");
        std::fs::write(&path, b"original").unwrap();
        let baseline = snapshot(&path).unwrap();
        let mut watch = Watch::new(Some(baseline.clone()));
        watch.observe(Ok(baseline));
        assert!(watch.notice.is_none());
        std::fs::write(&path, b"external revision").unwrap();
        let external = snapshot(&path).unwrap();
        watch.observe(Ok(external.clone()));
        assert_eq!(watch.notice.take(), Some(external.clone()));
        watch.observe(Ok(external));
        assert!(watch.notice.is_none());
        std::fs::remove_file(&path).unwrap();
        watch.observe(snapshot(&path));
        assert_eq!(watch.notice.take(), Some(Snapshot::Missing));
        std::fs::write(&path, b"reappeared").unwrap();
        watch.observe(snapshot(&path));
        assert!(watch.notice.take().is_some());
        let own = snapshot(&path).unwrap();
        watch.reset(own.clone());
        watch.observe(Ok(own));
        assert!(watch.notice.is_none());
        watch.observe(Err("permission denied".into()));
        assert_eq!(watch.warning.as_deref(), Some("permission denied"));
        assert_eq!(std::fs::read(&path).unwrap(), b"reappeared");
    }
    #[test]
    fn unchanged_length_and_timestamp_content_is_detected_once_and_restoration_clears_notice() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/file-watch-content");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("동일 시간 내용.bin");
        let bytes: Vec<u8> = (0..131_073usize)
            .map(|i| (i.wrapping_mul(31).wrapping_add(7)) as u8)
            .collect();
        std::fs::write(&path, &bytes).unwrap();
        let baseline = snapshot(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut changed = bytes.clone();
        changed[65_536] ^= 0x35;
        changed[131_072] ^= 0x89;
        std::fs::write(&path, &changed).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let current = snapshot(&path).unwrap();
        let Snapshot::Present {
            hash: original_hash,
            length,
            modified: original_time,
        } = baseline.clone()
        else {
            panic!()
        };
        let Snapshot::Present {
            hash: current_hash,
            length: new_length,
            modified: new_time,
        } = current.clone()
        else {
            panic!()
        };
        assert_eq!((length, original_time), (new_length, new_time));
        assert_ne!(original_hash, current_hash);
        let cancelled = AtomicBool::new(false);
        // Metadata polling retains the last actual content result until the content scan.
        assert_eq!(
            scan(&path, Some(&baseline), false, &cancelled).unwrap(),
            baseline
        );
        let mut watch = Watch::new(Some(baseline.clone()));
        watch.observe(scan(&path, watch.checked.as_ref(), true, &cancelled));
        assert_eq!(watch.notice.take(), Some(current.clone()));
        watch.observe(scan(&path, watch.checked.as_ref(), false, &cancelled));
        assert!(watch.notice.is_none());
        assert_eq!(watch.checked.as_ref(), Some(&current));
        assert_eq!(watch.baseline.as_ref(), Some(&baseline));
        watch.observe(Err("busy file".into()));
        assert_eq!(watch.baseline.as_ref(), Some(&baseline));
        std::fs::write(&path, bytes).unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        watch.observe(scan(&path, watch.checked.as_ref(), true, &cancelled));
        assert!(watch.notice.is_none());
        assert!(watch.warning.is_none());
        std::fs::write(
            dir.join("hashes.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "file":path.file_name().unwrap().to_string_lossy(),
                "bytes":length,"original":original_hash,"changed":current_hash,
                "mutations":[[65536,0x35],[131072,0x89]]
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(scan(&path, None, true, &AtomicBool::new(true)).is_err());
        assert!(snapshot(&dir).is_err());
    }
    #[test]
    fn background_content_scan_reset_and_drop_cancel_old_work_without_repeated_notices() {
        let dir =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/file-watch-content-background");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("source.bin");
        std::fs::write(&path, b"original").unwrap();
        let baseline = snapshot(&path).unwrap();
        let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
        let mut watch = Watch::new(Some(baseline.clone()));
        std::fs::write(&path, b"changed!").unwrap();
        File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(modified))
            .unwrap();
        let ctx = eframe::egui::Context::default();
        watch.content_deadline = None;
        watch.poll(&path, &ctx);
        assert!(watch.pending.is_some());
        let old_cancel = watch.pending.as_ref().unwrap().cancelled.clone();
        watch.reset(baseline);
        assert!(old_cancel.load(Ordering::Acquire));
        assert!(watch.pending.is_none());
        assert!(watch.notice.is_none());
        watch.content_deadline = None;
        watch.poll(&path, &ctx);
        let end = Instant::now() + Duration::from_secs(5);
        while watch.pending.is_some() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(2));
            watch.poll(&path, &ctx);
        }
        let current = snapshot(&path).unwrap();
        assert_eq!(watch.notice.take(), Some(current.clone()));
        watch.deadline = None;
        watch.poll(&path, &ctx);
        while watch.pending.is_some() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(2));
            watch.poll(&path, &ctx);
        }
        assert!(watch.notice.is_none());
        watch.reset(current);
        watch.deadline = None;
        watch.poll(&path, &ctx);
        let on_drop = watch.pending.as_ref().unwrap().cancelled.clone();
        drop(watch);
        assert!(on_drop.load(Ordering::Acquire));
    }
}
