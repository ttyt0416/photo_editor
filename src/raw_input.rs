// This product includes DNG technology under license by Adobe.
use image::{DynamicImage, Rgba32FImage};
use rawler::{
    RawImage, RawImageData,
    imgop::{
        develop::{Intermediate, ProcessingStep, RawDevelop},
        xyz::Illuminant,
    },
    rawimage::RawPhotometricInterpretation,
    rawsource::RawSource,
};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::Arc};

// A current allocation budget, not a format-size limit.
pub const MAX_RAW_BYTES: u64 = 512 * 1024 * 1024;
pub const EXTENSIONS: &[&str] = &[
    "ari", "arw", "cr2", "cr3", "crm", "crw", "dcr", "dcs", "dng", "erf", "iiq", "kdc", "mef",
    "mos", "mrw", "nef", "nrw", "orf", "ori", "pef", "raf", "raw", "rw2", "rwl", "srw", "3fr",
    "fff", "x3f", "qtk",
];
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum WhiteBalance {
    #[default]
    Camera,
    Neutral,
    Custom,
    Temperature,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub white_balance: WhiteBalance,
    pub gains: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gains_fourth: Option<f32>,
    pub exposure: f32,
    #[serde(
        default = "default_temperature",
        skip_serializing_if = "is_default_temperature"
    )]
    pub temperature: f32,
    #[serde(default, skip_serializing_if = "is_zero_tint")]
    pub tint: f32,
}
fn default_temperature() -> f32 {
    6500.
}
fn is_default_temperature(value: &f32) -> bool {
    *value == default_temperature()
}
fn is_zero_tint(value: &f32) -> bool {
    *value == 0.
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            white_balance: Default::default(),
            gains: [1.; 3],
            gains_fourth: None,
            exposure: 0.,
            temperature: default_temperature(),
            tint: 0.,
        }
    }
}
impl Settings {
    pub fn needs_v10(&self) -> bool {
        self.white_balance == WhiteBalance::Temperature
            || !is_default_temperature(&self.temperature)
            || !is_zero_tint(&self.tint)
    }
    pub fn valid(&self) -> bool {
        self.gains_fourth
            .is_none_or(|g| g.is_finite() && (0.01..=32.).contains(&g))
            && self.temperature.is_finite()
            && (1000. ..=50000.).contains(&self.temperature)
            && self.tint.is_finite()
            && (-200. ..=200.).contains(&self.tint)
            && self.exposure.is_finite()
            && (-16.0..=16.0).contains(&self.exposure)
            && self
                .gains
                .iter()
                .all(|g| g.is_finite() && (0.01..=32.).contains(g))
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Info {
    pub make: String,
    pub model: String,
    pub sensor: [u32; 2],
    pub bits: u8,
    pub orientation: u8,
    pub camera_wb: [f32; 3],
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_wb_fourth: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_colors: Option<[u8; 4]>,
    pub calibrated: bool,
    pub monochrome: bool,
    #[serde(
        default,
        skip_serializing_if = "crate::raw_metadata::Shooting::is_empty"
    )]
    pub shooting: crate::raw_metadata::Shooting,
}
impl Info {
    pub fn channel_label(&self, index: usize) -> &'static str {
        if let Some(colors) = self.camera_colors {
            return colors
                .get(index)
                .and_then(|color| {
                    ["빨강", "초록", "파랑", "청록", "자홍", "노랑"].get(*color as usize)
                })
                .copied()
                .unwrap_or("센서 채널");
        }
        let labels = if self.camera_wb_fourth.is_some() {
            [
                "첫 번째 채널",
                "두 번째 채널",
                "세 번째 채널",
                "네 번째 채널",
            ]
        } else {
            ["빨강", "초록", "파랑", "네 번째 채널"]
        };
        labels.get(index).copied().unwrap_or("센서 채널")
    }

    fn valid(&self) -> bool {
        crate::document::validate_size(self.sensor[0], self.sensor[1]).is_ok()
            && self.shooting.valid()
            && self.bits > 0
            && self.bits <= 32
            && self.orientation <= 8
            && self.make.len() <= 4096
            && self.model.len() <= 4096
            && self.camera_colors.is_none_or(|colors| {
                self.camera_wb_fourth.is_some()
                    && colors.iter().all(|v| *v <= 5)
                    && colors
                        .into_iter()
                        .collect::<std::collections::HashSet<_>>()
                        .len()
                        == 4
            })
            && self
                .camera_wb_fourth
                .is_none_or(|g| g.is_finite() && g > 0.)
            && self.camera_wb.iter().all(|g| g.is_finite() && *g > 0.)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub filename: String,
    pub settings: Settings,
    pub info: Info,
}
impl Record {
    pub fn needs_v11(&self) -> bool {
        self.info.camera_wb_fourth.is_some() || self.settings.gains_fourth.is_some()
    }
    pub fn valid(&self) -> bool {
        !self.filename.is_empty()
            && self.filename.len() <= 4096
            && self.settings.valid()
            && self.info.valid()
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BalanceSample {
    pub gains: [f32; 3],
    pub gains_fourth: Option<f32>,
}
#[derive(Clone, Debug)]
pub struct Source {
    pub record: Record,
    pub bytes: Arc<Vec<u8>>,
}
impl Source {
    pub(crate) fn upgrade_legacy_info(&mut self) {
        if self.record.info.camera_wb_fourth.is_none()
            && let Ok(Some((wb, colors))) = dng_balance::legacy_info(&self.bytes)
        {
            let mut info = self.record.info.clone();
            info.camera_wb = wb[..3].try_into().unwrap();
            info.camera_wb_fourth = Some(wb[3]);
            info.camera_colors = colors;
            if info.valid() {
                self.record.info = info;
            }
        }
    }

    pub(crate) fn refresh_shot_white(&mut self) {
        if let Ok(Some(shot)) = dng_white::info(&self.bytes) {
            let mut info = self.record.info.clone();
            info.camera_wb = shot.gains[..3].try_into().unwrap();
            info.camera_wb_fourth = (shot.channels == 4).then_some(shot.gains[3]);
            info.camera_colors = shot.colors;
            if info.valid() {
                self.record.info = info;
            }
        }
    }
    pub fn valid(&self) -> bool {
        self.record.valid() && !self.bytes.is_empty() && self.bytes.len() as u64 <= MAX_RAW_BYTES
    }
    pub fn white_balance_at(&self, focus: [f32; 2], radius: u32) -> Result<BalanceSample, String> {
        guarded(|| white_balance::sample(self, focus, radius))
    }
    pub fn develop(&self, settings: &Settings) -> Result<Rgba32FImage, String> {
        guarded(|| {
            decode(&self.bytes, &self.record.filename, settings, false).map(|(_, pixels)| pixels)
        })
    }
}
fn guarded<T>(f: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|_| {
        Err(
            "RAW 해석 또는 현상 중 처리하지 못하는 데이터가 발견됐습니다. 원본 파일은 유지합니다."
                .into(),
        )
    })
}
pub fn open(path: &Path, name: String) -> Result<(crate::document::Document, Vec<String>), String> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if file.metadata().map_err(|e| e.to_string())?.len() > MAX_RAW_BYTES {
        return Err("RAW 원본이 현재 메모리 예산을 초과합니다.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_RAW_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.is_empty() || bytes.len() as u64 > MAX_RAW_BYTES {
        return Err("RAW 원본이 비어 있거나 현재 메모리 예산을 초과합니다.".into());
    }
    let bytes = Arc::new(bytes);
    let filename = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let settings = Settings::default();
    let (info, pixels) = guarded(|| decode(&bytes, &filename, &settings, true))?;
    let notes = vec![
        format!(
            "RAW 센서 데이터에서 {} {}의 {}비트 원본을 현상했습니다. 검정/흰색 기준·디모자이크·카메라 화이트 밸런스·색 행렬·기본 자르기·방향을 적용하고 32비트 작업 버퍼와 RAW 원본을 프로젝트에 보존합니다.",
            info.make, info.model, info.bits
        ),
        if info.calibrated || info.monochrome {
            "RAW 읽기는 카메라/압축 변형에 따라 처리 범위가 다릅니다. 추가 광원/프로파일·센서/CFA 경계·DNG 룩 테이블·ICC와 실제 카메라 전수 비교는 남은 목표입니다.".into()
        } else {
            "카메라 색 행렬이 없어 센서 채널 색상을 사용했습니다. 정확한 색 보정은 남은 목표입니다."
                .into()
        },
    ];
    let mut document = crate::precision::document(name, DynamicImage::ImageRgba32F(pixels))?;
    document.layers[0].raw_source = Some(Arc::new(Source {
        record: Record {
            filename,
            settings,
            info,
        },
        bytes,
    }));
    Ok((document, notes))
}
pub fn apply(
    layer: &mut crate::document::Layer,
    pixels: Arc<Rgba32FImage>,
    settings: Settings,
    source: &Arc<Source>,
) -> Result<(), String> {
    if !settings.valid() || !source.valid() || pixels.as_raw().iter().any(|v| !v.is_finite()) {
        return Err("RAW 현상 결과 또는 설정이 잘못되었습니다.".into());
    }
    let size = [pixels.width(), pixels.height()];
    crate::document::validate_size(size[0], size[1])?;
    if !layer.transform.valid_for(size) {
        return Err("RAW 현상 결과 크기에서 기존 원근 변형을 유지할 수 없습니다. 변형을 조절한 뒤 다시 적용하세요.".into());
    }
    let changed_size = layer.pixels.dimensions() != pixels.dimensions();
    let mask = if changed_size && layer.mask.is_some() {
        Some((
            crate::masks::world(layer),
            crate::masks::linked(layer),
            crate::masks::sampling(layer),
        ))
    } else {
        None
    };
    let mut updated = layer.clone();
    let mut raw = (**source).clone();
    raw.record.settings = settings;
    updated.pixels = Arc::new(crate::precision::preview(&pixels));
    updated.precision = Some(pixels);
    updated.bit_depth = 32;
    updated.raw_source = Some(Arc::new(raw));
    if let Some((matrix, linked, sampling)) = mask {
        crate::masks::set_world(&mut updated, matrix, linked, sampling)?;
    }
    if !crate::masks::valid(&updated) {
        return Err("RAW 현상 결과에서 마스크 배치를 유지할 수 없습니다.".into());
    }
    *layer = updated;
    Ok(())
}
// Check TIFF image dimensions before invoking the camera decoder. Some upstream
// dummy decodes still allocate pixels. Other container formats have separate parsers.
fn preflight(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() < 8 {
        return Err("RAW 파일이 너무 짧습니다.".into());
    }
    let little = match &bytes[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Ok(()),
    };
    let u16_at = |o: usize| -> Result<u16, String> {
        let a: [u8; 2] = bytes
            .get(o..o + 2)
            .ok_or("RAW TIFF 헤더가 잘렸습니다.")?
            .try_into()
            .unwrap();
        Ok(if little {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        })
    };
    let u32_at = |o: usize| -> Result<u32, String> {
        let a: [u8; 4] = bytes
            .get(o..o + 4)
            .ok_or("RAW TIFF 레코드가 잘렸습니다.")?
            .try_into()
            .unwrap();
        Ok(if little {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    };
    // Vendor RAWs also use proprietary TIFF magic values. The standard layout
    // traversal here applies to classic TIFF/DNG; their decoder checks the rest.
    if u16_at(2)? != 42 {
        return Ok(());
    }
    let mut pending = vec![u32_at(4)? as usize];
    let mut seen = std::collections::HashSet::new();
    while let Some(offset) = pending.pop() {
        if offset == 0 || !seen.insert(offset) {
            continue;
        }
        if seen.len() > 1024 {
            return Err("RAW TIFF의 디렉터리가 너무 많습니다.".into());
        }
        let count = u16_at(offset)? as usize;
        let end = offset
            .checked_add(2 + count * 12)
            .ok_or("RAW TIFF 길이가 잘못되었습니다.")?;
        if end + 4 > bytes.len() {
            return Err("RAW TIFF 디렉터리가 잘렸습니다.".into());
        }
        let mut dimensions = [None, None];
        for i in 0..count {
            let p = offset + 2 + i * 12;
            let tag = u16_at(p)?;
            let typ = u16_at(p + 2)?;
            let n = u32_at(p + 4)? as usize;
            if [256, 257].contains(&tag) && n == 1 && [3, 4].contains(&typ) {
                dimensions[(tag - 256) as usize] = Some(if typ == 3 {
                    u16_at(p + 8)? as u32
                } else {
                    u32_at(p + 8)?
                });
            }
            if tag == 330 && [4, 13].contains(&typ) {
                if n > 1024 {
                    return Err("RAW TIFF의 하위 디렉터리가 너무 많습니다.".into());
                }
                let start = if n == 1 {
                    p + 8
                } else {
                    u32_at(p + 8)? as usize
                };
                for j in 0..n {
                    pending.push(u32_at(start + j * 4)? as usize);
                }
            }
        }
        if let [Some(w), Some(h)] = dimensions {
            crate::document::validate_size(w, h)?;
        }
        pending.push(u32_at(end)? as usize);
    }
    Ok(())
}
fn develop_sensor(
    bytes: &Arc<Vec<u8>>,
    filename: &str,
    read_metadata: bool,
) -> Result<(Info, RawImage, Intermediate), String> {
    preflight(bytes)?;
    let source = RawSource::new_from_shared_vec(bytes.clone()).with_path(filename);
    let decoder =
        rawler::get_decoder(&source).map_err(|e| format!("RAW를 읽을 수 없습니다: {e}"))?;
    let mut raw = decoder
        .raw_image(&source, &Default::default(), false)
        .map_err(|e| format!("RAW를 읽을 수 없습니다: {e}"))?;
    let four_channel = raw.cpp == 4
        || matches!(&raw.photometric,RawPhotometricInterpretation::Cfa(config) if config.cfa.unique_colors()==4);
    if four_channel {
        if let RawPhotometricInterpretation::Cfa(config) = &mut raw.photometric {
            if let Some(colors) = dng_balance::plane_colors(bytes)? {
                if config.cfa.width != 2 || config.cfa.height != 2 {
                    return Err("이 4색 DNG CFA 배열은 아직 처리할 수 없습니다.".into());
                }
                let mut pattern = String::new();
                for y in 0..2 {
                    for x in 0..2 {
                        let index = config.cfa.color_at(y, x);
                        let color = *colors
                            .get(index)
                            .ok_or("4색 DNG CFA 인덱스가 색 목록을 넘습니다.")?;
                        pattern.push(['R', 'G', 'B', 'C', 'M', 'Y'][color as usize]);
                    }
                }
                config.cfa = rawler::CFA::new(&pattern);
                config.colors = rawler::cfa::PlaneColor::from(
                    colors.map(|v| rawler::cfa::CFAColor::try_from(v).unwrap()),
                );
            }
            if config.colors.plane_count() != 4 {
                return Err("4색 RAW 센서의 색 목록이 없습니다. 원본은 유지합니다.".into());
            }
        }
        if let Some(wb) = dng_balance::four_channel(bytes)? {
            raw.wb_coeffs = wb;
        }
        if !raw.wb_coeffs[3].is_finite() || raw.wb_coeffs[3] <= 0. {
            raw.wb_coeffs[3] = 1.;
        }
    }
    let analog_channels = if four_channel {
        4
    } else if raw.is_monochrome() {
        1
    } else {
        3
    };
    dng_balance::apply_analog(&mut raw, bytes, analog_channels)?;
    if let Some(shot) = dng_white::info(bytes)? {
        if shot.channels != analog_channels {
            return Err("DNG 촬영 광원과 센서 채널 수가 서로 다릅니다.".into());
        }
        raw.wb_coeffs = shot.gains;
    }
    let width = u32::try_from(raw.width).map_err(|_| "RAW 폭이 너무 큽니다.")?;
    let height = u32::try_from(raw.height).map_err(|_| "RAW 높이가 너무 큽니다.")?;
    crate::document::validate_size(width, height)?;
    if ![1, 3, 4].contains(&raw.cpp)
        || match &raw.data {
            RawImageData::Integer(p) => p.len() != raw.width * raw.height * raw.cpp,
            RawImageData::Float(p) => {
                p.len() != raw.width * raw.height * raw.cpp || p.iter().any(|v| !v.is_finite())
            }
        }
    {
        return Err("RAW 센서 픽셀 데이터가 잘못되었습니다.".into());
    }
    for area in [raw.active_area, raw.crop_area].into_iter().flatten() {
        if area.d.w == 0
            || area.d.h == 0
            || area.p.x.checked_add(area.d.w).is_none_or(|v| v > raw.width)
            || area
                .p
                .y
                .checked_add(area.d.h)
                .is_none_or(|v| v > raw.height)
        {
            return Err("RAW 자르기 영역이 센서 범위를 넘습니다.".into());
        }
    }
    let camera_wb: [f32; 3] = std::array::from_fn(|i| {
        if raw.wb_coeffs[i].is_finite() && raw.wb_coeffs[i] > 0. {
            raw.wb_coeffs[i]
        } else {
            1.
        }
    });
    let monochrome = raw.is_monochrome();
    let calibrated = !raw.color_matrix.is_empty();
    let info = Info {
        make: raw.clean_make.clone(),
        model: raw.clean_model.clone(),
        sensor: [width, height],
        bits: raw.bps as u8,
        orientation: raw.orientation.to_u16() as u8,
        camera_wb,
        camera_wb_fourth: four_channel.then_some(raw.wb_coeffs[3]),
        camera_colors: match &raw.photometric {
            RawPhotometricInterpretation::Cfa(config) if four_channel => Some(
                config
                    .colors
                    .colors
                    .iter()
                    .map(|v| *v as u8)
                    .collect::<Vec<_>>()
                    .try_into()
                    .map_err(|_| "4색 RAW 센서 목록이 잘못되었습니다.")?,
            ),
            _ => None,
        },
        calibrated,
        monochrome,
        shooting: if read_metadata {
            crate::raw_metadata::Shooting::read(decoder.as_ref(), &source)
        } else {
            Default::default()
        },
    };
    if !info.valid() {
        return Err("RAW 카메라 메타데이터가 잘못되었습니다.".into());
    }
    let black = raw.blacklevel.as_vec();
    let white = raw.whitelevel.as_vec();
    if black.is_empty()
        || white.is_empty()
        || black.iter().any(|v| !v.is_finite())
        || black
            .iter()
            .enumerate()
            .any(|(i, v)| *v >= white[i % white.len()])
    {
        return Err("RAW 검정/흰색 기준 값이 잘못되었습니다.".into());
    }
    let mut steps = vec![
        ProcessingStep::Demosaic,
        ProcessingStep::FujiRotate,
        ProcessingStep::CropActiveArea,
        ProcessingStep::CropDefault,
    ];
    if !matches!(raw.photometric, RawPhotometricInterpretation::Cfa(_)) {
        steps.retain(|s| *s != ProcessingStep::CropActiveArea);
    }
    if matches!(raw.photometric, RawPhotometricInterpretation::BlackIsZero) {
        let mut data = raw.data.as_f32().into_owned();
        for (i, v) in data.iter_mut().enumerate() {
            let b = black[i % black.len()];
            *v = (*v - b).max(0.) / (white[i % white.len()] - b);
        }
        raw.data = RawImageData::Float(data);
    } else {
        steps.insert(0, ProcessingStep::Rescale);
    }
    let developed = RawDevelop::new_with(&steps)
        .develop_intermediate(&raw)
        .map_err(|e| e.to_string())?;
    Ok((info, raw, developed))
}
fn decode(
    bytes: &Arc<Vec<u8>>,
    filename: &str,
    settings: &Settings,
    read_metadata: bool,
) -> Result<(Info, Rgba32FImage), String> {
    if !settings.valid() {
        return Err("RAW 현상 설정이 잘못되었습니다.".into());
    }
    let (info, raw, developed) = develop_sensor(bytes, filename, read_metadata)?;
    let monochrome = info.monochrome;
    let camera_wb = info.camera_wb;
    let dim = developed.dim();
    crate::document::validate_size(dim.w as u32, dim.h as u32)?;
    let channels = match &developed {
        Intermediate::FourColor(_) => 4,
        _ => 3,
    };
    let calibration = dng_calibration::Calibration::read(bytes, channels)?;
    let temperature_model = if settings.white_balance == WhiteBalance::Temperature {
        Some(temperature::prepare_calibrated(
            &raw,
            match &developed {
                Intermediate::FourColor(_) => 4,
                _ => 3,
            },
            settings.temperature,
            settings.tint,
            &calibration,
        )?)
    } else {
        None
    };
    let temperature_gains = temperature_model.as_ref().map(|p| p.gains);
    let wb = match settings.white_balance {
        WhiteBalance::Camera => camera_wb,
        WhiteBalance::Neutral => [1.; 3],
        WhiteBalance::Custom => settings.gains,
        WhiteBalance::Temperature => temperature_gains.unwrap()[..3].try_into().unwrap(),
    };
    let camera_fourth = info.camera_wb_fourth.unwrap_or(1.);
    let fourth = match settings.white_balance {
        WhiteBalance::Camera => camera_fourth,
        WhiteBalance::Neutral => 1.,
        WhiteBalance::Custom => settings.gains_fourth.unwrap_or(camera_fourth),
        WhiteBalance::Temperature => temperature_gains.unwrap()[3],
    };
    let gains = [wb[0], wb[1], wb[2], fourth];
    let matrix = if let Some(model) = temperature_model {
        model.matrix
    } else if matches!(
        settings.white_balance,
        WhiteBalance::Camera | WhiteBalance::Custom
    ) {
        balance_calibration(
            &raw,
            match &developed {
                Intermediate::FourColor(_) => 4,
                _ => 3,
            },
            gains,
            &calibration,
        )?
    } else {
        calibration_calibrated(&raw, &calibration)?
    };
    let transform = ColorTransform::new(&matrix, &gains, settings.exposure, monochrome)?;
    let convert = |p: &[f32]| transform.pixel(p);
    let mut data = Vec::with_capacity(dim.w * dim.h * 4);
    match developed {
        Intermediate::Monochrome(p) => {
            for v in p.data {
                data.extend(convert(&[v])?);
            }
        }
        Intermediate::ThreeColor(p) => {
            for v in p.data {
                data.extend(convert(&v)?);
            }
        }
        Intermediate::FourColor(p) => {
            for v in p.data {
                data.extend(convert(&v)?);
            }
        }
    }
    let pixels = Rgba32FImage::from_raw(dim.w as u32, dim.h as u32, data)
        .ok_or("RAW 결과 크기가 잘못되었습니다.")?;
    let mut image = DynamicImage::ImageRgba32F(pixels);
    image.apply_orientation(
        image::metadata::Orientation::from_exif(info.orientation)
            .unwrap_or(image::metadata::Orientation::NoTransforms),
    );
    Ok((info, image.into_rgba32f()))
}
// Validate and precompute once per image, keeping each pixel's intermediates wide.
struct ColorTransform {
    weighted: [[f64; 4]; 3],
    gain: f64,
    monochrome: bool,
}
impl ColorTransform {
    fn new(
        matrix: &[[f32; 4]; 3],
        wb: &[f32; 4],
        exposure: f32,
        monochrome: bool,
    ) -> Result<Self, String> {
        if matrix.as_flattened().iter().any(|v| !v.is_finite())
            || wb.iter().any(|v| !v.is_finite() || *v <= 0.)
            || !exposure.is_finite()
            || !(-16. ..=16.).contains(&exposure)
        {
            return Err("RAW 색 변환 입력이 잘못되었습니다.".into());
        }
        Ok(Self {
            weighted: matrix
                .map(|row| std::array::from_fn(|i| f64::from(row[i]) * f64::from(wb[i]))),
            gain: f64::from(exposure).exp2(),
            monochrome,
        })
    }
    fn pixel(&self, p: &[f32]) -> Result<[f32; 4], String> {
        if p.is_empty() || p.len() > 4 || p.iter().any(|v| !v.is_finite()) {
            return Err("RAW 색 변환 입력이 잘못되었습니다.".into());
        }
        let mut rgb = [0f64; 3];
        if self.monochrome || p.len() == 1 {
            rgb.fill(f64::from(p[0]) * self.gain);
        } else {
            for (channel, row) in rgb.iter_mut().zip(&self.weighted) {
                *channel = p
                    .iter()
                    .zip(row)
                    .map(|(value, weight)| f64::from(*value) * weight)
                    .sum::<f64>()
                    * self.gain;
            }
        }
        let encoded = rgb.map(|v| {
            if v <= 0.0031308 {
                12.92 * v
            } else {
                1.055 * v.powf(1. / 2.4) - 0.055
            }
        });
        if encoded.iter().any(|v| !v.is_finite()) {
            return Err("RAW 색 변환 결과가 부동소수점 범위를 넘습니다.".into());
        }
        let stored = encoded.map(crate::precision::float_channel);
        Ok([stored[0], stored[1], stored[2], 1.])
    }
}
#[cfg(test)]
fn convert_pixel(
    p: &[f32],
    matrix: &[[f32; 4]; 3],
    wb: &[f32; 4],
    exposure: f32,
    monochrome: bool,
) -> Result<[f32; 4], String> {
    ColorTransform::new(matrix, wb, exposure, monochrome)?.pixel(p)
}
type NativeMatrix = (Illuminant, [[f32; 3]; 4]);
fn native_camera_matrix(raw: &RawImage) -> Result<Option<NativeMatrix>, String> {
    let Some((illuminant, flat)) = raw.color_matrix_find_first([
        Illuminant::D65,
        Illuminant::A,
        Illuminant::B,
        Illuminant::C,
        Illuminant::D50,
        Illuminant::D55,
        Illuminant::D75,
        Illuminant::Daylight,
        Illuminant::Flash,
    ]) else {
        return Ok(None);
    };
    if ![9, 12].contains(&flat.len()) || flat.iter().any(|v| !v.is_finite()) {
        return Err("RAW 색 행렬이 잘못되었습니다.".into());
    }
    let mut xyz2cam = [[0.; 3]; 4];
    for (i, row) in flat.as_chunks::<3>().0.iter().enumerate() {
        xyz2cam[i].copy_from_slice(row);
    }
    Ok(Some((illuminant, xyz2cam)))
}
fn camera_matrix(raw: &RawImage) -> Result<Option<[[f32; 3]; 4]>, String> {
    let Some((illuminant, xyz2cam)) = native_camera_matrix(raw)? else {
        return Ok(None);
    };
    color_matrix::adapt_input(xyz2cam, illuminant).map(Some)
}

fn balance_calibration(
    raw: &RawImage,
    channels: usize,
    gains: [f32; 4],
    cal: &dng_calibration::Calibration,
) -> Result<[[f32; 4]; 3], String> {
    if let Some((native, white)) =
        color_profile::balance_profile_calibrated(raw, channels, gains, cal)?
    {
        calibration_matrix(color_matrix::adapt_white(native, white)?)
    } else {
        calibration_calibrated(raw, cal)
    }
}
fn calibration_calibrated(
    raw: &RawImage,
    cal: &dng_calibration::Calibration,
) -> Result<[[f32; 4]; 3], String> {
    if raw.is_monochrome() {
        return calibration(raw);
    }
    let Some((light, native)) = native_camera_matrix(raw)? else {
        return calibration(raw);
    };
    calibration_matrix(color_matrix::adapt_input(
        cal.at_light(native, light)?,
        light,
    )?)
}
fn calibration(raw: &RawImage) -> Result<[[f32; 4]; 3], String> {
    let identity = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]];
    if raw.is_monochrome() {
        return Ok(identity);
    }
    let Some(xyz2cam) = camera_matrix(raw)? else {
        return Ok(identity);
    };
    calibration_matrix(xyz2cam)
}
fn calibration_matrix(xyz2cam: [[f32; 3]; 4]) -> Result<[[f32; 4]; 3], String> {
    use rawler::imgop::xyz::SRGB_TO_XYZ_D65;
    let rgb2cam = xyz2cam.map(|row| {
        std::array::from_fn::<_, 3, _>(|c| {
            row.iter()
                .zip(SRGB_TO_XYZ_D65)
                .map(|(a, b)| f64::from(*a) * f64::from(b[c]))
                .sum::<f64>()
        })
    });
    let normalized = rgb2cam.map(|row| {
        let sum = row.iter().sum::<f64>();
        if sum == 0. {
            [0.; 3]
        } else {
            row.map(|v| v / sum)
        }
    });
    let result = color_matrix::pseudo_inverse(normalized)?.map(|row| row.map(|v| v as f32));
    if result.as_flattened().iter().any(|v| !v.is_finite()) {
        return Err("RAW 색 행렬을 역변환할 수 없습니다.".into());
    }
    Ok(result)
}

#[cfg(test)]
#[path = "raw_input_tests.rs"]
mod tests;

#[path = "raw_white_balance.rs"]
mod white_balance;

#[path = "raw_temperature.rs"]
mod temperature;

#[cfg(test)]
#[path = "raw_hdr_tests.rs"]
mod hdr_tests;

#[path = "raw_dng_balance.rs"]
mod dng_balance;

#[cfg(test)]
#[path = "raw_color_tests.rs"]
mod color_tests;

#[path = "raw_matrix.rs"]
mod color_matrix;

#[path = "raw_color_profile.rs"]
mod color_profile;

#[cfg(test)]
#[path = "raw_dual_tests.rs"]
mod dual_tests;

#[cfg(test)]
#[path = "raw_analog_tests.rs"]
mod analog_tests;

#[path = "raw_dng_white.rs"]
mod dng_white;

#[cfg(test)]
#[path = "raw_white_tests.rs"]
mod dng_white_tests;

#[path = "raw_dng_calibration.rs"]
mod dng_calibration;

#[cfg(test)]
#[path = "raw_calibration_tests.rs"]
mod calibration_tests;
