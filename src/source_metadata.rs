use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Seek, Write},
    sync::Arc,
};
pub const MAX_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub exif: Option<Arc<Vec<u8>>>,
    pub xmp: Option<Arc<Vec<u8>>>,
    pub iptc: Option<Arc<Vec<u8>>>,
    pub iptc_tiff: Option<TiffIptc>,
    pub photoshop: Option<Arc<Vec<u8>>>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TiffIptc {
    pub kind: u16,
    pub little: bool,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    #[serde(default, skip_serializing_if = "is_false")]
    pub exif: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub xmp: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub iptc: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iptc_tiff: Option<TiffIptc>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub photoshop: bool,
}
fn is_false(value: &bool) -> bool {
    !value
}
impl Record {
    pub fn is_empty(&self) -> bool {
        !self.exif && !self.xmp && !self.iptc && !self.photoshop && self.iptc_tiff.is_none()
    }
}
impl Metadata {
    pub fn record(&self) -> Record {
        Record {
            exif: self.exif.is_some(),
            xmp: self.xmp.is_some(),
            iptc: self.iptc.is_some(),
            iptc_tiff: self.iptc_tiff.clone(),
            photoshop: self.photoshop.is_some(),
        }
    }
    pub fn assets(&self) -> impl Iterator<Item = (&'static str, &Arc<Vec<u8>>)> {
        [
            ("metadata/source.exif", self.exif.as_ref()),
            ("metadata/source.xmp", self.xmp.as_ref()),
            ("metadata/source.iptc", self.iptc.as_ref()),
            ("metadata/source.photoshop", self.photoshop.as_ref()),
        ]
        .into_iter()
        .filter_map(|(name, data)| data.map(|data| (name, data)))
    }
    pub fn valid(&self) -> bool {
        self.assets().all(|(_, data)| data.len() <= MAX_BYTES)
            && self.iptc.is_some() == self.iptc_tiff.is_some()
            && self
                .iptc_tiff
                .as_ref()
                .is_none_or(|v| matches!(v.kind, 1 | 4 | 7))
            && self
                .iptc
                .as_ref()
                .zip(self.iptc_tiff.as_ref())
                .is_none_or(|(bytes, format)| format.kind != 4 || bytes.len().is_multiple_of(4))
    }
    pub fn from_bytes(
        exif: Option<Vec<u8>>,
        xmp: Option<Vec<u8>>,
        notes: &mut Vec<String>,
    ) -> Self {
        fn asset(
            bytes: Option<Vec<u8>>,
            name: &str,
            notes: &mut Vec<String>,
        ) -> Option<Arc<Vec<u8>>> {
            bytes.and_then(|bytes| {
                if bytes.len() > MAX_BYTES {
                    notes.push(format!(
                        "원본 {name} 메타데이터가 보관 예산을 초과했습니다."
                    ));
                    None
                } else {
                    Some(Arc::new(bytes))
                }
            })
        }
        let metadata = Self {
            exif: asset(exif, "EXIF", notes),
            xmp: asset(xmp, "XMP", notes),
            ..Default::default()
        };
        if !metadata.record().is_empty() {
            notes.push("원본 촬영 메타데이터를 프로젝트에 보존합니다.".into());
        }
        metadata
    }
    pub fn decode(
        decoder: &mut impl image::ImageDecoder,
        path: &std::path::Path,
        format: Option<image::ImageFormat>,
        notes: &mut Vec<String>,
    ) -> Self {
        fn result(
            result: Result<Option<Vec<u8>>, String>,
            label: &str,
            notes: &mut Vec<String>,
        ) -> Option<Vec<u8>> {
            match result {
                Ok(bytes) => bytes,
                Err(error) => {
                    notes.push(format!(
                        "원본 {label} 메타데이터를 읽지 못했습니다: {error}"
                    ));
                    None
                }
            }
        }
        let exif = result(
            if format == Some(image::ImageFormat::Tiff) {
                Ok(crate::tiff_exif::read(path, notes))
            } else {
                decoder.exif_metadata().map_err(|e| e.to_string())
            },
            "EXIF",
            notes,
        );
        let xmp = result(
            if format == Some(image::ImageFormat::Tiff) {
                (|| {
                    let mut limits = tiff::decoder::Limits::default();
                    limits.ifd_value_size = MAX_BYTES;
                    limits.decoding_buffer_size = MAX_BYTES;
                    let mut decoder = tiff::decoder::Decoder::new(std::io::BufReader::new(
                        std::fs::File::open(path).map_err(|e| e.to_string())?,
                    ))
                    .map_err(|e| e.to_string())?
                    .with_limits(limits);
                    decoder
                        .find_tag_unsigned_vec::<u8>(tiff::tags::Tag::Unknown(700))
                        .map_err(|e| e.to_string())
                })()
            } else {
                decoder.xmp_metadata().map_err(|e| e.to_string())
            },
            "XMP",
            notes,
        );
        let mut metadata = Self::from_bytes(exif, xmp, notes);
        if format == Some(image::ImageFormat::Tiff) {
            crate::tiff_iptc::remember(path, &mut metadata, notes);
        } else if format == Some(image::ImageFormat::Jpeg) {
            crate::jpeg_iptc::remember(path, &mut metadata, notes);
        }
        metadata
    }
    pub fn save<W: Write + Seek>(
        &self,
        zip: &mut zip::ZipWriter<W>,
        options: zip::write::SimpleFileOptions,
    ) -> Result<(), String> {
        for (name, data) in self.assets() {
            zip.start_file(name, options).map_err(|e| e.to_string())?;
            zip.write_all(data).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    pub fn load<R: Read + Seek>(
        record: Record,
        zip: &mut zip::ZipArchive<R>,
    ) -> Result<Self, String> {
        if record.iptc != record.iptc_tiff.is_some()
            || record
                .iptc_tiff
                .as_ref()
                .is_some_and(|v| !matches!(v.kind, 1 | 4 | 7))
        {
            return Err("IPTC 원문 저장 형식이 잘못되었습니다".into());
        }
        let metadata = Self {
            exif: if record.exif {
                Some(read_asset(zip, "metadata/source.exif")?)
            } else {
                None
            },
            xmp: if record.xmp {
                Some(read_asset(zip, "metadata/source.xmp")?)
            } else {
                None
            },
            iptc: if record.iptc {
                Some(read_asset(zip, "metadata/source.iptc")?)
            } else {
                None
            },
            iptc_tiff: record.iptc_tiff,
            photoshop: if record.photoshop {
                Some(read_asset(zip, "metadata/source.photoshop")?)
            } else {
                None
            },
        };
        if !metadata.valid() {
            return Err("IPTC 원문 저장 형식 또는 크기가 잘못되었습니다".into());
        }
        Ok(metadata)
    }
}
pub fn read_asset<R: Read + Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Arc<Vec<u8>>, String> {
    let mut asset = zip.by_name(name).map_err(|e| e.to_string())?;
    if asset.size() > MAX_BYTES as u64 {
        return Err("원본 메타데이터가 보관 예산을 초과합니다.".into());
    }
    let mut data = Vec::new();
    (&mut asset)
        .take(MAX_BYTES as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > MAX_BYTES {
        return Err("원본 메타데이터가 보관 예산을 초과합니다.".into());
    }
    Ok(Arc::new(data))
}

#[cfg(test)]
#[path = "source_metadata_tests.rs"]
mod tests;
