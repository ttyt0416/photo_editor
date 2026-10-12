use crate::source_metadata::{MAX_BYTES, Metadata};
use std::{fs::File, io::Read, path::Path, sync::Arc};

const PHOTOSHOP: &[u8] = b"Photoshop 3.0\0";

pub(crate) fn remember(path: &Path, metadata: &mut Metadata, notes: &mut Vec<String>) {
    store(
        read(
            File::open(path)
                .map(std::io::BufReader::new)
                .map_err(|e| e.to_string()),
        ),
        metadata,
        notes,
    );
}

pub(crate) fn remember_bytes(bytes: &[u8], metadata: &mut Metadata, notes: &mut Vec<String>) {
    store(read(Ok(std::io::Cursor::new(bytes))), metadata, notes);
}

fn store(
    result: Result<Option<Vec<u8>>, String>,
    metadata: &mut Metadata,
    notes: &mut Vec<String>,
) {
    match result {
        Ok(Some(bytes)) => {
            metadata.photoshop=Some(Arc::new(bytes));
            notes.push("IPTC를 포함할 수 있는 원본 JPEG Photoshop 리소스를 프로젝트에 보존합니다.".into());
        }
        Ok(None) => {}
        Err(error) => notes.push(format!("원본 JPEG Photoshop/IPTC를 읽지 못했습니다: {error}. 다른 촬영정보와 픽셀을 유지합니다.")),
    }
}

