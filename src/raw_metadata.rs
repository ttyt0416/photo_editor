use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Shooting {
    pub lens_make: Option<String>,
    pub lens_model: Option<String>,
    pub lens_spec: Option<[[u32; 2]; 4]>,
    pub focal_length: Option<[u32; 2]>,
    pub aperture: Option<[u32; 2]>,
    pub shutter: Option<[u32; 2]>,
    pub iso: Option<u32>,
    pub warning: Option<String>,
}
fn positive(r: [u32; 2]) -> bool {
    r[0] > 0 && r[1] > 0
}
impl Shooting {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
    pub fn valid(&self) -> bool {
        [&self.lens_make, &self.lens_model, &self.warning]
            .into_iter()
            .all(|s| {
                s.as_ref()
                    .is_none_or(|s| !s.trim().is_empty() && s.len() <= 4096)
            })
            && [self.focal_length, self.aperture, self.shutter]
                .into_iter()
                .all(|r| r.is_none_or(positive))
            && self
                .lens_spec
                .is_none_or(|s| s.iter().all(|r| r[1] > 0) && positive(s[0]) && positive(s[1]))
            && self.iso.is_none_or(|v| v > 0)
    }
    pub fn from_exif(exif: rawler::exif::Exif) -> Self {
        let mut invalid = false;
        let mut name = |s: Option<String>| {
            s.and_then(|s| {
                let s = s.trim().to_owned();
                if s.is_empty() {
                    None
                } else if s.len() > 4096 {
                    invalid = true;
                    None
                } else {
                    Some(s)
                }
            })
        };
        let lens_make = name(exif.lens_make);
        let lens_model = name(exif.lens_model);
        let mut rational = |r: Option<rawler::formats::tiff::Rational>| {
            r.and_then(|r| {
                if r.n > 0 && r.d > 0 {
                    Some([r.n, r.d])
                } else {
                    invalid = true;
                    None
                }
            })
        };
        let focal_length = rational(exif.focal_length);
        let aperture = rational(exif.fnumber);
        let shutter = rational(exif.exposure_time);
        let lens_spec = exif.lens_spec.and_then(|s| {
            let s = s.map(|r| [r.n, r.d]);
            if s.iter().all(|r| r[1] > 0) && positive(s[0]) && positive(s[1]) {
                Some(s)
            } else {
                invalid = true;
                None
            }
        });
        let iso = exif
            .iso_speed
            .filter(|v| *v > 0)
            .or_else(|| exif.iso_speed_ratings.map(u32::from).filter(|v| *v > 0));
        Self {
            lens_make,
            lens_model,
            lens_spec,
            focal_length,
            aperture,
            shutter,
            iso,
            warning: invalid.then(|| {
                "잘못되거나 너무 큰 촬영 정보 일부를 제외했습니다. RAW 원본은 유지합니다.".into()
            }),
        }
    }
    pub fn read(
        decoder: &dyn rawler::decoders::Decoder,
        source: &rawler::rawsource::RawSource,
    ) -> Self {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            decoder.raw_metadata(source, &Default::default())
        })) {
            Ok(Ok(metadata)) => Self::from_exif(metadata.exif),
            _ => Self {
                warning: Some(
                    "촬영 정보를 해석하지 못했습니다. 센서 픽셀과 RAW 원본은 유지합니다.".into(),
                ),
                ..Default::default()
            },
        }
    }
    pub fn ui(&self, ui: &mut eframe::egui::Ui) {
        if self.lens_make.is_some() || self.lens_model.is_some() {
            ui.label(format!(
                "렌즈: {} {}",
                self.lens_make.as_deref().unwrap_or(""),
                self.lens_model.as_deref().unwrap_or("")
            ));
        }
        let mut items = Vec::new();
        if let Some(r) = self.focal_length {
            items.push(format!("{:.1} mm", f64::from(r[0]) / f64::from(r[1])));
        }
        if let Some(r) = self.aperture {
            items.push(format!("f/{:.1}", f64::from(r[0]) / f64::from(r[1])));
        }
        if let Some(r) = self.shutter {
            if r[0] == 1 {
                items.push(format!("1/{} s", r[1]));
            } else {
                items.push(format!("{:.4} s", f64::from(r[0]) / f64::from(r[1])));
            }
        }
        if let Some(iso) = self.iso {
            items.push(format!("ISO {iso}"));
        }
        if !items.is_empty() {
            ui.label(items.join(" · "));
        }
        if let Some(warning) = &self.warning {
            ui.label(warning);
        }
    }
}
#[cfg(test)]
#[path = "raw_metadata_tests.rs"]
mod tests;
