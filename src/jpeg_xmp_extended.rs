use crate::source_metadata::{MAX_BYTES, Metadata};
use std::{collections::BTreeMap, io::Read, path::Path, sync::Arc};
const SIGNATURE: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
const NOTE: &str = "http://ns.adobe.com/xmp/note/";
pub(crate) const CHUNK_BYTES: usize = 65533 - (SIGNATURE.len() + 32 + 4 + 4);

pub(crate) struct Packets {
    pub basic: Vec<u8>,
    pub extended: Option<ExtendedPacket>,
}
pub(crate) struct ExtendedPacket {
    pub bytes: Vec<u8>,
    guid: String,
}
impl ExtendedPacket {
    pub(crate) fn chunk(&self, index: usize, part: &[u8]) -> Vec<u8> {
        let mut data = SIGNATURE.to_vec();
        data.extend(self.guid.as_bytes());
        data.extend((self.bytes.len() as u32).to_be_bytes());
        data.extend(((index * CHUNK_BYTES) as u32).to_be_bytes());
        data.extend(part);
        data
    }
}
pub(crate) fn prepare_output(
    doc: &crate::document::Document,
    include: bool,
) -> Result<Option<Packets>, String> {
    if !include {
        return Ok(None);
    }
    let Some(extended) = &doc.source_metadata.xmp_extended else {
        return crate::xmp_output::prepare(doc, true).map(|basic| {
            basic.map(|basic| Packets {
                basic,
                extended: None,
            })
        });
    };
    let basic = doc
        .source_metadata
        .xmp
        .as_deref()
        .ok_or("확장 XMP의 기본 원문이 없습니다")?;
    let guid = doc
        .source_metadata
        .xmp_extended_guid
        .as_deref()
        .ok_or("확장 XMP의 식별자가 없습니다")?;
    if !valid(extended, guid, Some(basic)) {
        return Err("확장 XMP 원문·기본 연결·MD5가 일치하지 않습니다".into());
    }
    let bytes = crate::xmp_output::normalize_extended(extended, [doc.width, doc.height])?;
    let guid = format!("{:x}", md5::compute(&bytes)).to_ascii_uppercase();
    let basic = crate::xmp_output::normalize_extended(basic, [doc.width, doc.height])?;
    let basic = replace_guid(&basic, &guid)?;
    if !valid(&bytes, &guid, Some(&basic)) {
        return Err("출력 확장 XMP 연결 검증 실패".into());
    }
    Ok(Some(Packets {
        basic,
        extended: Some(ExtendedPacket { bytes, guid }),
    }))
}
fn replace_guid(bytes: &[u8], guid: &str) -> Result<Vec<u8>, String> {
    let text = std::str::from_utf8(bytes).map_err(|e| e.to_string())?;
    let xml = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    let mut patches = vec![];
    for node in xml.descendants().filter(|n| n.is_element()) {
        for attr in node.attributes() {
            if attr.namespace() == Some(NOTE) && attr.name() == "HasExtendedXMP" {
                let range = attr.range();
                let raw = &text[range.clone()];
                let eq = raw.find('=').ok_or("확장 XMP 속성 범위 오류")?;
                let tail = &raw[eq + 1..];
                let trimmed = tail.trim_start();
                let quote = *trimmed
                    .as_bytes()
                    .first()
                    .ok_or("확장 XMP 속성 범위 오류")?;
                if !matches!(quote, b'\'' | b'"') || raw.as_bytes().last() != Some(&quote) {
                    return Err("확장 XMP 속성 범위 오류".into());
                }
                let at = range.start + eq + 1 + tail.len() - trimmed.len() + 1;
                patches.push((at..range.end - 1, guid.to_owned()));
            }
        }
        if node.has_tag_name((NOTE, "HasExtendedXMP")) {
            let children = node.children().filter(|n| n.is_text()).collect::<Vec<_>>();
            if children.is_empty() {
                return Err("확장 XMP 식별자 텍스트가 없습니다".into());
            }
            for (index, child) in children.iter().enumerate() {
                let range = child.range();
                let raw = &text[range.clone()];
                let value = if index != 0 {
                    String::new()
                } else if raw.starts_with("<![CDATA[") && raw.ends_with("]]>") {
                    format!("<![CDATA[{guid}]]>")
                } else {
                    guid.to_owned()
                };
                patches.push((range, value));
            }
        }
    }
    patches.sort_by_key(|(range, _)| range.start);
    if patches.windows(2).any(|p| p[0].0.end > p[1].0.start) {
        return Err("확장 XMP 식별자 범위가 겹칩니다".into());
    }
    let mut result = text.to_owned();
    for (range, value) in patches.into_iter().rev() {
        result.replace_range(range, &value);
    }
    if result.len() > MAX_BYTES || basic_guid(Some(result.as_bytes()))? != guid {
        return Err("출력 확장 XMP 식별자 오류".into());
    }
    Ok(result.into_bytes())
}

