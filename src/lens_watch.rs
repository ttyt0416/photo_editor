use eframe::egui;
use std::{
    collections::HashMap,
    hash::Hasher,
    io::Read,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
const INTERVAL: Duration = Duration::from_secs(5);
const LIMIT: usize = 64 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Stamp {
    Present { length: u64, hash: u64 },
    Unavailable(String),
}
pub(super) type Snapshot = HashMap<PathBuf, Stamp>;
pub(super) fn scan(paths: &[PathBuf], cancelled: &AtomicBool) -> Result<Snapshot, String> {
    if paths.len() > 256 {
        return Err("렌즈 변경 검사 경로가 현재256개 예산을 넘습니다.".into());
    }
    let mut result = Snapshot::new();
    let mut remaining = LIMIT;
    let mut buffer = [0u8; 65536];
    for path in paths {
        if cancelled.load(Ordering::Acquire) {
            return Err("렌즈 변경 검사를 취소했습니다.".into());
        }
        let mut file = match std::fs::File::open(path) {
            Ok(file) => file,
            Err(e) => {
                result.insert(path.clone(), Stamp::Unavailable(format!("{:?}", e.kind())));
                continue;
            }
        };
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        let mut length = 0u64;
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err("렌즈 변경 검사를 취소했습니다.".into());
            }
            let allowed = buffer.len().min(remaining.saturating_add(1));
            let read = file
                .read(&mut buffer[..allowed])
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if read == 0 {
                break;
            }
            if read > remaining {
                return Err("렌즈 변경 검사 XML이 현재64MiB 읽기 예산을 넘습니다.".into());
            }
            remaining -= read;
            length += read as u64;
            hash.write(&buffer[..read]);
        }
        result.insert(
            path.clone(),
            Stamp::Present {
                length,
                hash: hash.finish(),
            },
        );
    }
    Ok(result)
}
struct Pending {
    cancelled: Arc<AtomicBool>,
    receive: mpsc::Receiver<Result<Snapshot, String>>,
}
pub(super) struct Watcher {
    observed: Snapshot,
    pending: Option<Pending>,
    next: Instant,
    running: Arc<AtomicBool>,
}
impl Default for Watcher {
    fn default() -> Self {
        Self {
            observed: Snapshot::new(),
            pending: None,
            next: Instant::now() + INTERVAL,
            running: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl Watcher {
    #[cfg(test)]
    pub(super) fn due(&mut self) {
        self.next = Instant::now();
    }
    #[cfg(test)]
    pub(super) fn busy(&self) -> bool {
        self.pending.is_some() || self.running.load(Ordering::Acquire)
    }
    pub fn invalidate(&mut self) {
        if let Some(pending) = self.pending.take() {
            pending.cancelled.store(true, Ordering::Release);
        }
        self.next = Instant::now() + INTERVAL;
    }
    pub fn clear(&mut self) {
        self.invalidate();
        self.observed.clear();
    }
    pub fn record(&mut self, snapshot: Snapshot, replace: bool) {
        self.invalidate();
        if replace {
            self.observed = snapshot;
        } else {
            self.observed.extend(snapshot);
        }
    }
    pub fn poll(&mut self, paths: &[PathBuf], ctx: &egui::Context) -> Option<Result<(), String>> {
        if let Some(pending) = &self.pending {
            match pending.receive.try_recv() {
                Ok(result) => {
                    self.pending = None;
                    self.next = Instant::now() + INTERVAL;
                    return match result {
                        Ok(snapshot) if snapshot == self.observed => None,
                        Ok(snapshot) => {
                            self.observed = snapshot;
                            Some(Ok(()))
                        }
                        Err(error) => Some(Err(error)),
                    };
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.pending = None;
                    self.next = Instant::now() + INTERVAL;
                    return Some(Err("렌즈 변경 검사 작업이 중단되었습니다.".into()));
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(100));
                    return None;
                }
            }
        }
        let now = Instant::now();
        if now < self.next {
            ctx.request_repaint_after(self.next - now);
            return None;
        }
        if self.running.load(Ordering::Acquire) {
            ctx.request_repaint_after(Duration::from_millis(100));
            return None;
        }
        let paths = paths.to_vec();
        let repaint = ctx.clone();
        let running = self.running.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        let stop = cancelled.clone();
        let (sender, receive) = mpsc::channel();
        running.store(true, Ordering::Release);
        if let Err(error) = std::thread::Builder::new()
            .name("lens-file-watch".into())
            .spawn(move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| scan(&paths, &stop)))
                        .unwrap_or_else(|_| Err("렌즈 변경 검사를 완료하지 못했습니다.".into()));
                let _ = sender.send(result);
                running.store(false, Ordering::Release);
                repaint.request_repaint();
            })
        {
            self.running.store(false, Ordering::Release);
            self.next = now + INTERVAL;
            return Some(Err(error.to_string()));
        }
        self.pending = Some(Pending { cancelled, receive });
        ctx.request_repaint_after(Duration::from_millis(100));
        None
    }
}
impl Drop for Watcher {
    fn drop(&mut self) {
        self.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lens_watch_content_budget_cancel_missing_and_stale_receiver() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/lens-watch/scan");
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("same-size.xml");
        std::fs::write(&path, b"abcdefgh").unwrap();
        let stop = AtomicBool::new(false);
        let a = scan(std::slice::from_ref(&path), &stop).unwrap();
        std::fs::write(&path, b"abcdXfgh").unwrap();
        let b = scan(std::slice::from_ref(&path), &stop).unwrap();
        assert_ne!(a, b);
        let missing = scan(&[root.join("missing.xml")], &stop).unwrap();
        assert!(matches!(
            missing.values().next(),
            Some(Stamp::Unavailable(_))
        ));
        stop.store(true, Ordering::Release);
        assert!(scan(std::slice::from_ref(&path), &stop).is_err());
        stop.store(false, Ordering::Release);
        let big = root.join("budget.xml");
        std::fs::File::create(&big)
            .unwrap()
            .set_len(LIMIT as u64 + 1)
            .unwrap();
        assert!(scan(&[big], &stop).unwrap_err().contains("64MiB"));
        let (send, receive) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let mut watcher = Watcher::default();
        watcher.pending = Some(Pending {
            receive,
            cancelled: cancelled.clone(),
        });
        watcher.invalidate();
        assert!(cancelled.load(Ordering::Acquire));
        assert!(send.send(Ok(b)).is_err());
        assert!(watcher.observed.is_empty());
    }
}
