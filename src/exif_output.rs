pub fn prepare(
    document: &crate::document::Document,
    include: bool,
) -> Result<Option<Vec<u8>>, String> {
    if !include {
        return Ok(None);
    }
    document
        .source_metadata
        .exif
        .as_ref()
        .map(|bytes| normalize(bytes, document.width, document.height))
        .transpose()
}
fn normalize(bytes: &[u8], width: u32, height: u32) -> Result<Vec<u8>, String> {
    let error = || {
        "원본 EXIF 구조를 출력용으로 갱신하지 못했습니다. EXIF 포함 옵션을 끄거나 원본 메타데이터를 확인하세요.".to_owned()
    };
    if bytes.len() > crate::source_metadata::MAX_BYTES {
        return Err(error());
    }
    let little = match bytes.get(..4) {
        Some(b"II\x2a\0") => true,
        Some(b"MM\0\x2a") => false,
        _ => return Err(error()),
    };
    let word = |data: &[u8], at: usize| -> Result<u16, String> {
        let v = data.get(at..at + 2).ok_or_else(error)?.try_into().unwrap();
        Ok(if little {
            u16::from_le_bytes(v)
        } else {
            u16::from_be_bytes(v)
        })
    };
    let long = |data: &[u8], at: usize| -> Result<u32, String> {
        let v = data.get(at..at + 4).ok_or_else(error)?.try_into().unwrap();
        Ok(if little {
            u32::from_le_bytes(v)
        } else {
            u32::from_be_bytes(v)
        })
    };
    let root = long(bytes, 4)? as usize;
    if root < 8 {
        return Err(error());
    }
    let mut result = bytes.to_vec();
    let mut child = None;
    for is_root in [true, false] {
        let at = if is_root {
            root
        } else if let Some(at) = child {
            at
        } else {
            break;
        };
        if !is_root && at == root {
            return Err(error());
        }
        let count = usize::from(word(&result, at)?);
        let end = at.checked_add(2 + count * 12 + 4).ok_or_else(error)?;
        if end > result.len() {
            return Err(error());
        }
        for index in 0..count {
            let entry = at + 2 + index * 12;
            let tag = word(&result, entry)?;
            let kind = word(&result, entry + 2)?;
            let n = long(&result, entry + 4)?;
            if is_root && tag == 0x8769 {
                if kind != 4 || n != 1 {
                    return Err(error());
                }
                let offset = long(&result, entry + 8)? as usize;
                if offset != 0 {
                    if offset < 8 {
                        return Err(error());
                    }
                    child = Some(offset);
                }
                continue;
            }
            let value = match tag {
                0x112 => Some(1),
                0x100 | 0xa002 => Some(width),
                0x101 | 0xa003 => Some(height),
                _ => None,
            };
            if let Some(value) = value {
                if n != 1 || !matches!(kind, 3 | 4) || (tag == 0x112 && kind != 3) {
                    return Err(error());
                }
                if kind == 3 && value <= u16::MAX as u32 {
                    result[entry + 8..entry + 10].copy_from_slice(&if little {
                        (value as u16).to_le_bytes()
                    } else {
                        (value as u16).to_be_bytes()
                    });
                } else {
                    result[entry + 2..entry + 4].copy_from_slice(&if little {
                        4u16.to_le_bytes()
                    } else {
                        4u16.to_be_bytes()
                    });
                    result[entry + 8..entry + 12].copy_from_slice(&if little {
                        value.to_le_bytes()
                    } else {
                        value.to_be_bytes()
                    });
                }
            }
        }
        // The original thumbnail describes the source image, so detach its IFD.
        // In-place updates keep opaque MakerNote offsets and the source bytes intact.
        if is_root {
            result[end - 4..end].fill(0);
        }
    }
    Ok(result)
}

#[cfg(test)]
#[path = "exif_output_tests.rs"]
mod tests;
