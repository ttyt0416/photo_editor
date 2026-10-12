use crate::document::{Document, Layer, MAX_PIXELS, validate_size};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::Arc,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub const IMAGE_EXTENSIONS: &[&str] = &[
    "png", "apng", "jpg", "jpeg", "jpe", "jfif", "webp", "gif", "bmp", "tif", "tiff", "ico", "qoi",
    "tga", "pnm", "ppm", "pgm", "pbm", "pam", "ff", "dds", "avif", "svg", "svgz", "psd", "psb",
    "heic", "heif", "pcx", "jxl", "jp2", "j2k", "jpc", "exr", "hdr", "jpf", "jpx", "ari", "arw",
    "cr2", "cr3", "crm", "crw", "dcr", "dcs", "dng", "erf", "iiq", "kdc", "mef", "mos", "mrw",
    "nef", "nrw", "orf", "ori", "pef", "raf", "raw", "rw2", "rwl", "srw", "3fr", "fff", "x3f",
    "qtk",
];
pub const PROJECT_EXTENSION: &str = "rphoto";

pub struct Opened {
    pub source_stamp: Option<crate::file_watch::Snapshot>,
    pub document: Document,
    pub project_path: Option<PathBuf>,
    pub title: String,
    pub notes: Vec<String>,
}

fn error(e: impl std::fmt::Display) -> String {
    e.to_string()
}

pub fn open(path: &Path) -> Result<Opened, String> {
    open_impl(path, false)
}
pub(crate) fn open_legacy_photoshop(path: &Path) -> Result<Opened, String> {
    if !path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("psd") || e.eq_ignore_ascii_case("psb"))
    {
        return Err("이전 개발 파일 열기는 PSD/PSB에서 사용하세요.".into());
    }
    open_impl(path, true)
}
fn open_impl(path: &Path, legacy: bool) -> Result<Opened, String> {
    // Only editable native documents need the persistent-file conflict baseline.
    let native = !legacy
        && path.extension().is_some_and(|ext| {
            ext.eq_ignore_ascii_case(PROJECT_EXTENSION)
                || ext.eq_ignore_ascii_case("psd")
                || ext.eq_ignore_ascii_case("psb")
        });
    let stamp = native
        .then(|| crate::file_watch::snapshot(path))
        .transpose()?;
    let opened = decode_impl(path, legacy, stamp.clone())?;
    if let Some(stamp) = stamp
        && crate::file_watch::snapshot(path)? != stamp
    {
        return Err("파일을 여는 동안 원본이 바뀌었습니다. 저장된 파일을 다시 열어 주세요.".into());
    }
    Ok(opened)
}
fn decode_impl(
    path: &Path,
    legacy: bool,
    source_stamp: Option<crate::file_watch::Snapshot>,
) -> Result<Opened, String> {
    let title = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(PROJECT_EXTENSION))
    {
        return Ok(Opened {
            document: load_project(path)?,
            project_path: Some(path.into()),
            source_stamp,
            title,
            notes: vec![],
        });
    }
    let extension = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "psd" | "psb") {
        if std::fs::metadata(path).map_err(error)?.len() > MAX_PIXELS * 32 + 64 * 1024 * 1024 {
            return Err("PSD가 메모리 예산을 초과합니다.".into());
        }
        let bytes = std::fs::read(path).map_err(error)?;
        let document = if legacy {
            crate::psd::read_legacy(&bytes)?
        } else {
            crate::psd::read(&bytes)?
        };
        let mut notes = Vec::new();
        if legacy {
            notes.push("이전 개발 버전의 위→아래 레이어 순서로 열었습니다. 원본을 보관하며 저장 위치 없는 문서로 엽니다. 새 PSD/PSB 출력은 외부 파일의 표준 레이어 순서를 사용합니다.".into());
        }
        if bytes[24..26] == [0, 1] {
            notes.push("회색조 PSD/PSB의 값을 같은 정밀도의 RGB 작업 버퍼에 유지했습니다. PSD/PSB 내보내기에서 RGB 또는 회색조를 선택합니다. 원래 회색조 ICC는 .rphoto와 회색조 출력에 보존하고 RGB 출력에는 포함하지 않습니다. 회색조 색 관리와 추가 알파/별색 채널은 남은 목표입니다.".into());
        }
        if !document.psd_resources.is_empty() {
            notes.push("원본 Photoshop 이미지 리소스는 PSD/PSB와 프로젝트 저장에 보존합니다. 내장 ICC 프로파일의 화면 색상 변환은 현재 적용되지 않습니다.".into());
        }
        if document.layers.iter().any(|l| !l.psd_extra.is_empty()) {
            notes.push("해석하지 못한 Photoshop 추가 레코드를 보관했습니다. 편집 후 해당 레코드의 유효성은 저장 전에 확인이 필요합니다.".into());
        }
        if document
            .layers
            .iter()
            .any(|l| l.bit_depth > 8 && l.mask.is_some())
        {
            notes.push("고정밀 PSD/PSB의 픽셀 마스크는 현재 8비트 편집 마스크로 변환됩니다. 마스크 정밀도가 중요한 경우 원본 파일을 보관하세요.".into());
        }
        return Ok(Opened {
            document,
            project_path: (!legacy).then(|| path.into()),
            source_stamp,
            title,
            notes,
        });
    }
    if crate::raw_input::EXTENSIONS.contains(&extension.as_str()) {
        let (document, notes) = crate::raw_input::open(path, title.clone())?;
        return Ok(Opened {
            document,
            project_path: None,
            source_stamp: None,
            title,
            notes,
        });
    }
    if matches!(extension.as_str(), "svg" | "svgz") {
        let (source, notes) = crate::svg_assets::load(path)?;
        let content = crate::vector::Content::Svg { source };
        let pixels = content.render()?;
        let mut document = Document::from_image(title.clone(), pixels);
        document.active_layer_mut().unwrap().content = Some(content);
        return Ok(Opened {
            document,
            project_path: None,
            source_stamp: None,
            title,
            notes,
        });
    }
    if let Some(mut image) = crate::codecs::decode(path, &extension)? {
        let mut document = crate::precision::document(title.clone(), image.pixels)?;
        crate::color_management::remember(&mut document, image.source_icc, &mut image.notes);
        document.source_metadata = image.source_metadata;
        return Ok(Opened {
            document,
            project_path: None,
            source_stamp: None,
            title,
            notes: image.notes,
        });
    }
    let mut reader = ImageReader::open(path)
        .map_err(error)?
        .with_guessed_format()
        .map_err(error)?;
    let format = reader.format();
    if format == Some(ImageFormat::Jpeg) {
        let mut decoded = crate::jpeg_output::read(path)?;
        let mut document = crate::precision::document(title.clone(), decoded.pixels)?;
        crate::color_management::remember(&mut document, decoded.source_icc, &mut decoded.notes);
        document.source_metadata = decoded.source_metadata;
        return Ok(Opened {
            document,
            project_path: None,
            source_stamp: None,
            title,
            notes: decoded.notes,
        });
    }
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_PIXELS * 16);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|e| format!("이 파일을 열 수 없습니다: {e}"))?;
    let (width, height) = decoder.dimensions();
    validate_size(width, height)?;
    let mut notes = vec![];
    let profile = if format == Some(ImageFormat::Tiff) {
        match crate::color_management::tiff_profile(path) {
            Ok(profile) => profile,
            Err(error) => {
                notes.push(format!(
                    "TIFF의 내장 ICC 프로파일을 읽지 못했습니다: {error}. 원래 픽셀을 유지합니다."
                ));
                None
            }
        }
    } else {
        decoder.icc_profile().map_err(error)?
    };
    let source_metadata =
        crate::source_metadata::Metadata::decode(&mut decoder, path, format, &mut notes);
    let orientation = decoder.orientation().map_err(error)?;
    let mut pixels = DynamicImage::from_decoder(decoder).map_err(error)?;
    pixels.apply_orientation(orientation);
    if let Some(profile) = &profile {
        pixels = crate::color_management::apply(pixels, profile, &mut notes);
    }
    if format == Some(ImageFormat::Dds) {
        notes.push("DDS는 기본 이미지 한 장으로 엽니다. 밉맵·큐브맵·배열 텍스처는 아직 편집할 수 없습니다.".into());
    }
    if has_multiple_images(path, format)? {
        notes.push("현재 한 장의 이미지로 엽니다. 애니메이션·여러 페이지·여러 아이콘 크기는 아직 편집할 수 없습니다.".into());
    }
    let mut document = crate::precision::document(title.clone(), pixels)?;
    crate::color_management::remember(&mut document, profile, &mut notes);
    document.source_metadata = source_metadata;
    Ok(Opened {
        document,
        project_path: None,
        source_stamp: None,
        title,
        notes,
    })
}

fn has_multiple_images(path: &Path, format: Option<ImageFormat>) -> Result<bool, String> {
    use image::AnimationDecoder;
    use std::io::BufReader;
    let reader = || File::open(path).map(BufReader::new).map_err(error);
    match format {
        Some(ImageFormat::Png) => image::codecs::png::PngDecoder::new(reader()?)
            .map_err(error)?
            .is_apng()
            .map_err(error),
        Some(ImageFormat::WebP) => Ok(image::codecs::webp::WebPDecoder::new(reader()?)
            .map_err(error)?
            .has_animation()),
        Some(ImageFormat::Gif) => Ok(image::codecs::gif::GifDecoder::new(reader()?)
            .map_err(error)?
            .into_frames()
            .take(2)
            .count()
            > 1),
        Some(ImageFormat::Tiff) => Ok(tiff::decoder::Decoder::new(reader()?)
            .map_err(error)?
            .more_images()),
        Some(ImageFormat::Ico) => {
            let mut header = [0; 6];
            reader()?.read_exact(&mut header).map_err(error)?;
            Ok(u16::from_le_bytes([header[4], header[5]]) > 1)
        }
        _ => Ok(false),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    #[serde(default)]
    document_id: Option<String>,
    width: u32,
    height: u32,
    active: u64,
    layers: Vec<LayerRecord>,
    #[serde(default)]
    psd_resources: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    source_icc: bool,
    #[serde(
        default,
        skip_serializing_if = "crate::source_metadata::Record::is_empty"
    )]
    source_metadata: crate::source_metadata::Record,
    #[serde(default)]
    layout: crate::guides::Layout,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayerRecord {
    id: u64,
    name: String,
    offset: [i32; 2],
    visible: bool,
    locked: bool,
    opacity: f32,
    #[serde(default)]
    blend: crate::blend::BlendMode,
    #[serde(default)]
    adjustments: crate::adjustments::Adjustments,
    #[serde(default)]
    mask: bool,
    #[serde(default = "enabled")]
    mask_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mask_placement: Option<crate::masks::Placement>,
    #[serde(default)]
    content: Option<crate::vector::Content>,
    #[serde(default, skip_serializing_if = "is_false")]
    svg_source: bool,
    #[serde(default)]
    transform: crate::transform::Transform,
    #[serde(default = "eight")]
    bit_depth: u8,
    #[serde(default)]
    effects: crate::effects::Effects,
    #[serde(default)]
    parent: Option<u64>,
    #[serde(default)]
    group: bool,
    #[serde(default)]
    adjustment_layer: bool,
    #[serde(default = "enabled")]
    pass_through: bool,
    #[serde(default)]
    clipped: bool,
    #[serde(default)]
    psd_extra: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw_source: Option<crate::raw_input::Record>,
}
fn is_false(value: &bool) -> bool {
    !*value
}
fn eight() -> u8 {
    8
}

