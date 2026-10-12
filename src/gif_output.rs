use crate::{document::Document, files::ExportOptions};
use std::{borrow::Cow, collections::BTreeSet, path::Path};

#[derive(Clone, Copy, Default)]
struct Bin {
    count: u64,
    sum: [u64; 3],
}
impl Bin {
    fn color(self) -> [u8; 3] {
        self.sum.map(|v| ((v + self.count / 2) / self.count) as u8)
    }
}
fn key(p: [u8; 3]) -> usize {
    ((p[0] as usize >> 3) << 10) | ((p[1] as usize >> 3) << 5) | (p[2] as usize >> 3)
}
fn nearest(color: [u8; 3], palette: &[[u8; 3]]) -> u8 {
    palette
        .iter()
        .enumerate()
        .min_by_key(|(_, p)| {
            (0..3)
                .map(|c| {
                    let d = i32::from(color[c]) - i32::from(p[c]);
                    d * d
                })
                .sum::<i32>()
        })
        .unwrap()
        .0 as u8
}
fn range(bins: &[Bin]) -> ([u8; 3], u64) {
    let mut lo = [255; 3];
    let mut hi = [0; 3];
    let mut count = 0;
    for b in bins {
        let p = b.color();
        count += b.count;
        for c in 0..3 {
            lo[c] = lo[c].min(p[c]);
            hi[c] = hi[c].max(p[c]);
        }
    }
    (std::array::from_fn(|c| hi[c] - lo[c]), count)
}
fn palette(pixels: &image::RgbaImage, capacity: usize) -> (Vec<[u8; 3]>, bool) {
    let mut unique = BTreeSet::new();
    for p in pixels.pixels().filter(|p| p[3] != 0) {
        unique.insert([p[0], p[1], p[2]]);
        if unique.len() > capacity {
            break;
        }
    }
    if unique.len() <= capacity {
        return (
            if unique.is_empty() {
                vec![[0; 3]]
            } else {
                unique.into_iter().collect()
            },
            true,
        );
    }
    // A bounded 5-bit histogram keeps palette construction independent of image area.
    let mut histogram = vec![Bin::default(); 32768];
    for p in pixels.pixels().filter(|p| p[3] != 0) {
        let b = &mut histogram[key([p[0], p[1], p[2]])];
        b.count += 1;
        for c in 0..3 {
            b.sum[c] += u64::from(p[c]);
        }
    }
    let mut boxes = vec![
        histogram
            .into_iter()
            .filter(|b| b.count > 0)
            .collect::<Vec<_>>(),
    ];
    while boxes.len() < capacity {
        let candidate = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.len() > 1)
            .max_by_key(|(_, b)| {
                let (r, n) = range(b);
                u64::from(*r.iter().max().unwrap()).pow(2) * n
            })
            .map(|(i, _)| i);
        let Some(i) = candidate else { break };
        let mut bins = boxes.swap_remove(i);
        let (r, n) = range(&bins);
        let axis = (0..3).max_by_key(|&c| r[c]).unwrap();
        bins.sort_unstable_by_key(|b| b.color()[axis]);
        let mut cumulative = 0;
        let mut split = 1;
        for (i, b) in bins.iter().enumerate() {
            cumulative += b.count;
            if cumulative >= n.div_ceil(2) {
                split = (i + 1).clamp(1, bins.len() - 1);
                break;
            }
        }
        let right = bins.split_off(split);
        boxes.push(bins);
        boxes.push(right);
    }
    (
        boxes
            .iter()
            .map(|bins| {
                let mut total = Bin::default();
                for b in bins {
                    total.count += b.count;
                    for c in 0..3 {
                        total.sum[c] += b.sum[c];
                    }
                }
                total.color()
            })
            .collect(),
        false,
    )
}
fn indexed(
    pixels: &image::RgbaImage,
    palette: &[[u8; 3]],
    exact: bool,
    transparent: Option<u8>,
    dither: bool,
) -> Vec<u8> {
    let width = pixels.width() as usize;
    let mut out = Vec::with_capacity(pixels.len() / 4);
    let mut current = vec![[0f32; 3]; width + 2];
    let mut next = current.clone();
    let mut lookup = vec![None; 32768];
    for y in 0..pixels.height() {
        for x in 0..width {
            let p = pixels.get_pixel(x as u32, y);
            if p[3] == 0 {
                out.push(transparent.unwrap());
                continue;
            }
            let adjusted: [f32; 3] = std::array::from_fn(|c| {
                (f32::from(p[c]) + if dither { current[x + 1][c] } else { 0. }).clamp(0., 255.)
            });
            let color = adjusted.map(|v| v.round() as u8);
            let index = if exact {
                palette
                    .binary_search(&color)
                    .unwrap_or_else(|_| usize::from(nearest(color, palette))) as u8
            } else {
                let bin = key(color);
                *lookup[bin].get_or_insert_with(|| {
                    nearest(
                        [
                            ((bin >> 10) * 8 + 4) as u8,
                            (((bin >> 5) & 31) * 8 + 4) as u8,
                            ((bin & 31) * 8 + 4) as u8,
                        ],
                        palette,
                    )
                })
            };
            out.push(index);
            if dither {
                for c in 0..3 {
                    let error = adjusted[c] - f32::from(palette[index as usize][c]);
                    current[x + 2][c] += error * 7. / 16.;
                    next[x][c] += error * 3. / 16.;
                    next[x + 1][c] += error * 5. / 16.;
                    next[x + 2][c] += error / 16.;
                }
            }
        }
        std::mem::swap(&mut current, &mut next);
        next.fill([0.; 3]);
    }
    out
}
pub fn save(doc: &Document, path: &Path, options: &ExportOptions) -> Result<(), String> {
    if !(2..=256).contains(&options.gif_colors) || options.gif_alpha == 0 {
        return Err("GIF 색상 수·투명도 경계를 확인하세요.".into());
    }
    let width = u16::try_from(doc.width).map_err(|_| "GIF의 가로는 65535px 이하여야 합니다.")?;
    let height = u16::try_from(doc.height).map_err(|_| "GIF의 세로는 65535px 이하여야 합니다.")?;
    let mut pixels = doc.composite();
    for p in pixels.pixels_mut() {
        if options.gif_transparent && p[3] < options.gif_alpha {
            p.0 = [0; 4];
            continue;
        }
        let alpha = u16::from(p[3]);
        for c in 0..3 {
            p[c] =
                ((u16::from(p[c]) * alpha + u16::from(options.background[c]) * (255 - alpha) + 127)
                    / 255) as u8;
        }
        p[3] = 255;
    }
    let has_transparency = pixels.pixels().any(|p| p[3] == 0);
    let capacity = usize::from(options.gif_colors) - usize::from(has_transparency);
    let (colors, exact) = palette(&pixels, capacity);
    let transparent = has_transparency.then_some(colors.len() as u8);
    let mut buffer = indexed(&pixels, &colors, exact, transparent, options.gif_dither);
    if options.gif_interlaced {
        let mut reordered = Vec::with_capacity(buffer.len());
        for (start, step) in [(0, 8), (4, 8), (2, 4), (1, 2)] {
            for y in (start..usize::from(height)).step_by(step) {
                reordered.extend_from_slice(
                    &buffer[y * usize::from(width)..(y + 1) * usize::from(width)],
                );
            }
        }
        buffer = reordered;
    }
    let mut table: Vec<u8> = colors.into_iter().flatten().collect();
    if has_transparency {
        table.extend([0; 3]);
    }
    let frame = gif::Frame {
        width,
        height,
        buffer: Cow::Owned(buffer),
        palette: Some(table),
        transparent,
        interlaced: options.gif_interlaced,
        ..Default::default()
    };
    crate::files::atomic_write(path, |file| {
        let mut encoder = gif::Encoder::new(file, width, height, &[]).map_err(|e| e.to_string())?;
        encoder.write_frame(&frame).map_err(|e| e.to_string())?;
        encoder.into_inner().map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn dir() -> std::path::PathBuf {
        let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/gif-output");
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    fn expected(p: image::Rgba<u8>, options: &ExportOptions) -> image::Rgba<u8> {
        if options.gif_transparent && p[3] < options.gif_alpha {
            return image::Rgba([0; 4]);
        }
        let alpha = u16::from(p[3]);
        image::Rgba([
            ((u16::from(p[0]) * alpha + 20 * (255 - alpha) + 127) / 255) as u8,
            ((u16::from(p[1]) * alpha + 30 * (255 - alpha) + 127) / 255) as u8,
            ((u16::from(p[2]) * alpha + 40 * (255 - alpha) + 127) / 255) as u8,
            255,
        ])
    }
    #[test]
    fn gif_palette_dither_alpha_interlace_and_precision_sources_roundtrip() {
        let dir = dir();
        let image = image::RgbaImage::from_fn(47, 33, |x, y| {
            image::Rgba([
                (x * 5) as u8,
                (y * 7) as u8,
                ((x + y) * 3) as u8,
                [0, 127, 128, 255][(x % 4) as usize],
            ])
        });
        image.save(dir.join("source.png")).unwrap();
        let doc = Document::from_image("GIF".into(), image);
        let original = doc.layers[0].pixels.clone();
        for count in [2, 16, 256] {
            for dither in [false, true] {
                for transparent in [false, true] {
                    for interlaced in [false, true] {
                        let options = ExportOptions {
                            gif_colors: count,
                            gif_dither: dither,
                            gif_transparent: transparent,
                            gif_interlaced: interlaced,
                            background: [20, 30, 40],
                            ..Default::default()
                        };
                        let path = dir.join(format!(
                            "c{count}-d{}-t{}-i{}.gif",
                            u8::from(dither),
                            u8::from(transparent),
                            u8::from(interlaced)
                        ));
                        crate::files::export(&doc, &path, image::ImageFormat::Gif, &options)
                            .unwrap();
                        let mut decoder = gif::DecodeOptions::new()
                            .read_info(std::fs::File::open(&path).unwrap())
                            .unwrap();
                        let frame = decoder.read_next_frame().unwrap().unwrap();
                        assert_eq!((frame.width, frame.height), (47, 33));
                        assert_eq!(frame.transparent.is_some(), transparent);
                        let used: BTreeSet<_> = frame.buffer.iter().copied().collect();
                        assert!(used.len() <= usize::from(count));
                        assert!(
                            frame.palette.as_ref().unwrap().len() / 3
                                <= usize::from(count.next_power_of_two())
                        );
                        let restored = crate::files::open(&path).unwrap().document.composite();
                        let mut sum = 0f64;
                        let mut samples = 0;
                        for (a, p) in restored.pixels().zip(doc.layers[0].pixels.pixels()) {
                            let b = expected(*p, &options);
                            assert_eq!(a[3], b[3]);
                            if b[3] > 0 {
                                sum += (0..3).map(|c| f64::from(a[c].abs_diff(b[c]))).sum::<f64>();
                                samples += 3;
                            }
                        }
                        if count == 256 {
                            assert!(
                                sum / f64::from(samples) < 10.,
                                "{} error {}",
                                path.display(),
                                sum / f64::from(samples)
                            );
                        }
                        assert!(decoder.read_next_frame().unwrap().is_none());
                    }
                }
            }
        }
        assert!(std::sync::Arc::ptr_eq(&original, &doc.layers[0].pixels));
        for threshold in [1, 127, 128, 255] {
            let options = ExportOptions {
                gif_alpha: threshold,
                background: [20, 30, 40],
                ..Default::default()
            };
            let path = dir.join(format!("threshold{threshold}.gif"));
            save(&doc, &path, &options).unwrap();
            let image = crate::files::open(&path).unwrap().document.composite();
            for (actual, p) in image.pixels().zip(doc.layers[0].pixels.pixels()) {
                assert_eq!(actual[3], expected(*p, &options)[3]);
            }
        }
        let all_transparent = Document::new(1, 9, true).unwrap();
        for interlaced in [false, true] {
            let path = dir.join(format!("transparent{}.gif", u8::from(interlaced)));
            save(
                &all_transparent,
                &path,
                &ExportOptions {
                    gif_colors: 2,
                    gif_interlaced: interlaced,
                    ..Default::default()
                },
            )
            .unwrap();
            assert!(
                crate::files::open(&path)
                    .unwrap()
                    .document
                    .composite()
                    .pixels()
                    .all(|p| p[3] == 0)
            );
        }
        let hdr = crate::precision::document(
            "HDR GIF".into(),
            image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
                16,
                8,
                image::Rgba([2., -0.25, 0.5, 0.75]),
            )),
        )
        .unwrap();
        let original = hdr.layers[0].precision.clone().unwrap();
        save(&hdr, &dir.join("hdr.gif"), &Default::default()).unwrap();
        assert!(std::sync::Arc::ptr_eq(
            &original,
            hdr.layers[0].precision.as_ref().unwrap()
        ));
    }
    #[test]
    fn gif_exact_colors_and_dither_reduce_local_ramp_error_without_transparency_collision() {
        let dir = dir();
        let doc = Document::from_image(
            "exact".into(),
            image::RgbaImage::from_fn(13, 11, |x, y| {
                image::Rgba(
                    [
                        [0, 0, 0, 0],
                        [0, 0, 0, 255],
                        [255, 0, 0, 255],
                        [0, 255, 0, 255],
                    ][((x + y) % 4) as usize],
                )
            }),
        );
        let path = dir.join("exact.gif");
        save(
            &doc,
            &path,
            &ExportOptions {
                gif_colors: 4,
                gif_dither: true,
                gif_interlaced: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            crate::files::open(&path).unwrap().document.composite(),
            doc.composite()
        );
        let ramp = Document::from_image(
            "ramp".into(),
            image::RgbaImage::from_fn(128, 64, |x, _| image::Rgba([((x * 255) / 127) as u8; 4])),
        );
        let mut ramp = ramp;
        let pixels = std::sync::Arc::make_mut(&mut ramp.layers[0].pixels);
        for p in pixels.pixels_mut() {
            p[3] = 255;
        }
        ramp.composite().save(dir.join("ramp.png")).unwrap();
        let mut errors = vec![];
        for dither in [false, true] {
            let path = dir.join(format!("ramp{}.gif", u8::from(dither)));
            save(
                &ramp,
                &path,
                &ExportOptions {
                    gif_colors: 2,
                    gif_dither: dither,
                    ..Default::default()
                },
            )
            .unwrap();
            let restored = crate::files::open(&path).unwrap().document.composite();
            let error = (32..96)
                .map(|x| {
                    let actual = (0..64)
                        .map(|y| f64::from(restored.get_pixel(x, y)[0]))
                        .sum::<f64>()
                        / 64.;
                    (actual - f64::from(ramp.layers[0].pixels.get_pixel(x, 0)[0])).abs()
                })
                .sum::<f64>();
            errors.push(error);
        }
        assert!(errors[1] < errors[0] * 0.2, "local ramp errors {errors:?}");
    }
    #[test]
    fn gif_invalid_options_and_dimensions_preserve_existing_output() {
        let dir = dir();
        let path = dir.join("previous.gif");
        std::fs::write(&path, b"previous").unwrap();
        let doc = Document::new(8, 8, true).unwrap();
        for options in [
            ExportOptions {
                gif_colors: 1,
                ..Default::default()
            },
            ExportOptions {
                gif_colors: 257,
                ..Default::default()
            },
            ExportOptions {
                gif_alpha: 0,
                ..Default::default()
            },
        ] {
            assert!(save(&doc, &path, &options).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), b"previous");
        }
        assert!(
            save(
                &Document::new(65536, 1, true).unwrap(),
                &path,
                &Default::default()
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"previous");
    }
}
