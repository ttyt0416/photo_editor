use image::{ImageBuffer, Luma, Rgba32FImage};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Effect {
    pub enabled: bool,
    pub color: [u8; 4],
    pub opacity: f32,
    pub radius: f32,
    pub offset: [i32; 2],
}
impl Default for Effect {
    fn default() -> Self {
        Self {
            enabled: false,
            color: [0, 0, 0, 255],
            opacity: 0.7,
            radius: 8.0,
            offset: [8, 8],
        }
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Effects {
    pub stroke: Effect,
    pub shadow: Effect,
    pub overlay: Effect,
    pub inner_shadow: Effect,
    pub outer_glow: Effect,
    pub inner_glow: Effect,
}
impl Effects {
    pub fn entries(&self) -> [(&str, &Effect); 6] {
        [
            ("외곽선", &self.stroke),
            ("드롭 섀도", &self.shadow),
            ("색상 오버레이", &self.overlay),
            ("내부 그림자", &self.inner_shadow),
            ("외부 글로우", &self.outer_glow),
            ("내부 글로우", &self.inner_glow),
        ]
    }
    pub fn enabled(&self) -> bool {
        self.entries().iter().any(|(_, e)| e.enabled)
    }
    pub fn padding(&self) -> u32 {
        self.entries()
            .iter()
            .filter(|(name, e)| e.enabled && *name != "색상 오버레이")
            .map(|(_, e)| {
                crate::precision::blur_padding(e.radius)
                    + e.offset[0].unsigned_abs().max(e.offset[1].unsigned_abs())
                    + 1
            })
            .max()
            .unwrap_or(0)
    }
    pub fn valid(&self) -> bool {
        self.entries().iter().all(|(_, e)| {
            e.opacity.is_finite()
                && (0.0..=1.0).contains(&e.opacity)
                && e.radius.is_finite()
                && (0.0..=100.0).contains(&e.radius)
                && e.offset.iter().all(|v| v.unsigned_abs() <= 200)
        })
    }
}

// Linear-time square dilation/erosion, shared by strokes and selection expansion.
pub fn extremum(
    values: &[f32],
    width: usize,
    height: usize,
    radius: usize,
    maximum: bool,
) -> Vec<f32> {
    fn pass(
        values: &[f32],
        width: usize,
        height: usize,
        radius: usize,
        maximum: bool,
        vertical: bool,
    ) -> Vec<f32> {
        let mut result = vec![0.0; values.len()];
        let (lines, len) = if vertical {
            (width, height)
        } else {
            (height, width)
        };
        for line in 0..lines {
            let at = |i: usize| {
                if vertical {
                    i * width + line
                } else {
                    line * width + i
                }
            };
            let mut queue = std::collections::VecDeque::<usize>::new();
            let mut added = 0;
            for i in 0..len {
                let right = (i + radius + 1).min(len);
                while added < right {
                    while queue.back().is_some_and(|j| {
                        if maximum {
                            values[at(*j)] <= values[at(added)]
                        } else {
                            values[at(*j)] >= values[at(added)]
                        }
                    }) {
                        queue.pop_back();
                    }
                    queue.push_back(added);
                    added += 1;
                }
                let left = i.saturating_sub(radius);
                while queue.front().is_some_and(|j| *j < left) {
                    queue.pop_front();
                }
                result[at(i)] = values[at(*queue.front().unwrap())];
            }
        }
        result
    }
    let horizontal = pass(values, width, height, radius, maximum, false);
    pass(&horizontal, width, height, radius, maximum, true)
}

pub fn apply(source: &Rgba32FImage, settings: &Effects) -> (Rgba32FImage, i32) {
    if !settings.enabled() {
        return (source.clone(), 0);
    }
    let pad = settings.padding();
    let width = source.width() + pad * 2;
    let height = source.height() + pad * 2;
    let mut image = Rgba32FImage::new(width, height);
    image::imageops::replace(&mut image, source, i64::from(pad), i64::from(pad));
    let alpha: Vec<f32> = image.pixels().map(|p| p[3].clamp(0.0, 1.0)).collect();
    let mut result = Rgba32FImage::new(width, height);
    let sample = |values: &[f32], x: i32, y: i32| {
        if x < 0 || y < 0 || x >= width as i32 || y >= height as i32 {
            0.0
        } else {
            values[y as usize * width as usize + x as usize]
        }
    };
    for (kind, e) in [
        (0, &settings.shadow),
        (1, &settings.outer_glow),
        (2, &settings.stroke),
    ] {
        if !e.enabled {
            continue;
        }
        let field = if kind == 2 {
            extremum(
                &alpha,
                width as usize,
                height as usize,
                e.radius.ceil() as usize,
                true,
            )
        } else {
            let map: ImageBuffer<Luma<f32>, Vec<f32>> =
                ImageBuffer::from_raw(width, height, alpha.clone()).unwrap();
            image::imageops::blur(&map, e.radius.max(0.1)).into_raw()
        };
        for (x, y, p) in result.enumerate_pixels_mut() {
            let value = if kind == 0 {
                sample(&field, x as i32 - e.offset[0], y as i32 - e.offset[1])
            } else {
                sample(&field, x as i32, y as i32)
            };
            let coverage = if kind == 2 {
                (value - alpha[y as usize * width as usize + x as usize]).max(0.0)
            } else {
                value
            };
            let mut color = e.color.map(|c| c as f32 / 255.0);
            color[3] *= coverage * e.opacity;
            p.0 = crate::precision::over(color, p.0, 1.0, crate::blend::BlendMode::Normal);
        }
    }
    for (kind, e) in [
        (0, &settings.overlay),
        (1, &settings.inner_shadow),
        (2, &settings.inner_glow),
    ] {
        if !e.enabled {
            continue;
        }
        let field = if kind == 0 {
            Vec::new()
        } else {
            let map: ImageBuffer<Luma<f32>, Vec<f32>> =
                ImageBuffer::from_raw(width, height, alpha.clone()).unwrap();
            image::imageops::blur(&map, e.radius.max(0.1)).into_raw()
        };
        for (x, y, p) in image.enumerate_pixels_mut() {
            let value = if kind == 0 {
                1.0
            } else if kind == 1 {
                1.0 - sample(&field, x as i32 - e.offset[0], y as i32 - e.offset[1])
            } else {
                1.0 - sample(&field, x as i32, y as i32)
            };
            let amount = (value * e.opacity * e.color[3] as f32 / 255.0).clamp(0.0, 1.0);
            for c in 0..3 {
                p[c] = p[c] * (1.0 - amount) + e.color[c] as f32 / 255.0 * amount;
            }
        }
    }
    for (source, target) in image.pixels().zip(result.pixels_mut()) {
        target.0 = crate::precision::over(source.0, target.0, 1.0, crate::blend::BlendMode::Normal);
    }
    (result, pad as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn shadow_and_stroke_expand_bounds_overlay_preserves_source_and_alpha() {
        let source = Rgba32FImage::from_pixel(2, 2, Rgba([2.0, 0.0, 0.0, 1.0]));
        let mut effects = Effects::default();
        effects.shadow.enabled = true;
        effects.shadow.radius = 1.0;
        effects.shadow.offset = [4, 0];
        effects.stroke.enabled = true;
        effects.stroke.radius = 1.0;
        effects.stroke.color = [0, 255, 0, 255];
        effects.stroke.opacity = 1.0;
        let (result, pad) = apply(&source, &effects);
        let p = pad as u32;
        assert_eq!(result.get_pixel(p, p)[0], 2.0);
        assert_eq!(result.get_pixel(p - 1, p)[1], 1.0);
        assert!(result.get_pixel(p + 4, p)[3] > 0.0);
        effects.overlay.enabled = true;
        effects.overlay.opacity = 1.0;
        effects.overlay.color = [0, 0, 255, 255];
        let (result, _) = apply(&source, &effects);
        assert_eq!(result.get_pixel(p, p).0, [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(source.get_pixel(0, 0)[0], 2.0);
    }
}