fn enabled() -> bool {
    true
}

pub fn save_project(document: &Document, path: &Path) -> Result<(), String> {
    validate_size(document.width, document.height)?;
    let mut ids = std::collections::HashSet::new();
    if !crate::document_id::valid(&document.document_id)
        || !document.layout.valid()
        || !document.hierarchy_valid()
        || document.active_layer().is_none()
        || document.layers.len() > 10_000
        || document.pixel_count() > MAX_PIXELS
        || document
            .layers
            .iter()
            .filter_map(|l| {
                l.adjustments
                    .scope
                    .as_ref()
                    .and_then(|scope| scope.mask.as_ref())
            })
            .map(|m| u64::from(m.width()) * u64::from(m.height()))
            .sum::<u64>()
            > MAX_PIXELS
        || document.layers.iter().any(|layer| {
            layer.id == 0
                || layer.id == u64::MAX
                || !ids.insert(layer.id)
                || validate_size(layer.pixels.width(), layer.pixels.height()).is_err()
                || !layer.opacity.is_finite()
                || !(0.0..=1.0).contains(&layer.opacity)
                || !layer.transform.valid()
                || (layer.canvas_space()
                    && layer.mask.as_ref().is_some_and(|mask| {
                        !layer.transform.valid_for([mask.width(), mask.height()])
                    }))
                || (!layer.canvas_space()
                    && !layer
                        .transform
                        .valid_for([layer.pixels.width(), layer.pixels.height()]))
                || !crate::adjustment_layer::valid(layer)
                || !crate::masks::valid(layer)
                || !layer.adjustments.valid()
                || !layer.effects.valid()
                || layer
                    .raw_source
                    .as_ref()
                    .is_some_and(|r| !r.valid() || layer.canvas_space() || layer.content.is_some())
                || !crate::filter_bounds::valid(layer)
                || layer
                    .content
                    .as_ref()
                    .is_some_and(|c| c.validate().is_err())
                || ![8, 16, 32].contains(&layer.bit_depth)
                || layer
                    .precision
                    .as_ref()
                    .is_some_and(|p| p.dimensions() != layer.pixels.dimensions())
        })
    {
        return Err(
            "프로젝트의 문서·레이어 또는 가이드 설정이 잘못되었습니다. 기존 파일을 보존했습니다."
                .into(),
        );
    }
    if document
        .layers
        .iter()
        .filter_map(|l| l.raw_source.as_ref())
        .map(|r| r.bytes.len() as u64)
        .sum::<u64>()
        > crate::raw_input::MAX_RAW_BYTES
    {
        return Err(
            "RAW 원본 자산이 현재 메모리 예산을 초과합니다. 기존 파일을 보존했습니다.".into(),
        );
    }
    if document
        .source_icc
        .as_ref()
        .is_some_and(|p| p.len() > crate::color_management::MAX_PROFILE_BYTES)
    {
        return Err("원본 ICC 프로파일이 보관 예산을 초과합니다. 기존 파일을 보존했습니다.".into());
    }
    if !document.source_metadata.valid() {
        return Err(
            "원본 촬영 메타데이터가 보관 예산을 초과합니다. 기존 파일을 보존했습니다.".into(),
        );
    }
    atomic_write(path, |file| {
        let mut zip = ZipWriter::new(file);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        let manifest = Manifest {
            format: "photo-editor.project".into(),
            version: if document.source_metadata.xmp_extended.is_some() {
                15
            } else if document.source_metadata.iptc.is_some()
                || document.source_metadata.photoshop.is_some()
            {
                14
            } else if !document.source_metadata.record().is_empty() {
                13
            } else if document.source_icc.is_some() {
                12
            } else if document
                .layers
                .iter()
                .filter_map(|l| l.raw_source.as_ref())
                .any(|s| s.record.needs_v11())
            {
                11
            } else if document
                .layers
                .iter()
                .filter_map(|l| l.raw_source.as_ref())
                .any(|s| s.record.settings.needs_v10())
            {
                10
            } else if document.layers.iter().any(|l| l.raw_source.is_some()) {
                9
            } else {
                8
            },
            document_id: Some(document.document_id.clone()),
            width: document.width,
            height: document.height,
            active: document.active,
            psd_resources: !document.psd_resources.is_empty(),
            source_icc: document.source_icc.is_some(),
            source_metadata: document.source_metadata.record(),
            layout: document.layout.clone(),
            layers: document
                .layers
                .iter()
                .map(|l| LayerRecord {
                    id: l.id,
                    name: l.name.clone(),
                    offset: l.offset,
                    visible: l.visible,
                    locked: l.locked,
                    opacity: l.opacity,
                    blend: l.blend,
                    adjustments: l.adjustments.clone(),
                    mask: l.mask.is_some(),
                    mask_enabled: l.mask_enabled,
                    mask_placement: l.mask_placement.clone(),
                    content: if matches!(l.content, Some(crate::vector::Content::Svg { .. })) {
                        None
                    } else {
                        l.content.clone()
                    },
                    svg_source: matches!(l.content, Some(crate::vector::Content::Svg { .. })),
                    transform: l.transform,
                    bit_depth: l.bit_depth,
                    effects: l.effects.clone(),
                    parent: l.parent,
                    group: l.group,
                    adjustment_layer: l.adjustment_layer,
                    pass_through: l.pass_through,
                    clipped: l.clipped,
                    psd_extra: !l.psd_extra.is_empty(),
                    raw_source: l.raw_source.as_ref().map(|r| r.record.clone()),
                })
                .collect(),
        };
        zip.start_file("manifest.json", options).map_err(error)?;
        let json = serde_json::to_vec_pretty(&manifest).map_err(error)?;
        if json.len() > 4 * 1024 * 1024 {
            return Err("프로젝트 설명 파일이 너무 큽니다. 기존 파일을 보존했습니다.".into());
        }
        zip.write_all(&json).map_err(error)?;
        if !document.psd_resources.is_empty() {
            zip.start_file("metadata/photoshop-resources.bin", options)
                .map_err(error)?;
            zip.write_all(&document.psd_resources).map_err(error)?;
        }
        if let Some(profile) = &document.source_icc {
            zip.start_file("metadata/source.icc", options)
                .map_err(error)?;
            zip.write_all(profile).map_err(error)?;
        }
        document.source_metadata.save(&mut zip, options)?;
        for layer in &document.layers {
            if let Some(raw) = &layer.raw_source {
                zip.start_file(format!("layers/{}-source.raw", layer.id), options)
                    .map_err(error)?;
                zip.write_all(&raw.bytes).map_err(error)?;
            }
            if let Some(mask) = layer
                .adjustments
                .scope
                .as_ref()
                .and_then(|scope| scope.mask.as_ref())
            {
                zip.start_file(
                    format!("layers/{}-adjustment-selection.png", layer.id),
                    options,
                )
                .map_err(error)?;
                let mut png = Cursor::new(Vec::new());
                DynamicImage::ImageLuma8((**mask).clone())
                    .write_to(&mut png, ImageFormat::Png)
                    .map_err(error)?;
                zip.write_all(png.get_ref()).map_err(error)?;
            }
            if let Some(crate::vector::Content::Svg { source }) = &layer.content {
                zip.start_file(format!("layers/{}-source.svg", layer.id), options)
                    .map_err(error)?;
                zip.write_all(source.as_bytes()).map_err(error)?;
            }
            if !layer.psd_extra.is_empty() {
                zip.start_file(format!("layers/{}-photoshop.bin", layer.id), options)
                    .map_err(error)?;
                zip.write_all(&layer.psd_extra).map_err(error)?;
            }
            if let Some(mask) = &layer.mask {
                zip.start_file(format!("layers/{}-mask.png", layer.id), options)
                    .map_err(error)?;
                let mut png = Cursor::new(Vec::new());
                DynamicImage::ImageLuma8((**mask).clone())
                    .write_to(&mut png, ImageFormat::Png)
                    .map_err(error)?;
                zip.write_all(png.get_ref()).map_err(error)?;
            }
            let extension = if layer.bit_depth == 32 { "exr" } else { "png" };
            zip.start_file(format!("layers/{}.{extension}", layer.id), options)
                .map_err(error)?;
            let mut png = Cursor::new(Vec::new());
            let image = if let Some(pixels) = &layer.precision {
                if layer.bit_depth == 32 {
                    DynamicImage::ImageRgba32F((**pixels).clone())
                } else {
                    DynamicImage::ImageRgba16(crate::precision::image16(pixels))
                }
            } else {
                DynamicImage::ImageRgba8((*layer.pixels).clone())
            };
            image
                .write_to(
                    &mut png,
                    if layer.bit_depth == 32 {
                        ImageFormat::OpenExr
                    } else {
                        ImageFormat::Png
                    },
                )
                .map_err(error)?;
            zip.write_all(png.get_ref()).map_err(error)?;
        }
        let file = zip.finish().map_err(error)?;
        file.sync_all().map_err(error)
    })
}

