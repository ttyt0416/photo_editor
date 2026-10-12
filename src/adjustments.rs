use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Curve {
    pub points: Vec<[f32; 2]>,
}

impl Default for Curve {
    fn default() -> Self {
        Self {
            points: vec![[0.0, 0.0], [1.0, 1.0]],
        }
    }
}

impl Curve {
    pub fn valid(&self) -> bool {
        (2..=64).contains(&self.points.len())
            && self.points.first().is_some_and(|p| p[0] == 0.0)
            && self.points.last().is_some_and(|p| p[0] == 1.0)
            && self
                .points
                .iter()
                .flatten()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(v))
            && self.points.windows(2).all(|p| p[1][0] - p[0][0] >= 0.001)
    }
    #[cfg(test)]
    pub fn value(&self, input: f32) -> f32 {
        crate::precision::float_channel(self.value_precise(f64::from(input)))
    }
    fn value_precise(&self, input: f64) -> f64 {
        if self.points == [[0., 0.], [1., 1.]] {
            return input;
        }
        let right = self
            .points
            .partition_point(|p| f64::from(p[0]) < input)
            .clamp(1, self.points.len() - 1);
        let (a, b) = (
            self.points[right - 1].map(f64::from),
            self.points[right].map(f64::from),
        );
        a[1] + (input - a[0]) * (b[1] - a[1]) / (b[0] - a[0])
    }
}

