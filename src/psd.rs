//! Layered PSD/PSB reader and writer. See Adobe's Photoshop File Formats specification.
use crate::{
    blend::BlendMode,
    document::{Document, Layer, MAX_PIXELS, validate_size},
};
use image::{GrayImage, RgbaImage};
use std::{
    io::{Read, Write},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum OutputColor {
    #[default]
    Rgb,
    Grayscale,
}
impl OutputColor {
    pub const ALL: [Self; 2] = [Self::Rgb, Self::Grayscale];
    pub fn label(self) -> &'static str {
        match self {
            Self::Rgb => "RGB",
            Self::Grayscale => "회색조",
        }
    }
    fn channels(self) -> &'static [usize] {
        match self {
            Self::Rgb => &[0, 1, 2, 3],
            Self::Grayscale => &[0, 3],
        }
    }
    fn profile(self) -> &'static [u8; 4] {
        match self {
            Self::Rgb => b"RGB ",
            Self::Grayscale => b"GRAY",
        }
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct OutputOptions {
    pub compression: crate::psd_compression::Compression,
    pub color: OutputColor,
    pub grayscale_layers: bool,
}
pub(crate) fn neutral(document: &Document) -> bool {
    document.layers.iter().filter(|l| !l.group).all(|l| {
        l.precision.as_ref().map_or_else(
            || l.pixels.pixels().all(|p| p[0] == p[1] && p[1] == p[2]),
            |image| {
                image
                    .pixels()
                    .all(|p| p[0].to_bits() == p[1].to_bits() && p[1].to_bits() == p[2].to_bits())
            },
        )
    })
}
pub(crate) fn grayscale(pixel: &mut image::Rgba<f32>) {
    let gray = if pixel[0] == pixel[1] && pixel[1] == pixel[2] {
        pixel[0]
    } else {
        (f64::from(pixel[0]) * 0.2126 + f64::from(pixel[1]) * 0.7152 + f64::from(pixel[2]) * 0.0722)
            as f32
    };
    pixel.0[..3].fill(gray);
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn take(&mut self, len: usize) -> Result<&'a [u8], String> {
        let end = self.at.checked_add(len).ok_or("PSD 길이 초과")?;
        let bytes = self
            .bytes
            .get(self.at..end)
            .ok_or("PSD 데이터가 잘렸습니다.")?;
        self.at = end;
        Ok(bytes)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(self.u32()? as i32)
    }
    fn length(&mut self, large: bool) -> Result<usize, String> {
        let n = if large {
            u64::from_be_bytes(self.take(8)?.try_into().unwrap())
        } else {
            u64::from(self.u32()?)
        };
        usize::try_from(n).map_err(|_| "PSD 길이 초과".into())
    }
    fn section(&mut self, large: bool) -> Result<Reader<'a>, String> {
        let len = self.length(large)?;
        Ok(Self::new(self.take(len)?))
    }
}
fn word(out: &mut Vec<u8>, n: u16) {
    out.extend(n.to_be_bytes());
}
fn int(out: &mut Vec<u8>, n: u32) {
    out.extend(n.to_be_bytes());
}
fn length(out: &mut Vec<u8>, n: u64, large: bool) -> Result<(), String> {
    if large {
        out.extend(n.to_be_bytes());
    } else {
        out.extend(
            u32::try_from(n)
                .map_err(|_| "PSD가 너무 큽니다. PSB로 저장하세요.")?
                .to_be_bytes(),
        );
    }
    Ok(())
}
fn blend_key(mode: BlendMode) -> [u8; 4] {
    *match mode {
        BlendMode::Normal => b"norm",
        BlendMode::Dissolve => b"diss",
        BlendMode::Multiply => b"mul ",
        BlendMode::Screen => b"scrn",
        BlendMode::Overlay => b"over",
        BlendMode::Darken => b"dark",
        BlendMode::Lighten => b"lite",
        BlendMode::ColorDodge => b"div ",
        BlendMode::ColorBurn => b"idiv",
        BlendMode::HardLight => b"hLit",
        BlendMode::SoftLight => b"sLit",
        BlendMode::Difference => b"diff",
        BlendMode::Exclusion => b"smud",
        BlendMode::Add => b"lddg",
        BlendMode::Subtract => b"fsub",
        BlendMode::LinearBurn => b"lbrn",
        BlendMode::DarkerColor => b"dkCl",
        BlendMode::LighterColor => b"lgCl",
        BlendMode::LinearLight => b"lLit",
        BlendMode::VividLight => b"vLit",
        BlendMode::PinLight => b"pLit",
        BlendMode::HardMix => b"hMix",
        BlendMode::Divide => b"fdiv",
        BlendMode::Hue => b"hue ",
        BlendMode::Saturation => b"sat ",
        BlendMode::Color => b"colr",
        BlendMode::Luminosity => b"lum ",
    }
}
fn blend_mode(key: &[u8]) -> Result<BlendMode, String> {
    BlendMode::ALL
        .into_iter()
        .find(|m| blend_key(*m) == key)
        .ok_or_else(|| {
            format!(
                "아직 합성하지 못하는 PSD 블렌드 모드입니다: {}",
                String::from_utf8_lossy(key)
            )
        })
}

#[cfg(test)]
fn channel(bytes: &[u8], width: u32, height: u32, large: bool) -> Result<Vec<u8>, String> {
    channel_depth(bytes, width, height, large, 8)
}
fn prediction(output: &mut [u8], width: usize, depth: u16) {
    let row_bytes = width * depth as usize / 8;
    for row in output.chunks_exact_mut(row_bytes) {
        if depth == 16 {
            let mut previous = 0u16;
            for sample in row.as_chunks_mut::<2>().0 {
                previous = previous.wrapping_add(u16::from_be_bytes(*sample));
                sample.copy_from_slice(&previous.to_be_bytes());
            }
        } else {
            for i in 1..row.len() {
                row[i] = row[i].wrapping_add(row[i - 1]);
            }
            if depth == 32 {
                let planes = row.to_vec();
                for pixel in 0..width {
                    for byte in 0..4 {
                        row[pixel * 4 + byte] = planes[byte * width + pixel];
                    }
                }
            }
        }
    }
}
pub(crate) fn channel_depth(
    bytes: &[u8],
    width: u32,
    height: u32,
    large: bool,
    depth: u16,
) -> Result<Vec<u8>, String> {
    validate_size(width, height)?;
    if ![8, 16, 32].contains(&depth) {
        return Err("지원하지 않는 PSD 정밀도입니다.".into());
    }
    let row_bytes = width as usize * depth as usize / 8;
    let count = row_bytes * height as usize;
    let mut r = Reader::new(bytes);
    let compression = r.u16()?;
    let output = match compression {
        0 => r.take(count)?.to_vec(),
        1 => {
            let sizes = (0..height)
                .map(|_| {
                    if large {
                        r.u32().map(|v| v as usize)
                    } else {
                        r.u16().map(|v| v as usize)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            let mut output = Vec::with_capacity(count);
            for size in sizes {
                let row = r.take(size)?;
                let begin = output.len();
                let mut row = Reader::new(row);
                while row.at < row.bytes.len() {
                    let header = row.u8()? as i8;
                    if header >= 0 {
                        output.extend_from_slice(row.take(header as usize + 1)?);
                    } else if header != -128 {
                        let value = row.u8()?;
                        output.extend(std::iter::repeat_n(value, (1 - i16::from(header)) as usize));
                    }
                    if output.len() - begin > row_bytes {
                        return Err("PSD RLE 행 길이가 잘못되었습니다.".into());
                    }
                }
                if output.len() - begin != row_bytes {
                    return Err("PSD RLE 행 데이터가 잘렸습니다.".into());
                }
            }
            output
        }
        2 | 3 => {
            let mut output = Vec::new();
            flate2::read::ZlibDecoder::new(r.take(r.bytes.len() - r.at)?)
                .take(count as u64 + 1)
                .read_to_end(&mut output)
                .map_err(|e| e.to_string())?;
            if output.len() != count {
                return Err("PSD ZIP 채널 길이가 잘못되었습니다.".into());
            }
            if compression == 3 {
                prediction(&mut output, width as usize, depth);
            }
            output
        }
        _ => return Err("지원하지 않는 PSD 압축입니다.".into()),
    };
    Ok(output)
}

fn sample(bytes: &[u8], index: usize, depth: u16) -> f32 {
    match depth {
        16 => {
            u16::from_be_bytes(bytes[index * 2..index * 2 + 2].try_into().unwrap()) as f32 / 65535.0
        }
        32 => f32::from_be_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap()),
        _ => bytes[index] as f32 / 255.0,
    }
}

fn layer_info<'a>(outer: &mut Reader<'a>, large: bool, depth: u16) -> Result<Reader<'a>, String> {
    let mut info = outer.section(large)?;
    if outer.bytes.len() - outer.at >= 4 {
        outer.section(false)?;
    }
    // High-bit Photoshop documents may keep the native layer records in a global Lr16/Lr32 block.
    while outer.bytes.len() - outer.at >= 12 {
        while outer.at < outer.bytes.len() && outer.bytes[outer.at] == 0 {
            outer.at += 1;
        }
        if outer.bytes.len() - outer.at < 12 {
            break;
        }
        let signature = outer.take(4)?;
        if signature != b"8BIM" && signature != b"8B64" {
            return Err("PSD 전역 레이어 레코드 서명이 잘못되었습니다.".into());
        }
        let key = outer.take(4)?;
        let extended = large
            && [
                b"LMsk".as_slice(),
                b"Lr16",
                b"Lr32",
                b"Layr",
                b"Mt16",
                b"Mt32",
                b"Mtrn",
                b"Alph",
                b"FMsk",
                b"lnk2",
                b"FEid",
                b"FXid",
                b"PxSD",
            ]
            .contains(&key);
        let len = outer.length(extended)?;
        let data = outer.take(len)?;
        if (depth == 16 && key == b"Lr16")
            || (depth == 32 && key == b"Lr32")
            || (depth == 8 && key == b"Layr")
        {
            info = Reader::new(data);
        }
    }
    Ok(info)
}