pub enum CheckedSave {
    Saved(crate::file_watch::Snapshot),
    Conflict(crate::file_watch::Snapshot),
}
pub fn save_document_checked(
    document: &Document,
    path: &Path,
    expected: Option<&crate::file_watch::Snapshot>,
) -> Result<CheckedSave, String> {
    let Some(expected) = expected else {
        save_document(document, path)?;
        return Ok(CheckedSave::Saved(crate::file_watch::snapshot(path)?));
    };
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let extension = path
        .extension()
        .unwrap_or_else(|| std::ffi::OsStr::new(PROJECT_EXTENSION))
        .to_string_lossy();
    let staged = parent.join(format!(
        ".photo-editor-checked-{}.{extension}",
        crate::document_id::new()
    ));
    drop(
        File::options()
            .write(true)
            .create_new(true)
            .open(&staged)
            .map_err(error)?,
    );
    let result = (|| {
        save_document(document, &staged)?;
        let current = crate::file_watch::snapshot(path)?;
        if &current != expected {
            return Ok(CheckedSave::Conflict(current));
        }
        replace_file(&staged, path)?;
        Ok(CheckedSave::Saved(crate::file_watch::snapshot(path)?))
    })();
    let _ = std::fs::remove_file(staged);
    result
}

pub fn save_document(document: &Document, path: &Path) -> Result<(), String> {
    match path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase()
        .as_str()
    {
        "psd" | "psb" => {
            if document.layers.iter().any(|l| {
                l.mask_placement.is_some()
                    || l.content.is_some()
                    || l.adjustments != Default::default()
                    || l.transform != Default::default()
                    || l.effects.enabled()
                    || !l.psd_extra.is_empty()
            }) {
                return Err("PSD/PSB 저장 시 편집 설정 또는 정밀도 변환이 필요합니다. PSD/PSB 내보내기에서 변환 내용을 확인하거나 .rphoto로 저장하세요.".into());
            }
            save_photoshop(document, path)
        }
        _ => save_project(document, path),
    }
}

pub fn save_photoshop(document: &Document, path: &Path) -> Result<(), String> {
    save_photoshop_compressed(document, path, crate::psd_compression::Compression::Raw)
}
pub fn save_photoshop_compressed(
    document: &Document,
    path: &Path,
    compression: crate::psd_compression::Compression,
) -> Result<(), String> {
    save_photoshop_options(
        document,
        path,
        crate::psd::OutputOptions {
            compression,
            ..Default::default()
        },
    )
}
pub(crate) fn save_photoshop_options(
    document: &Document,
    path: &Path,
    options: crate::psd::OutputOptions,
) -> Result<(), String> {
    let compression = options.compression;
    let large = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("psb"));
    if document.layers.iter().any(|layer| {
        layer.adjustment_layer || crate::adjustment_scope::needs_backdrop(document, layer)
    }) {
        let rendered = crate::editing::copy_pixels(document, None, true);
        let mut flattened = crate::precision::document("보정 합성".into(), rendered)?;
        flattened.document_id = document.document_id.clone();
        flattened.psd_resources = document.psd_resources.clone();
        flattened.layout = document.layout.clone();
        return save_photoshop_options(&flattened, path, options);
    }
    let mut document = document.clone();
    for layer in &mut document.layers {
        if !layer.canvas_space() && layer.adjustments.expand_filters && layer.adjustments.active() {
            crate::filter_bounds::bake(layer)?;
        }
        crate::masks::align_source(layer);
    }
    let groups: Vec<u64> = document
        .displayed_layers()
        .into_iter()
        .rev()
        .filter_map(|(id, _)| {
            document
                .layers
                .iter()
                .find(|l| l.id == id && l.group && (l.effects.enabled() || l.adjustments.active()))
                .map(|l| l.id)
        })
        .collect();
    for id in groups {
        let mut isolated = document.clone();
        isolated.active = id;
        let group = isolated.active_layer_mut().unwrap();
        group.opacity = 1.0;
        group.blend = crate::blend::BlendMode::Normal;
        let image = crate::editing::copy_pixels(&isolated, None, false);
        let rendered = crate::precision::document("그룹 출력".into(), image)?
            .layers
            .remove(0);
        let children: Vec<u64> = document
            .layers
            .iter()
            .filter(|l| l.id != id && document.descendant_of(l.id, id))
            .map(|l| l.id)
            .collect();
        document.layers.retain(|l| !children.contains(&l.id));
        let group = document.layers.iter_mut().find(|l| l.id == id).unwrap();
        group.pixels = rendered.pixels;
        group.offset = [0, 0];
        group.group = false;
        group.transform = Default::default();
        group.mask = None;
        group.precision = rendered.precision;
        group.bit_depth = rendered.bit_depth;
        group.adjustments = Default::default();
        group.effects = Default::default();
    }
    for layer in &mut document.layers {
        if layer.group {
            if layer.transform != Default::default()
                && let Some(mask) = &layer.mask
            {
                let inverse = crate::matrix::inverse(
                    layer
                        .transform
                        .mapping(layer.offset, [mask.width(), mask.height()]),
                )
                .ok_or("그룹 마스크 변형이 유효하지 않습니다.")?;
                let pixels = image::GrayImage::from_fn(document.width, document.height, |x, y| {
                    let p = crate::matrix::point(inverse, [x as f32 + 0.5, y as f32 + 0.5]);
                    image::Luma(
                        [(crate::sampling::gray(p, layer.transform.sampling, |x, y| {
                            if x < 0
                                || y < 0
                                || x >= mask.width() as i32
                                || y >= mask.height() as i32
                            {
                                0.
                            } else {
                                mask.get_pixel(x as u32, y as u32)[0] as f32 / 255.
                            }
                        }) * 255.)
                            .round() as u8],
                    )
                });
                layer.mask = Some(Arc::new(pixels));
                layer.offset = [0, 0];
            }
            layer.transform = Default::default();
            continue;
        }
        if layer.precision.is_some() && !layer.group && layer.adjustments != Default::default() {
            let pixels = layer
                .adjustments
                .apply_float(layer.precision.as_ref().unwrap());
            layer.pixels = Arc::new(crate::precision::preview(&pixels));
            layer.precision = Some(Arc::new(pixels));
            layer.adjustments = Default::default();
        }
        if layer.effects.enabled() {
            let mut source = layer.precision.as_ref().map_or_else(
                || DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f(),
                |p| (**p).clone(),
            );
            source = layer.adjustments.apply_float(&source);
            if layer.mask_enabled
                && let Some(mask) = &layer.mask
            {
                for (x, y, p) in source.enumerate_pixels_mut() {
                    p[3] *= mask.get_pixel(x, y)[0] as f32 / 255.0;
                }
            }
            let (pixels, pad) = crate::effects::apply(&source, &layer.effects);
            layer.pixels = Arc::new(crate::precision::preview(&pixels));
            layer.precision = Some(Arc::new(pixels));
            layer.offset = layer.offset.map(|v| v.saturating_sub(pad));
            layer.transform = layer.transform.padded(pad);
            layer.effects = Default::default();
            layer.adjustments = Default::default();
            layer.mask = None;
        }
        if layer.transform != Default::default() {
            crate::editing::rasterize(layer)?;
        }
    }
    if options.color == crate::psd::OutputColor::Grayscale {
        // Apply remaining 8-bit adjustments before deciding whether the resulting layers are gray.
        for layer in document
            .layers
            .iter_mut()
            .filter(|l| !l.group && l.adjustments != Default::default())
        {
            let original = layer.precision.as_ref().map_or_else(
                || DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f(),
                |p| (**p).clone(),
            );
            let pixels = layer.adjustments.apply_float(&original);
            layer.pixels = Arc::new(crate::precision::preview(&pixels));
            layer.precision = Some(Arc::new(pixels));
            layer.adjustments = Default::default();
        }
        if !crate::psd::neutral(&document) {
            if options.grayscale_layers {
                for layer in document.layers.iter_mut().filter(|l| !l.group) {
                    let mut pixels = layer.precision.as_ref().map_or_else(
                        || DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f(),
                        |p| (**p).clone(),
                    );
                    for pixel in pixels.pixels_mut() {
                        crate::psd::grayscale(pixel);
                    }
                    layer.pixels = Arc::new(crate::precision::preview(&pixels));
                    layer.precision = Some(Arc::new(pixels));
                }
            } else {
                let mut pixels = crate::precision::composite(&document);
                for pixel in pixels.pixels_mut() {
                    crate::psd::grayscale(pixel);
                }
                let depth = document
                    .layers
                    .iter()
                    .map(|l| l.bit_depth)
                    .max()
                    .unwrap_or(8);
                let rendered = match depth {
                    32 => DynamicImage::ImageRgba32F(pixels),
                    16 => DynamicImage::ImageRgba16(crate::precision::image16(&pixels)),
                    _ => DynamicImage::ImageRgba8(crate::precision::preview(&pixels)),
                };
                let mut flattened = crate::precision::document("회색조 합성".into(), rendered)?;
                flattened.document_id = document.document_id.clone();
                flattened.psd_resources = document.psd_resources.clone();
                flattened.layout = document.layout.clone();
                document = flattened;
            }
        }
    }
    if document.pixel_count() > MAX_PIXELS {
        return Err("PSD 변환이 메모리 예산을 초과합니다.".into());
    }
    atomic_write(path, |file| {
        if options.color == crate::psd::OutputColor::Grayscale {
            crate::psd::write_options(&document, file, large, options)
        } else if compression == crate::psd_compression::Compression::Raw {
            crate::psd::write(&document, file, large)
        } else {
            crate::psd::write_compressed(&document, file, large, compression)
        }
    })
}

