use super::{Database, Model, Profile};
use std::{
    collections::{HashMap, HashSet},
    hash::{Hash, Hasher},
    path::PathBuf,
    sync::Arc,
};

const RECORD_LIMIT: usize = 16_384;
const BYTE_LIMIT: usize = 64 * 1024 * 1024;

impl Database {
    /// Build a new catalog; callers only replace the current one after full success.
    pub fn merge(&self, incoming: &Self) -> Result<Self, String> {
        let mut out = Self {
            profiles: Vec::new(),
            bodies: Vec::new(),
            notes: Vec::new(),
        };
        let mut xml_hashes = HashMap::new();
        let mut buckets: HashMap<(u64, usize, u64, Model), Vec<usize>> = HashMap::new();
        let mut assets = HashSet::new();
        let mut bytes = 0usize;
        for p in self.profiles.iter().chain(&incoming.profiles) {
            if !p.valid() {
                return Err("잘못된 렌즈 교정값이 있어 목록을 변경하지 않았습니다.".into());
            }
            let ptr = Arc::as_ptr(&p.xml) as usize;
            let hash = *xml_hashes.entry(ptr).or_insert_with(|| {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                p.xml.hash(&mut h);
                h.finish()
            });
            let key = (hash, p.calibration, p.focal.to_bits(), p.model);
            let bucket = buckets.entry(key).or_default();
            if bucket.iter().any(|&i| out.profiles[i] == *p) {
                continue;
            }
            if out.profiles.len() == RECORD_LIMIT {
                return Err("렌즈 목록이 현재 교정값 개수 예산을 넘습니다.".into());
            }
            if assets.insert((0, ptr)) {
                bytes = bytes.saturating_add(p.xml.len());
            }
            let vg_ptr = Arc::as_ptr(&p.vignetting) as usize;
            if assets.insert((1, vg_ptr)) {
                bytes = bytes.saturating_add(
                    p.vignetting
                        .len()
                        .saturating_mul(std::mem::size_of::<super::Vignetting>()),
                );
            }
            if bytes > BYTE_LIMIT {
                return Err("렌즈 목록이 현재64MiB 자산 예산을 넘습니다.".into());
            }
            bucket.push(out.profiles.len());
            out.profiles.push(p.clone());
        }
        let mut bodies = HashSet::new();
        for b in self.bodies.iter().chain(&incoming.bodies) {
            if !b.crop.is_finite()
                || !(0.1..=30.).contains(&b.crop)
                || [&b.maker, &b.name, &b.mount].iter().any(|s| s.len() > 4096)
            {
                return Err("잘못된 카메라 정보가 있어 목록을 변경하지 않았습니다.".into());
            }
            if bodies.insert((&b.maker, &b.name, &b.mount, b.crop.to_bits())) {
                if out.bodies.len() == RECORD_LIMIT {
                    return Err("카메라 목록이 현재 개수 예산을 넘습니다.".into());
                }
                out.bodies.push(b.clone());
            }
        }
        let mut notes = HashSet::new();
        for note in self.notes.iter().chain(&incoming.notes) {
            if notes.insert(note) {
                bytes = bytes.saturating_add(note.len());
                if bytes > BYTE_LIMIT {
                    return Err("렌즈 목록 안내가 현재 자산 예산을 넘습니다.".into());
                }
                out.notes.push(note.clone());
            }
        }
        Ok(out)
    }

    #[cfg(test)]
    pub fn import(&self, paths: &[PathBuf]) -> Result<Self, String> {
        self.import_cancellable(paths, &std::sync::atomic::AtomicBool::new(false))
    }
    pub(super) fn import_cancellable(
        &self,
        paths: &[PathBuf],
        cancelled: &std::sync::atomic::AtomicBool,
    ) -> Result<Self, String> {
        if paths.len() > 256 {
            return Err("한 번에 불러오는 XML 개수가 현재256개 예산을 넘습니다.".into());
        }
        let mut bytes = 0u64;
        for path in paths {
            if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                return Err("렌즈 목록 불러오기를 취소했습니다.".into());
            }
            bytes = bytes.saturating_add(
                std::fs::metadata(path)
                    .map_err(|e| format!("{}: {e}", path.display()))?
                    .len(),
            );
            if bytes > BYTE_LIMIT as u64 {
                return Err("선택한 XML이 현재64MiB 읽기 예산을 넘습니다.".into());
            }
        }
        let mut result = self.clone();
        let mut remaining = BYTE_LIMIT;
        for path in paths {
            if cancelled.load(std::sync::atomic::Ordering::Acquire) {
                return Err("렌즈 목록 불러오기를 취소했습니다.".into());
            }
            let incoming = super::xml::load_budget(path, &mut remaining)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            result = result.merge(&incoming)?;
        }
        Ok(result)
    }
}

/// Precompute searchable strings and distinguish calibration groups once per catalog.
pub(super) struct View {
    labels: Vec<String>,
    search: Vec<String>,
}
impl View {
    pub fn new(database: &Database) -> Self {
        let mut duplicates = HashMap::new();
        for p in &database.profiles {
            *duplicates
                .entry((&p.maker, &p.name, p.focal.to_bits(), p.model))
                .or_insert(0usize) += 1;
        }
        let labels: Vec<_> = database
            .profiles
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let base = format!("{} {} · {} mm · {:?}", p.maker, p.name, p.focal, p.model);
                if duplicates[&(&p.maker, &p.name, p.focal.to_bits(), p.model)] > 1 {
                    format!(
                        "{base} · 교정 {} · 크롭 {} · 비율 {} · #{}",
                        p.calibration + 1,
                        p.crop,
                        p.aspect,
                        i + 1
                    )
                } else {
                    base
                }
            })
            .collect();
        let search = labels
            .iter()
            .zip(&database.profiles)
            .map(|(label, p)| {
                format!("{label} {} {}", p.projection, p.mounts.join(" ")).to_lowercase()
            })
            .collect();
        Self { labels, search }
    }
    pub fn find(&self, query: &str) -> Vec<usize> {
        let query = query.to_lowercase();
        let tokens: Vec<_> = query.split_whitespace().collect();
        self.search
            .iter()
            .enumerate()
            .filter_map(|(i, text)| tokens.iter().all(|token| text.contains(token)).then_some(i))
            .collect()
    }
    pub fn label(&self, i: usize) -> &str {
        &self.labels[i]
    }
}
pub(super) fn same_selection(a: &Profile, b: &Profile) -> bool {
    a.xml == b.xml
        && a.calibration == b.calibration
        && a.model == b.model
        && a.focal == b.focal
        && a.crop == b.crop
        && a.aspect == b.aspect
        && a.coefficients == b.coefficients
}

#[cfg(test)]
#[path = "lens_catalog_tests.rs"]
mod tests;