struct Record {
    name: String,
    bounds: [i32; 4],
    channels: Vec<(i16, usize)>,
    blend: BlendMode,
    opacity: f32,
    visible: bool,
    locked: bool,
    mask_bounds: Option<[i32; 4]>,
    mask_default: u8,
    mask_enabled: bool,
    clipped: bool,
    section: u32,
    pass_through: bool,
    extra: Vec<u8>,
    source_id: Option<u64>,
}

type ResourceBlock<'a> = (u16, &'a [u8], &'a [u8]);
fn resource_blocks(bytes: &[u8]) -> Result<Vec<ResourceBlock<'_>>, String> {
    let mut reader = Reader::new(bytes);
    let mut blocks = Vec::new();
    while reader.at < bytes.len() {
        let start = reader.at;
        if reader.take(4)? != b"8BIM" {
            return Err("Photoshop 이미지 리소스 서명이 잘못되었습니다.".into());
        }
        let id = reader.u16()?;
        let name = reader.u8()? as usize;
        reader.take(name)?;
        if !(name + 1).is_multiple_of(2) {
            reader.take(1)?;
        }
        let length = reader.u32()? as usize;
        let data = reader.take(length)?;
        if !length.is_multiple_of(2) {
            reader.take(1)?;
        }
        blocks.push((id, data, &bytes[start..reader.at]));
    }
    Ok(blocks)
}

fn read_layout(resources: &[u8]) -> Result<crate::guides::Layout, String> {
    let mut layout = crate::guides::Layout::default();
    for (id, data, _) in resource_blocks(resources)? {
        if id != 1032 {
            continue;
        }
        let mut reader = Reader::new(data);
        if reader.u32()? != 1 {
            return Err("지원하지 않는 Photoshop 가이드 버전입니다.".into());
        }
        // Adobe resource 1032 uses 1/32 document pixel coordinates.
        let spacing = [reader.u32()? as f32 / 32.0, reader.u32()? as f32 / 32.0];
        if spacing.iter().all(|s| (1.0..=1e6).contains(s)) {
            layout.spacing = spacing;
        }
        let count = reader.u32()?;
        if count > 4096 {
            return Err("Photoshop 가이드가 현재 한도를 초과합니다.".into());
        }
        layout.guides.clear();
        for _ in 0..count {
            let position = reader.i32()? as f32 / 32.0;
            let axis = reader.u8()? as usize;
            if axis > 1 {
                return Err("Photoshop 가이드 방향이 잘못되었습니다.".into());
            }
            layout.guides.push(crate::guides::Guide { axis, position });
        }
    }
    Ok(layout)
}

#[cfg(test)]
pub(crate) fn has_non_rgb_profile(document: &Document) -> bool {
    has_incompatible_profile(document, OutputColor::Rgb)
}
pub(crate) fn has_incompatible_profile(document: &Document, color: OutputColor) -> bool {
    resource_blocks(&document.psd_resources).is_ok_and(|blocks| {
        blocks.iter().any(|(id, data, _)| {
            *id == 1039 && data.len() >= 20 && &data[16..20] != color.profile()
        })
    })
}
fn write_resources(document: &Document, color: OutputColor) -> Result<Vec<u8>, String> {
    let blocks: Vec<_> = resource_blocks(&document.psd_resources)?
        .into_iter()
        .filter(|(id, data, _)| {
            !(*id == 1039 && data.len() >= 20 && &data[16..20] != color.profile())
        })
        .collect();
    let layout = &document.layout;
    let write_guides = !layout.guides.is_empty()
        || layout.spacing != crate::guides::Layout::default().spacing
        || blocks.iter().any(|(id, _, _)| *id == 1032);
    if !write_guides {
        return Ok(blocks
            .iter()
            .flat_map(|(_, _, raw)| raw.iter().copied())
            .collect());
    }
    if !layout.valid() {
        return Err("가이드 또는 격자 설정이 잘못되었습니다.".into());
    }
    let mut output = Vec::new();
    for (id, _, raw) in blocks {
        if id != 1032 {
            output.extend(raw);
        }
    }
    let mut data = Vec::new();
    int(&mut data, 1);
    for spacing in layout.spacing {
        int(&mut data, (spacing * 32.0).round() as u32);
    }
    int(&mut data, layout.guides.len() as u32);
    for guide in &layout.guides {
        let position = (guide.position as f64 * 32.0).round();
        if position < i32::MIN as f64 || position > i32::MAX as f64 {
            return Err(
                "가이드 위치를 PSD/PSB 좌표로 표현할 수 없습니다. .rphoto로 저장하세요.".into(),
            );
        }
        int(&mut data, position as i32 as u32);
        data.push(guide.axis as u8);
    }
    output.extend(b"8BIM");
    word(&mut output, 1032);
    word(&mut output, 0); // Empty even-padded Pascal name.
    int(&mut output, data.len() as u32);
    output.extend(data);
    if output.len() % 2 != 0 {
        output.push(0);
    }
    Ok(output)
}