pub fn load_project(path: &Path) -> Result<Document, String> {
    let mut zip = ZipArchive::new(File::open(path).map_err(error)?).map_err(error)?;
    let mut manifest_bytes = Vec::new();
    zip.by_name("manifest.json")
        .map_err(error)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut manifest_bytes)
        .map_err(error)?;
    if manifest_bytes.len() > 4 * 1024 * 1024 {
        return Err("프로젝트 설명 파일이 너무 큽니다.".into());
    }
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes).map_err(error)?;
    if manifest.format != "photo-editor.project" || !(1..=15).contains(&manifest.version) {
        return Err("지원하지 않는 프로젝트 형식 또는 버전입니다.".into());
    }
    if manifest.version < 13 && !manifest.source_metadata.is_empty() {
        return Err("구버전 프로젝트에 촬영 메타데이터 자산이 포함되어 있습니다.".into());
    }
    if manifest.version < 15
        && (manifest.source_metadata.xmp_extended
            || manifest.source_metadata.xmp_extended_guid.is_some())
    {
        return Err("구버전 프로젝트에 확장 XMP 자산이 포함되어 있습니다".into());
    }
    if manifest.version < 14
        && (manifest.source_metadata.iptc
            || manifest.source_metadata.photoshop
            || manifest.source_metadata.iptc_tiff.is_some())
    {
        return Err("구버전 프로젝트에 IPTC/Photoshop 원문 자산이 포함되어 있습니다".into());
    }
    if manifest.version < 12 && manifest.source_icc {
        return Err("구버전 프로젝트에 원본 ICC 자산이 포함되어 있습니다.".into());
    }
    if manifest.version < 8 && manifest.layers.iter().any(|l| l.adjustment_layer) {
        return Err("구버전 프로젝트에 조정 레이어가 포함되어 있습니다.".into());
    }
    if manifest.version < 7
        && manifest
            .layers
            .iter()
            .any(|l| l.adjustments.scope.is_some())
    {
        return Err("구버전 프로젝트에 보정 선택 영역이 포함되어 있습니다.".into());
    }
    if manifest.version < 6 && manifest.layers.iter().any(|l| l.mask_placement.is_some()) {
        return Err("구버전 프로젝트에 독립 마스크 변형이 포함되어 있습니다.".into());
    }
    if manifest
        .document_id
        .as_ref()
        .is_some_and(|id| !crate::document_id::valid(id))
        || (manifest.version >= 5 && manifest.document_id.is_none())
    {
        return Err("문서 식별자가 잘못되었거나 없습니다.".into());
    }
    validate_size(manifest.width, manifest.height)?;
    if !manifest.layout.valid() {
        return Err("가이드 또는 격자 설정이 잘못되었습니다.".into());
    }
    if manifest.layers.is_empty() || manifest.layers.len() > 10_000 {
        return Err("레이어 목록이 잘못되었습니다.".into());
    }
    let mut layers = Vec::new();
    let mut ids = std::collections::HashSet::new();
    let mut total_pixels = 0;
    let mut scope_pixels = 0u64;
    let mut raw_bytes = 0u64;
    for mut layer in manifest.layers {
        if layer.svg_source {
            if manifest.version < 4 || layer.content.is_some() {
                return Err("SVG 자산 참조와 프로젝트 버전이 일치하지 않습니다.".into());
            }
            let asset = zip
                .by_name(&format!("layers/{}-source.svg", layer.id))
                .map_err(error)?;
            if asset.size() > 64 * 1024 * 1024 {
                return Err("SVG 원본 자산이 너무 큽니다.".into());
            }
            let mut source = String::new();
            asset
                .take(64 * 1024 * 1024 + 1)
                .read_to_string(&mut source)
                .map_err(error)?;
            if source.len() > 64 * 1024 * 1024 {
                return Err("SVG 원본 자산이 너무 큽니다.".into());
            }
            layer.content = Some(crate::vector::Content::Svg { source });
        }
        if let Some(scope) = &mut layer.adjustments.scope
            && let Some(expected) = scope.mask_size
        {
            let asset = zip
                .by_name(&format!("layers/{}-adjustment-selection.png", layer.id))
                .map_err(error)?;
            if asset.size() > MAX_PIXELS * 2 {
                return Err("보정 선택 자산이 너무 큽니다.".into());
            }
            let mut bytes = Vec::new();
            asset
                .take(MAX_PIXELS * 2 + 1)
                .read_to_end(&mut bytes)
                .map_err(error)?;
            let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(MAX_PIXELS);
            reader.limits(limits);
            let decoder = reader.into_decoder().map_err(error)?;
            let (width, height) = decoder.dimensions();
            validate_size(width, height)?;
            scope_pixels += u64::from(width) * u64::from(height);
            if [width, height] != expected
                || decoder.color_type() != image::ColorType::L8
                || scope_pixels > MAX_PIXELS
            {
                return Err("보정 선택 자산의 크기·형식 또는 메모리 예산이 잘못되었습니다.".into());
            }
            scope.mask = Some(Arc::new(
                DynamicImage::from_decoder(decoder)
                    .map_err(error)?
                    .into_luma8(),
            ));
        }
        if layer.id == 0
            || layer.id == u64::MAX
            || !ids.insert(layer.id)
            || !layer.opacity.is_finite()
            || !(0.0..=1.0).contains(&layer.opacity)
            || !layer.adjustments.valid()
            || !layer.transform.valid()
            || ![8, 16, 32].contains(&layer.bit_depth)
            || !layer.effects.valid()
            || layer
                .content
                .as_ref()
                .is_some_and(|c| c.validate().is_err())
        {
            return Err("레이어 ID 또는 불투명도 값이 잘못되었습니다.".into());
        }
        let name = format!(
            "layers/{}.{}",
            layer.id,
            if layer.bit_depth == 32 { "exr" } else { "png" }
        );
        let asset = zip.by_name(&name).map_err(error)?;
        if asset.size() > MAX_PIXELS * 20 {
            return Err("레이어 자산이 너무 큽니다.".into());
        }
        let mut bytes = Vec::new();
        asset
            .take(MAX_PIXELS * 20 + 1)
            .read_to_end(&mut bytes)
            .map_err(error)?;
        let mut reader = ImageReader::with_format(
            Cursor::new(bytes),
            if layer.bit_depth == 32 {
                ImageFormat::OpenExr
            } else {
                ImageFormat::Png
            },
        );
        let mut limits = image::Limits::default();
        limits.max_alloc = Some(MAX_PIXELS * 16);
        reader.limits(limits);
        let decoder = reader.into_decoder().map_err(error)?;
        let (width, height) = decoder.dimensions();
        validate_size(width, height)?;
        total_pixels += u64::from(width) * u64::from(height);
        if total_pixels > MAX_PIXELS {
            return Err("프로젝트가 현재 메모리 예산을 초과합니다.".into());
        }
        let expected = match layer.bit_depth {
            16 => image::ColorType::Rgba16,
            32 => image::ColorType::Rgba32F,
            _ => image::ColorType::Rgba8,
        };
        if decoder.color_type() != expected {
            return Err("프로젝트 레이어의 픽셀 형식이 잘못되었습니다.".into());
        }
        let decoded = DynamicImage::from_decoder(decoder).map_err(error)?;
        let pixels = decoded.to_rgba8();
        if !layer.group
            && !layer.adjustment_layer
            && !layer.transform.valid_for([pixels.width(), pixels.height()])
        {
            return Err("프로젝트에 원본 영역을 가로지르는 변형이 있습니다.".into());
        }
        let precision = (layer.bit_depth > 8).then(|| Arc::new(decoded.into_rgba32f()));
        if precision
            .as_ref()
            .is_some_and(|p| p.pixels().any(|v| v.0.iter().any(|c| !c.is_finite())))
        {
            return Err("프로젝트에 유효하지 않은 고정밀 픽셀 값이 있습니다.".into());
        }
        let mask = if layer.mask {
            let mut bytes = Vec::new();
            zip.by_name(&format!("layers/{}-mask.png", layer.id))
                .map_err(error)?
                .take(MAX_PIXELS * 2 + 1)
                .read_to_end(&mut bytes)
                .map_err(error)?;
            let mut reader = ImageReader::with_format(Cursor::new(bytes), ImageFormat::Png);
            let mut limits = image::Limits::default();
            limits.max_alloc = Some(MAX_PIXELS);
            reader.limits(limits);
            let mask = reader.decode().map_err(error)?.to_luma8();
            if !layer.group
                && !layer.adjustment_layer
                && layer.mask_placement.is_none()
                && mask.dimensions() != pixels.dimensions()
            {
                return Err("마스크 크기가 레이어와 일치하지 않습니다.".into());
            }
            Some(Arc::new(mask))
        } else {
            None
        };
        if (layer.group || layer.adjustment_layer)
            && mask
                .as_ref()
                .is_some_and(|mask| !layer.transform.valid_for([mask.width(), mask.height()]))
        {
            return Err("그룹 마스크의 원근 변형이 유효하지 않습니다.".into());
        }
        let mut psd_extra = Vec::new();
        if layer.psd_extra {
            let asset = zip
                .by_name(&format!("layers/{}-photoshop.bin", layer.id))
                .map_err(error)?;
            if asset.size() > 64 * 1024 * 1024 {
                return Err("Photoshop 추가 데이터가 너무 큽니다.".into());
            }
            asset
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut psd_extra)
                .map_err(error)?;
        }
        let raw_source = if let Some(record) = layer.raw_source {
            if manifest.version < 9
                || (manifest.version < 11 && record.needs_v11())
                || (manifest.version < 10 && record.settings.needs_v10())
                || !record.valid()
                || layer.group
                || layer.adjustment_layer
                || layer.content.is_some()
            {
                return Err("RAW 원본 참조 또는 프로젝트 버전이 잘못되었습니다.".into());
            }
            let asset = zip
                .by_name(&format!("layers/{}-source.raw", layer.id))
                .map_err(error)?;
            raw_bytes = raw_bytes
                .checked_add(asset.size())
                .ok_or("RAW 자산 길이가 너무 큽니다.")?;
            if asset.size() == 0 || raw_bytes > crate::raw_input::MAX_RAW_BYTES {
                return Err("RAW 원본 자산이 현재 메모리 예산을 초과합니다.".into());
            }
            let mut bytes = Vec::new();
            asset
                .take(crate::raw_input::MAX_RAW_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(error)?;
            let mut source = crate::raw_input::Source {
                record,
                bytes: Arc::new(bytes),
            };
            if manifest.version < 11 {
                source.upgrade_legacy_info();
            }
            source.refresh_shot_white();
            if !source.valid() {
                return Err("RAW 원본 자산이 잘못되었습니다.".into());
            }
            Some(Arc::new(source))
        } else {
            None
        };
        layers.push(Layer {
            id: layer.id,
            name: layer.name,
            pixels: Arc::new(pixels),
            offset: layer.offset,
            visible: layer.visible,
            locked: layer.locked,
            opacity: layer.opacity,
            blend: layer.blend,
            adjustments: layer.adjustments,
            mask,
            mask_enabled: layer.mask_enabled,
            mask_placement: layer.mask_placement,
            content: layer.content,
            transform: layer.transform,
            precision,
            bit_depth: layer.bit_depth,
            effects: layer.effects,
            parent: layer.parent,
            group: layer.group,
            adjustment_layer: layer.adjustment_layer,
            pass_through: layer.pass_through,
            clipped: layer.clipped,
            psd_extra: Arc::new(psd_extra),
            raw_source,
        });
        if !crate::filter_bounds::valid(layers.last().unwrap()) {
            return Err("프로젝트 필터 외곽이 크기 또는 원근 변형 한계를 넘습니다.".into());
        }
        if !crate::adjustment_layer::valid(layers.last().unwrap()) {
            return Err("조정 레이어 구조가 유효하지 않습니다.".into());
        }
    }
    if !ids.contains(&manifest.active) {
        return Err("활성 레이어가 없습니다.".into());
    }
    let next_id = ids.iter().max().unwrap() + 1;
    let mut resources = Vec::new();
    if manifest.psd_resources {
        let asset = zip
            .by_name("metadata/photoshop-resources.bin")
            .map_err(error)?;
        if asset.size() > 64 * 1024 * 1024 {
            return Err("Photoshop 메타데이터가 너무 큽니다.".into());
        }
        asset
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut resources)
            .map_err(error)?;
    }
    let source_icc = if manifest.source_icc {
        Some(crate::source_metadata::read_asset(
            &mut zip,
            "metadata/source.icc",
        )?)
    } else {
        None
    };
    let source_metadata =
        crate::source_metadata::Metadata::load(manifest.source_metadata, &mut zip)?;
    let document = Document {
        document_id: match manifest.document_id {
            Some(id) => id,
            None => crate::document_id::new(),
        },
        width: manifest.width,
        height: manifest.height,
        layers,
        active: manifest.active,
        next_id,
        psd_resources: Arc::new(resources),
        source_icc,
        source_metadata,
        layout: manifest.layout,
    };
    if document.layers.iter().any(|l| !crate::masks::valid(l)) {
        return Err("마스크 위치 또는 변형이 유효하지 않습니다.".into());
    }
    if !document.hierarchy_valid() {
        return Err("레이어 그룹에 순환 참조 또는 잘못된 부모가 있습니다.".into());
    }
    Ok(document)
}

