use std::{
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    ops::Range,
    path::Path,
};

const SIGNATURE: &[u8] = b"\0\0\0\x0cJXL \x0d\x0a\x87\x0a";
struct BoxHeader {
    kind: [u8; 4],
    span: Range<u64>,
    body: u64,
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "JPEG XL 컨테이너 박스 길이가 손상되었습니다.",
    )
}
fn header(file: &mut File, length: u64) -> io::Result<Option<BoxHeader>> {
    let start = file.stream_position()?;
    if start == length {
        return Ok(None);
    }
    let mut bytes = [0; 8];
    file.read_exact(&mut bytes)?;
    let size = u32::from_be_bytes(bytes[..4].try_into().unwrap());
    let size = match size {
        0 => length.checked_sub(start).ok_or_else(invalid)?,
        1 => {
            let mut extended = [0; 8];
            file.read_exact(&mut extended)?;
            u64::from_be_bytes(extended)
        }
        size => u64::from(size),
    };
    let body = file.stream_position()?;
    let end = start
        .checked_add(size)
        .filter(|end| *end >= body && *end <= length)
        .ok_or_else(invalid)?;
    Ok(Some(BoxHeader {
        kind: bytes[4..8].try_into().unwrap(),
        span: start..end,
        body,
    }))
}

// Feed only the decoding boxes. The image library otherwise accumulates every
// auxiliary box (including decompressed brob) outside its pixel allocator.
pub struct PixelReader {
    file: File,
    length: u64,
    remaining: u64,
    raw: bool,
}
impl Read for PixelReader {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if out.is_empty() {
            return Ok(0);
        }
        if self.raw {
            return self.file.read(out);
        }
        while self.remaining == 0 {
            let Some(BoxHeader { kind, span, .. }) = header(&mut self.file, self.length)? else {
                return Ok(0);
            };
            if matches!(&kind, b"JXL " | b"ftyp" | b"jxlc" | b"jxlp") {
                self.file.seek(SeekFrom::Start(span.start))?;
                self.remaining = span.end - span.start;
            } else {
                self.file.seek(SeekFrom::Start(span.end))?;
            }
        }
        let count = out.len().min(self.remaining as usize);
        let used = self.file.read(&mut out[..count])?;
        if used == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.remaining -= used as u64;
        Ok(used)
    }
}

pub fn open(
    path: &Path,
    notes: &mut Vec<String>,
) -> Result<(PixelReader, crate::source_metadata::Metadata), String> {
    let mut file = File::open(path).map_err(|e| e.to_string())?;
    let length = file.metadata().map_err(|e| e.to_string())?.len();
    if length > crate::document::MAX_PIXELS * 16 {
        return Err("파일이 현재 메모리 예산을 초과합니다.".into());
    }
    let mut signature = [0; 12];
    let count = length.min(signature.len() as u64) as usize;
    file.read_exact(&mut signature[..count])
        .map_err(|e| e.to_string())?;
    let raw = signature[..count].starts_with(b"\xff\x0a");
    if !raw && signature[..count] != *SIGNATURE {
        return Err("JPEG XL 컨테이너 머리글이 손상되었습니다.".into());
    }
    file.rewind().map_err(|e| e.to_string())?;
    let mut assets: [Option<Vec<u8>>; 2] = [None, None];
    let mut seen = [false; 2];
    if !raw {
        let result = (|| -> io::Result<()> {
            while let Some(BoxHeader { kind, span, body }) = header(&mut file, length)? {
                if matches!(&kind, b"Exif" | b"xml ") {
                    let index = usize::from(&kind == b"xml ");
                    if seen[index] {
                        assets[index] = None;
                        notes.push(format!(
                            "JPEG XL에 여러 {} 박스가 있어 원본 보관을 생략했습니다.",
                            if index == 0 { "EXIF" } else { "XMP" }
                        ));
                    } else {
                        seen[index] = true;
                        let size = span.end - body;
                        if size
                            > (crate::source_metadata::MAX_BYTES + if index == 0 { 4 } else { 0 })
                                as u64
                        {
                            notes
                                .push("JPEG XL 원본 메타데이터가 보관 예산을 초과했습니다.".into());
                        } else {
                            let mut bytes = vec![0; size as usize];
                            file.read_exact(&mut bytes)?;
                            if index == 0 {
                                if let Some(exif) = exif(&bytes) {
                                    assets[index] = Some(exif.to_vec());
                                } else {
                                    notes.push("JPEG XL EXIF의 TIFF 시작 위치가 손상되어 원본 보관을 생략했습니다.".into());
                                }
                            } else {
                                assets[index] = Some(bytes);
                            }
                        }
                    }
                } else if &kind == b"brob" && span.end - body >= 4 {
                    let mut compressed_kind = [0; 4];
                    file.read_exact(&mut compressed_kind)?;
                    if matches!(&compressed_kind, b"Exif" | b"xml ") {
                        notes.push("JPEG XL의 Brotli 압축 EXIF/XMP 보관은 아직 지원하지 않습니다. 원본 파일을 함께 보관하세요.".into());
                        let index = usize::from(&compressed_kind == b"xml ");
                        seen[index] = true;
                        assets[index] = None;
                    }
                }
                file.seek(SeekFrom::Start(span.end))?;
            }
            Ok(())
        })();
        if let Err(error) = result {
            assets = [None, None];
            notes.push(format!(
                "JPEG XL의 원본 메타데이터 검색을 마치지 못했습니다: {error}"
            ));
        }
    }
    file.rewind().map_err(|e| e.to_string())?;
    let [exif, xmp] = assets;
    let metadata = crate::source_metadata::Metadata::from_bytes(exif, xmp, notes);
    Ok((
        PixelReader {
            file,
            length,
            remaining: 0,
            raw,
        },
        metadata,
    ))
}

fn exif(bytes: &[u8]) -> Option<&[u8]> {
    let offset = u32::from_be_bytes(bytes.get(..4)?.try_into().ok()?) as usize;
    let tiff = bytes.get(4usize.checked_add(offset)?..)?;
    matches!(tiff.get(..4), Some(b"II\x2a\0" | b"MM\0\x2a")).then_some(tiff)
}

#[cfg(test)]
#[path = "jxl_metadata_tests.rs"]
mod tests;
