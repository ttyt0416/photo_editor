//! Capture image dependencies into editable SVG source. Rendering never reads a working directory.
use base64::Engine;
use image::{ImageDecoder, ImageFormat, ImageReader};
use std::{
    collections::HashMap,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

const LIMIT: usize = 64 * 1024 * 1024;
const DEPTH: usize = 32;
const XLINK: &str = "http://www.w3.org/1999/xlink";
const XML: &str = "http://www.w3.org/XML/1998/namespace";

#[derive(Clone)]
struct Asset {
    uri: String,
    original: Option<String>,
    name: String,
}
#[derive(Default)]
struct Capture {
    used: usize,
    stack: Vec<PathBuf>,
    cache: HashMap<PathBuf, Asset>,
    notes: Vec<String>,
    count: usize,
}
fn charge(used: &mut usize, size: usize) -> Result<(), String> {
    *used = used
        .checked_add(size)
        .filter(|n| *n <= LIMIT)
        .ok_or("SVG 이미지 자산이 현재 64MiB 보관 예산을 초과합니다.")?;
    Ok(())
}
fn read(path: &Path) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| format!("SVG 이미지 {}: {e}", path.display()))?
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > LIMIT {
        return Err("SVG 소스/이미지가 현재 64MiB 보관 예산을 초과합니다.".into());
    }
    Ok(bytes)
}
fn data(mime: &str, bytes: &[u8]) -> String {
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}
fn decode_data(value: &str) -> Result<(String, Vec<u8>), String> {
    let url = data_url::DataUrl::process(value).map_err(|e| format!("SVG 데이터 이미지: {e}"))?;
    let mime = url
        .mime_type()
        .to_string()
        .split(';')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let (bytes, fragment) = url
        .decode_to_vec()
        .map_err(|e| format!("SVG 데이터 이미지: {e}"))?;
    if fragment.is_some() {
        return Err("SVG 이미지의 외부 조각 참조는 아직 지원하지 않습니다.".into());
    }
    Ok((mime, bytes))
}
/// A file URI or URI-encoded local reference; XML base uses file-vs-directory semantics.
fn local(value: &str) -> Result<PathBuf, String> {
    if value.contains(['?', '#']) {
        return Err("SVG 이미지의 쿼리/외부 조각 참조는 아직 지원하지 않습니다.".into());
    }
    let value = if value
        .get(..5)
        .is_some_and(|s| s.eq_ignore_ascii_case("file:"))
    {
        let rest = &value[5..];
        if let Some(rest) = rest.strip_prefix("//") {
            let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
            if host.is_empty() || host.eq_ignore_ascii_case("localhost") {
                #[cfg(windows)]
                {
                    if path.as_bytes().get(1) == Some(&b':') {
                        path.to_owned()
                    } else {
                        format!("/{path}")
                    }
                }
                #[cfg(not(windows))]
                {
                    format!("/{path}")
                }
            } else {
                format!("//{host}/{path}")
            }
        } else {
            rest.to_owned()
        }
    } else {
        let drive =
            value.as_bytes().get(1) == Some(&b':') && value.as_bytes()[0].is_ascii_alphabetic();
        if !drive && value.contains(':') {
            return Err(format!(
                "SVG 원격 이미지 주소는 아직 가져올 수 없습니다: {value}"
            ));
        }
        value.to_owned()
    };
    let value = percent_encoding::percent_decode_str(&value)
        .decode_utf8()
        .map_err(|_| "SVG 이미지 경로의 UTF-8 인코딩을 확인하세요.")?;
    if value.contains('\0') {
        return Err("SVG 이미지 경로가 잘못되었습니다.".into());
    }
    Ok(PathBuf::from(value.as_ref()))
}
fn resolve(base: &Path, value: &str) -> Result<PathBuf, String> {
    let path = local(value)?;
    let mut result = if path.is_absolute() {
        path
    } else {
        base.parent().unwrap_or(Path::new("")).join(path)
    };
    if value.ends_with(['/', '\\']) {
        result.push(".rphoto-base");
    }
    Ok(result)
}
fn image_node(node: roxmltree::Node<'_, '_>) -> bool {
    node.is_element()
        && matches!(
            node.tag_name().namespace(),
            None | Some("http://www.w3.org/2000/svg")
        )
        && matches!(node.tag_name().name(), "image" | "feImage")
}
fn href<'a, 'b>(node: roxmltree::Node<'a, 'b>) -> Option<roxmltree::Attribute<'a, 'b>> {
    node.attributes()
        .find(|a| a.name() == "href" && a.namespace().is_none())
        .or_else(|| {
            node.attributes()
                .find(|a| a.name() == "href" && a.namespace() == Some(XLINK))
        })
}
fn reader<'a>(bytes: &'a [u8], mime: &str) -> Result<ImageReader<Cursor<&'a [u8]>>, String> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let expected = match mime {
        "image/png" => ImageFormat::Png,
        "image/jpeg" | "image/jpg" => ImageFormat::Jpeg,
        "image/gif" => ImageFormat::Gif,
        "image/webp" => ImageFormat::WebP,
        _ => return Err("SVG 직접 표시 이미지 형식을 확인하세요.".into()),
    };
    if reader.format() != Some(expected) {
        return Err("SVG 데이터 이미지의 형식과 내용이 다릅니다.".into());
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(crate::document::MAX_PIXELS * 16);
    reader.limits(limits);
    Ok(reader)
}
fn raster(bytes: &[u8], mime: &str, decode: bool) -> Result<(), String> {
    if decode && matches!(mime, "image/jpeg" | "image/jpg") {
        crate::jpeg_output::read_bytes(bytes)?;
        return Ok(());
    }
    let decoder = reader(bytes, mime)?
        .into_decoder()
        .map_err(|e| e.to_string())?;
    let (w, h) = decoder.dimensions();
    crate::document::validate_size(w, h)?;
    if decode {
        image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    }
    Ok(())
}
/// Normalize EXIF, GIF logical canvas, first animation frame and high-bit PNG consistently
/// with the common image loader. Keep encoded originals in the editable SVG source.
fn native_frame(bytes: &[u8], mime: &str) -> Result<Option<Vec<u8>>, String> {
    let image = if matches!(mime, "image/jpeg" | "image/jpg") {
        crate::jpeg_output::read_bytes(bytes)?.0
    } else {
        let mut decoder = reader(bytes, mime)?
            .into_decoder()
            .map_err(|e| e.to_string())?;
        let (w, h) = decoder.dimensions();
        crate::document::validate_size(w, h)?;
        let orientation = decoder.orientation().map_err(|e| e.to_string())?;
        let profile = decoder.icc_profile().map_err(|e| e.to_string())?;
        let color = decoder.color_type();
        let animated = match mime {
            "image/png" => image::codecs::png::PngDecoder::new(Cursor::new(bytes))
                .map_err(|e| e.to_string())?
                .is_apng()
                .map_err(|e| e.to_string())?,
            "image/webp" => image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))
                .map_err(|e| e.to_string())?
                .has_animation(),
            _ => false,
        };
        if mime != "image/gif"
            && color.bits_per_pixel() / u16::from(color.channel_count()) <= 8
            && orientation == image::metadata::Orientation::NoTransforms
            && !animated
            && profile.is_none()
        {
            return Ok(None);
        }
        let mut image = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
        image.apply_orientation(orientation);
        if let Some(profile) = profile {
            image = crate::color_management::apply(image, &profile, &mut vec![]);
        }
        image
    };
    let mut output = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image.to_rgba8())
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(Some(output.into_inner()))
}
fn expanded(bytes: &[u8], used: &mut usize) -> Result<Vec<u8>, String> {
    if !bytes.starts_with(&[0x1f, 0x8b]) {
        return Ok(bytes.to_vec());
    }
    let mut output = vec![];
    flate2::read::GzDecoder::new(bytes)
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut output)
        .map_err(|e| format!("압축 SVG: {e}"))?;
    charge(used, output.len())?;
    Ok(output)
}
/// Path-only codecs receive a private copy of the captured bytes, never a second read of the source.
struct CopyFile(PathBuf);
impl Drop for CopyFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
fn preview(bytes: &[u8], extension: &str) -> Result<(Vec<u8>, Vec<String>), String> {
    let path = std::env::temp_dir().join(format!(
        "photo-editor-svg-{}.{}",
        crate::document_id::new(),
        extension
    ));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&path)
        .map_err(|e| e.to_string())?;
    let temporary = CopyFile(path);
    let written = file.write_all(bytes).map_err(|e| e.to_string());
    drop(file);
    written?;
    let opened = crate::files::open(&temporary.0)?;
    let pixels = opened.document.composite();
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(pixels)
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok((png.into_inner(), opened.notes))
}
impl Capture {
    fn file(&mut self, path: &Path, depth: usize) -> Result<Asset, String> {
        let path = std::fs::canonicalize(path)
            .map_err(|e| format!("SVG 이미지 {}: {e}", path.display()))?;
        if self.stack.contains(&path) {
            return Err(format!("SVG 이미지가 순환 참조합니다: {}", path.display()));
        }
        if let Some(asset) = self.cache.get(&path) {
            return Ok(asset.clone());
        }
        let bytes = read(&path)?;
        charge(&mut self.used, bytes.len())?;
        self.stack.push(path.clone());
        let result = self.asset(&bytes, "", &path, depth);
        self.stack.pop();
        let asset = result?;
        self.cache.insert(path, asset.clone());
        Ok(asset)
    }
    fn asset(
        &mut self,
        bytes: &[u8],
        mime: &str,
        path: &Path,
        depth: usize,
    ) -> Result<Asset, String> {
        if depth > DEPTH {
            return Err("SVG 이미지 중첩이 현재 보관 예산을 초과합니다.".into());
        }
        let extension = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        let svg = mime == "image/svg+xml" || matches!(extension.as_str(), "svg" | "svgz");
        if svg {
            let original_bytes = bytes;
            let expanded;
            let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
                expanded = {
                    let mut output = vec![];
                    flate2::read::GzDecoder::new(bytes)
                        .take((LIMIT + 1) as u64)
                        .read_to_end(&mut output)
                        .map_err(|e| e.to_string())?;
                    charge(&mut self.used, output.len())?;
                    output
                };
                expanded.as_slice()
            } else {
                bytes
            };
            let source = std::str::from_utf8(bytes)
                .map_err(|_| "SVG 이미지의 UTF-8 인코딩을 확인하세요.")?;
            let captured = self.source(source, path, depth)?;
            return Ok(Asset {
                uri: data("image/svg+xml", captured.as_bytes()),
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                original: (captured != source || original_bytes.starts_with(&[0x1f, 0x8b]))
                    .then(|| data("application/octet-stream", original_bytes)),
            });
        }
        let native = match image::guess_format(bytes).ok() {
            Some(ImageFormat::Png) => Some("image/png"),
            Some(ImageFormat::Jpeg) => Some("image/jpeg"),
            Some(ImageFormat::Gif) => Some("image/gif"),
            Some(ImageFormat::WebP) => Some("image/webp"),
            _ => None,
        };
        if let Some(mime) = native {
            if let Some(png) = native_frame(bytes, mime)? {
                self.notes.push(format!("SVG 이미지 {}의 방향·첫 화면·8비트 표시를 공통 이미지 열기와 맞췄습니다. 원본 바이트/메타데이터/프레임은 함께 보관하며 SVG 애니메이션/고정밀·ICC 표시는 남은 목표입니다.",path.file_name().unwrap_or_default().to_string_lossy()));
                return Ok(Asset {
                    uri: data("image/png", &png),
                    original: Some(data("application/octet-stream", bytes)),
                    name: path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                });
            }
            raster(bytes, mime, true)?;
            return Ok(Asset {
                uri: data(mime, bytes),
                original: None,
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            });
        }
        if !crate::files::IMAGE_EXTENSIONS.contains(&extension.as_str()) {
            return Err(format!(
                "SVG 연결 이미지 형식을 확인하세요: {}",
                path.display()
            ));
        }
        let (png, notes) = preview(bytes, &extension)?;
        self.notes.extend(notes.into_iter().map(|note|note.replace("32비트 작업 버퍼와 RAW 원본을 프로젝트에 보존합니다.","SVG에서는 PNG 미리보기로 표시하고 RAW 원본 바이트를 SVG 속성과 프로젝트에 보관합니다.")));
        self.notes.push(format!("SVG 연결 이미지 {}는 현재 렌더러의 PNG 미리보기로 표시하며, 원본 바이트를 SVG 속성과 프로젝트에 함께 보관합니다. 고비트/HDR/ICC·여러 장의 네이티브 SVG 표시는 남은 목표입니다.", path.file_name().unwrap_or_default().to_string_lossy()));
        Ok(Asset {
            uri: data("image/png", &png),
            original: Some(data("application/octet-stream", bytes)),
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        })
    }
    fn source(&mut self, source: &str, path: &Path, depth: usize) -> Result<String, String> {
        if depth > DEPTH || source.len() > LIMIT {
            return Err("SVG 소스/중첩이 현재 보관 예산을 초과합니다.".into());
        }
        let doc = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
        let mut patches = Vec::new();
        let mut size = source.len();
        for node in doc.descendants().filter(|n| image_node(*n)) {
            let Some(selected) = href(node) else { continue };
            let value = selected.value().trim();
            if value.is_empty() || value.starts_with('#') {
                continue;
            }
            let asset = if value.starts_with("data:") {
                let (mime, bytes) = decode_data(value)?;
                charge(&mut self.used, bytes.len())?;
                let name = if mime == "image/svg+xml" {
                    path.to_owned()
                } else {
                    let ext = match mime.as_str() {
                        "image/tiff" => "tiff",
                        "image/bmp" => "bmp",
                        "image/avif" => "avif",
                        "image/jxl" => "jxl",
                        _ => "bin",
                    };
                    path.with_extension(ext)
                };
                self.asset(&bytes, &mime, &name, depth + 1)?
            } else {
                let mut base = path.to_owned();
                for ancestor in node.ancestors().collect::<Vec<_>>().into_iter().rev() {
                    if let Some(value) = ancestor.attribute((XML, "base")) {
                        base = resolve(&base, value)?;
                    }
                }
                self.file(&resolve(&base, value)?, depth + 1)?
            };
            self.count += 1;
            // Normalize both spellings to SVG2's preferred unqualified href. Some renderers
            // otherwise choose xlink:href according to XML attribute order.
            for attr in node
                .attributes()
                .filter(|a| a.name() == "href" && matches!(a.namespace(), None | Some(XLINK)))
            {
                let replacement = crate::vector::escape(&asset.uri);
                size = size
                    .checked_sub(attr.range_value().len())
                    .and_then(|n| n.checked_add(replacement.len()))
                    .filter(|n| *n <= LIMIT)
                    .ok_or("SVG 내장 이미지가 현재 64MiB 소스 예산을 초과합니다.")?;
                patches.push((attr.range_value(), replacement));
            }
            let mut metadata = Vec::new();
            if !value.starts_with("data:") {
                metadata.push(("data-rphoto-image-name", asset.name.clone()));
                metadata.push(("data-rphoto-image-href", value.to_owned()));
            }
            if let Some(original) = asset.original {
                metadata.push(("data-rphoto-image-original", original));
                if node.attribute("data-rphoto-image-name").is_none() && value.starts_with("data:")
                {
                    metadata.push(("data-rphoto-image-name", asset.name));
                }
            }
            let mut additions = String::new();
            for (name, value) in metadata {
                let escaped = crate::vector::escape(&value);
                if let Some(attr) = node
                    .attributes()
                    .find(|a| a.name() == name && a.namespace().is_none())
                {
                    size = size
                        .checked_sub(attr.range_value().len())
                        .and_then(|n| n.checked_add(escaped.len()))
                        .filter(|n| *n <= LIMIT)
                        .ok_or("SVG 원본 보관 예산을 초과합니다.")?;
                    patches.push((attr.range_value(), escaped));
                } else {
                    additions += &format!(" {name}=\"{escaped}\" ");
                }
            }
            if !additions.is_empty() {
                charge(&mut size, additions.len())?;
                patches.push((selected.range().start..selected.range().start, additions));
            }
        }
        patches.sort_by_key(|(range, _)| range.start);
        let mut result = source.to_owned();
        for (range, value) in patches.into_iter().rev() {
            result.replace_range(range, &value);
        }
        Ok(result)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFile {
    Original,
    Display,
}
pub struct Resource {
    pub name: String,
    pub bytes: Vec<u8>,
}
fn extension(mime: &str, bytes: &[u8]) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" | "image/jpg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/svg+xml" => "svg",
        _ => image::guess_format(bytes)
            .ok()
            .and_then(|f| f.extensions_str().first().copied())
            .unwrap_or("bin"),
    }
}
fn filename(value: &str) -> Option<String> {
    let value = value
        .rsplit(['/', '\\'])
        .next()?
        .trim()
        .trim_end_matches([' ', '.']);
    if value.is_empty()
        || matches!(value, "." | "..")
        || value
            .chars()
            .any(|c| c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
    {
        return None;
    }
    let stem = value.split('.').next()?.to_ascii_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || ((stem.starts_with("COM") || stem.starts_with("LPT"))
            && stem[3..].chars().count() == 1
            && stem[3..]
                .chars()
                .next()
                .is_some_and(|c| matches!(c, '1'..='9' | '¹' | '²' | '³')))
    {
        return None;
    }
    Some(value.to_owned())
}
/// Encoded file extraction preserves originals instead of baking geometry/opacity into them.
pub fn image_file(source: &str, index: usize, kind: ImageFile) -> Result<Resource, String> {
    if source.len() > LIMIT {
        return Err("SVG 소스 보관 예산을 초과합니다.".into());
    }
    let doc = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let node = doc
        .descendants()
        .filter(|n| n.is_element())
        .nth(index)
        .ok_or("SVG 요소가 없습니다.")?;
    if !image_node(node) {
        return Err("이미지 또는 필터 이미지 요소를 선택하세요.".into());
    }
    let href = href(node)
        .map(|a| a.value())
        .ok_or("내장 이미지 파일이 없습니다.")?;
    let encoded = if kind == ImageFile::Original {
        node.attribute("data-rphoto-image-original").unwrap_or(href)
    } else {
        href
    };
    if !encoded.starts_with("data:") {
        return Err("외부/내부 요소 참조는 이미지 파일로 저장할 수 없습니다. 먼저 이미지 파일을 포함하세요.".into());
    }
    let (mut mime, mut bytes) = decode_data(encoded)?;
    if bytes.is_empty() || bytes.len() > LIMIT {
        return Err("내장 이미지의 내용/보관 예산을 확인하세요.".into());
    }
    if kind == ImageFile::Display {
        if matches!(
            mime.as_str(),
            "image/png" | "image/jpeg" | "image/jpg" | "image/gif" | "image/webp"
        ) {
            if let Some(png) = native_frame(&bytes, &mime)? {
                bytes = png;
                mime = "image/png".into();
            }
        } else if mime == "image/svg+xml" {
            let expanded = expanded(&bytes, &mut 0)?;
            let text = std::str::from_utf8(&expanded).map_err(|e| e.to_string())?;
            validate(text)?;
            bytes = expanded;
        } else {
            return Err("현재 표시 이미지의 저장 형식을 확인하세요.".into());
        }
    }
    let suggested = node.attribute("data-rphoto-image-name").and_then(filename);
    let ext = extension(&mime, &bytes);
    let name = match (suggested, kind) {
        (Some(name), ImageFile::Original) => name,
        (Some(name), ImageFile::Display) => Path::new(&name)
            .with_extension(ext)
            .to_string_lossy()
            .into_owned(),
        (None, _) => format!("image-{index}.{ext}"),
    };
    Ok(Resource { name, bytes })
}
pub fn save_image(
    resource: &Resource,
    path: &Path,
    permit: impl Fn() -> bool,
) -> Result<(), String> {
    if resource.bytes.is_empty() || resource.bytes.len() > LIMIT {
        return Err("내장 이미지의 내용/보관 예산을 확인하세요.".into());
    }
    crate::files::atomic_write(path, |file| {
        for chunk in resource.bytes.chunks(64 * 1024) {
            if !permit() {
                return Err("이미지 파일 저장을 취소했습니다.".into());
            }
            file.write_all(chunk).map_err(|e| e.to_string())?;
        }
        if !permit() {
            return Err("이미지 파일 저장을 취소했습니다.".into());
        }
        Ok(())
    })
}

pub struct Replacement {
    pub source: String,
    pub notes: Vec<String>,
    pub pixels: image::RgbaImage,
}
/// Replace exactly one image's payload while preserving geometry, styles, defs and other images.
pub fn replace_image(source: &str, index: usize, path: &Path) -> Result<Replacement, String> {
    let doc = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let node = doc
        .descendants()
        .filter(|n| n.is_element())
        .nth(index)
        .ok_or("SVG 요소가 없습니다.")?;
    if !image_node(node) {
        return Err("이미지 또는 필터 이미지 요소를 선택하세요.".into());
    }
    let legacy = node.attribute((XLINK, "href")).is_some();
    let mut patches = node
        .attributes()
        .filter(|a| a.namespace().is_none() && a.name().starts_with("data-rphoto-image-"))
        .map(|a| a.range())
        .collect::<Vec<_>>();
    patches.sort_by_key(|r| r.start);
    let mut updated = source.to_owned();
    for range in patches.into_iter().rev() {
        updated.replace_range(range, "");
    }
    let mut capture = Capture::default();
    let asset = capture.file(path, 1)?;
    let set = |text: String, name: &str, value: &str| -> Result<String, String> {
        if text
            .len()
            .checked_add(value.len())
            .filter(|n| *n <= LIMIT)
            .is_none()
        {
            return Err("SVG 교체 이미지가 현재 소스 보관 예산을 초과합니다.".into());
        }
        let text = crate::svg_edit::attribute_text(&text, index, name, value)?;
        if text.len() > LIMIT {
            return Err("SVG 교체 이미지가 현재 소스 보관 예산을 초과합니다.".into());
        }
        Ok(text)
    };
    // Remove old payload before checking growth so replacing a large image need not count twice.
    updated = crate::svg_edit::attribute_text(&updated, index, "href", "")?;
    if legacy {
        updated = crate::svg_edit::attribute_text(&updated, index, "xlink:href", "")?;
    }
    updated = set(updated, "href", &asset.uri)?;
    if legacy {
        updated = set(updated, "xlink:href", &asset.uri)?;
    }
    updated = set(updated, "data-rphoto-image-name", &asset.name)?;
    updated = set(updated, "data-rphoto-image-href", &path.to_string_lossy())?;
    if let Some(original) = asset.original {
        updated = set(updated, "data-rphoto-image-original", &original)?;
    }
    validate(&updated)?;
    let pixels = crate::vector::render_svg(&updated)?;
    Ok(Replacement {
        source: updated,
        notes: capture.notes,
        pixels,
    })
}
pub fn load(path: &Path) -> Result<(String, Vec<String>), String> {
    let path = std::fs::canonicalize(path).map_err(|e| e.to_string())?;
    let bytes = read(&path)?;
    let mut capture = Capture {
        used: bytes.len(),
        stack: vec![path.clone()],
        ..Default::default()
    };
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svgz"))
        && !bytes.starts_with(&[0x1f, 0x8b])
    {
        return Err("SVGZ 파일의 gzip 압축을 확인하세요.".into());
    }
    let text = expanded(&bytes, &mut capture.used)?;
    let source = std::str::from_utf8(&text).map_err(|e| e.to_string())?;
    let source = capture.source(source, &path, 0)?;
    validate(&source)?;
    if capture.count > 0 {
        capture.notes.insert(0,format!("SVG 이미지 참조 {}개를 문서에 포함했습니다. 원본 이미지가 이동해도 프로젝트와 SVG 출력에서 유지됩니다.",capture.count));
    }
    Ok((source, capture.notes))
}
/// Reject unresolved dependencies explicitly instead of silently drawing an incomplete image.
pub fn validate(source: &str) -> Result<(), String> {
    fn walk(source: &str, depth: usize, used: &mut usize) -> Result<(), String> {
        if depth > DEPTH {
            return Err("SVG 내장 이미지 중첩 예산을 초과합니다.".into());
        }
        charge(used, source.len())?;
        let doc = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
        for node in doc.descendants().filter(|n| image_node(*n)) {
            let Some(attr) = href(node) else { continue };
            let value = attr.value().trim();
            if value.is_empty() || value.starts_with('#') {
                continue;
            }
            if !value.starts_with("data:") {
                return Err("SVG의 외부 이미지를 먼저 포함해야 합니다. 원래 SVG 파일을 다시 열어 이미지와 함께 가져오세요.".into());
            }
            let (mime, bytes) = decode_data(value)?;
            if mime == "image/svg+xml" {
                walk(
                    std::str::from_utf8(&bytes).map_err(|e| e.to_string())?,
                    depth + 1,
                    used,
                )?;
            } else {
                charge(used, bytes.len())?;
                raster(&bytes, &mime, false)?;
            }
        }
        Ok(())
    }
    walk(source, 0, &mut 0)
}
pub fn options() -> resvg::usvg::Options<'static> {
    resvg::usvg::Options {
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_string: Box::new(|_, _| None),
            resolve_data: Box::new(|mime, bytes, options| {
                if matches!(
                    mime,
                    "image/png" | "image/jpeg" | "image/jpg" | "image/gif" | "image/webp"
                ) {
                    match native_frame(&bytes, mime) {
                        Ok(Some(png)) => {
                            return Some(resvg::usvg::ImageKind::PNG(std::sync::Arc::new(png)));
                        }
                        Ok(None) => {}
                        Err(_) => return None,
                    }
                }
                (resvg::usvg::ImageHrefResolver::default_data_resolver())(mime, bytes, options)
            }),
        },
        ..Default::default()
    }
}
#[cfg(test)]
#[path = "svg_assets_tests.rs"]
mod tests;
