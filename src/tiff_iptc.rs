use crate::source_metadata::{MAX_BYTES, Metadata, TiffIptc};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
};

// Keep TIFF LONG storage bytes plus their word order. Do not silently byte-swap
// legacy IPTC tags or discard other Photoshop image resources.
pub(crate) fn remember(path: &Path, metadata: &mut Metadata, notes: &mut Vec<String>) {
    match read(path, notes) {
        Ok((iptc, photoshop)) => {
            if let Some((bytes, format)) = iptc {
                metadata.iptc = Some(Arc::new(bytes));
                metadata.iptc_tiff = Some(format);
                notes.push("원본 TIFF IPTC와 저장 형식을 프로젝트에 보존합니다.".into());
            }
            if let Some(bytes) = photoshop {
                metadata.photoshop = Some(Arc::new(bytes));
                notes.push("IPTC를 포함할 수 있는 원본 TIFF Photoshop 리소스 블록을 프로젝트에 보존합니다.".into());
            }
        }
        Err(error) => notes.push(format!("원본 TIFF IPTC/Photoshop 메타데이터를 읽지 못했습니다: {error}. 픽셀 읽기를 계속합니다.")),
    }
}
type Assets = (Option<(Vec<u8>, TiffIptc)>, Option<Vec<u8>>);
fn read(path: &Path, notes: &mut Vec<String>) -> Result<Assets, String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    let mut header = [0; 8];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    let little = matches!(&header[..4], b"II*\0" | b"II+\0");
    let big = matches!(&header[..4], b"II+\0" | b"MM\0+");
    if !matches!(&header[..4], b"II*\0" | b"II+\0" | b"MM\0*" | b"MM\0+") {
        return Err("TIFF 헤더 오류".into());
    }
    let number = |bytes: &[u8]| {
        if little {
            bytes
                .iter()
                .rev()
                .fold(0u64, |v, b| (v << 8) | u64::from(*b))
        } else {
            bytes.iter().fold(0u64, |v, b| (v << 8) | u64::from(*b))
        }
    };
    let (count_size, entry_size, inline) = if big { (8, 20, 8) } else { (2, 12, 4) };
    let root = if big {
        if number(&header[4..6]) != 8 || number(&header[6..8]) != 0 {
            return Err("BigTIFF 헤더 오류".into());
        }
        let mut offset = [0; 8];
        file.read_exact(&mut offset).map_err(|e| e.to_string())?;
        number(&offset)
    } else {
        number(&header[4..8])
    };
    if root < if big { 16 } else { 8 }
        || root
            .checked_add(count_size as u64)
            .is_none_or(|end| end > length)
    {
        return Err("TIFF 디렉터리 위치 오류".into());
    }
    file.seek(SeekFrom::Start(root))
        .map_err(|e| e.to_string())?;
    let mut count_bytes = vec![0; count_size];
    file.read_exact(&mut count_bytes)
        .map_err(|e| e.to_string())?;
    let count = number(&count_bytes);
    if count > 4096 {
        return Err("TIFF 메타데이터 디렉터리 항목 초과".into());
    }
    let mut entries = vec![0; count as usize * entry_size];
    file.read_exact(&mut entries).map_err(|e| e.to_string())?;
    let mut iptc = None;
    let mut photoshop = None;
    for entry in entries.chunks_exact(entry_size) {
        let tag = number(&entry[..2]);
        if !matches!(tag, 33723 | 34377) {
            continue;
        }
        let kind = number(&entry[2..4]) as u16;
        let result: Result<Vec<u8>, String> = (|| {
            let unit = match kind {
                1 | 7 => 1,
                4 if tag == 33723 => 4,
                _ => return Err("TIFF IPTC/Photoshop 태그 형식 오류".into()),
            };
            let size = number(&entry[4..4 + inline])
                .checked_mul(unit)
                .ok_or("IPTC 크기 초과")?;
            if size > MAX_BYTES as u64 {
                return Err("IPTC/Photoshop 보관 예산 초과".into());
            }
            let value = &entry[4 + inline..];
            let bytes = if size <= inline as u64 {
                value[..size as usize].to_vec()
            } else {
                let at = number(value);
                if at < if big { 16 } else { 8 }
                    || at.checked_add(size).is_none_or(|end| end > length)
                {
                    return Err("IPTC/Photoshop 원문 데이터가 잘렸습니다".into());
                }
                file.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
                let mut bytes = vec![0; size as usize];
                file.read_exact(&mut bytes).map_err(|e| e.to_string())?;
                bytes
            };
            Ok(bytes)
        })();
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(error) => {
                notes.push(format!("원본 TIFF 태그 {tag} 메타데이터를 읽지 못했습니다: {error}. 다른 원문과 픽셀을 유지합니다."));
                continue;
            }
        };
        if tag == 33723 {
            if iptc.is_some() {
                return Err("중복 IPTC 태그".into());
            }
            iptc = Some((bytes, TiffIptc { kind, little }));
        } else {
            if photoshop.is_some() {
                return Err("중복 Photoshop 태그".into());
            }
            photoshop = Some(bytes);
        }
    }
    Ok((iptc, photoshop))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iptc_tiff_native_storage_and_photoshop_history_project_roundtrip() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixtures = base.join("tests/fixtures/tiff-iptc");
        let root = base.join("qa-output/tiff-iptc");
        std::fs::create_dir_all(&root).unwrap();
        let cases: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(cases.len(), 80);
        for case in cases {
            let name = case["file"].as_str().unwrap();
            let path = fixtures.join(name);
            let original = std::fs::read(&path).unwrap();
            let opened = crate::files::open(&path).unwrap();
            let expected_photoshop = case["photoshop"].as_bool().unwrap();
            let expected_iptc = !case["kind"].is_null();
            let metadata = opened.document.source_metadata.clone();
            assert_eq!(
                metadata.iptc.is_some(),
                expected_iptc,
                "{name}: {:?}",
                opened.notes
            );
            assert_eq!(metadata.photoshop.is_some(), expected_photoshop, "{name}");
            if let Some(format) = &metadata.iptc_tiff {
                assert_eq!(format.kind, case["kind"].as_u64().unwrap() as u16);
                assert_eq!(format.little, case["endian"] == "<");
            }
            if let Some(photoshop) = &metadata.photoshop {
                assert_eq!(
                    photoshop.as_slice(),
                    std::fs::read(fixtures.join("reference.irb")).unwrap()
                );
            }
            let mut editor = crate::document::Editor::new(opened.document, false);
            let pixels = crate::precision::composite(&editor.document);
            editor.edit("rename", |d| d.layers[0].name = "IPTC retained".into());
            editor.undo();
            editor.redo();
            if let Some(iptc) = &metadata.iptc {
                assert!(Arc::ptr_eq(
                    iptc,
                    editor.document.source_metadata.iptc.as_ref().unwrap()
                ));
            }
            if let Some(photoshop) = &metadata.photoshop {
                assert!(Arc::ptr_eq(
                    photoshop,
                    editor.document.source_metadata.photoshop.as_ref().unwrap()
                ));
            }
            let project = root.join(format!("{name}.rphoto"));
            crate::files::save_project(&editor.document, &project).unwrap();
            let restored = crate::files::load_project(&project).unwrap();
            assert_eq!(restored.source_metadata.iptc, metadata.iptc);
            assert_eq!(restored.source_metadata.iptc_tiff, metadata.iptc_tiff);
            assert_eq!(restored.source_metadata.photoshop, metadata.photoshop);
            assert_eq!(crate::precision::composite(&restored), pixels);
            assert_eq!(std::fs::read(path).unwrap(), original);
        }
    }

    #[test]
    fn iptc_project_missing_oversize_crc_schema_and_atomic_save() {
        use std::io::Write;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/tiff-iptc-errors");
        std::fs::create_dir_all(&root).unwrap();
        let mut doc = crate::document::Document::new(2, 2, true).unwrap();
        doc.source_metadata.iptc = Some(Arc::new(b"IPTC-QA".to_vec()));
        doc.source_metadata.iptc_tiff = Some(TiffIptc {
            kind: 1,
            little: true,
        });
        doc.source_metadata.photoshop = Some(Arc::new(b"Photoshop-QA".to_vec()));
        let valid = root.join("valid.rphoto");
        crate::files::save_project(&doc, &valid).unwrap();
        let original = std::fs::read(&valid).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(&original)).unwrap();
        let mut entries = vec![];
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).unwrap();
            let mut bytes = vec![];
            entry.read_to_end(&mut bytes).unwrap();
            entries.push((entry.name().to_owned(), bytes));
        }
        for asset in ["iptc", "photoshop"] {
            for label in [
                "missing",
                "oversize",
                "old-version",
                "bad-kind",
                "missing-format",
            ] {
                let path = root.join(format!("{asset}-{label}.rphoto"));
                let mut writer = zip::ZipWriter::new(File::create(&path).unwrap());
                for (name, bytes) in &entries {
                    if label == "missing" && name == &format!("metadata/source.{asset}") {
                        continue;
                    }
                    let mut data = bytes.clone();
                    if label == "oversize" && name == &format!("metadata/source.{asset}") {
                        data = vec![0; MAX_BYTES + 1];
                    }
                    if name == "manifest.json"
                        && matches!(label, "old-version" | "bad-kind" | "missing-format")
                    {
                        let mut m: serde_json::Value = serde_json::from_slice(&data).unwrap();
                        match label {
                            "old-version" => m["version"] = serde_json::json!(13),
                            "bad-kind" => {
                                m["source_metadata"]["iptc_tiff"]["kind"] = serde_json::json!(99)
                            }
                            _ => {
                                m["source_metadata"]
                                    .as_object_mut()
                                    .unwrap()
                                    .remove("iptc_tiff");
                            }
                        }
                        data = serde_json::to_vec(&m).unwrap();
                    }
                    writer
                        .start_file(
                            name,
                            zip::write::SimpleFileOptions::default()
                                .compression_method(zip::CompressionMethod::Stored),
                        )
                        .unwrap();
                    writer.write_all(&data).unwrap();
                }
                writer.finish().unwrap();
                assert!(
                    crate::files::load_project(&path).is_err(),
                    "{asset}-{label}"
                );
            }
            let target = if asset == "iptc" {
                b"IPTC-QA".as_slice()
            } else {
                b"Photoshop-QA".as_slice()
            };
            let mut corrupt = original.clone();
            let at = corrupt
                .windows(target.len())
                .position(|v| v == target)
                .unwrap();
            corrupt[at] ^= 1;
            let path = root.join(format!("{asset}-crc.rphoto"));
            std::fs::write(&path, corrupt).unwrap();
            assert!(crate::files::load_project(&path).is_err());
        }
        doc.source_metadata.photoshop = Some(Arc::new(vec![0; MAX_BYTES + 1]));
        assert!(crate::files::save_project(&doc, &valid).is_err());
        assert_eq!(std::fs::read(valid).unwrap(), original);
    }

    #[test]
    fn bad_iptc_or_photoshop_tag_keeps_other_asset_pixels_and_original() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        let source = base.join("tests/fixtures/tiff-iptc/rgb8-both-b0-e0.tiff");
        let original = std::fs::read(&source).unwrap();
        let baseline = crate::files::open(&source).unwrap().document;
        let pixels = crate::precision::composite(&baseline);
        let root = u32::from_le_bytes(original[4..8].try_into().unwrap()) as usize;
        let count = u16::from_le_bytes(original[root..root + 2].try_into().unwrap()) as usize;
        let output = base.join("qa-output/tiff-iptc-invalid");
        std::fs::create_dir_all(&output).unwrap();
        for tag in [33723u16, 34377u16] {
            let at = (0..count)
                .map(|i| root + 2 + i * 12)
                .find(|i| u16::from_le_bytes(original[*i..*i + 2].try_into().unwrap()) == tag)
                .unwrap();
            for label in ["oversize", "outside", "bad-kind"] {
                let mut bytes = original.clone();
                match label {
                    "oversize" => bytes[at + 4..at + 8]
                        .copy_from_slice(&((MAX_BYTES + 1) as u32).to_le_bytes()),
                    "outside" => bytes[at + 8..at + 12]
                        .copy_from_slice(&(original.len() as u32).to_le_bytes()),
                    _ => bytes[at + 2..at + 4].copy_from_slice(&99u16.to_le_bytes()),
                }
                let path = output.join(format!("tag{tag}-{label}.tiff"));
                std::fs::write(&path, &bytes).unwrap();
                let opened = crate::files::open(&path).unwrap();
                assert!(
                    opened
                        .notes
                        .iter()
                        .any(|n| n.contains("메타데이터를 읽지 못"))
                );
                assert_eq!(opened.document.source_metadata.iptc.is_some(), tag != 33723);
                assert_eq!(
                    opened.document.source_metadata.photoshop.is_some(),
                    tag != 34377
                );
                if tag == 33723 {
                    assert_eq!(
                        opened.document.source_metadata.photoshop,
                        baseline.source_metadata.photoshop
                    );
                } else {
                    assert_eq!(
                        opened.document.source_metadata.iptc,
                        baseline.source_metadata.iptc
                    );
                }
                assert_eq!(crate::precision::composite(&opened.document), pixels);
                assert_eq!(std::fs::read(path).unwrap(), bytes);
            }
        }
        assert_eq!(std::fs::read(source).unwrap(), original);
    }
}