pub const EXPORT_FORMATS: &[(ImageFormat, &str, &str)] = &[
    (ImageFormat::Png, "PNG", "png"),
    (ImageFormat::Jpeg, "JPEG", "jpg"),
    (ImageFormat::WebP, "WebP", "webp"),
    (ImageFormat::Tiff, "TIFF", "tiff"),
    (ImageFormat::Bmp, "BMP", "bmp"),
    (ImageFormat::Tga, "TGA", "tga"),
    (ImageFormat::Qoi, "QOI", "qoi"),
    (ImageFormat::Gif, "GIF 한 장", "gif"),
    (ImageFormat::Avif, "AVIF", "avif"),
    (ImageFormat::OpenExr, "OpenEXR 32비트", "exr"),
    (ImageFormat::Hdr, "Radiance HDR", "hdr"),
    (ImageFormat::Ico, "ICO", "ico"),
    (ImageFormat::Pnm, "PPM", "ppm"),
    (ImageFormat::Farbfeld, "Farbfeld 16비트", "ff"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportOptions {
    pub include_icc: bool,
    pub include_exif: bool,
    pub include_xmp: bool,
    pub jp2_icc: bool,
    pub jp2_resolution_enabled: bool,
    pub jp2_resolution: u16,
    pub jp2_tile: u32,
    pub jp2_grayscale: bool,
    pub jp2_progression: usize,
    pub jp2_layers: u8,
    pub jp2_depth: u8,
    pub jp2_lossless: bool,
    pub jp2_ratio: u16,
    pub quality: u8,
    pub background: [u8; 3],
    pub png_level: u8,
    pub png_filter: usize,
    pub webp_lossless: bool,
    pub webp_method: u8,
    pub webp_quality: u8,
    pub tiff_compression: usize,
    pub tiff_depth: usize,
    pub tiff_grayscale: bool,
    pub tiff_predictor: bool,
    pub tiff_float_predictor: bool,
    pub tiff_big: bool,
    pub tiff_resolution: u32,
    pub avif_quality: u8,
    pub avif_alpha_quality: u8,
    pub avif_speed: u8,
    pub avif_depth: u8,
    pub avif_color: usize,
    pub avif_threads: u16,
    pub gif_colors: u16,
    pub gif_dither: bool,
    pub gif_transparent: bool,
    pub gif_alpha: u8,
    pub gif_interlaced: bool,
    pub jpeg_sampling: usize,
    pub jpeg_average: bool,
    pub jpeg_progressive: bool,
    pub jpeg_scans: u8,
    pub jpeg_optimize: bool,
    pub jpeg_resolution: u16,
    pub jpeg_restart: u16,
}
impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            include_icc: true,
            include_exif: true,
            include_xmp: true,
            jp2_icc: false,
            jp2_resolution_enabled: false,
            jp2_resolution: 72,
            jp2_tile: 0,
            jp2_grayscale: false,
            jp2_progression: 0,
            jp2_layers: 1,
            jp2_depth: 0,
            jp2_lossless: true,
            jp2_ratio: 8,
            quality: 90,
            background: [255; 3],
            png_level: 6,
            png_filter: 0,
            webp_lossless: true,
            webp_method: 4,
            webp_quality: 90,
            tiff_compression: 0,
            tiff_depth: 0,
            tiff_grayscale: false,
            tiff_predictor: false,
            tiff_float_predictor: false,
            tiff_big: false,
            tiff_resolution: 72,
            avif_quality: 80,
            avif_alpha_quality: 80,
            avif_speed: 4,
            avif_depth: 8,
            avif_color: 0,
            avif_threads: 1,
            gif_colors: 256,
            gif_dither: false,
            gif_transparent: true,
            gif_alpha: 128,
            gif_interlaced: false,
            jpeg_sampling: 1,
            jpeg_average: true,
            jpeg_progressive: false,
            jpeg_scans: 4,
            jpeg_optimize: false,
            jpeg_resolution: 72,
            jpeg_restart: 0,
        }
    }
}
pub fn export_by_index(
    document: &Document,
    path: &Path,
    index: usize,
    options: &ExportOptions,
) -> Result<(), String> {
    if index == EXPORT_FORMATS.len() {
        crate::svg_export::save(document, path)
    } else if index == EXPORT_FORMATS.len() + 1 {
        crate::pcx::save(document, path)
    } else if index == EXPORT_FORMATS.len() + 2 {
        crate::svg_export::save_compressed(document, path)
    } else if (EXPORT_FORMATS.len() + 3..export_count()).contains(&index) {
        crate::jpeg2000_output::save(document, path, index == EXPORT_FORMATS.len() + 4, options)
    } else {
        let format = EXPORT_FORMATS
            .get(index)
            .ok_or("출력 형식이 잘못되었습니다.")?
            .0;
        export(document, path, format, options)
    }
}
pub fn export_count() -> usize {
    EXPORT_FORMATS.len() + 3 + crate::jpeg2000_output::FORMATS.len()
}
pub fn export_label(index: usize) -> Result<&'static str, String> {
    match index.checked_sub(EXPORT_FORMATS.len()) {
        Some(0) => Ok("SVG"),
        Some(1) => Ok("PCX"),
        Some(2) => Ok("SVGZ"),
        Some(i) => crate::jpeg2000_output::FORMATS
            .get(i - 3)
            .map(|v| v.0)
            .ok_or_else(|| "출력 형식이 잘못되었습니다.".into()),
        None => EXPORT_FORMATS
            .get(index)
            .map(|v| v.1)
            .ok_or_else(|| "출력 형식이 잘못되었습니다.".into()),
    }
}
pub fn export_extension(index: usize) -> Result<&'static str, String> {
    if index == EXPORT_FORMATS.len() {
        Ok("svg")
    } else if index == EXPORT_FORMATS.len() + 1 {
        Ok("pcx")
    } else if index == EXPORT_FORMATS.len() + 2 {
        Ok("svgz")
    } else if index >= EXPORT_FORMATS.len() + 3 {
        crate::jpeg2000_output::FORMATS
            .get(index - EXPORT_FORMATS.len() - 3)
            .map(|v| v.1)
            .ok_or_else(|| "출력 형식이 잘못되었습니다.".into())
    } else {
        EXPORT_FORMATS
            .get(index)
            .map(|v| v.2)
            .ok_or_else(|| "출력 형식이 잘못되었습니다.".into())
    }
}

pub const PNG_FILTER_LABELS: &[&str] = &["자동", "없음", "왼쪽 픽셀", "위쪽 행", "평균", "Paeth"];

