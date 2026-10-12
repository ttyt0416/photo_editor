use crate::{document::Document, files};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    #[serde(default)]
    pub document_id: Option<String>,
    pub key: String,
    pub title: String,
    pub original: Option<PathBuf>,
    pub revision: u64,
    pub saved_at: u64,
}

pub struct Listing {
    pub entries: Vec<Entry>,
    pub warnings: Vec<String>,
}

#[derive(Clone)]
pub struct Store {
    root: PathBuf,
    session: String,
    _lock: Arc<File>,
}

pub fn data_directory() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PHOTO_EDITOR_DATA_DIR") {
        return Some(path.into());
    }
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("PhotoEditor"))
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME")
            .map(|p| PathBuf::from(p).join("Library/Application Support/PhotoEditor"))
    } else {
        std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/state")))
            .map(|p| p.join("photo-editor"))
    }
}

fn timestamp() -> Result<std::time::Duration, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())
}
fn session(key: &str) -> Result<&str, String> {
    let (session, tab) = key.split_once('_').ok_or("잘못된 복구 식별자입니다.")?;
    let (time, process) = session.split_once('-').ok_or("잘못된 복구 식별자입니다.")?;
    if [time, process, tab]
        .iter()
        .any(|p| p.is_empty() || !p.bytes().all(|c| c.is_ascii_digit()))
        || key.len() > 96
    {
        return Err("잘못된 복구 식별자입니다.".into());
    }
    Ok(session)
}