// A fixed coordinate seed keeps filter previews and exports identical.
fn noise(x: u32, y: u32, channel: u32) -> f32 {
    let mut seed =
        x.wrapping_mul(0x9e3779b9) ^ y.wrapping_mul(0x85ebca6b) ^ channel.wrapping_mul(0xc2b2ae35);
    seed ^= seed >> 16;
    seed = seed.wrapping_mul(0x7feb352d);
    seed ^= seed >> 15;
    seed = seed.wrapping_mul(0x846ca68b);
    seed ^= seed >> 16;
    seed as f32 / u32::MAX as f32 - 0.5
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Adjustments {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<crate::adjustment_scope::Scope>,
    pub brightness: f32,
    pub contrast: f32,
    pub exposure: f32,
    pub saturation: f32,
    pub hue: f32,
    pub temperature: f32,
    pub tint: f32,
    pub color_balance: crate::color_balance::ColorBalance,
    #[serde(skip_serializing_if = "crate::color_mixer::ColorMixer::disabled")]
    pub color_mixer: crate::color_mixer::ColorMixer,
    #[serde(skip_serializing_if = "crate::color_grading::ColorGrading::is_default")]
    pub color_grading: crate::color_grading::ColorGrading,
    #[serde(skip_serializing_if = "crate::denoise::Denoise::is_default")]
    pub denoise: crate::denoise::Denoise,
    #[serde(skip_serializing_if = "crate::detail_sharpen::DetailSharpen::is_default")]
    pub detail_sharpen: crate::detail_sharpen::DetailSharpen,
    pub tonal_contrast: crate::tonal_contrast::TonalContrast,
    pub bloom: crate::bloom::Bloom,
    pub lens: crate::lens::Lens,
    #[serde(skip_serializing_if = "crate::photo_geometry::Geometry::is_default")]
    pub geometry: crate::photo_geometry::Geometry,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lens_profile: Option<crate::lens_profile::Profile>,
    pub gamma: f32,
    pub black: f32,
    pub white: f32,
    pub monochrome: bool,
    pub invert: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub expand_filters: bool,
    pub blur: f32,
    pub sharpen: f32,
    pub curves: [Curve; 4],
    pub gradient_map: bool,
    pub gradient_colors: [[u8; 4]; 2],
    pub grain: f32,
    pub noise: f32,
    pub vignette: f32,
    pub motion_length: u32,
    pub motion_angle: f32,
}

impl Default for Adjustments {
    fn default() -> Self {
        Self {
            scope: None,
            brightness: 0.0,
            contrast: 0.0,
            exposure: 0.0,
            saturation: 0.0,
            hue: 0.0,
            temperature: 0.0,
            tint: 0.0,
            color_balance: Default::default(),
            color_mixer: Default::default(),
            color_grading: Default::default(),
            denoise: Default::default(),
            detail_sharpen: Default::default(),
            tonal_contrast: Default::default(),
            bloom: Default::default(),
            lens: Default::default(),
            geometry: Default::default(),
            lens_profile: None,
            gamma: 1.0,
            black: 0.0,
            white: 1.0,
            monochrome: false,
            invert: false,
            expand_filters: false,
            blur: 0.0,
            sharpen: 0.0,
            curves: std::array::from_fn(|_| Curve::default()),
            gradient_map: false,
            gradient_colors: [[0, 0, 0, 255], [255; 4]],
            grain: 0.0,
            noise: 0.0,
            vignette: 0.0,
            motion_length: 0,
            motion_angle: 0.0,
        }
    }
}

impl Adjustments {
    pub fn active(&self) -> bool {
        let defaults = Self {
            scope: self.scope.clone(),
            expand_filters: self.expand_filters,
            color_grading: if self.color_grading.enabled() {
                Default::default()
            } else {
                self.color_grading.clone()
            },
            denoise: if self.denoise.enabled() {
                Default::default()
            } else {
                self.denoise.clone()
            },
            detail_sharpen: if self.detail_sharpen.enabled() {
                Default::default()
            } else {
                self.detail_sharpen.clone()
            },
            lens_profile: self
                .lens_profile
                .as_ref()
                .filter(|p| !p.effect_enabled())
                .cloned(),
            tonal_contrast: if self.tonal_contrast.enabled() {
                Default::default()
            } else {
                self.tonal_contrast.clone()
            },
            ..Default::default()
        };
        *self != defaults
    }
    pub fn apply_float(&self, source: &image::Rgba32FImage) -> image::Rgba32FImage {
        self.apply_float_at(source, [0; 2])
    }
    pub fn apply_float_at(
        &self,
        source: &image::Rgba32FImage,
        origin: [u32; 2],
    ) -> image::Rgba32FImage {
        self.apply_frame(source, origin, 0)
            .unwrap_or_else(|_| source.clone())
    }
    pub fn padding(&self) -> u32 {
        if !self.expand_filters {
            return 0;
        }
        crate::precision::blur_padding(self.blur)
            + if self.motion_length > 1 {
                self.motion_length / 2
            } else {
                0
            }
            + if self.bloom.amount > 0. {
                crate::precision::blur_padding(self.bloom.radius)
            } else {
                0
            }
    }
    pub fn apply_expanded(
        &self,
        source: &image::Rgba32FImage,
    ) -> Result<(image::Rgba32FImage, i32), String> {
        if !self.valid() {
            return Err("보정 값이 유효하지 않습니다.".into());
        }
        let pad = self.padding();
        Ok((self.apply_frame(source, [0; 2], pad)?, pad as i32))
    }
    fn apply_frame(
        &self,
        source: &image::Rgba32FImage,
        origin: [u32; 2],
        pad: u32,
    ) -> Result<image::Rgba32FImage, String> {
        if !self.active() {
            return Ok(source.clone());
        }
        let size = [source.width(), source.height()];
        if pad > 0 {
            let size = size.map(|v| v.saturating_add(pad * 2));
            crate::document::validate_size(size[0], size[1])?;
        }
        let rotation = self.hue.to_radians().sin_cos();
        let mut points = self.detail_sharpen.apply(&self.denoise.apply(source));
        for (x, y, p) in points.enumerate_pixels_mut() {
            p.0 = self.apply_point(p.0, [x, y], size, rotation);
        }
        if self.tonal_contrast.enabled() {
            points = self.tonal_contrast.apply(&points);
        }
        let mut output = if pad == 0 {
            points
        } else {
            let padded = size.map(|v| v.saturating_add(pad * 2));
            crate::document::validate_size(padded[0], padded[1])?;
            let count = padded[0] as usize * padded[1] as usize * 4;
            let mut data = Vec::new();
            data.try_reserve_exact(count)
                .map_err(|_| "필터 경계 확장을 위한 메모리가 부족합니다.")?;
            data.resize(count, 0.);
            let mut expanded = image::Rgba32FImage::from_raw(padded[0], padded[1], data).unwrap();
            image::imageops::replace(&mut expanded, &points, pad as i64, pad as i64);
            expanded
        };
        if self.motion_length > 1 {
            output = motion_blur(&output, self.motion_length, self.motion_angle);
        }
        if self.blur > 0. {
            output = crate::precision::blur(&output, self.blur);
        }
        if self.sharpen > 0. {
            let low = crate::precision::blur(&output, 1.0);
            for (p, l) in output.pixels_mut().zip(low.pixels()) {
                for c in 0..3 {
                    p[c] = crate::precision::float_channel(
                        f64::from(p[c])
                            + (f64::from(p[c]) - f64::from(l[c])) * f64::from(self.sharpen),
                    );
                }
            }
        }
        if self.bloom.amount > 0. {
            output = self.bloom.apply_alpha(&output, self.expand_filters);
        }
        if let Some(profile) = &self.lens_profile {
            output = profile.apply_frame(&output, size, pad, self.lens.sampling);
        }
        if self.lens.enabled() {
            output = self.lens.apply_frame(&output, size, pad);
        }
        if self.geometry.enabled() {
            output = self.geometry.apply_frame(&output, size, pad);
        }
        if let Some(scope) = &self.scope {
            for (x, y, p) in output.enumerate_pixels_mut() {
                let local = [x as i32 - pad as i32, y as i32 - pad as i32];
                let original = if local[0] >= 0
                    && local[1] >= 0
                    && local[0] < size[0] as i32
                    && local[1] < size[1] as i32
                {
                    source.get_pixel(local[0] as u32, local[1] as u32).0
                } else {
                    [0.; 4]
                };
                p.0 = crate::adjustment_scope::mix(
                    original,
                    p.0,
                    scope.coverage([
                        local[0] as f32 + origin[0] as f32 + 0.5,
                        local[1] as f32 + origin[1] as f32 + 0.5,
                    ]),
                );
            }
        }
        Ok(output)
    }
    pub fn local(&self) -> bool {
        self.motion_length <= 1
            && self.blur == 0.0
            && self.sharpen == 0.0
            && self.bloom.amount == 0.0
            && !self.lens.enabled()
            && !self.geometry.enabled()
            && self
                .lens_profile
                .as_ref()
                .is_none_or(|p| !p.effect_enabled())
            && !self.tonal_contrast.enabled()
            && !self.denoise.enabled()
            && !self.detail_sharpen.enabled()
    }
    pub fn sample(&self, pixel: [f32; 4], position: [u32; 2], size: [u32; 2]) -> [f32; 4] {
        let adjusted = self.apply_point(pixel, position, size, self.hue.to_radians().sin_cos());
        crate::adjustment_scope::mix(
            pixel,
            adjusted,
            self.scope
                .as_ref()
                .map_or(1., |s| s.coverage(position.map(|v| v as f32 + 0.5))),
        )
    }
    fn apply_point(
        &self,
        pixel: [f32; 4],
        [x, yy]: [u32; 2],
        size: [u32; 2],
        (sin, cos): (f32, f32),
    ) -> [f32; 4] {
        if self.brightness == 0.
            && self.contrast == 0.
            && self.exposure == 0.
            && self.saturation == 0.
            && self.hue == 0.
            && self.temperature == 0.
            && self.tint == 0.
            && self.black == 0.
            && self.white == 1.
            && self.gamma == 1.
            && !self.monochrome
            && !self.invert
            && !self.gradient_map
            && self.vignette == 0.
            && self.grain == 0.
            && self.noise == 0.
            && self.color_balance.shifts == [[0.; 3]; 3]
            && self.color_mixer.disabled()
            && !self.color_grading.enabled()
            && self.curves.iter().all(|c| c.points == [[0., 0.], [1., 1.]])
        {
            return pixel;
        }
        let mut c = [pixel[0], pixel[1], pixel[2]].map(f64::from);
        for v in &mut c {
            if self.black != 0. || self.white != 1. || self.gamma != 1. {
                *v = ((*v - f64::from(self.black))
                    / (f64::from(self.white) - f64::from(self.black)))
                .max(0.)
                .powf(1. / f64::from(self.gamma));
                // A valid gamma can exceed f64 at extreme inputs; cap at the storage boundary.
                *v = f64::from(crate::precision::float_channel(*v));
            }
            if self.exposure != 0. || self.contrast != 0. || self.brightness != 0. {
                *v = (*v * 2f64.powf(f64::from(self.exposure)) - 0.5)
                    * (1. + f64::from(self.contrast))
                    + 0.5
                    + f64::from(self.brightness);
            }
        }
        c[0] += f64::from(self.temperature) * 0.15;
        c[2] -= f64::from(self.temperature) * 0.15;
        c[1] += f64::from(self.tint) * 0.15;
        if self.color_balance.shifts != [[0.; 3]; 3] {
            c = self
                .color_balance
                .apply(c.map(crate::precision::float_channel))
                .map(f64::from);
        }
        if !self.color_mixer.disabled() {
            c = self
                .color_mixer
                .apply(c.map(crate::precision::float_channel))
                .map(f64::from);
        }
        if self.color_grading.enabled() {
            c = self
                .color_grading
                .apply(c.map(crate::precision::float_channel))
                .map(f64::from);
        }
        let y = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
        if self.hue != 0. {
            let i = 0.596 * c[0] - 0.274 * c[1] - 0.322 * c[2];
            let q = 0.211 * c[0] - 0.523 * c[1] + 0.312 * c[2];
            let ii = i * f64::from(cos) - q * f64::from(sin);
            let qq = i * f64::from(sin) + q * f64::from(cos);
            c = [
                y + 0.956 * ii + 0.621 * qq,
                y - 0.272 * ii - 0.647 * qq,
                y - 1.106 * ii + 1.703 * qq,
            ];
        }
        for (n, value) in c.iter_mut().enumerate() {
            *value = if self.monochrome {
                y
            } else if self.saturation != 0. {
                y + (*value - y) * (1. + f64::from(self.saturation))
            } else {
                *value
            };
            if self.invert {
                *value = 1. - *value;
            }
            *value = self.curves[n + 1].value_precise(self.curves[0].value_precise(*value));
        }
        let mut alpha = pixel[3];
        if self.gradient_map {
            let t = (0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]).clamp(0., 1.);
            for (n, value) in c.iter_mut().enumerate() {
                *value = (f64::from(self.gradient_colors[0][n]) * (1. - t)
                    + f64::from(self.gradient_colors[1][n]) * t)
                    / 255.;
            }
            alpha = (f64::from(alpha)
                * (f64::from(self.gradient_colors[0][3]) * (1. - t)
                    + f64::from(self.gradient_colors[1][3]) * t)
                / 255.) as f32;
        }
        let distance = if self.vignette == 0. {
            0.
        } else {
            ((2. * (f64::from(x) + 0.5) / f64::from(size[0].max(1)) - 1.).powi(2)
                + (2. * (f64::from(yy) + 0.5) / f64::from(size[1].max(1)) - 1.).powi(2))
                * 0.5
        };
        for (n, value) in c.iter_mut().enumerate() {
            *value *= 1. - f64::from(self.vignette) * distance;
            *value += f64::from(noise(x, yy, 0)) * f64::from(self.grain)
                + f64::from(noise(x, yy, n as u32 + 1)) * f64::from(self.noise);
        }
        let c = c.map(crate::precision::float_channel);
        [c[0], c[1], c[2], alpha]
    }
    pub fn valid(&self) -> bool {
        [
            self.brightness,
            self.contrast,
            self.exposure,
            self.saturation,
            self.hue,
            self.temperature,
            self.tint,
            self.gamma,
            self.black,
            self.white,
            self.blur,
            self.sharpen,
            self.grain,
            self.noise,
            self.vignette,
            self.motion_angle,
        ]
        .iter()
        .all(|v| v.is_finite())
            && self.scope.as_ref().is_none_or(|s| s.valid())
            && (0.1..=5.0).contains(&self.gamma)
            && self.white > self.black
            && (0.0..=100.0).contains(&self.blur)
            && (0.0..=10.0).contains(&self.sharpen)
            && (-1.0..=1.0).contains(&self.brightness)
            && (-1.0..=2.0).contains(&self.contrast)
            && (-5.0..=5.0).contains(&self.exposure)
            && (-1.0..=2.0).contains(&self.saturation)
            && (-180.0..=180.0).contains(&self.hue)
            && (-1.0..=1.0).contains(&self.temperature)
            && (-1.0..=1.0).contains(&self.tint)
            && (0.0..=0.95).contains(&self.black)
            && (0.05..=1.0).contains(&self.white)
            && self.curves.iter().all(Curve::valid)
            && self.color_balance.valid()
            && self.color_mixer.valid()
            && self.color_grading.valid()
            && self.denoise.valid()
            && self.detail_sharpen.valid()
            && self.tonal_contrast.valid()
            && self.bloom.valid()
            && self.lens.valid()
            && self.geometry.valid()
            && self.lens_profile.as_ref().is_none_or(|p| p.valid())
            && (0.0..=1.0).contains(&self.grain)
            && (0.0..=1.0).contains(&self.noise)
            && (-1.0..=1.0).contains(&self.vignette)
            && self.motion_length <= 100
            && (-180.0..=180.0).contains(&self.motion_angle)
    }

    pub fn apply(&self, source: &RgbaImage) -> RgbaImage {
        let pixels = image::DynamicImage::ImageRgba8(source.clone()).into_rgba32f();
        crate::precision::preview(&self.apply_float(&pixels))
    }

    pub fn auto_levels(&mut self, source: &image::Rgba32FImage) {
        let mut counts = [0u64; 256];
        for (x, y, p) in source.enumerate_pixels().filter(|(_, _, p)| p[3] > 0.0) {
            let weight = self.scope.as_ref().map_or(1, |s| {
                (s.coverage([x as f32 + 0.5, y as f32 + 0.5]) * 65535.).round() as u64
            });
            if weight == 0 {
                continue;
            }
            let v = (0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]).clamp(0.0, 1.0);
            counts[(v * 255.0).round() as usize] += weight;
        }
        let total: u64 = counts.iter().sum();
        if total == 0 {
            return;
        }
        let cutoff = total / 200;
        let mut accumulated = 0;
        let low = counts
            .iter()
            .position(|v| {
                accumulated += v;
                accumulated > cutoff
            })
            .unwrap_or(0);
        accumulated = 0;
        let high = 255
            - counts
                .iter()
                .rev()
                .position(|v| {
                    accumulated += v;
                    accumulated > cutoff
                })
                .unwrap_or(0);
        if high > low {
            self.black = (low as f32 / 255.0).min(0.95);
            self.white = (high as f32 / 255.0).max(0.05).max(self.black + 0.01);
            self.gamma = 1.0;
        }
    }
}