pub fn export(
    document: &Document,
    path: &Path,
    format: ImageFormat,
    options: &ExportOptions,
) -> Result<(), String> {
    if !(1..=100).contains(&options.quality)
        || options.png_level > 9
        || options.png_filter >= PNG_FILTER_LABELS.len()
        || (format == ImageFormat::WebP && (options.webp_method > 6 || options.webp_quality > 100))
    {
        return Err("출력 품질·압축 설정이 유효하지 않습니다.".into());
    }
    if format == ImageFormat::Gif {
        return crate::gif_output::save(document, path, options);
    }
    if format == ImageFormat::Jpeg {
        return crate::jpeg_output::save(document, path, options);
    }
    if format == ImageFormat::Avif {
        return crate::avif_output::save(document, path, options);
    }
    if format == ImageFormat::Tiff {
        return crate::tiff_output::save(document, path, options);
    }
    if format == ImageFormat::WebP
        && !options.webp_lossless
        && (document.width > 16383 || document.height > 16383)
    {
        return Err("손실 WebP의 가로·세로는 16383px 이하여야 합니다.".into());
    }
    if format == ImageFormat::Hdr {
        let source = crate::precision::composite(document);
        let maximum = (255.0 / 256.0) * 2.0f32.powi(127);
        let rgb = image::Rgb32FImage::from_fn(source.width(), source.height(), |x, y| {
            let p = source.get_pixel(x, y);
            let a = p[3].clamp(0., 1.);
            image::Rgb(std::array::from_fn(|c| {
                (p[c] * a + options.background[c] as f32 / 255. * (1. - a)).clamp(0., maximum)
            }))
        });
        if rgb.pixels().any(|p| p.0.iter().any(|v| !v.is_finite())) {
            return Err("HDR 출력에 잘못된 픽셀 값이 있습니다.".into());
        }
        return atomic_write(path, |file| {
            DynamicImage::ImageRgb32F(rgb)
                .write_to(file, format)
                .map_err(error)
        });
    }
    let exif = if matches!(format, ImageFormat::Png | ImageFormat::WebP) {
        crate::exif_output::prepare(document, options.include_exif)?
    } else {
        None
    };
    let xmp = if matches!(format, ImageFormat::Png | ImageFormat::WebP) {
        crate::xmp_output::prepare(document, options.include_xmp)?
    } else {
        None
    };
    let image = document.composite();
    atomic_write(path, |file| {
        if format == ImageFormat::WebP {
            return crate::webp_output::write(
                &image,
                file,
                options,
                exif.as_deref(),
                xmp.as_deref(),
            );
        }
        if format == ImageFormat::Pnm {
            let mut rgb = image::RgbImage::new(image.width(), image.height());
            for (source, target) in image.pixels().zip(rgb.pixels_mut()) {
                let alpha = u16::from(source.0[3]);
                for c in 0..3 {
                    target.0[c] = ((u16::from(source.0[c]) * alpha
                        + u16::from(options.background[c]) * (255 - alpha)
                        + 127)
                        / 255) as u8;
                }
            }
            image::codecs::pnm::PnmEncoder::new(file)
                .with_subtype(image::codecs::pnm::PnmSubtype::Pixmap(
                    image::codecs::pnm::SampleEncoding::Binary,
                ))
                .encode(
                    rgb.as_raw().as_slice(),
                    rgb.width(),
                    rgb.height(),
                    image::ExtendedColorType::Rgb8,
                )
                .map_err(error)
        } else {
            let high = document.layers.iter().any(|l| l.bit_depth > 8);
            let image = if format == ImageFormat::OpenExr {
                DynamicImage::ImageRgba32F(crate::precision::composite(document))
            } else if (high && matches!(format, ImageFormat::Png | ImageFormat::Tiff))
                || format == ImageFormat::Farbfeld
            {
                DynamicImage::ImageRgba16(crate::precision::image16(&crate::precision::composite(
                    document,
                )))
            } else {
                DynamicImage::ImageRgba8(image)
            };
            if format == ImageFormat::Png {
                use image::codecs::png::{CompressionType, FilterType, PngEncoder};
                let compression = if options.png_level == 0 {
                    CompressionType::Uncompressed
                } else {
                    CompressionType::Level(options.png_level)
                };
                let filter = [
                    FilterType::Adaptive,
                    FilterType::NoFilter,
                    FilterType::Sub,
                    FilterType::Up,
                    FilterType::Avg,
                    FilterType::Paeth,
                ]
                .into_iter()
                .nth(options.png_filter)
                .unwrap();
                use image::ImageEncoder;
                let mut encoder = PngEncoder::new_with_quality(
                    crate::xmp_output::PngWriter::new(file, xmp.as_deref()),
                    compression,
                    filter,
                );
                if options.include_icc {
                    encoder
                        .set_icc_profile(crate::color_management::srgb_profile().to_vec())
                        .map_err(error)?;
                }
                if let Some(exif) = exif {
                    encoder.set_exif_metadata(exif).map_err(error)?;
                }
                image.write_with_encoder(encoder).map_err(error)
            } else {
                image.write_to(file, format).map_err(error)
            }
        }
    })
}