fn read(reader: Result<impl Read, String>) -> Result<Option<Vec<u8>>, String> {
    let mut reader = reader?;
    let mut signature = [0; 2];
    reader
        .read_exact(&mut signature)
        .map_err(|e| e.to_string())?;
    if signature != [0xff, 0xd8] {
        return Err("JPEG 헤더 오류".into());
    }
    let mut resources = vec![];
    let mut found = false;
    let mut scanned = 2usize;
    for _ in 0..4096 {
        let mut marker = [0; 1];
        reader.read_exact(&mut marker).map_err(|e| e.to_string())?;
        scanned += 1;
        if marker[0] != 0xff {
            return Err("JPEG 마커 오류".into());
        }
        loop {
            reader.read_exact(&mut marker).map_err(|e| e.to_string())?;
            scanned += 1;
            if scanned > MAX_BYTES {
                return Err("JPEG 촬영정보 헤더 읽기 예산 초과".into());
            }
            if marker[0] != 0xff {
                break;
            }
        }
        match marker[0] {
            0xda | 0xd9 => return Ok(found.then_some(resources)), // Before first scan.
            0x01 | 0xd0..=0xd7 => continue,
            0x00 | 0xd8 => return Err("JPEG 마커 오류".into()),
            _ => {}
        }
        let mut length = [0; 2];
        reader.read_exact(&mut length).map_err(|e| e.to_string())?;
        let size = usize::from(u16::from_be_bytes(length));
        if size < 2 {
            return Err("JPEG 촬영정보 마커 길이 오류".into());
        }
        scanned = scanned
            .checked_add(size)
            .ok_or("JPEG 헤더 읽기 예산 초과")?;
        if scanned > MAX_BYTES {
            return Err("JPEG 촬영정보 헤더 읽기 예산 초과".into());
        }
        let mut bytes = vec![0; size - 2];
        reader.read_exact(&mut bytes).map_err(|e| e.to_string())?;
        if marker[0] == 0xed && bytes.starts_with(PHOTOSHOP) {
            found = true;
            let data = &bytes[PHOTOSHOP.len()..];
            if resources.len() + data.len() > MAX_BYTES {
                return Err("JPEG Photoshop 원문 보관 예산 초과".into());
            }
            resources.extend_from_slice(data);
        }
    }
    Err("JPEG 촬영정보 마커 개수 초과".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_photoshop_app13_split_resources_pixels_history_and_project() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixtures = base.join("tests/fixtures/jpeg-iptc");
        let root = base.join("qa-output/jpeg-iptc");
        std::fs::create_dir_all(&root).unwrap();
        let cases: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(cases.len(), 24);
        for case in cases {
            let name = case["file"].as_str().unwrap();
            let path = fixtures.join(name);
            let original = std::fs::read(&path).unwrap();
            let baseline =
                crate::files::open(&fixtures.join(case["base"].as_str().unwrap())).unwrap();
            assert!(baseline.document.source_metadata.photoshop.is_none());
            let opened = crate::files::open(&path).unwrap();
            let metadata = opened.document.source_metadata.clone();
            let resources = metadata.photoshop.as_ref().unwrap();
            assert_eq!(
                resources.as_slice(),
                std::fs::read(fixtures.join(case["resource"].as_str().unwrap())).unwrap()
            );
            assert_eq!(metadata.exif, baseline.document.source_metadata.exif);
            assert_eq!(metadata.xmp, baseline.document.source_metadata.xmp);
            assert!(metadata.iptc.is_none() && metadata.iptc_tiff.is_none());
            let pixels = crate::precision::composite(&opened.document);
            assert_eq!(pixels, crate::precision::composite(&baseline.document));
            let mut editor = crate::document::Editor::new(opened.document, false);
            editor.edit("rename", |d| d.layers[0].name = "APP13 retained".into());
            editor.undo();
            editor.redo();
            assert!(Arc::ptr_eq(
                resources,
                editor.document.source_metadata.photoshop.as_ref().unwrap()
            ));
            let project = root.join(format!("{name}.rphoto"));
            crate::files::save_project(&editor.document, &project).unwrap();
            let restored = crate::files::load_project(&project).unwrap();
            assert_eq!(restored.source_metadata.photoshop, metadata.photoshop);
            assert_eq!(crate::precision::composite(&restored), pixels);
            assert_eq!(std::fs::read(path).unwrap(), original);
        }
    }

    #[test]
    fn jpeg_app13_marker_lengths_fill_counts_and_budget_are_bounded() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jpeg-iptc-errors");
        std::fs::create_dir_all(&root).unwrap();
        for (label, bytes) in [
            ("not-jpeg", vec![0, 0]),
            ("no-marker", vec![0xff, 0xd8, 0]),
            ("zero-length", vec![0xff, 0xd8, 0xff, 0xed, 0, 0]),
            ("one-length", vec![0xff, 0xd8, 0xff, 0xed, 0, 1]),
            ("truncated", vec![0xff, 0xd8, 0xff, 0xed, 0, 100, 0]),
            ("bad-fill", vec![0xff, 0xd8, 0xff, 0]),
        ] {
            assert!(
                read(Ok(std::io::Cursor::new(bytes.clone()))).is_err(),
                "{label}"
            );
            std::fs::write(root.join(label), bytes).unwrap();
        }
        let mut too_many = vec![0xff, 0xd8];
        too_many.extend([0xff, 1].repeat(4097));
        assert!(read(Ok(std::io::Cursor::new(too_many))).is_err());
        let mut too_large = vec![0xff, 0xd8];
        too_large.extend(std::iter::repeat_n(0xff, MAX_BYTES));
        assert!(read(Ok(std::io::Cursor::new(too_large))).is_err());
        let mut length_budget = vec![0xff, 0xd8];
        let mut marker = vec![0xff, 0xed, 0xff, 0xff];
        marker.extend([0; 65533]);
        length_budget.extend(marker.repeat(257));
        assert!(read(Ok(std::io::Cursor::new(length_budget))).is_err());
        assert_eq!(
            read(Ok(std::io::Cursor::new(vec![0xff, 0xd8, 0xff, 0xda]))).unwrap(),
            None
        );
        assert_eq!(
            read(Ok(std::io::Cursor::new(vec![
                0xff, 0xd8, 0xff, 0xff, 0xff, 0xd9
            ])))
            .unwrap(),
            None
        );
    }
}
