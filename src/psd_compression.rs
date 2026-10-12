use std::io::Write;
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub enum Compression {
    #[default]
    Raw,
    Rle,
    Zip,
    Prediction,
}
impl Compression {
    pub const ALL: [Self; 4] = [Self::Raw, Self::Rle, Self::Zip, Self::Prediction];
    pub fn label(self) -> &'static str {
        match self {
            Self::Raw => "무압축",
            Self::Rle => "RLE",
            Self::Zip => "ZIP",
            Self::Prediction => "ZIP 예측",
        }
    }
    fn code(self) -> u16 {
        match self {
            Self::Raw => 0,
            Self::Rle => 1,
            Self::Zip => 2,
            Self::Prediction => 3,
        }
    }
}
#[derive(Default)]
struct Counter(u64);
impl Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 += bytes.len() as u64;
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn pack(row: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(row.len() + row.len() / 128 + 2);
    let run = |at: usize| {
        let mut count = 1;
        while count < 128 && at + count < row.len() && row[at + count] == row[at] {
            count += 1;
        }
        count
    };
    let mut at = 0;
    while at < row.len() {
        let count = run(at);
        if count >= 3 {
            output.extend([(1 - count as i16) as u8, row[at]]);
            at += count;
        } else {
            let begin = at;
            at += count;
            while at < row.len() && at - begin < 128 {
                let count = run(at);
                if count >= 3 {
                    break;
                }
                at += (count).min(128 - (at - begin));
            }
            let count = at - begin;
            output.push((count - 1) as u8);
            output.extend(&row[begin..at]);
        }
    }
    output
}
fn row<I: Iterator<Item = f32>>(values: &mut I, width: u32, depth: u16) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(width as usize * depth as usize / 8);
    crate::psd::write_plane(&mut bytes, values.take(width as usize), depth)?;
    if bytes.len() != width as usize * depth as usize / 8 {
        return Err("PSD 채널의 픽셀 수가 일치하지 않습니다.".into());
    }
    Ok(bytes)
}
fn predict(row: &mut [u8], depth: u16) {
    if depth == 16 {
        let mut previous = 0u16;
        for sample in row.as_chunks_mut::<2>().0 {
            let current = u16::from_be_bytes(*sample);
            sample.copy_from_slice(&current.wrapping_sub(previous).to_be_bytes());
            previous = current;
        }
    } else {
        if depth == 32 {
            let original = row.to_vec();
            let width = row.len() / 4;
            for pixel in 0..width {
                for byte in 0..4 {
                    row[byte * width + pixel] = original[pixel * 4 + byte];
                }
            }
        }
        for index in (1..row.len()).rev() {
            row[index] = row[index].wrapping_sub(row[index - 1]);
        }
    }
}
pub fn channel<I: Iterator<Item = f32> + Clone>(
    out: &mut impl Write,
    mut values: I,
    size: [u32; 2],
    depth: u16,
    large: bool,
    compression: Compression,
) -> Result<(), String> {
    out.write_all(&compression.code().to_be_bytes())
        .map_err(|e| e.to_string())?;
    let [width, height] = size;
    match compression {
        Compression::Raw => crate::psd::write_plane(out, values, depth),
        Compression::Rle => {
            let mut first = values.clone();
            for _ in 0..height {
                let packed = pack(&row(&mut first, width, depth)?);
                if large {
                    out.write_all(&(packed.len() as u32).to_be_bytes())
                        .map_err(|e| e.to_string())?;
                } else {
                    let size=u16::try_from(packed.len()).map_err(|_|"PSD RLE 행 길이가 65535바이트를 초과합니다. ZIP 또는 PSB를 선택하세요.")?;
                    out.write_all(&size.to_be_bytes())
                        .map_err(|e| e.to_string())?;
                }
            }
            for _ in 0..height {
                out.write_all(&pack(&row(&mut values, width, depth)?))
                    .map_err(|e| e.to_string())?;
            }
            Ok(())
        }
        Compression::Zip | Compression::Prediction => {
            let mut zip = flate2::write::ZlibEncoder::new(out, flate2::Compression::default());
            if compression == Compression::Prediction {
                for _ in 0..height {
                    let mut bytes = row(&mut values, width, depth)?;
                    predict(&mut bytes, depth);
                    zip.write_all(&bytes).map_err(|e| e.to_string())?;
                }
            } else {
                crate::psd::write_plane(&mut zip, values, depth)?;
            }
            zip.finish().map_err(|e| e.to_string())?;
            Ok(())
        }
    }
}
pub fn size<I: Iterator<Item = f32> + Clone>(
    values: I,
    size: [u32; 2],
    depth: u16,
    large: bool,
    compression: Compression,
) -> Result<u64, String> {
    if compression == Compression::Raw {
        return Ok(2 + u64::from(size[0]) * u64::from(size[1]) * u64::from(depth / 8));
    }
    let mut count = Counter::default();
    channel(&mut count, values, size, depth, large, compression)?;
    Ok(count.0)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stream_compression_roundtrips_all_depths_psd_psb_and_run_boundaries() {
        for large in [false, true] {
            for depth in [8, 16, 32] {
                for compression in Compression::ALL {
                    let values: Vec<f32> = (0..260)
                        .map(|i| {
                            if i < 130 {
                                0.5
                            } else if depth == 32 {
                                i as f32 - 200.25
                            } else {
                                (i % 129) as f32 / 128.
                            }
                        })
                        .collect();
                    let mut bytes = Vec::new();
                    channel(
                        &mut bytes,
                        values.iter().copied(),
                        [130, 2],
                        depth,
                        large,
                        compression,
                    )
                    .unwrap();
                    assert_eq!(
                        size(values.iter().copied(), [130, 2], depth, large, compression).unwrap(),
                        bytes.len() as u64
                    );
                    let decoded = crate::psd::channel_depth(&bytes, 130, 2, large, depth).unwrap();
                    let mut expected = Vec::new();
                    crate::psd::write_plane(&mut expected, values.iter().copied(), depth).unwrap();
                    assert_eq!(decoded, expected);
                }
            }
        }
        let mut bytes = Vec::new();
        assert!(
            channel(
                &mut bytes,
                [f32::NAN].into_iter(),
                [1, 1],
                32,
                false,
                Compression::Zip
            )
            .is_err()
        );
        let mut bytes = Vec::new();
        let values = (0..30000).map(|i| f32::from_bits(0x3e800000 + i));
        assert!(channel(&mut bytes, values, [30000, 1], 32, false, Compression::Rle).is_err());
    }
}