pub fn read(bytes: &[u8]) -> Result<Document, String> {
    read_order(bytes, false)
}
pub(crate) fn read_legacy(bytes: &[u8]) -> Result<Document, String> {
    read_order(bytes, true)
}
fn read_order(bytes: &[u8], force_legacy: bool) -> Result<Document, String> {
    if bytes.len() as u64 > MAX_PIXELS * 32 + 64 * 1024 * 1024 {
        return Err("PSD 파일이 메모리 예산을 초과합니다.".into());
    }
    let mut r = Reader::new(bytes);
    if r.take(4)? != b"8BPS" {
        return Err("PSD 서명이 잘못되었습니다.".into());
    }
    let version = r.u16()?;
    if !(1..=2).contains(&version) {
        return Err("PSD 버전이 잘못되었습니다.".into());
    }
    let large = version == 2;
    if r.take(6)? != [0; 6] {
        return Err("PSD 예약 필드가 잘못되었습니다.".into());
    }
    let channels = r.u16()?;
    let height = r.u32()?;
    let width = r.u32()?;
    validate_size(width, height)?;
    let depth = r.u16()?;
    let mode = r.u16()?;
    if ![8, 16, 32].contains(&depth) || ![1, 3].contains(&mode) {
        return Err(
            "이 PSD/PSB의 색 모드는 아직 연결되지 않았습니다. 현재 RGB·회색조 8/16/32비트를 읽으며 원본을 변환하지 않았습니다."
                .into(),
        );
    }
    let color_channels = if mode == 1 { 1 } else { 3 };
    if !(color_channels..=56).contains(&channels) {
        return Err("PSD 채널 수가 잘못되었습니다.".into());
    }
    if mode == 1 && channels > 2 {
        return Err("회색조 PSD/PSB의 추가 알파·별색 채널 편집은 아직 연결되지 않았습니다. 원본을 변환하지 않았습니다.".into());
    }
    r.section(false)?;
    let resource_section = r.section(false)?;
    if resource_section.bytes.len() > 64 * 1024 * 1024 {
        return Err("PSD 이미지 리소스가 너무 큽니다.".into());
    }
    let resources = Arc::new(resource_section.bytes.to_vec());
    let layout = read_layout(&resources)?;
    let mut outer = r.section(large)?;
    let mut records = Vec::new();
    let mut layers = Vec::new();
    let mut total = 0u64;
    if !outer.bytes.is_empty() {
        let mut info = layer_info(&mut outer, large, depth)?;
        if !info.bytes.is_empty() {
            let count = info.i16()?.unsigned_abs() as usize;
            if count > 10_000 {
                return Err("PSD 레이어가 너무 많습니다.".into());
            }
            for _ in 0..count {
                let bounds = [info.i32()?, info.i32()?, info.i32()?, info.i32()?];
                let count = info.u16()?;
                if count > 56 {
                    return Err("PSD 레이어 채널 수가 잘못되었습니다.".into());
                }
                let channels = (0..count)
                    .map(|_| Ok((info.i16()?, info.length(large)?)))
                    .collect::<Result<Vec<_>, String>>()?;
                if info.take(4)? != b"8BIM" {
                    return Err("PSD 블렌드 서명이 잘못되었습니다.".into());
                }
                let key = info.take(4)?;
                let mut pass_through = key == b"pass";
                let mut blend = if pass_through {
                    BlendMode::Normal
                } else {
                    blend_mode(key)?
                };
                let opacity = info.u8()? as f32 / 255.0;
                let clipping = info.u8()?;
                let flags = info.u8()?;
                info.u8()?;
                if clipping > 1 {
                    return Err("PSD 클리핑 플래그가 잘못되었습니다.".into());
                }
                let mut extra = info.section(false)?;
                let mut mask = extra.section(false)?;
                let mut mask_bounds = None;
                let mut mask_default = 255;
                let mut mask_enabled = true;
                if mask.bytes.len() >= 18 {
                    mask_bounds = Some([mask.i32()?, mask.i32()?, mask.i32()?, mask.i32()?]);
                    mask_default = mask.u8()?;
                    let mask_flags = mask.u8()?;
                    mask_enabled = mask_flags & 2 == 0;
                    if mask_flags & 1 != 0 {
                        return Err("PSD 상대 위치 마스크는 아직 연결되지 않았습니다.".into());
                    }
                }
                extra.section(false)?;
                let name_length = extra.u8()? as usize;
                let mut name = String::from_utf8_lossy(extra.take(name_length)?).into_owned();
                let padding = (4 - (name_length + 1) % 4) % 4;
                extra.take(padding)?;
                let mut section = 0;
                let mut additional = Vec::new();
                let mut source_id = None;
                while extra.bytes.len() - extra.at >= 12 {
                    let block_start = extra.at;
                    let signature = extra.take(4)?;
                    if signature != b"8BIM" && signature != b"8B64" {
                        return Err("PSD 추가 레코드 서명이 잘못되었습니다.".into());
                    }
                    let key = extra.take(4)?;
                    let extended = large
                        && [
                            b"LMsk".as_slice(),
                            b"Lr16",
                            b"Lr32",
                            b"Layr",
                            b"Mt16",
                            b"Mt32",
                            b"Mtrn",
                            b"Alph",
                            b"FMsk",
                            b"lnk2",
                            b"FEid",
                            b"FXid",
                            b"PxSD",
                        ]
                        .contains(&key);
                    let len = extra.length(extended)?;
                    let data = extra.take(len)?;
                    if key == b"luni" {
                        let mut text = Reader::new(data);
                        let count = text.u32()? as usize;
                        if count > 1_000_000 {
                            return Err("PSD 레이어 이름이 너무 깁니다.".into());
                        }
                        let chars = (0..count)
                            .map(|_| text.u16())
                            .collect::<Result<Vec<_>, _>>()?;
                        name = String::from_utf16_lossy(&chars);
                    } else if key == b"lyid" {
                        let id = Reader::new(data).u32()?;
                        source_id = (id != 0).then_some(u64::from(id));
                    } else if key == b"lsct" || key == b"lsdk" {
                        section = Reader::new(data).u32()?;
                        if section > 3 {
                            return Err("PSD 그룹 구분 값이 잘못되었습니다.".into());
                        }
                        if data.len() >= 12 {
                            if &data[4..8] != b"8BIM" {
                                return Err("PSD 그룹 블렌드 서명이 잘못되었습니다.".into());
                            }
                            if section == 1 || section == 2 {
                                pass_through = &data[8..12] == b"pass";
                                blend = if pass_through {
                                    BlendMode::Normal
                                } else {
                                    blend_mode(&data[8..12])?
                                };
                            }
                        }
                    } else if [
                        b"TySh".as_slice(),
                        b"SoLd",
                        b"SoLE",
                        b"vmsk",
                        b"vstk",
                        b"lfx2",
                        b"lrFX",
                        b"levl",
                        b"curv",
                        b"hue2",
                        b"hue ",
                        b"brit",
                        b"expA",
                        b"vibA",
                        b"blnc",
                        b"blwh",
                        b"phfl",
                        b"mixr",
                        b"clrL",
                        b"nvrt",
                        b"post",
                        b"thrs",
                        b"grdm",
                        b"selc",
                        b"SoCo",
                        b"GdFl",
                        b"PtFl",
                        b"PlLd",
                        b"plLd",
                    ]
                    .contains(&key)
                    {
                        return Err(format!(
                            "PSD 구조 '{}' 해석·보존은 아직 연결되지 않았습니다. 원본을 변환하지 않았습니다.",
                            String::from_utf8_lossy(key)
                        ));
                    }
                    if len % 2 != 0 && extra.at < extra.bytes.len() {
                        extra.take(1)?;
                    }
                    if ![b"luni".as_slice(), b"lsct", b"lsdk", b"lyid"].contains(&key) {
                        if additional.len() + extra.at - block_start > 64 * 1024 * 1024 {
                            return Err("PSD 레이어 추가 데이터가 너무 큽니다.".into());
                        }
                        additional.extend(&extra.bytes[block_start..extra.at]);
                    }
                }
                records.push(Record {
                    name,
                    bounds,
                    channels,
                    blend,
                    opacity,
                    visible: flags & 2 == 0,
                    locked: flags & 1 != 0,
                    mask_bounds,
                    mask_default,
                    mask_enabled,
                    clipped: clipping == 1,
                    section,
                    pass_through,
                    extra: additional,
                    source_id,
                });
            }
            // Pair each record with its own encoded channel slices before changing traversal.
            // External records run bottom-to-top; traverse top-to-bottom to build parent links.
            // Earlier development exports ran top-to-bottom. Their first folder divider identifies
            // the old group order; flat multi-layer files use the explicit legacy opening path.
            let mut encoded_records = Vec::with_capacity(records.len());
            for record in records {
                let encoded = record
                    .channels
                    .iter()
                    .map(|(_, len)| info.take(*len))
                    .collect::<Result<Vec<_>, _>>()?;
                encoded_records.push((record, encoded));
            }
            let legacy_groups = mode == 3
                && encoded_records
                    .iter()
                    .find_map(|(record, _)| (record.section != 0).then_some(record.section))
                    .is_some_and(|section| matches!(section, 1 | 2));
            if mode == 1 || !(force_legacy || legacy_groups) {
                encoded_records.reverse();
            }
            let mut parents = Vec::new();
            let mut next_fallback = encoded_records
                .iter()
                .filter_map(|(r, _)| r.source_id)
                .max()
                .unwrap_or(0)
                + 1;
            let mut used_ids = std::collections::HashSet::new();
            for (record, encoded_channels) in encoded_records {
                if record.section == 3 {
                    if parents.pop().is_none() {
                        return Err("PSD 그룹 경계가 잘못되었습니다.".into());
                    }
                    continue;
                }
                let group = record.section == 1 || record.section == 2;
                let [top, left, bottom, right] = record.bounds;
                let w = if group {
                    1
                } else {
                    i64::from(right) - i64::from(left)
                };
                let h = if group {
                    1
                } else {
                    i64::from(bottom) - i64::from(top)
                };
                if w <= 0 || h <= 0 || w > 300_000 || h > 300_000 {
                    return Err("PSD 레이어 영역이 잘못되었거나 비픽셀 레이어입니다.".into());
                }
                let (w, h) = (w as u32, h as u32);
                validate_size(w, h)?;
                total += u64::from(w) * u64::from(h);
                if total > MAX_PIXELS {
                    return Err("PSD 레이어가 메모리 예산을 초과합니다.".into());
                }
                let mut pixels = RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]));
                let mut high = (depth > 8 && !group).then(|| {
                    image::Rgba32FImage::from_pixel(w, h, image::Rgba([0.0, 0.0, 0.0, 1.0]))
                });
                let mut mask_image = None;
                if mode == 1
                    && !group
                    && (record.channels.iter().filter(|(id, _)| *id == 0).count() != 1
                        || record.channels.iter().filter(|(id, _)| *id == -1).count() > 1
                        || record.channels.iter().any(|(id, _)| *id > 0))
                {
                    return Err("PSD 회색조 레이어 채널이 잘못되었거나 추가 채널 편집이 아직 연결되지 않았습니다.".into());
                }
                for ((id, _), encoded) in record.channels.into_iter().zip(encoded_channels) {
                    if id == -2 {
                        if let Some([mt, ml, mb, mr]) = record.mask_bounds {
                            let mw = i64::from(mr) - i64::from(ml);
                            let mh = i64::from(mb) - i64::from(mt);
                            if mw <= 0 || mh <= 0 || mw > 300_000 || mh > 300_000 {
                                return Err("PSD 마스크 영역이 잘못되었습니다.".into());
                            }
                            let decoded =
                                channel_depth(encoded, mw as u32, mh as u32, large, depth)?;
                            if depth == 32
                                && decoded
                                    .as_chunks::<4>()
                                    .0
                                    .iter()
                                    .any(|value| !f32::from_be_bytes(*value).is_finite())
                            {
                                return Err(
                                    "PSD 마스크에 유효하지 않은 부동소수점 값이 있습니다.".into()
                                );
                            }
                            let (w, h) = if group { (width, height) } else { (w, h) };
                            mask_image = Some(Arc::new(GrayImage::from_fn(w, h, |x, y| {
                                let dx = i64::from(x) + if group { 0 } else { i64::from(left) }
                                    - i64::from(ml);
                                let dy = i64::from(y) + if group { 0 } else { i64::from(top) }
                                    - i64::from(mt);
                                image::Luma([if dx >= 0 && dy >= 0 && dx < mw && dy < mh {
                                    (sample(&decoded, (dy * mw + dx) as usize, depth)
                                        .clamp(0.0, 1.0)
                                        * 255.0)
                                        .round() as u8
                                } else {
                                    record.mask_default
                                }])
                            })));
                        }
                    } else if (-1..color_channels as i16).contains(&id) && !group {
                        let data = channel_depth(encoded, w, h, large, depth)?;
                        let c = if id == -1 { 3 } else { id as usize };
                        for (index, pixel) in pixels.pixels_mut().enumerate() {
                            let value = sample(&data, index, depth);
                            if !value.is_finite() {
                                return Err(
                                    "PSD 픽셀에 유효하지 않은 부동소수점 값이 있습니다.".into()
                                );
                            }
                            let components = if mode == 1 && id == 0 { 0..3 } else { c..c + 1 };
                            for c in components {
                                pixel[c] = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
                                if let Some(high) = &mut high {
                                    high.as_mut()[index * 4 + c] = value;
                                }
                            }
                        }
                    }
                }
                let mut layer = Document::from_image(record.name, pixels).layers.remove(0);
                layer.precision = high.map(Arc::new);
                // A folder has only an 8-bit placeholder, not a high-precision pixel asset.
                layer.bit_depth = if group { 8 } else { depth as u8 };
                layer.id = record.source_id.unwrap_or_else(|| {
                    let id = next_fallback;
                    next_fallback += 1;
                    id
                });
                if !used_ids.insert(layer.id) {
                    return Err("PSD 레이어 ID가 중복되었습니다.".into());
                }
                layer.offset = if group { [0, 0] } else { [left, top] };
                layer.opacity = record.opacity;
                layer.visible = record.visible;
                layer.locked = record.locked;
                layer.blend = record.blend;
                layer.mask = mask_image;
                layer.mask_enabled = record.mask_enabled;
                layer.clipped = record.clipped;
                layer.parent = parents.last().copied();
                layer.group = group;
                layer.pass_through = record.pass_through;
                layer.psd_extra = Arc::new(record.extra);
                if group {
                    if parents.len() >= 63 {
                        return Err("PSD 그룹 중첩이 너무 깊습니다.".into());
                    }
                    parents.push(layer.id);
                }
                layers.push(layer);
            }
            if !parents.is_empty() {
                return Err("PSD 그룹이 닫히지 않았습니다.".into());
            }
        }
    }
    if layers.is_empty() {
        let compression = r.u16()?;
        let mut planes = Vec::new();
        if compression == 0 {
            for _ in 0..channels {
                planes.push(
                    r.take(width as usize * height as usize * depth as usize / 8)?
                        .to_vec(),
                );
            }
        } else if compression == 1 {
            let sizes = (0..u32::from(channels) * height)
                .map(|_| {
                    if large {
                        r.u32().map(|v| v as usize)
                    } else {
                        r.u16().map(|v| v as usize)
                    }
                })
                .collect::<Result<Vec<_>, _>>()?;
            for c in 0..channels as usize {
                let mut bytes = Vec::new();
                word(&mut bytes, 1);
                for size in &sizes[c * height as usize..(c + 1) * height as usize] {
                    if large {
                        int(&mut bytes, *size as u32);
                    } else {
                        word(&mut bytes, *size as u16);
                    }
                }
                for size in &sizes[c * height as usize..(c + 1) * height as usize] {
                    bytes.extend(r.take(*size)?);
                }
                planes.push(channel_depth(&bytes, width, height, large, depth)?);
            }
        } else if compression == 2 || compression == 3 {
            let count =
                u64::from(width) * u64::from(height) * u64::from(depth / 8) * u64::from(channels);
            if count > MAX_PIXELS * 32 {
                return Err("PSD 합성 데이터가 현재 메모리 예산을 초과합니다.".into());
            }
            let mut decoded = Vec::new();
            flate2::read::ZlibDecoder::new(r.take(r.bytes.len() - r.at)?)
                .take(count + 1)
                .read_to_end(&mut decoded)
                .map_err(|e| e.to_string())?;
            if decoded.len() as u64 != count {
                return Err("PSD 합성 ZIP 데이터 길이가 잘못되었습니다.".into());
            }
            if compression == 3 {
                prediction(&mut decoded, width as usize, depth);
            }
            let count = width as usize * height as usize * depth as usize / 8;
            planes.extend(decoded.chunks_exact(count).map(|plane| plane.to_vec()));
        } else {
            return Err("지원하지 않는 PSD 합성 압축입니다.".into());
        }
        let mut image = image::Rgba32FImage::from_fn(width, height, |x, y| {
            let i = y as usize * width as usize + x as usize;
            image::Rgba([
                sample(&planes[0], i, depth),
                sample(&planes[if mode == 1 { 0 } else { 1 }], i, depth),
                sample(&planes[if mode == 1 { 0 } else { 2 }], i, depth),
                if channels > color_channels {
                    sample(&planes[color_channels as usize], i, depth)
                } else {
                    1.0
                },
            ])
        });
        if image.as_raw().iter().any(|p| !p.is_finite()) {
            return Err("PSD 픽셀에 유효하지 않은 부동소수점 값이 있습니다.".into());
        }
        if mode == 3 && channels > color_channels && !force_legacy {
            // The RGB merged preview is matted on white. Native layer planes are straight RGB.
            for pixel in image.pixels_mut() {
                let alpha = pixel[3];
                if !(0.0..=1.0).contains(&alpha) {
                    return Err("PSD 합성 투명도 값이 잘못되었습니다.".into());
                }
                if alpha == 0.0 {
                    pixel.0[..3].fill(0.0);
                } else if alpha < 1.0 {
                    for color in &mut pixel.0[..3] {
                        *color = ((f64::from(*color) - (1.0 - f64::from(alpha))) / f64::from(alpha))
                            as f32;
                        if !color.is_finite() {
                            return Err("PSD 합성 색을 표현할 수 없습니다.".into());
                        }
                    }
                }
            }
        }
        let mut document = crate::precision::document(
            "배경".into(),
            match depth {
                32 => image::DynamicImage::ImageRgba32F(image),
                16 => image::DynamicImage::ImageRgba16(crate::precision::image16(&image)),
                _ => image::DynamicImage::ImageRgba8(crate::precision::preview(&image)),
            },
        )?;
        document.psd_resources = resources;
        document.layout = layout;
        return Ok(document);
    }
    layers.reverse();
    let active = layers.last().unwrap().id;
    let next_id = layers.iter().map(|l| l.id).max().unwrap() + 1;
    Ok(Document {
        document_id: crate::document_id::new(),
        width,
        height,
        layers,
        active,
        next_id,
        psd_resources: resources,
        source_icc: None,
        source_metadata: Default::default(),
        layout,
    })
}