fn motion_blur(source: &image::Rgba32FImage, length: u32, angle: f32) -> image::Rgba32FImage {
    if length <= 1 {
        return source.clone();
    }
    let (sin, cos) = angle.to_radians().sin_cos();
    image::Rgba32FImage::from_fn(source.width(), source.height(), |x, y| {
        let mut sum = [0.0_f64; 4];
        for n in 0..length {
            let t = n as f32 - (length - 1) as f32 * 0.5;
            let xx = (x as f32 + t * cos).round() as i32;
            let yy = (y as f32 + t * sin).round() as i32;
            if xx >= 0 && yy >= 0 && xx < source.width() as i32 && yy < source.height() as i32 {
                let p = source.get_pixel(xx as u32, yy as u32);
                let alpha = f64::from(p[3]);
                for c in 0..3 {
                    sum[c] += f64::from(p[c]) * alpha;
                }
                sum[3] += alpha;
            }
        }
        if sum[3] <= 0. {
            return Rgba([0.; 4]);
        }
        Rgba([
            crate::precision::float_channel(sum[0] / sum[3]),
            crate::precision::float_channel(sum[1] / sum[3]),
            crate::precision::float_channel(sum[2] / sum[3]),
            (sum[3] / f64::from(length)).clamp(0., 1.) as f32,
        ])
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn curves_auto_levels_and_filters_preserve_alpha_precision_and_source() {
        let source = image::Rgba32FImage::from_fn(5, 3, |x, _| Rgba([0.2 + x as f32 * 0.15; 4]));
        let unchanged = source.clone();
        let mut a = Adjustments::default();
        a.auto_levels(&source);
        let output = a.apply_float(&source);
        assert!(output.get_pixel(0, 1)[0].abs() < 0.0001);
        assert!((output.get_pixel(4, 1)[0] - 1.0).abs() < 0.0001);
        assert_eq!(output.get_pixel(2, 1)[3], source.get_pixel(2, 1)[3]);
        a = Adjustments::default();
        a.curves[0].points.insert(1, [0.5, 0.75]);
        let output = a.apply_float(&source);
        assert_eq!(output.get_pixel(2, 1)[0], 0.75);
        assert_eq!(a.curves[0].value(2.0), 1.5);
        a.curves[0].points.insert(1, [0.5, 0.8]);
        assert!(!a.valid());
        a = Adjustments {
            grain: 0.2,
            noise: 0.1,
            vignette: 0.5,
            ..Default::default()
        };
        assert_eq!(a.apply_float(&source), a.apply_float(&source));
        assert_eq!(source, unchanged);
        let mut impulse = image::Rgba32FImage::new(5, 3);
        *impulse.get_pixel_mut(2, 1) = Rgba([2.25, 0.0, 0.0, 1.0]);
        a = Adjustments {
            motion_length: 3,
            ..Default::default()
        };
        let blurred = a.apply_float(&impulse);
        assert_eq!(blurred.get_pixel(1, 1)[0], 2.25);
        assert!((blurred.get_pixel(1, 1)[3] - 1.0 / 3.0).abs() < 0.0001);
        a = Adjustments {
            gradient_map: true,
            gradient_colors: [[255, 0, 0, 128], [0, 0, 255, 128]],
            ..Default::default()
        };
        let mapped = a.apply_float(&image::Rgba32FImage::from_pixel(
            1,
            1,
            Rgba([0.5, 0.5, 0.5, 0.8]),
        ));
        assert_eq!(&mapped.get_pixel(0, 0).0[..3], &[0.5, 0.0, 0.5]);
        assert!((mapped.get_pixel(0, 0)[3] - 0.8 * 128.0 / 255.0).abs() < 0.00001);
    }
}

#[cfg(test)]
#[path = "hdr_adjustment_tests.rs"]
mod hdr_tests;

#[cfg(test)]
#[path = "spatial_hdr_tests.rs"]
mod spatial_hdr_tests;
