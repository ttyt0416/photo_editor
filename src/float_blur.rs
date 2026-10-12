use image::{Rgba, Rgba32FImage};

/// Gaussian convolution in the current working RGB, with premultiplied f64 intermediates.
pub fn blur(source: &Rgba32FImage, sigma: f32) -> Rgba32FImage {
    if sigma <= 0. || !sigma.is_normal() || source.width() == 0 || source.height() == 0 {
        return source.clone();
    }
    // Retain the optimized existing convolution for ordinary values; wide/very small data
    // uses double intermediates to avoid overflow and alpha-premultiplication underflow.
    if sigma >= 0.2
        && source.pixels().all(|p| {
            (p[3] == 0. || (1e-4..=1.).contains(&p[3]))
                && p.0[..3]
                    .iter()
                    .all(|v| *v == 0. || (1e-20..=4096.).contains(&v.abs()))
        })
    {
        let mut premultiplied = source.clone();
        for p in premultiplied.pixels_mut() {
            for c in 0..3 {
                p[c] *= p[3];
            }
        }
        let mut output = image::imageops::blur(&premultiplied, sigma);
        for p in output.pixels_mut() {
            if p[3] > 0. {
                for c in 0..3 {
                    p[c] /= p[3];
                }
            }
        }
        return output;
    }
    // Match the installed image 0.25.10 kernel extent and clamped-border policy.
    let width = ((((sigma - 0.8) / 0.3 + 1.) * 2.) + 1.).max(3.) as usize;
    let width = if width.is_multiple_of(2) {
        width + 1
    } else {
        width
    };
    let radius = width / 2;
    let sigma = f64::from(sigma);
    let mut kernel: Vec<f64> = (0..width)
        .map(|i| {
            let d = i as f64 - radius as f64;
            (-0.5 * (d / sigma).powi(2)).exp()
        })
        .collect();
    let total = kernel.iter().sum::<f64>();
    for weight in &mut kernel {
        *weight /= total;
    }
    let (w, h) = (source.width() as usize, source.height() as usize);
    let mut horizontal = vec![[0f64; 4]; w * h];
    for y in 0..h {
        for x in 0..w {
            let sum = &mut horizontal[y * w + x];
            for (i, weight) in kernel.iter().enumerate() {
                let nx = (x as i64 + i as i64 - radius as i64).clamp(0, w as i64 - 1) as u32;
                let p = source.get_pixel(nx, y as u32).0;
                let alpha = f64::from(p[3]).clamp(0., 1.);
                let weight = alpha * weight;
                for c in 0..3 {
                    sum[c] += f64::from(p[c]) * weight;
                }
                sum[3] += weight;
            }
        }
    }
    Rgba32FImage::from_fn(w as u32, h as u32, |x, y| {
        let mut sum = [0f64; 4];
        for (i, weight) in kernel.iter().enumerate() {
            let ny = (i64::from(y) + i as i64 - radius as i64).clamp(0, h as i64 - 1) as usize;
            let p = horizontal[ny * w + x as usize];
            for c in 0..4 {
                sum[c] += p[c] * weight;
            }
        }
        if sum[3] <= 0. {
            return Rgba([0.; 4]);
        }
        Rgba([
            crate::precision::float_channel(sum[0] / sum[3]),
            crate::precision::float_channel(sum[1] / sum[3]),
            crate::precision::float_channel(sum[2] / sum[3]),
            sum[3].clamp(0., 1.) as f32,
        ])
    })
}

#[cfg(test)]
#[path = "float_blur_tests.rs"]
mod tests;