fn layer_record(
    layer: &Layer,
    large: bool,
    section: u32,
    lengths: &[u64],
    color: OutputColor,
) -> Result<Vec<u8>, String> {
    let mut output = Vec::new();
    let (w, h) = if section > 0 {
        (0, 0)
    } else {
        layer.pixels.dimensions()
    };
    let offset = if section > 0 { [0, 0] } else { layer.offset };
    for value in [
        offset[1],
        offset[0],
        offset[1].checked_add(h as i32).ok_or("레이어 위치 초과")?,
        offset[0].checked_add(w as i32).ok_or("레이어 위치 초과")?,
    ] {
        int(&mut output, value as u32);
    }
    let channels: Vec<i16> = if section == 3 {
        vec![]
    } else {
        color
            .channels()
            .iter()
            .map(|c| if *c == 3 { -1 } else { *c as i16 })
            .filter(|_| section == 0)
            .chain(layer.mask.as_ref().map(|_| -2))
            .collect()
    };
    word(&mut output, channels.len() as u16);
    for (id, size) in channels.into_iter().zip(lengths) {
        word(&mut output, id as u16);
        length(&mut output, *size, large)?;
    }
    output.extend(b"8BIM");
    output.extend(if section > 0 && layer.pass_through {
        *b"pass"
    } else {
        blend_key(layer.blend)
    });
    output.push((layer.opacity * 255.0).round() as u8);
    output.push(u8::from(layer.clipped));
    output.push(u8::from(layer.locked) | if layer.visible { 0 } else { 2 });
    output.push(0);
    let mut extra = Vec::new();
    if section != 3
        && let Some(mask) = &layer.mask
    {
        int(&mut extra, 20);
        for value in [
            layer.offset[1],
            layer.offset[0],
            layer.offset[1] + mask.height() as i32,
            layer.offset[0] + mask.width() as i32,
        ] {
            int(&mut extra, value as u32);
        }
        extra.extend([255, if layer.mask_enabled { 0 } else { 2 }, 0, 0]);
    } else {
        int(&mut extra, 0);
    }
    int(&mut extra, 0);
    extra.extend([0; 4]);
    let mut name = Vec::new();
    let text: Vec<u16> = layer.name.encode_utf16().collect();
    int(&mut name, text.len() as u32);
    for c in text {
        word(&mut name, c);
    }
    extra.extend(b"8BIMluni");
    int(&mut extra, name.len() as u32);
    extra.extend(name);
    if extra.len() % 2 != 0 {
        extra.push(0);
    }
    if section > 0 {
        extra.extend(b"8BIMlsct");
        int(&mut extra, 12);
        int(&mut extra, section);
        extra.extend(b"8BIM");
        extra.extend(if layer.pass_through {
            *b"pass"
        } else {
            blend_key(layer.blend)
        });
    }
    if section != 3 {
        extra.extend(b"8BIMlyid");
        int(&mut extra, 4);
        int(
            &mut extra,
            u32::try_from(layer.id).map_err(|_| "PSD 레이어 ID 범위를 초과합니다.")?,
        );
        extra.extend(layer.psd_extra.iter());
    }
    int(&mut output, extra.len() as u32);
    output.extend(extra);
    Ok(output)
}