struct TemporaryOutput {
    path: PathBuf,
    owned: bool,
}
impl Drop for TemporaryOutput {
    fn drop(&mut self) {
        if self.owned {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

pub(crate) fn atomic_write(
    path: &Path,
    write: impl FnOnce(&mut File) -> Result<(), String>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(error)?
        .as_nanos();
    // Windows wall-clock reads can coincide across saving/preview workers.
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temporary = parent.join(format!(
        ".photo-editor-{}-{stamp}-{sequence}.tmp",
        std::process::id()
    ));
    let mut cleanup = TemporaryOutput {
        path: temporary.clone(),
        owned: false,
    };
    (|| {
        let mut file = File::options()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(error)?;
        cleanup.owned = true;
        write(&mut file)?;
        file.sync_all().map_err(error)?;
        drop(file);
        replace_file(&temporary, path)
    })()
}

#[cfg(windows)]
fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // Both UTF-16 paths remain alive and null-terminated for the duration of the call.
    if unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    } == 0
    {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::rename(from, to).map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn output_dir(name: &str) -> PathBuf {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("qa-output")
            .join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn concurrent_atomic_outputs_in_same_directory_keep_unique_temporaries_and_failed_destinations()
    {
        let directory = output_dir("atomic-concurrent");
        let barrier = Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8u8)
            .map(|worker| {
                let directory = directory.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    let path = directory.join(format!("worker-{worker}.bin"));
                    let mut expected = vec![worker; 4096];
                    std::fs::write(&path, &expected).unwrap();
                    barrier.wait();
                    for round in 0..32u8 {
                        let value = vec![worker.wrapping_mul(32).wrapping_add(round); 4096];
                        let fail = round % 3 == 0;
                        let result = atomic_write(&path, |file| {
                            file.write_all(&value).map_err(error)?;
                            std::thread::sleep(std::time::Duration::from_millis(2));
                            if fail {
                                Err("simulated interrupted write".into())
                            } else {
                                Ok(())
                            }
                        });
                        assert_eq!(result.is_err(), fail);
                        if !fail {
                            expected = value;
                        }
                        assert_eq!(std::fs::read(&path).unwrap(), expected);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert!(std::fs::read_dir(&directory).unwrap().all(|entry| {
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".photo-editor")
        }));
    }
    #[test]
    fn atomic_output_unwind_cleans_only_owned_temp_and_keeps_previous_file() {
        let dir = output_dir("atomic-unwind");
        let path = dir.join("previous.jpg");
        std::fs::write(&path, b"previous").unwrap();
        let unrelated = dir.join("unrelated.tmp");
        std::fs::write(&unrelated, b"keep").unwrap();
        let before: std::collections::BTreeSet<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let panic = std::panic::catch_unwind(|| {
            atomic_write(&path, |file| {
                file.write_all(b"partial").unwrap();
                panic!("encoder failure");
            })
        });
        assert!(panic.is_err());
        let after: std::collections::BTreeSet<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(before, after);
        assert_eq!(std::fs::read(path).unwrap(), b"previous");
        assert_eq!(std::fs::read(unrelated).unwrap(), b"keep");
    }

    #[test]
    fn webp_lossless_and_lossy_quality_effort_preserve_alpha_source_and_atomic_failures() {
        let dir = output_dir("webp-output");
        let pixels = RgbaImage::from_fn(128, 96, |x, y| {
            let alpha = if x < 8 {
                0
            } else if x < 16 {
                128
            } else {
                ((x + y) % 256) as u8
            };
            image::Rgba([(x * 2) as u8, (y * 2) as u8, ((x + y) % 256) as u8, alpha])
        });
        let doc = Document::from_image("WebP RGBA".into(), pixels);
        let original = doc.layers[0].pixels.clone();
        let expected = doc.composite();
        expected.save(dir.join("source.png")).unwrap();
        let path = dir.join("lossless.webp");
        export(&doc, &path, ImageFormat::WebP, &ExportOptions::default()).unwrap();
        assert_eq!(open(&path).unwrap().document.composite(), expected);
        assert!(
            std::fs::read(&path)
                .unwrap()
                .windows(4)
                .any(|w| w == b"VP8L")
        );
        let mut sizes = Vec::new();
        for quality in [0, 25, 50, 90, 100] {
            for method in [0, 4, 6] {
                let options = ExportOptions {
                    webp_lossless: false,
                    webp_quality: quality,
                    webp_method: method,
                    ..Default::default()
                };
                let path = dir.join(format!("q{quality}-m{method}.webp"));
                export(&doc, &path, ImageFormat::WebP, &options).unwrap();
                let bytes = std::fs::read(&path).unwrap();
                assert!(bytes.windows(4).any(|w| w == b"VP8 "));
                assert!(bytes.windows(4).any(|w| w == b"ALPH"));
                let restored = open(&path).unwrap().document.composite();
                assert_eq!(restored.dimensions(), expected.dimensions());
                let mut error = 0.;
                let mut coverage = 0.;
                for (a, b) in restored.pixels().zip(expected.pixels()) {
                    assert_eq!(a[3], b[3]);
                    for c in 0..3 {
                        error += f64::from(a[c].abs_diff(b[c])) * f64::from(b[3]) / 255.;
                    }
                    coverage += 3. * f64::from(b[3]) / 255.;
                }
                if quality >= 90 {
                    assert!(
                        error / coverage < 4.,
                        "q{quality} m{method} weighted RGB error {}",
                        error / coverage
                    );
                }
                if method == 4 {
                    sizes.push((quality, bytes.len()));
                }
            }
        }
        assert!(
            sizes
                .iter()
                .map(|v| v.1)
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 2
        );
        assert!(Arc::ptr_eq(&original, &doc.layers[0].pixels));
        assert_eq!(doc.composite(), expected);
        let path = dir.join("existing.webp");
        std::fs::write(&path, b"previous").unwrap();
        for options in [
            ExportOptions {
                webp_method: 7,
                ..Default::default()
            },
            ExportOptions {
                webp_quality: 101,
                ..Default::default()
            },
        ] {
            assert!(export(&doc, &path, ImageFormat::WebP, &options).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"previous");
        }
        let oversized = Document::new(16384, 1, true).unwrap();
        assert!(
            export(
                &oversized,
                &path,
                ImageFormat::WebP,
                &ExportOptions {
                    webp_lossless: false,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"previous");
        assert!(!std::fs::read_dir(&dir).unwrap().any(|p| {
            p.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".photo-editor-")
        }));
    }
    #[test]
    fn large_svg_sources_use_separate_assets_and_missing_assets_or_oversized_manifest_fail_safely()
    {
        let dir = output_dir("svg-assets");
        let source = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' width='2' height='2'><!--{}--><rect width='2' height='2' fill='red'/></svg>",
            "large source ".repeat(400_000)
        );
        assert!(source.len() > 4 * 1024 * 1024);
        let content = crate::vector::Content::Svg {
            source: source.clone(),
        };
        let mut doc = Document::from_image("large svg".into(), content.render().unwrap());
        doc.active_layer_mut().unwrap().content = Some(content);
        let path = dir.join("large-source.rphoto");
        save_project(&doc, &path).unwrap();
        let restored = load_project(&path).unwrap();
        assert_eq!(
            restored.active_layer().unwrap().content,
            doc.active_layer().unwrap().content
        );
        let mut zip = ZipArchive::new(File::open(&path).unwrap()).unwrap();
        assert!(zip.by_name("manifest.json").unwrap().size() < 16 * 1024);
        assert_eq!(
            zip.by_name("layers/1-source.svg").unwrap().size(),
            source.len() as u64
        );
        let broken = dir.join("missing-source.rphoto");
        let mut writer = ZipWriter::new(File::create(&broken).unwrap());
        for index in 0..zip.len() {
            let mut asset = zip.by_index(index).unwrap();
            if asset.name().ends_with("-source.svg") {
                continue;
            }
            writer
                .start_file(asset.name(), SimpleFileOptions::default())
                .unwrap();
            std::io::copy(&mut asset, &mut writer).unwrap();
        }
        writer.finish().unwrap();
        assert!(load_project(&broken).is_err());
        let before = std::fs::read(&path).unwrap();
        doc.layers[0].name = "x".repeat(4 * 1024 * 1024);
        assert!(save_project(&doc, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    #[test]
    fn older_project_versions_default_layout_and_invalid_layout_cannot_replace_file() {
        let dir = output_dir("project-versions");
        let current = dir.join("current.rphoto");
        let mut doc = Document::new(3, 2, true).unwrap();
        doc.layout.guides.push(crate::guides::Guide {
            axis: 0,
            position: 1.0,
        });
        save_project(&doc, &current).unwrap();
        let original = std::fs::read(&current).unwrap();
        for version in [1, 2, 3, 4] {
            let mut archive = ZipArchive::new(Cursor::new(&original)).unwrap();
            let path = dir.join(format!("version-{version}.rphoto"));
            let mut writer = ZipWriter::new(File::create(&path).unwrap());
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index).unwrap();
                let name = entry.name().to_owned();
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                if name == "manifest.json" {
                    let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    manifest["version"] = version.into();
                    manifest.as_object_mut().unwrap().remove("layout");
                    manifest.as_object_mut().unwrap().remove("document_id");
                    bytes = serde_json::to_vec(&manifest).unwrap();
                }
                writer
                    .start_file(
                        name,
                        SimpleFileOptions::default()
                            .compression_method(zip::CompressionMethod::Stored),
                    )
                    .unwrap();
                writer.write_all(&bytes).unwrap();
            }
            writer.finish().unwrap();
            let restored = load_project(&path).unwrap();
            assert_eq!(restored.layout, crate::guides::Layout::default());
            assert_eq!(restored.composite(), doc.composite());
            assert!(crate::document_id::valid(&restored.document_id));
            assert_ne!(restored.document_id, doc.document_id);
            let migrated = dir.join(format!("migrated-{version}.rphoto"));
            save_project(&restored, &migrated).unwrap();
            assert_eq!(
                load_project(&migrated).unwrap().document_id,
                restored.document_id
            );
        }
        doc.layout.guides[0].axis = 2;
        assert!(save_project(&doc, &current).is_err());
        assert_eq!(std::fs::read(current).unwrap(), original);
    }

    #[test]
    fn document_identity_survives_rename_save_as_history_and_invalid_ids_are_rejected() {
        let dir = output_dir("document-id");
        let mut editor = crate::document::Editor::new(Document::new(3, 2, true).unwrap(), false);
        let id = editor.document.document_id.clone();
        editor.edit("rename", |doc| doc.layers[0].name = "renamed".into());
        editor.undo();
        assert_eq!(editor.document.document_id, id);
        editor.redo();
        assert_eq!(editor.document.document_id, id);
        for name in ["original.rphoto", "save-as.rphoto"] {
            let path = dir.join(name);
            save_project(&editor.document, &path).unwrap();
            assert_eq!(load_project(&path).unwrap().document_id, id);
        }
        let path = dir.join("original.rphoto");
        let before = std::fs::read(&path).unwrap();
        editor.document.document_id = "../../invalid".into();
        assert!(save_project(&editor.document, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        for value in [None, Some(serde_json::json!("../../invalid"))] {
            let broken = dir.join(if value.is_none() {
                "missing.rphoto"
            } else {
                "invalid.rphoto"
            });
            let mut archive = ZipArchive::new(Cursor::new(&before)).unwrap();
            let mut writer = ZipWriter::new(File::create(&broken).unwrap());
            for index in 0..archive.len() {
                let mut entry = archive.by_index(index).unwrap();
                let name = entry.name().to_owned();
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes).unwrap();
                if name == "manifest.json" {
                    let mut manifest: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    if let Some(value) = value.as_ref() {
                        manifest["document_id"] = value.clone();
                    } else {
                        manifest.as_object_mut().unwrap().remove("document_id");
                    }
                    bytes = serde_json::to_vec(&manifest).unwrap();
                }
                writer
                    .start_file(name, SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(&bytes).unwrap();
            }
            writer.finish().unwrap();
            assert!(load_project(&broken).is_err());
        }
    }

    #[test]
    fn project_roundtrip_and_overwrite_preserve_layers() {
        let path = output_dir("project").join("한글 문서.rphoto");
        let mut doc = Document::new(4, 3, true).unwrap();
        doc.add_layer(
            "사진".into(),
            RgbaImage::from_pixel(2, 2, image::Rgba([220, 20, 10, 128])),
        )
        .unwrap();
        let layer = doc.active_layer_mut().unwrap();
        layer.offset = [-1, 1];
        layer.transform.translation = [0.375, -0.125];
        layer.opacity = 0.5;
        layer.locked = true;
        save_project(&doc, &path).unwrap();
        doc.layers[0].name = "바뀐 배경".into();
        save_project(&doc, &path).unwrap();
        let restored = load_project(&path).unwrap();
        assert_eq!(restored.layers[0].name, "바뀐 배경");
        assert_eq!(restored.layers[1].offset, [-1, 1]);
        assert_eq!(restored.layers[1].transform.translation, [0.375, -0.125]);
        assert!(restored.layers[1].locked);
        assert_eq!(restored.active, doc.active);
        assert_eq!(restored.composite(), doc.composite());
    }

    #[test]
    fn editable_content_all_effects_and_curves_survive_project_save_and_undo() {
        let content = crate::vector::Content::Shape {
            kind: crate::vector::ShapeKind::Rounded,
            width: 4,
            height: 3,
            fill: [20, 30, 240, 255],
            stroke: [255; 4],
            thickness: 1.0,
        };
        let mut doc = Document::from_image("도형".into(), content.render().unwrap());
        let l = doc.active_layer_mut().unwrap();
        l.content = Some(content.clone());
        l.transform.angle = 13.0;
        l.adjustments.curves[1].points.insert(1, [0.5, 0.65]);
        l.adjustments.gradient_map = true;
        l.adjustments.motion_length = 3;
        l.adjustments.grain = 0.1;
        for e in [
            &mut l.effects.stroke,
            &mut l.effects.shadow,
            &mut l.effects.overlay,
            &mut l.effects.inner_shadow,
            &mut l.effects.outer_glow,
            &mut l.effects.inner_glow,
        ] {
            e.enabled = true;
            e.radius = 2.0;
            e.offset = [1, 0];
            e.color = [180, 80, 10, 190];
        }
        let mut editor = crate::document::Editor::new(doc, false);
        editor.edit("커브", |doc| {
            doc.layers[0].adjustments.curves[1].points[1][1] = 0.8
        });
        let path = output_dir("editable").join("all-properties.rphoto");
        save_project(&editor.document, &path).unwrap();
        let restored = load_project(&path).unwrap();
        assert_eq!(restored.layers[0].content, Some(content));
        assert_eq!(
            restored.layers[0].effects,
            editor.document.layers[0].effects
        );
        assert_eq!(
            restored.layers[0].adjustments,
            editor.document.layers[0].adjustments
        );
        assert_eq!(restored.composite(), editor.document.composite());
        editor.undo();
        assert_eq!(
            editor.document.layers[0].adjustments.curves[1].points[1][1],
            0.65
        );
        editor.redo();
        assert_eq!(
            editor.document.layers[0].adjustments.curves[1].points[1][1],
            0.8
        );
    }

    #[test]
    fn checked_native_save_keeps_external_revisions_and_cleans_staging_files() {
        let dir = output_dir("file-watch-save");
        for extension in ["rphoto", "psd", "psb"] {
            let path = dir.join(format!("checked.{extension}"));
            let mut doc = Document::new(3, 2, false).unwrap();
            save_document(&doc, &path).unwrap();
            let baseline = crate::file_watch::snapshot(&path).unwrap();
            std::fs::write(&path, b"external revision").unwrap();
            let outcome = save_document_checked(&doc, &path, Some(&baseline)).unwrap();
            let CheckedSave::Conflict(current) = outcome else {
                panic!("external version was overwritten");
            };
            assert_eq!(std::fs::read(&path).unwrap(), b"external revision");
            doc.layers[0].name = "updated document".into();
            assert!(matches!(
                save_document_checked(&doc, &path, Some(&current)).unwrap(),
                CheckedSave::Saved(_)
            ));
            assert_eq!(
                open(&path).unwrap().document.layers[0].name,
                doc.layers[0].name
            );
            std::fs::remove_file(&path).unwrap();
            assert!(matches!(
                save_document_checked(&doc, &path, Some(&current)).unwrap(),
                CheckedSave::Conflict(crate::file_watch::Snapshot::Missing)
            ));
            assert!(!path.exists());
            assert!(matches!(
                save_document_checked(&doc, &path, Some(&crate::file_watch::Snapshot::Missing))
                    .unwrap(),
                CheckedSave::Saved(_)
            ));
        }
        assert!(std::fs::read_dir(dir).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".photo-editor")
        }));
    }

    #[test]
    fn checked_save_rejects_same_metadata_different_content_for_project_psd_and_psb() {
        let dir = output_dir("file-watch-content-save");
        for extension in ["rphoto", "psd", "psb"] {
            let path = dir.join(format!("한글 충돌.{extension}"));
            let mut doc = Document::new(3, 2, false).unwrap();
            save_document(&doc, &path).unwrap();
            let baseline = crate::file_watch::snapshot(&path).unwrap();
            let original = std::fs::read(&path).unwrap();
            let modified = std::fs::metadata(&path).unwrap().modified().unwrap();
            let mut external = original.clone();
            let last = external.len() - 1;
            external[last] ^= 0x41;
            std::fs::write(&path, &external).unwrap();
            File::options()
                .write(true)
                .open(&path)
                .unwrap()
                .set_times(std::fs::FileTimes::new().set_modified(modified))
                .unwrap();
            doc.layers[0].name = "user edit".into();
            let current = match save_document_checked(&doc, &path, Some(&baseline)).unwrap() {
                CheckedSave::Conflict(current) => current,
                _ => panic!("same length/time content was overwritten"),
            };
            assert_eq!(std::fs::read(&path).unwrap(), external);
            match (&baseline, &current) {
                (
                    crate::file_watch::Snapshot::Present {
                        length: a,
                        modified: b,
                        hash: c,
                    },
                    crate::file_watch::Snapshot::Present {
                        length: x,
                        modified: y,
                        hash: z,
                    },
                ) => {
                    assert_eq!((a, b), (x, y));
                    assert_ne!(c, z);
                }
                _ => panic!(),
            }
            assert!(matches!(
                save_document_checked(&doc, &path, Some(&current)).unwrap(),
                CheckedSave::Saved(_)
            ));
            assert_eq!(open(&path).unwrap().document.layers[0].name, "user edit");
            assert_eq!(doc.layers[0].name, "user edit");
        }
        assert!(std::fs::read_dir(dir).unwrap().all(|e| {
            !e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".photo-editor")
        }));
    }
    #[test]
    fn png_levels_filters_and_export_background_preserve_precision_and_existing_files() {
        let dir = output_dir("export-options");
        for depth in [8, 16] {
            let source = if depth == 8 {
                DynamicImage::ImageRgba8(image::RgbaImage::from_fn(8, 4, |x, y| {
                    image::Rgba([x as u8 * 31, y as u8 * 60, 113, x as u8 * 32])
                }))
            } else {
                DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(8, 4, |x, y| {
                    image::Rgba([x as u16 * 8123, y as u16 * 16001, 30123, x as u16 * 8192])
                }))
            };
            let doc = crate::precision::document("PNG".into(), source).unwrap();
            let expected = crate::precision::composite(&doc);
            for level in 0..=9 {
                for filter in 0..PNG_FILTER_LABELS.len() {
                    let path = dir.join(format!("depth-{depth}-level-{level}-filter-{filter}.png"));
                    let options = ExportOptions {
                        png_level: level,
                        png_filter: filter,
                        ..Default::default()
                    };
                    export(&doc, &path, ImageFormat::Png, &options).unwrap();
                    let restored = open(&path).unwrap().document;
                    assert_eq!(restored.layers[0].bit_depth, depth);
                    let actual = crate::precision::composite(&restored);
                    for (a, b) in actual.pixels().zip(expected.pixels()) {
                        for c in 0..4 {
                            assert!(
                                (a[c] - b[c]).abs()
                                    <= if depth == 8 { 1. / 255. } else { 1. / 65535. }
                            );
                        }
                    }
                }
            }
        }
        let doc = Document::from_image(
            "alpha".into(),
            RgbaImage::from_pixel(16, 8, image::Rgba([230, 110, 20, 128])),
        );
        let options = ExportOptions {
            quality: 100,
            background: [10, 30, 210],
            ..Default::default()
        };
        let expected: [u8; 3] = std::array::from_fn(|c| {
            ((u16::from(doc.layers[0].pixels.get_pixel(0, 0)[c]) * 128
                + u16::from(options.background[c]) * 127
                + 127)
                / 255) as u8
        });
        for format in [ImageFormat::Pnm, ImageFormat::Jpeg, ImageFormat::Hdr] {
            let path = dir.join(format!(
                "background-{:?}.{}",
                format,
                format.extensions_str()[0]
            ));
            export(&doc, &path, format, &options).unwrap();
            let restored = open(&path).unwrap().document.composite();
            for p in restored.pixels() {
                assert_eq!(p[3], 255);
                for c in 0..3 {
                    assert!(
                        p[c].abs_diff(expected[c]) <= 2,
                        "{format:?} {p:?} {expected:?}"
                    );
                }
            }
        }
        let path = dir.join("existing.png");
        std::fs::write(&path, b"previous").unwrap();
        let invalid = ExportOptions {
            png_level: 10,
            ..Default::default()
        };
        assert!(export(&doc, &path, ImageFormat::Png, &invalid).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"previous");
    }
    #[test]
    fn radiance_export_retains_hdr_mattes_alpha_and_explicitly_clips_negative_channels() {
        let dir = output_dir("hdr-output");
        for width in [3, 16] {
            let source = image::Rgba32FImage::from_fn(width, 2, |x, y| {
                image::Rgba([
                    4. + x as f32 / 8.,
                    if y == 0 { -0.2 } else { 2.5 },
                    0.5,
                    if x == 0 {
                        0.
                    } else if x == 1 {
                        0.5
                    } else {
                        1.
                    },
                ])
            });
            let doc = crate::precision::document(
                "HDR".into(),
                DynamicImage::ImageRgba32F(source.clone()),
            )
            .unwrap();
            let original = doc.layers[0].precision.clone();
            let path = dir.join(format!("width-{width}.hdr"));
            export(&doc, &path, ImageFormat::Hdr, &ExportOptions::default()).unwrap();
            let restored = open(&path).unwrap().document;
            let pixels = restored.layers[0].precision.as_ref().unwrap();
            assert_eq!(restored.layers[0].bit_depth, 32);
            let expected = crate::precision::composite(&doc);
            for (p, q) in expected.pixels().zip(pixels.pixels()) {
                let a = p[3].clamp(0., 1.);
                for c in 0..3 {
                    let value = (p[c] * a + 1. - a).max(0.);
                    let maximum = (0..3)
                        .map(|i| (p[i] * a + 1. - a).max(0.))
                        .fold(1.0f32, f32::max);
                    assert!((value - q[c]).abs() <= maximum / 128., "{p:?} {q:?}");
                }
                assert_eq!(q[3], 1.);
            }
            assert!(pixels.pixels().any(|p| p[0] > 4.));
            assert_eq!(doc.layers[0].precision, original);
            let max =
                image::Rgba32FImage::from_pixel(width, 1, image::Rgba([f32::MAX, 1., 0., 1.]));
            let doc =
                crate::precision::document("max".into(), DynamicImage::ImageRgba32F(max)).unwrap();
            let path = dir.join(format!("max-{width}.hdr"));
            export(&doc, &path, ImageFormat::Hdr, &ExportOptions::default()).unwrap();
            assert!(
                open(&path).unwrap().document.layers[0]
                    .precision
                    .as_ref()
                    .unwrap()
                    .pixels()
                    .all(|p| p[0].is_finite() && p[0] > 1e38)
            );
        }
    }
    #[test]
    fn common_codecs_export_and_open() {
        let dir = output_dir("codecs");
        let mut doc = Document::new(12, 8, true).unwrap();
        let image = Arc::make_mut(&mut doc.layers[0].pixels);
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            pixel.0 = [
                (x * 20) as u8,
                (y * 30) as u8,
                120,
                if x < 3 { 100 } else { 255 },
            ];
        }
        for (format, _, extension) in EXPORT_FORMATS {
            let path = dir.join(format!("sample.{extension}"));
            export(&doc, &path, *format, &ExportOptions::default()).unwrap();
            if *format == ImageFormat::Pnm {
                assert!(std::fs::read(&path).unwrap().starts_with(b"P6\n"));
            }
            let opened = open(&path).unwrap();
            assert_eq!((opened.document.width, opened.document.height), (12, 8));
            if matches!(
                format,
                ImageFormat::Png
                    | ImageFormat::WebP
                    | ImageFormat::Tiff
                    | ImageFormat::Tga
                    | ImageFormat::Qoi
            ) {
                assert_eq!(opened.document.composite(), doc.composite());
            }
        }
    }

    #[test]
    fn malformed_project_does_not_replace_existing_file() {
        let path = output_dir("failure").join("existing.rphoto");
        std::fs::write(&path, b"keep me").unwrap();
        assert!(load_project(&path).is_err());
        let result = atomic_write(&path, |file| {
            file.write_all(b"unfinished").unwrap();
            Err("interrupted".into())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"keep me");
    }

    #[test]
    fn high_precision_projects_and_exports_roundtrip_without_eight_bit_reduction() {
        let dir = output_dir("precision");
        let samples = [
            image::DynamicImage::ImageRgba16(image::ImageBuffer::from_pixel(
                3,
                2,
                image::Rgba([12345u16, 23456, 34567, 65535]),
            )),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
                3,
                2,
                image::Rgba([4.25, 0.1234567, -0.0625, 1.0]),
            )),
        ];
        for sample in samples {
            let depth = crate::precision::bit_depth(&sample);
            let mut doc = crate::precision::document("정밀도".into(), sample.clone()).unwrap();
            doc.layers[0].mask = Some(Arc::new(image::GrayImage::from_pixel(
                3,
                2,
                image::Luma([128]),
            )));
            doc.layers[0].transform.angle = 35.0;
            doc.layers[0].adjustments.exposure = 0.25;
            doc.layers[0].adjustments.curves[0]
                .points
                .insert(1, [0.5, 0.6]);
            doc.layers[0].adjustments.grain = 0.1;
            doc.layers[0].adjustments.vignette = 0.3;
            let path = dir.join(format!("depth-{depth}.rphoto"));
            save_project(&doc, &path).unwrap();
            let restored = load_project(&path).unwrap();
            assert_eq!(restored.layers[0].bit_depth, depth);
            assert_eq!(restored.layers[0].precision, doc.layers[0].precision);
            assert_eq!(restored.layers[0].mask, doc.layers[0].mask);
            assert_eq!(restored.layers[0].transform, doc.layers[0].transform);
            assert_eq!(restored.layers[0].adjustments, doc.layers[0].adjustments);
            assert_eq!(
                crate::precision::composite(&restored),
                crate::precision::composite(&doc)
            );
            assert!(save_document(&doc, &dir.join(format!("depth-{depth}.psd"))).is_err());
            doc.layers[0].mask = None;
            doc.layers[0].transform = Default::default();
            doc.layers[0].adjustments = Default::default();
            let format = if depth == 16 {
                ImageFormat::Png
            } else {
                ImageFormat::OpenExr
            };
            let path = dir.join(if depth == 16 {
                "output16.png"
            } else {
                "output32.exr"
            });
            export(&doc, &path, format, &ExportOptions::default()).unwrap();
            let reopened = open(&path).unwrap();
            assert_eq!(reopened.document.layers[0].bit_depth, depth);
            assert_eq!(
                reopened.document.layers[0].precision,
                doc.layers[0].precision
            );
        }
    }
}