fn basic_guid(bytes: Option<&[u8]>) -> Result<String, String> {
    let text = std::str::from_utf8(bytes.ok_or("확장 XMP의 기본 패킷이 없습니다")?)
        .map_err(|e| e.to_string())?;
    let xml = roxmltree::Document::parse(text).map_err(|e| e.to_string())?;
    let mut values = vec![];
    for node in xml.descendants().filter(|n| n.is_element()) {
        for attribute in node.attributes() {
            if attribute.namespace() == Some(NOTE) && attribute.name() == "HasExtendedXMP" {
                values.push(attribute.value());
            }
        }
        if node.tag_name().namespace() == Some(NOTE) && node.tag_name().name() == "HasExtendedXMP" {
            values.push(node.text().ok_or("확장 XMP 식별자 값이 없습니다")?);
        }
    }
    if values.len() != 1 {
        return Err("확장 XMP 기본 연결 식별자가 없거나 중복됩니다".into());
    }
    let guid = values[0].trim();
    if guid.len() != 32 || !guid.bytes().all(|v| v.is_ascii_hexdigit()) {
        return Err("확장 XMP 식별자 형식 오류".into());
    }
    Ok(guid.to_ascii_uppercase())
}
pub(crate) fn valid(bytes: &[u8], guid: &str, basic: Option<&[u8]>) -> bool {
    bytes.len() <= MAX_BYTES
        && basic_guid(basic).is_ok_and(|v| v == guid)
        && format!("{:x}", md5::compute(bytes)).to_ascii_uppercase() == guid
}
pub(crate) fn remember_bytes(bytes: &[u8], metadata: &mut Metadata, notes: &mut Vec<String>) {
    store(
        read(
            std::io::Cursor::new(bytes),
            metadata.xmp.as_deref().map(|v| v.as_slice()),
        ),
        metadata,
        notes,
    );
}
pub(crate) fn remember_file(path: &Path, metadata: &mut Metadata, notes: &mut Vec<String>) {
    let result = std::fs::File::open(path)
        .map_err(|e| e.to_string())
        .and_then(|file| {
            read(
                std::io::BufReader::new(file),
                metadata.xmp.as_deref().map(|v| v.as_slice()),
            )
        });
    store(result, metadata, notes);
}
fn store(
    result: Result<Option<(Vec<u8>, String)>, String>,
    metadata: &mut Metadata,
    notes: &mut Vec<String>,
) {
    match result {
        Ok(Some((bytes, guid))) => {
            metadata.xmp_extended = Some(Arc::new(bytes));
            metadata.xmp_extended_guid = Some(guid);
            notes.push(
                "JPEG 확장 XMP의 조각·식별자·MD5를 확인하고 별도 원문을 프로젝트에 보존합니다."
                    .into(),
            );
        }
        Ok(None) => {}
        Err(error) => notes.push(format!(
            "JPEG 확장 XMP를 복원하지 못했습니다: {error}. 기본 XMP와 다른 원문·픽셀을 유지합니다."
        )),
    }
}
fn read(reader: impl Read, basic: Option<&[u8]>) -> Result<Option<(Vec<u8>, String)>, String> {
    let mut guid: Option<String> = None;
    let mut total: Option<usize> = None;
    let mut parts = BTreeMap::new();
    let mut received = 0usize;
    crate::jpeg_iptc::visit_headers(reader, |marker, bytes| {
        if marker != 0xe1 || !bytes.starts_with(SIGNATURE) {
            return Ok(());
        }
        let bytes = &bytes[SIGNATURE.len()..];
        if bytes.len() < 40 {
            return Err("확장 XMP 조각 헤더가 잘렸습니다".into());
        }
        let id = std::str::from_utf8(&bytes[..32])
            .map_err(|e| e.to_string())?
            .to_ascii_uppercase();
        if id.len() != 32 || !id.bytes().all(|v| v.is_ascii_hexdigit()) {
            return Err("확장 XMP 조각 식별자 오류".into());
        }
        let wanted = if let Some(guid) = &guid {
            guid.clone()
        } else {
            basic_guid(basic)?
        };
        if id != wanted {
            return Err("확장 XMP 조각 식별자와 기본 연결이 다릅니다".into());
        }
        guid = Some(wanted);
        let length = u32::from_be_bytes(bytes[32..36].try_into().unwrap()) as usize;
        let at = u32::from_be_bytes(bytes[36..40].try_into().unwrap()) as usize;
        let data = &bytes[40..];
        if length == 0
            || length > MAX_BYTES
            || data.is_empty()
            || at.checked_add(data.len()).is_none_or(|end| end > length)
        {
            return Err("확장 XMP 크기·조각 위치가 범위를 벗어납니다".into());
        }
        if total.is_some_and(|v| v != length) {
            return Err("확장 XMP 전체 크기가 조각마다 다릅니다".into());
        }
        total = Some(length);
        received += data.len();
        if received > length || parts.insert(at, data.to_vec()).is_some() {
            return Err("확장 XMP 조각이 중복되거나 전체 크기를 초과합니다".into());
        }
        Ok(())
    })?;
    let Some(guid) = guid else { return Ok(None) };
    let total = total.ok_or("확장 XMP 전체 크기가 없습니다")?;
    let mut bytes = Vec::with_capacity(total);
    for (at, part) in parts {
        if at != bytes.len() {
            return Err("확장 XMP 조각이 겹치거나 일부가 없습니다".into());
        }
        bytes.extend(part);
    }
    if bytes.len() != total || !valid(&bytes, &guid, basic) {
        return Err("확장 XMP 전체 크기 또는 MD5가 일치하지 않습니다".into());
    }
    Ok(Some((bytes, guid)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_jpeg_output_options_chunks_reimport_pixels_and_source_assets() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixtures = base.join("tests/fixtures/jpeg-extended-xmp");
        let root = base.join("qa-output/jpeg-extended-output");
        std::fs::create_dir_all(&root).unwrap();
        let cases: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap())
                .unwrap();
        let mut manifest = vec![];
        for case in cases {
            let name = case["file"].as_str().unwrap();
            let original = std::fs::read(fixtures.join(name)).unwrap();
            let opened = crate::files::open(&fixtures.join(name)).unwrap();
            let doc = opened.document;
            let metadata = doc.source_metadata.clone();
            for progressive in [false, true] {
                let mut baseline = None;
                for include in [false, true] {
                    let output_name = format!("{name}-p{progressive}-x{include}.jpg");
                    let output = root.join(&output_name);
                    let options = crate::files::ExportOptions {
                        include_xmp: include,
                        jpeg_progressive: progressive,
                        quality: 93,
                        ..Default::default()
                    };
                    crate::files::export(&doc, &output, image::ImageFormat::Jpeg, &options)
                        .unwrap();
                    let imported = crate::files::open(&output).unwrap();
                    let pixels = crate::precision::composite(&imported.document);
                    if let Some(baseline) = &baseline {
                        assert_eq!(&pixels, baseline);
                    } else {
                        baseline = Some(pixels);
                    }
                    assert_eq!(
                        imported.document.source_metadata.xmp_extended.is_some(),
                        include
                    );
                    if include {
                        let packets = prepare_output(&doc, true).unwrap().unwrap();
                        let extended = packets.extended.unwrap();
                        assert_eq!(
                            imported
                                .document
                                .source_metadata
                                .xmp
                                .as_deref()
                                .unwrap()
                                .as_slice(),
                            packets.basic
                        );
                        assert_eq!(
                            imported
                                .document
                                .source_metadata
                                .xmp_extended
                                .as_deref()
                                .unwrap()
                                .as_slice(),
                            extended.bytes
                        );
                        let project = root.join(format!("{output_name}.rphoto"));
                        crate::files::save_project(&imported.document, &project).unwrap();
                        assert_eq!(
                            crate::files::load_project(&project)
                                .unwrap()
                                .source_metadata
                                .xmp_extended,
                            imported.document.source_metadata.xmp_extended
                        );
                    }
                    manifest.push(serde_json::json!({"file":output_name,"source":name,"include":include,"progressive":progressive}));
                }
            }
            assert_eq!(doc.source_metadata.xmp, metadata.xmp);
            assert_eq!(doc.source_metadata.xmp_extended, metadata.xmp_extended);
            assert_eq!(
                doc.source_metadata.xmp_extended_guid,
                metadata.xmp_extended_guid
            );
            assert_eq!(std::fs::read(fixtures.join(name)).unwrap(), original);
        }
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.len(), 80);
    }

    #[test]
    fn output_normalizes_both_packets_regenerates_guid_preserves_unknown_and_atomic_failure() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jpeg-extended-geometry");
        std::fs::create_dir_all(&root).unwrap();
        let packet=b"<x xmlns:t='http://ns.adobe.com/tiff/1.0/' xmlns:e='http://ns.adobe.com/exif/1.0/' t:Orientation='8' t:ImageWidth='5' t:ImageLength='3'><!--keep--><e:PixelXDimension>5</e:PixelXDimension><e:PixelYDimension><![CDATA[3]]></e:PixelYDimension><unknown attr='preserve'>opaque</unknown></x>".to_vec();
        let guid = format!("{:x}", md5::compute(&packet)).to_ascii_uppercase();
        for element in [false, true] {
            let basic = if element {
                format!("<x xmlns:n='{NOTE}' xmlns:t='http://ns.adobe.com/tiff/1.0/' t:Orientation='6' t:ImageWidth='5' t:ImageLength='3'><n:HasExtendedXMP><![CDATA[{guid}]]></n:HasExtendedXMP><!--basic--></x>").into_bytes()
            } else {
                format!("<x xmlns:n='{NOTE}' xmlns:t='http://ns.adobe.com/tiff/1.0/' t:Orientation='6' t:ImageWidth='5' t:ImageLength='3' n:HasExtendedXMP = '{guid}'><!--basic--></x>").into_bytes()
            };
            let mut doc = crate::document::Document::from_image(
                "Geometry".into(),
                image::RgbaImage::from_pixel(9, 11, image::Rgba([71, 129, 201, 255])),
            );
            doc.source_metadata = Metadata::from_bytes(None, Some(basic.clone()), &mut vec![]);
            doc.source_metadata.xmp_extended = Some(Arc::new(packet.clone()));
            doc.source_metadata.xmp_extended_guid = Some(guid.clone());
            let name = format!("element{element}");
            std::fs::write(root.join(format!("{name}-source-basic.xmp")), &basic).unwrap();
            std::fs::write(root.join(format!("{name}-source-extended.xmp")), &packet).unwrap();
            let output = prepare_output(&doc, true).unwrap().unwrap();
            let extended = output.extended.unwrap();
            assert_ne!(extended.guid, guid);
            assert!(
                std::str::from_utf8(&extended.bytes)
                    .unwrap()
                    .contains("<!--keep-->")
            );
            assert!(
                std::str::from_utf8(&extended.bytes)
                    .unwrap()
                    .contains("attr='preserve'")
            );
            let path = root.join(format!("{name}.jpg"));
            crate::files::export(&doc, &path, image::ImageFormat::Jpeg, &Default::default())
                .unwrap();
            let reopened = crate::files::open(&path).unwrap();
            assert_eq!(
                reopened
                    .document
                    .source_metadata
                    .xmp_extended
                    .as_deref()
                    .unwrap()
                    .as_slice(),
                extended.bytes
            );
            assert_eq!(
                reopened
                    .document
                    .source_metadata
                    .xmp_extended_guid
                    .as_deref(),
                Some(extended.guid.as_str())
            );
            assert_eq!(
                doc.source_metadata.xmp.as_deref().unwrap().as_slice(),
                basic
            );
            assert_eq!(
                doc.source_metadata
                    .xmp_extended
                    .as_deref()
                    .unwrap()
                    .as_slice(),
                packet
            );
            let original = std::fs::read(&path).unwrap();
            doc.source_metadata.xmp_extended_guid = Some("0".repeat(32));
            assert!(
                crate::files::export(&doc, &path, image::ImageFormat::Jpeg, &Default::default())
                    .is_err()
            );
            assert_eq!(std::fs::read(&path).unwrap(), original);
        }
    }

    #[test]
    fn extended_xmp_jpeg_chunks_guid_pixels_history_project_and_export_guard() {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"));
        let fixtures = base.join("tests/fixtures/jpeg-extended-xmp");
        let root = base.join("qa-output/jpeg-extended-xmp");
        std::fs::create_dir_all(&root).unwrap();
        let cases: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(fixtures.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(cases.len(), 20);
        for case in cases {
            let name = case["file"].as_str().unwrap();
            let path = fixtures.join(name);
            let original = std::fs::read(&path).unwrap();
            let reference = crate::files::open(
                &base
                    .join("tests/fixtures/jpeg-iptc")
                    .join(case["base"].as_str().unwrap()),
            )
            .unwrap();
            let opened = crate::files::open(&path).unwrap();
            let metadata = opened.document.source_metadata.clone();
            let packet = metadata
                .xmp_extended
                .as_ref()
                .unwrap_or_else(|| panic!("{name}: {:?}", opened.notes));
            assert_eq!(
                packet.as_slice(),
                std::fs::read(fixtures.join(case["packet"].as_str().unwrap())).unwrap()
            );
            assert_eq!(metadata.xmp_extended_guid.as_deref(), case["guid"].as_str());
            assert_eq!(
                metadata.xmp.as_deref().unwrap().as_slice(),
                std::fs::read(fixtures.join(format!("{name}.basic.xmp"))).unwrap()
            );
            assert_eq!(metadata.exif, reference.document.source_metadata.exif);
            let pixels = crate::precision::composite(&opened.document);
            assert_eq!(pixels, crate::precision::composite(&reference.document));
            let mut editor = crate::document::Editor::new(opened.document, false);
            editor.edit("rename", |d| {
                d.layers[0].name = "Extended XMP retained".into()
            });
            editor.undo();
            editor.redo();
            assert!(Arc::ptr_eq(
                packet,
                editor
                    .document
                    .source_metadata
                    .xmp_extended
                    .as_ref()
                    .unwrap()
            ));
            let project = root.join(format!("{name}.rphoto"));
            crate::files::save_project(&editor.document, &project).unwrap();
            let restored = crate::files::load_project(&project).unwrap();
            assert_eq!(restored.source_metadata.xmp_extended, metadata.xmp_extended);
            assert_eq!(
                restored.source_metadata.xmp_extended_guid,
                metadata.xmp_extended_guid
            );
            assert_eq!(crate::precision::composite(&restored), pixels);
            let output = root.join(format!("{name}-guard.png"));
            std::fs::write(&output, b"existing output").unwrap();
            assert!(
                crate::files::export(
                    &restored,
                    &output,
                    image::ImageFormat::Png,
                    &Default::default()
                )
                .is_err()
            );
            assert_eq!(std::fs::read(&output).unwrap(), b"existing output");
            let output = root.join(format!("{name}-no-xmp.jpg"));
            crate::files::export(
                &restored,
                &output,
                image::ImageFormat::Jpeg,
                &crate::files::ExportOptions {
                    include_xmp: false,
                    ..Default::default()
                },
            )
            .unwrap();
            let exported = crate::files::open(&output).unwrap();
            assert!(
                exported.document.source_metadata.xmp.is_none()
                    && exported.document.source_metadata.xmp_extended.is_none()
            );
            assert_eq!(std::fs::read(path).unwrap(), original);
        }
    }

    fn basic(guid: &str) -> Vec<u8> {
        format!("<x xmlns:n='{NOTE}' n:HasExtendedXMP='{guid}'/>").into_bytes()
    }
    fn jpeg(parts: &[(String, u32, u32, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8];
        for (guid, total, at, part) in parts {
            let mut payload = SIGNATURE.to_vec();
            payload.extend(guid.as_bytes());
            payload.extend(total.to_be_bytes());
            payload.extend(at.to_be_bytes());
            payload.extend(part);
            bytes.extend([0xff, 0xe1]);
            bytes.extend(((payload.len() + 2) as u16).to_be_bytes());
            bytes.extend(payload);
        }
        bytes.extend([0xff, 0xda]);
        bytes
    }
    #[test]
    fn missing_overlapping_inconsistent_truncated_hash_and_budget_fail_without_other_metadata_loss()
    {
        let data = b"<packet>value</packet>".to_vec();
        let guid = format!("{:x}", md5::compute(&data)).to_ascii_uppercase();
        let total = data.len() as u32;
        let b = basic(&guid);
        let normal = (guid.clone(), total, 0, data.clone());
        assert_eq!(
            read(
                std::io::Cursor::new(jpeg(std::slice::from_ref(&normal))),
                Some(&b)
            )
            .unwrap()
            .unwrap()
            .0,
            data
        );
        let cases = vec![
            vec![(guid.clone(), total, 1, data[1..].to_vec())],
            vec![normal.clone(), normal.clone()],
            vec![
                (guid.clone(), total, 0, data[..10].to_vec()),
                (guid.clone(), total, 9, data[9..].to_vec()),
            ],
            vec![(guid.clone(), total, 0, data[..10].to_vec())],
            vec![
                (guid.clone(), total, 0, data[..10].to_vec()),
                (guid.clone(), total + 1, 10, data[10..].to_vec()),
            ],
            vec![(guid.clone(), MAX_BYTES as u32 + 1, 0, data.clone())],
            vec![(guid.clone(), total, u32::MAX, data.clone())],
            vec![("Z".repeat(32), total, 0, data.clone())],
            vec![("0".repeat(32), total, 0, data.clone())],
            vec![(guid.clone(), total, 0, vec![])],
            vec![(guid.clone(), total, 0, vec![0; data.len()])],
        ];
        for parts in cases {
            let result = read(std::io::Cursor::new(jpeg(&parts)), Some(&b));
            assert!(result.is_err());
            let mut metadata =
                Metadata::from_bytes(Some(b"EXIF".to_vec()), Some(b.clone()), &mut vec![]);
            metadata.photoshop = Some(Arc::new(b"IRB".to_vec()));
            let old = metadata.clone();
            let mut notes = vec![];
            store(result, &mut metadata, &mut notes);
            assert!(metadata.xmp_extended.is_none());
            assert_eq!(metadata.exif, old.exif);
            assert_eq!(metadata.xmp, old.xmp);
            assert_eq!(metadata.photoshop, old.photoshop);
            assert!(notes[0].contains("복원하지 못"));
        }
        assert!(read(std::io::Cursor::new(jpeg(&[normal])), None).is_err());
        assert!(basic_guid(Some(b"not XML")).is_err());
        assert!(
            basic_guid(Some(
                b"<x xmlns:n='http://ns.adobe.com/xmp/note/' n:HasExtendedXMP='1'/>"
            ))
            .is_err()
        );
    }

    #[test]
    fn extended_project_missing_crc_hash_guid_old_schema_and_atomic_save() {
        use std::io::Write;
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jpeg-extended-errors");
        std::fs::create_dir_all(&root).unwrap();
        let packet = b"<packet>value</packet>".to_vec();
        let guid = format!("{:x}", md5::compute(&packet)).to_ascii_uppercase();
        let mut doc = crate::document::Document::new(2, 2, true).unwrap();
        doc.source_metadata = Metadata::from_bytes(None, Some(basic(&guid)), &mut vec![]);
        doc.source_metadata.xmp_extended = Some(Arc::new(packet.clone()));
        doc.source_metadata.xmp_extended_guid = Some(guid);
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
        for label in [
            "missing",
            "oversize",
            "bad-hash",
            "bad-guid",
            "missing-guid",
            "old-version",
        ] {
            let path = root.join(format!("{label}.rphoto"));
            let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            for (name, bytes) in &entries {
                if label == "missing" && name == "metadata/source-extended.xmp" {
                    continue;
                }
                let mut data = bytes.clone();
                if name == "metadata/source-extended.xmp" {
                    if label == "oversize" {
                        data = vec![0; MAX_BYTES + 1];
                    } else if label == "bad-hash" {
                        data[0] ^= 1;
                    }
                }
                if name == "manifest.json"
                    && matches!(label, "bad-guid" | "missing-guid" | "old-version")
                {
                    let mut m: serde_json::Value = serde_json::from_slice(&data).unwrap();
                    match label {
                        "bad-guid" => {
                            m["source_metadata"]["xmp_extended_guid"] =
                                serde_json::json!("0".repeat(32))
                        }
                        "old-version" => m["version"] = serde_json::json!(14),
                        _ => {
                            m["source_metadata"]
                                .as_object_mut()
                                .unwrap()
                                .remove("xmp_extended_guid");
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
            assert!(crate::files::load_project(&path).is_err(), "{label}");
        }
        let mut corrupt = original.clone();
        let at = corrupt
            .windows(packet.len())
            .position(|v| v == packet)
            .unwrap();
        corrupt[at] ^= 1;
        let path = root.join("crc.rphoto");
        std::fs::write(&path, corrupt).unwrap();
        assert!(crate::files::load_project(&path).is_err());
        doc.source_metadata.xmp_extended = Some(Arc::new(vec![0; packet.len()]));
        assert!(crate::files::save_project(&doc, &valid).is_err());
        assert_eq!(std::fs::read(valid).unwrap(), original);
    }
}