pub(crate) fn write_plane(
    file: &mut impl Write,
    values: impl Iterator<Item = f32>,
    depth: u16,
) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(65536);
    for value in values {
        if !value.is_finite() {
            return Err("PSD 픽셀에 유효하지 않은 부동소수점 값이 있습니다.".into());
        }
        match depth {
            32 => bytes.extend(value.to_be_bytes()),
            16 => bytes.extend(((value.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes()),
            _ => bytes.push((value.clamp(0.0, 1.0) * 255.0).round() as u8),
        }
        if bytes.len() >= 65536 {
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            bytes.clear();
        }
    }
    file.write_all(&bytes).map_err(|e| e.to_string())
}

pub fn write(document: &Document, file: &mut std::fs::File, large: bool) -> Result<(), String> {
    write_compressed(
        document,
        file,
        large,
        crate::psd_compression::Compression::Raw,
    )
}
fn adjusted_pixels(layer: &Layer) -> Option<image::Rgba32FImage> {
    (layer.adjustments != Default::default()).then(|| {
        let original = layer.precision.as_ref().map_or_else(
            || image::DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f(),
            |p| (**p).clone(),
        );
        layer.adjustments.apply_float(&original)
    })
}
fn merged_sample(pixel: &image::Rgba<f32>, channel: usize, depth: u16) -> f32 {
    if channel == 3 {
        return pixel[3];
    }
    let alpha = pixel[3].clamp(0.0, 1.0);
    let color = if depth == 32 {
        pixel[channel]
    } else {
        pixel[channel].clamp(0.0, 1.0)
    };
    if alpha == 1.0 {
        return color;
    }
    // Calculate before the final f32 conversion to avoid an extra cancellation/rounding step.
    (f64::from(color) * f64::from(alpha) + (1.0 - f64::from(alpha))) as f32
}
pub fn write_compressed(
    document: &Document,
    file: &mut std::fs::File,
    large: bool,
    compression: crate::psd_compression::Compression,
) -> Result<(), String> {
    write_options(
        document,
        file,
        large,
        OutputOptions {
            compression,
            ..Default::default()
        },
    )
}
pub(crate) fn write_options(
    document: &Document,
    file: &mut std::fs::File,
    large: bool,
    options: OutputOptions,
) -> Result<(), String> {
    let compression = options.compression;
    let color = options.color;
    if color == OutputColor::Grayscale
        && (!neutral(document)
            || document.layers.iter().any(|l| {
                l.adjustments != Default::default()
                    || l.effects.enabled()
                    || l.transform != Default::default()
                    || l.mask_placement.is_some()
            }))
    {
        return Err("회색조 출력에는 변환 안내가 있는 Photoshop 저장 경로를 사용하세요.".into());
    }
    if document.layers.iter().any(|l| l.adjustment_layer) {
        return Err("조정 레이어는 변환 안내가 있는 Photoshop 저장 경로를 사용하세요.".into());
    }

    let maximum = if large { 300_000 } else { 30_000 };
    if document.width > maximum || document.height > maximum {
        return Err("문서가 형식의 최대 크기를 초과합니다. PSB를 선택하세요.".into());
    }
    if document.layers.len() > i16::MAX as usize {
        return Err("레이어가 너무 많습니다.".into());
    }
    fn collect<'a>(
        document: &'a Document,
        parent: Option<u64>,
        layers: &mut Vec<(&'a Layer, u32)>,
    ) {
        for layer in document.layers.iter().filter(|l| l.parent == parent) {
            if layer.group {
                layers.push((layer, 3));
                collect(document, Some(layer.id), layers);
                layers.push((layer, 1));
            } else {
                layers.push((layer, 0));
            }
        }
    }
    if !document.hierarchy_valid() {
        return Err("레이어 그룹 참조가 잘못되었습니다.".into());
    }
    let mut layers = Vec::new();
    collect(document, None, &mut layers);
    if layers.len() > i16::MAX as usize {
        return Err("PSD 그룹과 레이어가 너무 많습니다.".into());
    }
    let depth = document
        .layers
        .iter()
        .map(|l| l.bit_depth)
        .max()
        .unwrap_or(8) as u16;
    if ![8, 16, 32].contains(&depth) {
        return Err("지원하지 않는 PSD 정밀도입니다.".into());
    }
    let mut channel_lengths = Vec::with_capacity(layers.len());
    for (layer, section) in &layers {
        let mut lengths = vec![];
        if *section != 3 {
            let adjusted = if compression == crate::psd_compression::Compression::Raw {
                None
            } else {
                adjusted_pixels(layer)
            };
            let high = adjusted.as_ref().or(layer.precision.as_deref());
            for &c in color.channels().iter().filter(|_| *section == 0) {
                let size = [layer.pixels.width(), layer.pixels.height()];
                lengths.push(if let Some(image) = high {
                    crate::psd_compression::size(
                        image.pixels().map(|p| p[c]),
                        size,
                        depth,
                        large,
                        compression,
                    )?
                } else {
                    crate::psd_compression::size(
                        layer.pixels.pixels().map(|p| p[c] as f32 / 255.),
                        size,
                        depth,
                        large,
                        compression,
                    )?
                });
            }
            if let Some(mask) = &layer.mask {
                lengths.push(crate::psd_compression::size(
                    mask.as_raw().iter().map(|v| *v as f32 / 255.),
                    [mask.width(), mask.height()],
                    depth,
                    large,
                    compression,
                )?);
            }
        }
        channel_lengths.push(lengths);
    }
    let records = layers
        .iter()
        .zip(&channel_lengths)
        .map(|((layer, section), sizes)| layer_record(layer, large, *section, sizes, color))
        .collect::<Result<Vec<_>, _>>()?;
    let pixels_len: u64 = channel_lengths.iter().flatten().sum();
    let info_len = 2 + records.iter().map(|r| r.len() as u64).sum::<u64>() + pixels_len;
    let alignment = if depth > 8 { 4 } else { 2 };
    let padded = info_len.next_multiple_of(alignment);
    let length_size = if large { 8 } else { 4 };
    let outer_len = length_size + padded + 4 + if depth > 8 { 8 + length_size } else { 0 };
    let mut header = Vec::new();
    header.extend(b"8BPS");
    word(&mut header, if large { 2 } else { 1 });
    header.extend([0; 6]);
    word(&mut header, color.channels().len() as u16);
    int(&mut header, document.height);
    int(&mut header, document.width);
    word(&mut header, depth);
    word(&mut header, if color == OutputColor::Rgb { 3 } else { 1 });
    int(&mut header, 0);
    let resources = write_resources(document, color)?;
    int(
        &mut header,
        u32::try_from(resources.len()).map_err(|_| "PSD 메타데이터가 너무 큽니다.")?,
    );
    header.extend(resources);
    length(&mut header, outer_len, large)?;
    if depth > 8 {
        length(&mut header, 0, large)?;
        int(&mut header, 0); // Global mask.
        header.extend(if depth == 16 {
            b"8BIMLr16"
        } else {
            b"8BIMLr32"
        });
        length(&mut header, padded, large)?;
    } else {
        length(&mut header, padded, large)?;
    }
    word(&mut header, (-(layers.len() as i16)) as u16);
    file.write_all(&header).map_err(|e| e.to_string())?;
    for record in records {
        file.write_all(&record).map_err(|e| e.to_string())?;
    }
    for (layer, section) in layers {
        if section == 3 {
            continue;
        }
        let adjusted = adjusted_pixels(layer);
        let high = adjusted.as_ref().or(layer.precision.as_deref());
        for &c in color.channels().iter().filter(|_| section == 0) {
            let size = [layer.pixels.width(), layer.pixels.height()];
            if let Some(image) = high {
                crate::psd_compression::channel(
                    file,
                    image.pixels().map(|p| p[c]),
                    size,
                    depth,
                    large,
                    compression,
                )?;
            } else {
                crate::psd_compression::channel(
                    file,
                    layer.pixels.pixels().map(|p| p[c] as f32 / 255.),
                    size,
                    depth,
                    large,
                    compression,
                )?;
            }
        }
        if let Some(mask) = &layer.mask {
            crate::psd_compression::channel(
                file,
                mask.as_raw().iter().map(|v| *v as f32 / 255.),
                [mask.width(), mask.height()],
                depth,
                large,
                compression,
            )?;
        }
    }
    file.write_all(&[0; 3][..(padded - info_len) as usize])
        .map_err(|e| e.to_string())?;
    if depth == 8 {
        file.write_all(&[0; 4]).map_err(|e| e.to_string())?;
    }
    let composite = crate::precision::composite(document);
    let values = color.channels().iter().flat_map(|&c| {
        composite.pixels().map(move |p| {
            if color == OutputColor::Rgb {
                merged_sample(p, c, depth)
            } else {
                p[c]
            }
        })
    });
    crate::psd_compression::channel(
        file,
        values,
        [
            document.width,
            document.height * color.channels().len() as u32,
        ],
        depth,
        large,
        compression,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compressed_native_layers_masks_and_merged_preview_roundtrip_at_all_depths() {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/psd-compression");
        std::fs::create_dir_all(&dir).unwrap();
        for depth in [8, 16, 32] {
            let image = match depth {
                16 => {
                    image::DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(4, 2, |x, _| {
                        image::Rgba([32896 + x as u16, 513, 65530, 50000])
                    }))
                }
                32 => {
                    image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_fn(4, 2, |x, _| {
                        image::Rgba([2.125 + x as f32, -0.25, 0.3456789, 0.75])
                    }))
                }
                _ => image::DynamicImage::ImageRgba8(RgbaImage::from_fn(4, 2, |x, _| {
                    image::Rgba([20 + x as u8 * 40, 193, 75, 192])
                })),
            };
            let mut doc = crate::precision::document("compression".into(), image).unwrap();
            doc.layers[0].offset = [-1, 0];
            doc.layers[0].mask = Some(Arc::new(GrayImage::from_pixel(4, 2, image::Luma([192]))));
            let source_id = doc.active;
            doc.group_active().unwrap();
            doc.active_layer_mut().unwrap().mask =
                Some(Arc::new(GrayImage::from_pixel(4, 2, image::Luma([128]))));
            let expected = crate::precision::composite(&doc);
            for large in [false, true] {
                for (index, compression) in crate::psd_compression::Compression::ALL
                    .into_iter()
                    .enumerate()
                {
                    let extension = if large { "psb" } else { "psd" };
                    let path = dir.join(format!("depth-{depth}-mode-{index}.{extension}"));
                    crate::files::save_photoshop_compressed(&doc, &path, compression).unwrap();
                    let bytes = std::fs::read(&path).unwrap();
                    let restored = read(&bytes).unwrap();
                    let source = restored.layers.iter().find(|l| l.id == source_id).unwrap();
                    let original = doc.layers.iter().find(|l| l.id == source_id).unwrap();
                    assert_eq!(source.pixels, original.pixels);
                    assert_eq!(source.precision, original.precision);
                    assert_eq!(source.mask, original.mask);
                    assert_eq!(crate::precision::composite(&restored), expected);
                    let mut r = Reader::new(&bytes);
                    r.take(26).unwrap();
                    r.section(false).unwrap();
                    r.section(false).unwrap();
                    let start = r.at;
                    r.section(large).unwrap();
                    let end = r.at;
                    assert_eq!(
                        u16::from_be_bytes(bytes[end..end + 2].try_into().unwrap()),
                        index as u16
                    );
                    let mut flattened = bytes[..start].to_vec();
                    flattened.extend(if large { vec![0; 8] } else { vec![0; 4] });
                    flattened.extend(&bytes[end..]);
                    let flat = read(&flattened).unwrap();
                    let image = crate::precision::composite(&flat);
                    for (a, b) in image.pixels().zip(expected.pixels()) {
                        for c in 0..4 {
                            assert!(
                                (a[c] - b[c]).abs()
                                    <= if depth == 32 {
                                        1e-6
                                    } else {
                                        let quantum = 1.0 / if depth == 8 { 255. } else { 65535. };
                                        if c == 3 {
                                            quantum + 1e-6
                                        } else if a[3] == 0.0 {
                                            b[c].abs() + 1e-6
                                        } else {
                                            quantum * (1.0 + b[c].abs()) / a[3] + 1e-6
                                        }
                                    }
                            );
                        }
                    }
                    let expected_path = dir.join(format!("depth-{depth}-merged.json"));
                    std::fs::write(
                        expected_path,
                        serde_json::to_vec(expected.as_raw()).unwrap(),
                    )
                    .unwrap();
                }
            }
        }
    }
    #[test]
    fn high_bit_raw_rle_zip_channels_and_failed_float_save_keep_existing_file() {
        use flate2::{Compression, write::ZlibEncoder};
        for depth in [16, 32] {
            let raw = if depth == 16 {
                [0u16, 65535, 32896, 1024]
                    .into_iter()
                    .flat_map(u16::to_be_bytes)
                    .collect::<Vec<_>>()
            } else {
                [2.125f32, -0.25, 0.3456789, 1.0]
                    .into_iter()
                    .flat_map(f32::to_be_bytes)
                    .collect::<Vec<_>>()
            };
            for large in [false, true] {
                let mut encoded = vec![0, 0];
                encoded.extend(&raw);
                assert_eq!(channel_depth(&encoded, 2, 2, large, depth).unwrap(), raw);
                let row_len = raw.len() / 2;
                let mut encoded = vec![0, 1];
                for _ in 0..2 {
                    if large {
                        int(&mut encoded, (row_len + 1) as u32);
                    } else {
                        word(&mut encoded, (row_len + 1) as u16);
                    }
                }
                for row in raw.chunks_exact(row_len) {
                    encoded.push((row.len() - 1) as u8);
                    encoded.extend(row);
                }
                assert_eq!(channel_depth(&encoded, 2, 2, large, depth).unwrap(), raw);
                let mut zip = ZlibEncoder::new(Vec::new(), Compression::default());
                zip.write_all(&raw).unwrap();
                let mut encoded = vec![0, 2];
                encoded.extend(zip.finish().unwrap());
                assert_eq!(channel_depth(&encoded, 2, 2, large, depth).unwrap(), raw);
                assert!(channel_depth(&encoded, 3, 2, large, depth).is_err());
            }
        }
        let path = std::path::Path::new("qa-output/high-psd/preserve-invalid.psd");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"existing document").unwrap();
        let mut doc = crate::precision::document(
            "실패".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
                1,
                1,
                image::Rgba([2.125, 0.3, 0.4, 1.0]),
            )),
        )
        .unwrap();
        Arc::make_mut(doc.layers[0].precision.as_mut().unwrap()).get_pixel_mut(0, 0)[0] = f32::NAN;
        assert!(crate::files::save_document(&doc, path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"existing document");
    }
    #[test]
    fn independent_high_bit_zip_prediction_fixtures_decode_native_pixels_and_float_hdr() {
        let doc = read(include_bytes!(
            "../tests/fixtures/independent-layered16.psd"
        ))
        .unwrap();
        assert_eq!(
            (
                doc.width,
                doc.height,
                doc.layers[0].offset,
                doc.layers[0].bit_depth
            ),
            (5, 3, [-1, 1], 16)
        );
        let values = [0u16, 65535, 32896, 1024, 257, 50000, 65530, 4];
        let actual = crate::precision::image16(doc.layers[0].precision.as_ref().unwrap());
        for (index, pixel) in actual.pixels().enumerate() {
            for channel in 0..4 {
                assert_eq!(pixel[channel], values[(index + channel) % 8]);
            }
        }
        for (bytes, legacy) in [
            (
                include_bytes!("../tests/fixtures/independent-layered32.psb").as_slice(),
                false,
            ),
            // This earlier independently authored flat fixture stores straight merged RGB.
            (
                include_bytes!("../tests/fixtures/independent-flat32.psd").as_slice(),
                true,
            ),
        ] {
            let doc = if legacy {
                read_legacy(bytes)
            } else {
                read(bytes)
            }
            .unwrap();
            assert_eq!(doc.layers[0].bit_depth, 32);
            for (index, pixel) in doc.layers[0]
                .precision
                .as_ref()
                .unwrap()
                .pixels()
                .enumerate()
            {
                assert_eq!(
                    pixel.0,
                    [
                        2.125 + index as f32 / 8.0,
                        -0.25 + index as f32 / 16.0,
                        0.3456789,
                        (index + 1) as f32 / 8.0
                    ]
                );
            }
        }
    }
    #[test]
    fn high_bit_layered_psd_psb_and_merged_preview_preserve_precision_groups_and_masks() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/high-psd");
        std::fs::create_dir_all(&dir).unwrap();
        for depth in [16, 32] {
            let image = if depth == 16 {
                image::DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(4, 2, |x, _| {
                    image::Rgba([32896 + x as u16, 513, 65530, 50000])
                }))
            } else {
                image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_fn(4, 2, |x, _| {
                    image::Rgba([2.125 + x as f32, -0.25, 0.3456789, 0.75])
                }))
            };
            let mut doc = crate::precision::document("고정밀 원본".into(), image).unwrap();
            crate::editing::resize(&mut doc, 5, 3, false).unwrap();
            let source_id = doc.active;
            doc.layers[0].offset = [-1, 1];
            doc.layers[0].mask = Some(Arc::new(GrayImage::from_pixel(4, 2, image::Luma([192]))));
            doc.add_layer(
                "클리핑".into(),
                RgbaImage::from_pixel(1, 2, image::Rgba([0, 128, 255, 128])),
            )
            .unwrap();
            doc.active_layer_mut().unwrap().offset = [1, 1];
            doc.active_layer_mut().unwrap().clipped = true;
            doc.group_selected(&[source_id, doc.active]).unwrap();
            doc.active_layer_mut().unwrap().mask =
                Some(Arc::new(GrayImage::from_pixel(5, 3, image::Luma([128]))));
            let before = composite_for_test(&doc);
            for extension in ["psd", "psb"] {
                let path = dir.join(format!("native-{depth}.{extension}"));
                crate::files::save_document(&doc, &path).unwrap();
                let bytes = std::fs::read(&path).unwrap();
                assert_eq!(u16::from_be_bytes(bytes[22..24].try_into().unwrap()), depth);
                assert!(
                    bytes
                        .windows(4)
                        .any(|p| p == if depth == 16 { b"Lr16" } else { b"Lr32" })
                );
                let restored = read(&bytes).unwrap();
                assert!(restored.hierarchy_valid());
                assert!(restored.layers.iter().any(|l| l.group));
                assert!(restored.layers.iter().any(|l| l.clipped));
                let original = doc.layers.iter().find(|l| l.id == source_id).unwrap();
                let source = restored.layers.iter().find(|l| l.id == source_id).unwrap();
                assert_eq!(source.precision, original.precision);
                assert_eq!(source.mask, original.mask);
                assert_eq!(composite_for_test(&restored), before);
                // Remove only the layer/mask section to decode the writer's merged preview independently.
                let mut reader = Reader::new(&bytes);
                reader.take(26).unwrap();
                reader.section(false).unwrap();
                reader.section(false).unwrap();
                let outer_at = reader.at;
                reader.section(extension == "psb").unwrap();
                let mut flat = bytes[..outer_at].to_vec();
                length(&mut flat, 0, extension == "psb").unwrap();
                flat.extend(&bytes[reader.at..]);
                let preview = read(&flat).unwrap();
                for (a, b) in composite_for_test(&preview).pixels().zip(before.pixels()) {
                    assert!(
                        (a[3] - b[3]).abs()
                            <= if depth == 32 {
                                1e-6
                            } else {
                                1.0 / 65535.0 + 1e-6
                            }
                    );
                    for c in 0..3 {
                        assert!(
                            (a[c] * a[3] - b[c] * b[3]).abs()
                                <= if depth == 32 {
                                    2e-6
                                } else {
                                    3.0 / 65535.0 + 1e-6
                                }
                        );
                    }
                }
            }
        }
    }
    fn composite_for_test(doc: &Document) -> image::Rgba32FImage {
        crate::precision::composite(doc)
    }
    #[test]
    fn photoshop_resources_and_unknown_records_survive_native_and_project_roundtrips() {
        let mut doc = Document::new(3, 2, false).unwrap();
        let profile = include_bytes!("../tests/fixtures/srgb.icc");
        let mut resources = Vec::new();
        resources.extend(b"8BIM");
        word(&mut resources, 1041);
        word(&mut resources, 0);
        int(&mut resources, profile.len() as u32);
        resources.extend(profile);
        if resources.len() % 2 != 0 {
            resources.push(0);
        }
        doc.psd_resources = Arc::new(resources);
        let mut extra = b"8BIMtstR".to_vec();
        int(&mut extra, 4);
        extra.extend([12, 34, 56, 78]);
        doc.layers[0].psd_extra = Arc::new(extra);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/metadata");
        std::fs::create_dir_all(&dir).unwrap();
        for ext in ["psd", "psb", "rphoto"] {
            let path = dir.join(format!("preserved.{ext}"));
            if ext == "rphoto" {
                crate::files::save_project(&doc, &path).unwrap();
            } else {
                crate::files::save_photoshop(&doc, &path).unwrap();
            }
            let restored = crate::files::open(&path).unwrap().document;
            assert_eq!(restored.psd_resources, doc.psd_resources);
            assert_eq!(restored.layers[0].psd_extra, doc.layers[0].psd_extra);
            assert_eq!(restored.layers[0].id, doc.layers[0].id);
            assert!(
                crate::files::save_document(&restored, &dir.join("requires-report.psd")).is_err()
            );
        }
    }
    #[test]
    fn nested_groups_masks_and_clipping_roundtrip_in_psd_psb_and_project() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/groups");
        std::fs::create_dir_all(&dir).unwrap();
        let mut doc = Document::new(6, 4, false).unwrap();
        doc.add_layer(
            "실제 픽셀".into(),
            RgbaImage::from_pixel(3, 2, image::Rgba([230, 20, 10, 128])),
        )
        .unwrap();
        doc.group_active().unwrap();
        doc.active_layer_mut().unwrap().name = "안쪽 그룹".into();
        doc.active_layer_mut().unwrap().pass_through = false;
        doc.add_layer(
            "파란 클리핑".into(),
            RgbaImage::from_pixel(3, 2, image::Rgba([0, 40, 220, 255])),
        )
        .unwrap();
        doc.active_layer_mut().unwrap().clipped = true;
        let parent = doc.active_layer().unwrap().parent.unwrap();
        doc.active = parent;
        doc.group_active().unwrap();
        let group = doc.active_layer_mut().unwrap();
        group.name = "바깥 그룹".into();
        group.opacity = 128.0 / 255.0;
        group.mask = Some(Arc::new(GrayImage::from_pixel(6, 4, image::Luma([200]))));
        for ext in ["psd", "psb", "rphoto"] {
            let path = dir.join(format!("nested.{ext}"));
            if ext == "rphoto" {
                crate::files::save_project(&doc, &path).unwrap();
            } else {
                crate::files::save_document(&doc, &path).unwrap();
            }
            let restored = crate::files::open(&path).unwrap().document;
            assert!(restored.hierarchy_valid());
            assert_eq!(restored.layers.iter().filter(|l| l.group).count(), 2);
            assert_eq!(restored.layers.iter().filter(|l| l.clipped).count(), 1);
            assert_eq!(restored.composite(), doc.composite(), "{ext}");
        }
        doc.active_layer_mut().unwrap().effects.stroke.enabled = true;
        crate::files::save_project(&doc, &dir.join("with-effect.rphoto")).unwrap();
        let restored = crate::files::load_project(&dir.join("with-effect.rphoto")).unwrap();
        assert_eq!(restored.composite(), doc.composite());
        assert!(crate::files::save_document(&doc, &dir.join("needs-conversion.psd")).is_err());
        crate::files::save_photoshop(&doc, &dir.join("converted-effect.psd")).unwrap();
        let restored = crate::files::open(&dir.join("converted-effect.psd"))
            .unwrap()
            .document;
        assert_eq!(restored.composite(), doc.composite());
    }
    #[test]
    fn flattened_rle_uses_psd_and_psb_row_length_widths() {
        for large in [false, true] {
            let mut bytes = Vec::new();
            bytes.extend(b"8BPS");
            word(&mut bytes, if large { 2 } else { 1 });
            bytes.extend([0; 6]);
            word(&mut bytes, 3);
            int(&mut bytes, 1);
            int(&mut bytes, 2);
            word(&mut bytes, 8);
            word(&mut bytes, 3);
            int(&mut bytes, 0);
            int(&mut bytes, 0);
            length(&mut bytes, 0, large).unwrap();
            word(&mut bytes, 1);
            for _ in 0..3 {
                if large {
                    int(&mut bytes, 3);
                } else {
                    word(&mut bytes, 3);
                }
            }
            bytes.extend([1, 10, 20, 1, 30, 40, 1, 50, 60]);
            let doc = read(&bytes).unwrap();
            assert_eq!(doc.composite().get_pixel(1, 0).0, [20, 40, 60, 255]);
        }
    }
    #[test]
    fn layered_psd_and_psb_roundtrip_unicode_mask_blend_and_offsets() {
        let directory =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("qa-output/photoshop");
        std::fs::create_dir_all(&directory).unwrap();
        for large in [false, true] {
            let mut doc = Document::new(if large { 30001 } else { 7 }, 5, true).unwrap();
            doc.add_layer(
                "한글 레이어 🌱".into(),
                RgbaImage::from_pixel(4, 3, image::Rgba([220, 30, 80, 128])),
            )
            .unwrap();
            let layer = doc.active_layer_mut().unwrap();
            layer.offset = [-1, 1];
            layer.opacity = 128.0 / 255.0;
            layer.locked = true;
            layer.blend = BlendMode::Multiply;
            layer.mask = Some(Arc::new(GrayImage::from_fn(4, 3, |x, _| {
                image::Luma([if x < 2 { 255 } else { 100 }])
            })));
            let path = directory.join(if large { "layers.psb" } else { "layers.psd" });
            crate::files::save_photoshop(&doc, &path).unwrap();
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(&bytes[4..6], &if large { 2u16 } else { 1u16 }.to_be_bytes());
            let restored = read(&bytes).unwrap();
            assert_eq!(restored.layers.len(), 2);
            assert_eq!(restored.layers[1].name, "한글 레이어 🌱");
            assert_eq!(restored.layers[1].offset, [-1, 1]);
            assert!(restored.layers[1].locked);
            assert_eq!(restored.layers[1].blend, BlendMode::Multiply);
            assert_eq!(restored.layers[1].mask, doc.layers[1].mask);
            assert_eq!(restored.composite(), doc.composite());
            assert!(read(&bytes[..bytes.len() / 3]).is_err());
        }
    }
    #[test]
    fn rle_zip_and_prediction_reject_overruns_and_decode_rows() {
        let rle = [0, 1, 0, 5, 0, 2, 3, 1, 2, 3, 4, 253, 5];
        assert_eq!(
            channel(&rle, 4, 2, false).unwrap(),
            [1, 2, 3, 4, 5, 5, 5, 5]
        );
        assert!(channel(&[0, 1, 0, 2, 252, 5], 4, 1, false).is_err());
        let original = [5u8, 10, 20, 30, 80, 120, 180, 240];
        for prediction in [false, true] {
            let mut data = original.to_vec();
            if prediction {
                for row in data.as_chunks_mut::<4>().0 {
                    for i in (1..4).rev() {
                        row[i] = row[i].wrapping_sub(row[i - 1]);
                    }
                }
            }
            let mut zip =
                flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
            zip.write_all(&data).unwrap();
            let mut encoded = vec![0, if prediction { 3 } else { 2 }];
            encoded.extend(zip.finish().unwrap());
            assert_eq!(channel(&encoded, 4, 2, false).unwrap(), original);
        }
    }
}

#[cfg(test)]
#[path = "gray_psd_tests.rs"]
mod gray_psd_tests;

#[cfg(test)]
#[path = "psd_order_tests.rs"]
mod psd_order_tests;

#[cfg(test)]
#[path = "psd_matte_tests.rs"]
mod psd_matte_tests;

#[cfg(test)]
#[path = "gray_psd_output_tests.rs"]
mod gray_psd_output_tests;
