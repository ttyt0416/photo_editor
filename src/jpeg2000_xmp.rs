use std::io::{Seek, SeekFrom, Write};

// Adobe XMP Specification Part 3, JPEG 2000 UUID box.
const UUID: [u8; 16] = [
    0xbe, 0x7a, 0xcf, 0xcb, 0x97, 0xa9, 0x42, 0xe8, 0x9c, 0x71, 0x99, 0x94, 0x91, 0xe3, 0xaf, 0xac,
];

fn packet(bytes: &[u8]) -> Result<Option<&[u8]>, String> {
    if bytes.starts_with(&[0xff, 0x4f]) {
        return Ok(None);
    }
    let mut at = 0;
    let mut result = None;
    while at < bytes.len() {
        let (kind, body, next) = crate::jpeg2000_icc::next_box(bytes, at)?;
        at = next;
        if &kind != b"uuid" {
            continue;
        }
        if body.len() < UUID.len() {
            return Err("JP2 UUID 박스가 잘렸습니다.".into());
        }
        if body[..UUID.len()] == UUID {
            if result.is_some() {
                return Err("JP2에 여러 XMP 박스가 있어 원본을 하나로 결정할 수 없습니다.".into());
            }
            let data = &body[UUID.len()..];
            if data.len() > crate::source_metadata::MAX_BYTES {
                return Err("JP2 XMP가 보관 예산을 초과합니다.".into());
            }
            result = Some(data);
        }
    }
    Ok(result)
}

pub fn read(bytes: &[u8], notes: &mut Vec<String>) -> crate::source_metadata::Metadata {
    match packet(bytes) {
        Ok(data) => crate::source_metadata::Metadata::from_bytes(None, data.map(Vec::from), notes),
        Err(error) => {
            notes.push(format!("원본 JP2 XMP를 읽지 못했습니다: {error}"));
            Default::default()
        }
    }
}

pub fn append(file: &mut std::fs::File, packet: Option<&[u8]>) -> Result<(), String> {
    let Some(packet) = packet else { return Ok(()) };
    let length = u32::try_from(packet.len() + 24).map_err(|e| e.to_string())?;
    // Append only after OpenJPEG finishes. The UUID is outside the encoder's
    // rate budget, keeping every quality layer's codestream identical.
    file.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    file.write_all(&length.to_be_bytes())
        .map_err(|e| e.to_string())?;
    file.write_all(b"uuid").map_err(|e| e.to_string())?;
    file.write_all(&UUID).map_err(|e| e.to_string())?;
    file.write_all(packet).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "jpeg2000_xmp_tests.rs"]
mod tests;