impl Store {
    pub fn new(root: PathBuf) -> Result<Self, String> {
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let session = format!("{}-{}", timestamp()?.as_nanos(), std::process::id());
        let lock = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(root.join(format!("{session}.lock")))
            .map_err(|e| e.to_string())?;
        lock.try_lock().map_err(|e| e.to_string())?;
        Ok(Self {
            root,
            session,
            _lock: Arc::new(lock),
        })
    }
    pub fn key(&self, tab: u64) -> String {
        format!("{}_{tab}", self.session)
    }
    fn folder(&self, key: &str) -> Result<PathBuf, String> {
        session(key)?;
        Ok(self.root.join(key))
    }
    fn active(&self, key: &str) -> Result<bool, String> {
        let lock_path = self.root.join(format!("{}.lock", session(key)?));
        let lock = match File::options().read(true).write(true).open(lock_path) {
            Ok(lock) => lock,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(e.to_string()),
        };
        match lock.try_lock() {
            Ok(()) => Ok(false),
            Err(std::fs::TryLockError::WouldBlock) => Ok(true),
            Err(e) => Err(e.to_string()),
        }
    }
    pub fn write(&self, entry: &Entry, document: &Document) -> Result<(), String> {
        if session(&entry.key)? != self.session {
            return Err("다른 실행 창의 복구 사본은 변경할 수 없습니다.".into());
        }
        if entry
            .document_id
            .as_ref()
            .is_some_and(|id| id != &document.document_id)
        {
            return Err("복구 정보와 문서 식별자가 일치하지 않습니다.".into());
        }
        let folder = self.folder(&entry.key)?;
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let mut entry = entry.clone();
        entry.document_id = Some(document.document_id.clone());
        entry.saved_at = timestamp()?.as_secs();
        let bytes = serde_json::to_vec(&entry).map_err(|e| e.to_string())?;
        let metadata = || {
            files::atomic_write(&folder.join("entry.json"), |file| {
                file.write_all(&bytes).map_err(|e| e.to_string())
            })
        };
        // Register first: a crash after the atomic document replacement must leave a discoverable snapshot.
        if !folder.join("entry.json").is_file() {
            metadata()?;
        }
        files::save_project(document, &folder.join("document.rphoto"))?;
        metadata()
    }
    pub fn list(&self) -> Result<Listing, String> {
        let mut entries = Vec::new();
        let mut warnings = Vec::new();
        for item in std::fs::read_dir(&self.root).map_err(|e| e.to_string())? {
            let item = item.map_err(|e| e.to_string())?;
            let key = item.file_name().to_string_lossy().into_owned();
            if !item.file_type().map_err(|e| e.to_string())?.is_dir() || session(&key).is_err() {
                continue;
            }
            let read = || -> Result<Option<Entry>, String> {
                if self.active(&key)? {
                    return Ok(None);
                }
                let path = item.path().join("entry.json");
                if !path.exists() {
                    return Ok(None);
                }
                let mut bytes = Vec::new();
                File::open(&path)
                    .map_err(|e| e.to_string())?
                    .take(64 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| e.to_string())?;
                if bytes.len() > 64 * 1024 {
                    return Err("복구 정보가 너무 큽니다.".into());
                }
                let entry: Entry = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                if entry.key != key
                    || entry
                        .document_id
                        .as_ref()
                        .is_some_and(|id| !crate::document_id::valid(id))
                {
                    return Err("복구 정보의 식별자가 일치하지 않습니다.".into());
                }
                if !item.path().join("document.rphoto").is_file() {
                    return Err("문서 사본이 없습니다.".into());
                }
                Ok(Some(entry))
            };
            match read() {
                Ok(Some(entry)) => entries.push(entry),
                Ok(None) => {}
                Err(error) if warnings.len() < 32 => warnings.push(format!("사본 {key}: {error}")),
                Err(_) => {}
            }
        }
        entries.sort_by_key(|e| std::cmp::Reverse(e.saved_at));
        Ok(Listing { entries, warnings })
    }
    pub fn restore(&self, key: &str) -> Result<Document, String> {
        if self.active(key)? {
            return Err("다른 실행 창이 사용 중인 복구 사본입니다.".into());
        }
        let folder = self.folder(key)?;
        let mut bytes = Vec::new();
        File::open(folder.join("entry.json"))
            .map_err(|e| e.to_string())?
            .take(64 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 64 * 1024 {
            return Err("복구 정보가 너무 큽니다.".into());
        }
        let entry: Entry = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let document = files::load_project(&folder.join("document.rphoto"))?;
        if entry.key != key
            || entry
                .document_id
                .as_ref()
                .is_some_and(|id| id != &document.document_id)
        {
            return Err("복구 정보와 문서 식별자가 일치하지 않습니다.".into());
        }
        Ok(document)
    }
    pub fn remove(&self, key: &str) -> Result<(), String> {
        if session(key)? != self.session && self.active(key)? {
            return Err("사용 중인 복구 사본은 지울 수 없습니다.".into());
        }
        let folder = self.folder(key)?;
        // Remove only our two known files; never recursively remove unexpected contents.
        for name in ["entry.json", "document.rphoto"] {
            match std::fs::remove_file(folder.join(name)) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        if folder.exists() {
            std::fs::remove_dir(folder).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn damaged_metadata_does_not_hide_valid_snapshots_or_delete_originals() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("qa-output/recovery")
            .join(timestamp().unwrap().as_nanos().to_string());
        let writer = Store::new(root.clone()).unwrap();
        let doc = Document::new(3, 2, false).unwrap();
        let valid = Entry {
            document_id: Some(doc.document_id.clone()),
            key: writer.key(1),
            title: "유효한 사본".into(),
            original: None,
            revision: 1,
            saved_at: 0,
        };
        writer.write(&valid, &doc).unwrap();
        let damaged = Entry {
            key: writer.key(2),
            ..valid.clone()
        };
        writer.write(&damaged, &doc).unwrap();
        std::fs::write(
            root.join(&damaged.key).join("entry.json"),
            b"corrupt metadata",
        )
        .unwrap();
        drop(writer);
        let reader = Store::new(root.clone()).unwrap();
        let listing = reader.list().unwrap();
        assert_eq!(listing.entries.len(), 1);
        assert_eq!(listing.entries[0].key, valid.key);
        assert_eq!(listing.warnings.len(), 1);
        assert_eq!(reader.restore(&valid.key).unwrap().width, 3);
        assert!(root.join(&damaged.key).join("document.rphoto").is_file());
        assert_eq!(
            std::fs::read(root.join(&damaged.key).join("entry.json")).unwrap(),
            b"corrupt metadata"
        );
    }
    #[test]
    fn interrupted_session_restores_hdr_and_active_sessions_are_protected() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("qa-output/recovery")
            .join(timestamp().unwrap().as_nanos().to_string());
        let writer = Store::new(root.clone()).unwrap();
        let doc = crate::precision::document(
            "복구".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
                3,
                2,
                image::Rgba([4.125, -0.01, 0.34567, 1.0]),
            )),
        )
        .unwrap();
        let entry = Entry {
            document_id: Some(doc.document_id.clone()),
            key: writer.key(7),
            title: "미저장 한글".into(),
            original: Some(root.join("original.rphoto")),
            revision: 3,
            saved_at: 0,
        };
        std::fs::write(entry.original.as_ref().unwrap(), b"original unchanged").unwrap();
        writer.write(&entry, &doc).unwrap();
        let reader = Store::new(root).unwrap();
        assert!(reader.list().unwrap().entries.is_empty());
        assert!(reader.restore(&entry.key).is_err());
        assert!(reader.remove(&entry.key).is_err());
        assert!(reader.remove("../../original.rphoto").is_err());
        drop(writer);
        let available = reader.list().unwrap().entries;
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].title, entry.title);
        let restored = reader.restore(&entry.key).unwrap();
        assert_eq!(restored.layers[0].precision, doc.layers[0].precision);
        assert_eq!(restored.document_id, doc.document_id);
        let mut mismatch = entry.clone();
        mismatch.document_id = Some(crate::document_id::new());
        std::fs::write(
            reader.folder(&entry.key).unwrap().join("entry.json"),
            serde_json::to_vec(&mismatch).unwrap(),
        )
        .unwrap();
        assert!(reader.restore(&entry.key).is_err());
        assert!(
            reader
                .folder(&entry.key)
                .unwrap()
                .join("document.rphoto")
                .is_file()
        );
        std::fs::write(
            reader.folder(&entry.key).unwrap().join("entry.json"),
            serde_json::to_vec(&entry).unwrap(),
        )
        .unwrap();

        assert_eq!(
            std::fs::read(entry.original.as_ref().unwrap()).unwrap(),
            b"original unchanged"
        );
        reader.remove(&entry.key).unwrap();
        assert!(reader.list().unwrap().entries.is_empty());
    }
}
