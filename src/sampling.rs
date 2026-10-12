use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sampling {
    #[default]
    Nearest,
    Bilinear,
    Bicubic,
}
impl Sampling {
    pub fn label(self) -> &'static str {
        match self {
            Self::Nearest => "최근접",
            Self::Bilinear => "양선형",
            Self::Bicubic => "고품질 쌍입방",
        }
    }
}

fn weights(point: [f32; 2], mode: Sampling, mut visit: impl FnMut(i32, i32, f32)) {
    if point.iter().any(|v| !v.is_finite() || v.abs() > 1e9) {
        return;
    }
    if mode == Sampling::Nearest {
        visit(point[0].floor() as i32, point[1].floor() as i32, 1.);
        return;
    }
    let p = point.map(|v| v - 0.5);
    let base = p.map(|v| v.floor() as i32);
    let t = [p[0] - base[0] as f32, p[1] - base[1] as f32];
    if mode == Sampling::Bilinear {
        for y in 0..2 {
            for x in 0..2 {
                visit(
                    base[0] + x,
                    base[1] + y,
                    if x == 0 { 1. - t[0] } else { t[0] } * if y == 0 { 1. - t[1] } else { t[1] },
                );
            }
        }
        return;
    }
    let cubic = |x: f32| {
        let x = x.abs();
        if x <= 1. {
            1.5 * x * x * x - 2.5 * x * x + 1.
        } else if x < 2. {
            -0.5 * x * x * x + 2.5 * x * x - 4. * x + 2.
        } else {
            0.
        }
    };
    for y in -1..=2 {
        for x in -1..=2 {
            visit(
                base[0] + x,
                base[1] + y,
                cubic(t[0] - x as f32) * cubic(t[1] - y as f32),
            );
        }
    }
}
pub fn rgba(point: [f32; 2], mode: Sampling, pixel: impl Fn(i32, i32) -> [f32; 4]) -> [f32; 4] {
    if mode == Sampling::Nearest {
        let mut out = [0.; 4];
        weights(point, mode, |x, y, _| out = pixel(x, y));
        return out;
    }
    let mut sum = [0f64; 4];
    weights(point, mode, |x, y, w| {
        let p = pixel(x, y);
        let alpha = f64::from(p[3]).clamp(0., 1.);
        let w = f64::from(w);
        for c in 0..3 {
            sum[c] += f64::from(p[c]) * alpha * w;
        }
        sum[3] += alpha * w;
    });
    if sum[3] <= 0. {
        return [0.; 4];
    }
    let rgb = std::array::from_fn::<_, 3, _>(|c| crate::precision::float_channel(sum[c] / sum[3]));
    [rgb[0], rgb[1], rgb[2], sum[3].clamp(0., 1.) as f32]
}
// Kept for optical callers; all float warps now share the same precise accumulation.
pub fn rgba_precise(
    point: [f32; 2],
    mode: Sampling,
    pixel: impl Fn(i32, i32) -> [f32; 4],
) -> [f32; 4] {
    rgba(point, mode, pixel)
}
pub fn gray(point: [f32; 2], mode: Sampling, pixel: impl Fn(i32, i32) -> f32) -> f32 {
    let mut value = 0.;
    weights(point, mode, |x, y, w| value += pixel(x, y) * w);
    value.clamp(0., 1.)
}

#[cfg(test)]
#[path = "sampling_tests.rs"]
mod tests;
