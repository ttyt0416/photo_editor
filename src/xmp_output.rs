use std::{io::Write, ops::Range};
const TIFF: &str = "http://ns.adobe.com/tiff/1.0/";
const EXIF: &str = "http://ns.adobe.com/exif/1.0/";
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const XMP: &str = "http://ns.adobe.com/xap/1.0/";
fn error() -> String {
    "XMP 방향·치수 정보를 출력용으로 갱신하지 못했습니다. XMP 포함 옵션을 끄거나 원본 메타데이터를 확인하세요.".into()
}
pub fn prepare(
    document: &crate::document::Document,
    include: bool,
) -> Result<Option<Vec<u8>>, String> {
    if !include {
        return Ok(None);
    }
    if document.source_metadata.xmp_extended.is_some() {
        return Err("이 형식은 확장 XMP 저장을 아직 지원하지 않습니다. JPEG로 저장하거나 XMP 포함 옵션을 꺼 주세요. 원문은 프로젝트에 보존됩니다.".into());
    }
    document
        .source_metadata
        .xmp
        .as_ref()
        .map(|data| normalize(data, [document.width, document.height]))
        .transpose()
}
fn value(namespace: Option<&str>, name: &str, size: [u32; 2]) -> Option<u32> {
    match (namespace, name) {
        (Some(TIFF), "Orientation") => Some(1),
        (Some(TIFF), "ImageWidth") | (Some(EXIF), "PixelXDimension") => Some(size[0]),
        (Some(TIFF), "ImageLength") | (Some(EXIF), "PixelYDimension") => Some(size[1]),
        _ => None,
    }
}
fn scalar<'a, 'b>(node: roxmltree::Node<'a, 'b>) -> Result<roxmltree::Node<'a, 'b>, String> {
    if node.attribute((RDF, "resource")).is_some()
        || node.attribute((RDF, "nodeID")).is_some()
        || node
            .attribute((RDF, "parseType"))
            .is_some_and(|v| v != "Resource")
    {
        return Err(error());
    }
    if !node.children().any(|child| child.is_element()) {
        if node.attribute((RDF, "parseType")).is_some() {
            return Err(error());
        }
        return Ok(node);
    }
    let mut values = node
        .children()
        .filter(|child| child.has_tag_name((RDF, "value")));
    let target = values.next().ok_or_else(error)?;
    if values.next().is_some()
        || node
            .children()
            .any(|child| child.is_text() && child.text().is_some_and(|t| !t.trim().is_empty()))
        || target.children().any(|child| child.is_element())
    {
        return Err(error());
    }
    scalar(target)
}
fn normalize(bytes: &[u8], size: [u32; 2]) -> Result<Vec<u8>, String> {
    normalize_packet(bytes, size, false)
}

pub(crate) fn normalize_extended(bytes: &[u8], size: [u32; 2]) -> Result<Vec<u8>, String> {
    normalize_packet(bytes, size, true)
}

