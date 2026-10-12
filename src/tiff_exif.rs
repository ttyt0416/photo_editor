use std::{
    collections::HashSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub(crate) fn read(path: &Path, notes: &mut Vec<String>) -> Option<Vec<u8>> {
    match capture(path) {
        Ok((bytes, maker)) => {
            if maker {
                notes.push("TIFF MakerNote 원문 블록과 위치를 보존합니다. 블록 밖 제조사 전용 참조는 아직 보존하지 않습니다.".into());
            }
            bytes
        }
        Err(error) => {
            notes.push(format!(
                "원본 TIFF EXIF를 읽지 못했습니다: {error}. 픽셀 읽기를 계속합니다."
            ));
            None
        }
    }
}

struct Field {
    tag: u16,
    kind: u16,
    count: u32,
    bytes: Vec<u8>,
    offset: Option<usize>,
}
struct Capture {
    file: File,
    length: u64,
    little: bool,
    big: bool,
    read_bytes: usize,
    output: Vec<u8>,
    visited: HashSet<usize>,
    nodes: Vec<(usize, Vec<Field>)>,
    frames: Vec<(usize, usize)>,
    maker: bool,
}
impl Capture {
    fn number(&self, bytes: &[u8]) -> u64 {
        if self.little {
            bytes.iter().rev().fold(0, |v, b| (v << 8) | u64::from(*b))
        } else {
            bytes.iter().fold(0, |v, b| (v << 8) | u64::from(*b))
        }
    }
    fn encoded(&self, value: u32, width: usize) -> Vec<u8> {
        if self.little {
            value.to_le_bytes()[..width].to_vec()
        } else {
            value.to_be_bytes()[4 - width..].to_vec()
        }
    }
    fn reserve(&mut self, end: usize) -> Result<(), String> {
        if end > crate::source_metadata::MAX_BYTES {
            return Err("EXIF 참조 위치가 보관 예산을 초과합니다".into());
        }
        self.output.resize(self.output.len().max(end), 0);
        Ok(())
    }
    fn load(&mut self, at: usize, length: usize) -> Result<Vec<u8>, String> {
        self.read_bytes = self
            .read_bytes
            .checked_add(length)
            .ok_or("EXIF 읽기 예산 초과")?;
        let end = at.checked_add(length).ok_or("EXIF 범위 초과")?;
        if self.read_bytes > crate::source_metadata::MAX_BYTES || end as u64 > self.length {
            return Err("EXIF 데이터가 잘렸거나 읽기 예산을 초과합니다".into());
        }
        self.file
            .seek(SeekFrom::Start(at as u64))
            .map_err(|e| e.to_string())?;
        let mut bytes = vec![0; length];
        self.file
            .read_exact(&mut bytes)
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    }
    fn node(&mut self, at: usize, root: bool, depth: usize) -> Result<bool, String> {
        if at < if self.big { 16 } else { 8 }
            || depth > 8
            || self.visited.len() >= 16
            || !self.visited.insert(at)
        {
            return Err("EXIF 디렉터리 연결이 순환하거나 제한을 초과합니다".into());
        }
        let (count_size, entry_size, inline) = if self.big { (8, 20, 8) } else { (2, 12, 4) };
        let count_bytes = self.load(at, count_size)?;
        let count = self.number(&count_bytes);
        if count > 4096 {
            return Err("EXIF 디렉터리 항목이 너무 많습니다".into());
        }
        let raw = self.load(at + count_size, count as usize * entry_size + inline)?;
        let has_camera = !root
            || raw.chunks_exact(entry_size).take(count as usize).any(|e| {
                matches!(
                    self.number(&e[..2]),
                    271 | 272 | 306 | 315 | 33432 | 34665 | 34853
                )
            });
        if !has_camera {
            return Ok(false);
        }
        self.reserve(at + count_size + raw.len())?;
        self.frames.push((at, at + count_size + raw.len()));
        let mut fields = vec![];
        for entry in raw.chunks_exact(entry_size).take(count as usize) {
            let tag = self.number(&entry[..2]) as u16;
            if root
                && !matches!(
                    tag,
                    270 | 271
                        | 272
                        | 274
                        | 282
                        | 283
                        | 296
                        | 305
                        | 306
                        | 315
                        | 316
                        | 33432
                        | 34665
                        | 34853
                        | 40091..=40095
                )
            {
                continue;
            }
            let kind = self.number(&entry[2..4]) as u16;
            let count = self.number(&entry[4..4 + inline]);
            let count = u32::try_from(count).map_err(|_| "EXIF 항목 개수 초과")?;
            let pointer = matches!(tag, 34665 | 34853 | 40965);
            let unit = match kind {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 | 13 => 4,
                5 | 10 | 12 | 16 | 17 | 18 => 8,
                _ => return Err("EXIF 항목 형식 오류".into()),
            };
            let size = (count as usize)
                .checked_mul(unit)
                .ok_or("EXIF 항목 크기 초과")?;
            if size > crate::source_metadata::MAX_BYTES {
                return Err("EXIF 항목 보관 예산 초과".into());
            }
            let value = &entry[4 + inline..];
            let offset = if size > inline {
                Some(usize::try_from(self.number(value)).map_err(|_| "EXIF 위치 초과")?)
            } else {
                None
            };
            let bytes = if let Some(offset) = offset {
                self.reserve(offset.checked_add(size).ok_or("EXIF 위치 초과")?)?;
                let bytes = self.load(offset, size)?;
                self.output[offset..offset + size].copy_from_slice(&bytes);
                bytes
            } else {
                value[..size].to_vec()
            };
            let field = if pointer {
                if count != 1 || !matches!(kind, 4 | 13 | 16 | 18) {
                    return Err("EXIF 디렉터리 포인터 형식 오류".into());
                }
                let child = usize::try_from(self.number(&bytes)).map_err(|_| "EXIF 위치 초과")?;
                if child != 0 {
                    self.node(child, false, depth + 1)?;
                }
                Field {
                    tag,
                    kind: 4,
                    count: 1,
                    bytes: self.encoded(child as u32, 4),
                    offset: None,
                }
            } else {
                if matches!(kind, 13 | 16 | 17 | 18) {
                    return Err(
                        "EXIF의 확장 정수 또는 알 수 없는 포인터는 아직 보존하지 않습니다".into(),
                    );
                }
                self.maker |= tag == 37500;
                Field {
                    tag,
                    kind,
                    count,
                    bytes,
                    offset,
                }
            };
            fields.push(field);
        }
        self.nodes.push((at, fields));
        Ok(true)
    }
    fn finish(mut self, root: usize) -> Result<Vec<u8>, String> {
        for (_, fields) in &self.nodes {
            for field in fields {
                if let Some(at) = field.offset
                    && (at < if self.big { 16 } else { 8 }
                        || self
                            .frames
                            .iter()
                            .any(|(start, end)| at < *end && at + field.bytes.len() > *start))
                {
                    return Err("EXIF 값이 헤더 또는 디렉터리와 겹칩니다".into());
                }
            }
        }
        let header = [
            if self.little { b"II" } else { b"MM" }.as_slice(),
            self.encoded(42, 2).as_slice(),
            self.encoded(root as u32, 4).as_slice(),
        ]
        .concat();
        self.output[..8].copy_from_slice(&header);
        for (at, fields) in std::mem::take(&mut self.nodes) {
            let mut directory = self.encoded(fields.len() as u32, 2);
            for field in fields {
                directory.extend(self.encoded(u32::from(field.tag), 2));
                directory.extend(self.encoded(u32::from(field.kind), 2));
                directory.extend(self.encoded(field.count, 4));
                if field.bytes.len() <= 4 {
                    directory.extend(&field.bytes);
                    directory.resize(directory.len() + 4 - field.bytes.len(), 0);
                } else {
                    let offset = if let Some(offset) = field.offset {
                        offset
                    } else {
                        let offset = (self.output.len() + 1) & !1;
                        self.reserve(offset + field.bytes.len())?;
                        self.output[offset..offset + field.bytes.len()]
                            .copy_from_slice(&field.bytes);
                        offset
                    };
                    directory.extend(self.encoded(offset as u32, 4));
                }
            }
            directory.extend([0; 4]); // Source thumbnail/next-image IFDs are detached.
            self.output[at..at + directory.len()].copy_from_slice(&directory);
        }
        Ok(self.output)
    }
}
fn capture(path: &Path) -> Result<(Option<Vec<u8>>, bool), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    let mut header = [0; 8];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    let little = match &header[..4] {
        b"II*\0" | b"II+\0" => true,
        b"MM\0*" | b"MM\0+" => false,
        _ => return Err("TIFF 헤더 오류".into()),
    };
    let big = matches!(&header[..4], b"II+\0" | b"MM\0+");
    let mut capture = Capture {
        file,
        length,
        little,
        big,
        read_bytes: 8,
        output: vec![0; 8],
        visited: HashSet::new(),
        nodes: vec![],
        frames: vec![],
        maker: false,
    };
    let root = if big {
        if capture.number(&header[4..6]) != 8 || capture.number(&header[6..8]) != 0 {
            return Err("BigTIFF 헤더 오류".into());
        }
        let offset = capture.load(8, 8)?;
        capture.number(&offset)
    } else {
        capture.number(&header[4..8])
    };
    let root = usize::try_from(root).map_err(|_| "EXIF 위치 초과")?;
    if !capture.node(root, true, 0)? {
        return Ok((None, false));
    }
    let maker = capture.maker;
    Ok((Some(capture.finish(root)?), maker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_camera_metadata_notes_keep_pixels_and_original_file() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        let source = base.join("tests/fixtures/tiff-exif/rgb8-b0-e0-o6.tiff");
        let original = std::fs::read(&source).unwrap();
        let manifest: Vec<serde_json::Value> = serde_json::from_slice(
            &std::fs::read(base.join("tests/fixtures/tiff-exif/manifest.json")).unwrap(),
        )
        .unwrap();
        let case = manifest
            .iter()
            .find(|c| c["file"] == "rgb8-b0-e0-o6.tiff")
            .unwrap();
        let child = case["exif"].as_u64().unwrap() as usize;
        let parent = case["root"].as_u64().unwrap() as usize;
        let count = u16::from_le_bytes(original[parent..parent + 2].try_into().unwrap()) as usize;
        let pointer = (0..count)
            .map(|i| parent + 2 + i * 12)
            .find(|i| u16::from_le_bytes(original[*i..*i + 2].try_into().unwrap()) == 34665)
            .unwrap();
        let root = base.join("qa-output/tiff-exif-invalid");
        std::fs::create_dir_all(&root).unwrap();
        let pixels = crate::precision::composite(&crate::files::open(&source).unwrap().document);
        for label in [
            "cycle",
            "too-many",
            "bad-kind",
            "oversize",
            "header-value",
            "ifd-value",
            "outside",
            "count-overflow",
        ] {
            let mut bytes = original.clone();
            match label {
                "cycle" => {
                    bytes[pointer + 8..pointer + 12].copy_from_slice(&(parent as u32).to_le_bytes())
                }
                "too-many" => bytes[child..child + 2].copy_from_slice(&4097u16.to_le_bytes()),
                "bad-kind" => bytes[child + 4..child + 6].copy_from_slice(&99u16.to_le_bytes()),
                "oversize" => bytes[pointer + 8..pointer + 12].copy_from_slice(
                    &((crate::source_metadata::MAX_BYTES + 1) as u32).to_le_bytes(),
                ),
                "header-value" => {
                    bytes[child + 10..child + 14].copy_from_slice(&0u32.to_le_bytes())
                }
                "ifd-value" => {
                    bytes[child + 10..child + 14].copy_from_slice(&(child as u32).to_le_bytes())
                }
                "outside" => bytes[child + 10..child + 14]
                    .copy_from_slice(&(original.len() as u32).to_le_bytes()),
                _ => bytes[child + 6..child + 10].copy_from_slice(&u32::MAX.to_le_bytes()),
            }
            let path = root.join(format!("{label}.tiff"));
            std::fs::write(&path, &bytes).unwrap();
            let opened = crate::files::open(&path).unwrap();
            assert!(opened.document.source_metadata.exif.is_none(), "{label}");
            assert!(
                opened.notes.iter().any(|n| n.contains("EXIF를 읽지 못")),
                "{label}: {:?}",
                opened.notes
            );
            assert_eq!(
                crate::precision::composite(&opened.document),
                pixels,
                "{label}"
            );
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        assert_eq!(std::fs::read(source).unwrap(), original);
    }
}