fn normalize_packet(bytes: &[u8], size: [u32; 2], extended: bool) -> Result<Vec<u8>, String> {
    if bytes.len() > crate::source_metadata::MAX_BYTES {
        return Err(error());
    }
    let xml = std::str::from_utf8(bytes).map_err(|_| error())?;
    let document = roxmltree::Document::parse(xml).map_err(|_| error())?;
    let mut patches: Vec<(Range<usize>, String)> = vec![];
    let removed = document
        .descendants()
        .filter(|n| n.has_tag_name((XMP, "Thumbnails")))
        .map(|n| n.range())
        .collect::<Vec<_>>();
    for range in &removed {
        patches.push((range.clone(), String::new()));
    }
    for node in document.descendants().filter(|n| n.is_element()) {
        if removed
            .iter()
            .any(|r| r.start <= node.range().start && r.end >= node.range().end)
        {
            continue;
        }
        if !extended
            && (node.has_tag_name(("http://ns.adobe.com/xmp/note/", "HasExtendedXMP"))
                || node.attributes().any(|a| {
                    a.namespace() == Some("http://ns.adobe.com/xmp/note/")
                        && a.name() == "HasExtendedXMP"
                }))
        {
            return Err("확장 XMP 원문을 함께 출력하는 지원이 필요합니다. XMP 포함 옵션을 끄면 사진을 저장할 수 있습니다.".into());
        }
        for attr in node.attributes() {
            if attr.namespace() == Some(XMP) && attr.name() == "Thumbnails" {
                patches.push((attr.range(), String::new()));
                continue;
            }
            if let Some(value) = value(attr.namespace(), attr.name(), size) {
                // Find quoted value bounds from the whole attribute range; do not
                // rely on the parser's shortened qname/spacing counters.
                let range = attr.range();
                let raw = xml.get(range.clone()).ok_or_else(error)?;
                let eq = raw.find('=').ok_or_else(error)?;
                let tail = &raw[eq + 1..];
                let trimmed = tail.trim_start();
                let quote = trimmed.as_bytes().first().ok_or_else(error)?;
                if !matches!(quote, b'\'' | b'"') || raw.as_bytes().last() != Some(quote) {
                    return Err(error());
                }
                let start = range.start + eq + 1 + tail.len() - trimmed.len() + 1;
                patches.push((start..range.end - 1, value.to_string()));
            }
        }
        if let Some(value) = value(node.tag_name().namespace(), node.tag_name().name(), size) {
            let target = scalar(node)?;
            let text = target
                .children()
                .filter(|n| n.is_text())
                .collect::<Vec<_>>();
            if text.is_empty() {
                let range = target.range();
                let raw = xml.get(range.clone()).ok_or_else(error)?;
                if raw.ends_with("/>") {
                    let name = raw[1..]
                        .split(|c: char| c.is_whitespace() || c == '/' || c == '>')
                        .next()
                        .ok_or_else(error)?;
                    patches.push((range.end - 2..range.end, format!(">{value}</{name}>")));
                } else {
                    let close = raw.rfind("</").ok_or_else(error)?;
                    patches.push((range.start + close..range.start + close, value.to_string()));
                }
            } else {
                for (i, text) in text.iter().enumerate() {
                    let range = text.range();
                    let raw = xml.get(range.clone()).ok_or_else(error)?;
                    let updated = if i != 0 {
                        String::new()
                    } else if raw.starts_with("<![CDATA[")
                        && raw.ends_with("]]>")
                        && !raw[9..raw.len() - 3].contains("]]>")
                    {
                        format!("<![CDATA[{value}]]>")
                    } else {
                        value.to_string()
                    };
                    patches.push((range, updated));
                }
            }
        }
    }
    patches.sort_by_key(|(range, _)| range.start);
    if patches
        .windows(2)
        .any(|pair| pair[0].0.end > pair[1].0.start)
    {
        return Err(error());
    }
    let mut result = xml.to_owned();
    for (range, value) in patches.into_iter().rev() {
        result.replace_range(range, &value);
    }
    if result.len() > crate::source_metadata::MAX_BYTES {
        return Err(error());
    }
    let checked = roxmltree::Document::parse(&result).map_err(|_| error())?;
    for node in checked.descendants().filter(|n| n.is_element()) {
        for attr in node.attributes() {
            if let Some(expected) = value(attr.namespace(), attr.name(), size)
                && attr.value().trim() != expected.to_string()
            {
                return Err(error());
            }
        }
        if let Some(expected) = value(node.tag_name().namespace(), node.tag_name().name(), size) {
            let text = scalar(node)?
                .children()
                .filter(|n| n.is_text())
                .filter_map(|n| n.text())
                .collect::<String>();
            if text.trim() != expected.to_string() {
                return Err(error());
            }
        }
    }
    Ok(result.into_bytes())
}

// Insert the UTF-8 packet immediately after IHDR while keeping the image
// encoder's existing IDAT stream and high-precision/filter options untouched.
pub struct PngWriter<'a, W: Write> {
    writer: &'a mut W,
    header: usize,
    chunk: Option<Vec<u8>>,
}
impl<'a, W: Write> PngWriter<'a, W> {
    pub fn new(writer: &'a mut W, xmp: Option<&[u8]>) -> Self {
        let chunk = xmp.map(|packet| {
            let mut data = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
            data.extend(packet);
            let mut crc = flate2::Crc::new();
            crc.update(b"iTXt");
            crc.update(&data);
            let mut chunk = (data.len() as u32).to_be_bytes().to_vec();
            chunk.extend(b"iTXt");
            chunk.extend(data);
            chunk.extend(crc.sum().to_be_bytes());
            chunk
        });
        Self {
            writer,
            header: 0,
            chunk,
        }
    }
}
impl<W: Write> Write for PngWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.chunk.is_none() {
            return self.writer.write(buf);
        }
        let head = (33 - self.header).min(buf.len());
        self.writer.write_all(&buf[..head])?;
        self.header += head;
        if self.header == 33 {
            self.writer.write_all(&self.chunk.take().unwrap())?;
        }
        self.writer.write_all(&buf[head..])?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }
}

#[cfg(test)]
#[path = "xmp_output_tests.rs"]
mod tests;
