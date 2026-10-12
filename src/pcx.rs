// PCX layout/RLE follows the ZSoft Technical Reference Manual (Revision 5).
// https://techheap.packetizer.com/compression/graphics/pcxfmt.html
use image::{Rgba, RgbaImage};
use std::io::Write;
fn fail() -> String {
    "PCX 헤더·팔레트 또는 압축 데이터가 올바르지 않습니다.".into()
}
const EGA: [[u8; 3]; 16] = [
    [0, 0, 0],
    [0, 0, 170],
    [0, 170, 0],
    [0, 170, 170],
    [170, 0, 0],
    [170, 0, 170],
    [170, 85, 0],
    [170, 170, 170],
    [85, 85, 85],
    [85, 85, 255],
    [85, 255, 85],
    [85, 255, 255],
    [255, 85, 85],
    [255, 85, 255],
    [255, 255, 85],
    [255, 255, 255],
];
pub fn read(bytes: &[u8]) -> Result<RgbaImage, String> {
    if bytes.len() < 128 || bytes[0] != 10 || ![0, 2, 3, 4, 5].contains(&bytes[1]) || bytes[2] > 1 {
        return Err(fail());
    }
    let word = |i| u16::from_le_bytes([bytes[i], bytes[i + 1]]) as u32;
    let (xmin, ymin, xmax, ymax) = (word(4), word(6), word(8), word(10));
    if xmax < xmin || ymax < ymin {
        return Err(fail());
    }
    let (width, height) = (xmax - xmin + 1, ymax - ymin + 1);
    crate::document::validate_size(width, height)?;
    let (bits, planes) = (bytes[3] as usize, bytes[65] as usize);
    let rgb = bits == 8 && planes == 3;
    let indexed = matches!(bits, 1 | 2 | 4) && (1..=4).contains(&planes) && bits * planes <= 4;
    if !rgb && !indexed && !(bits == 8 && planes == 1) {
        return Err("이 PCX의 색 평면 조합은 아직 지원되지 않습니다.".into());
    }
    let stride = word(66) as usize;
    if stride < (width as usize * bits).div_ceil(8) {
        return Err(fail());
    }
    let row_len = stride.checked_mul(planes).ok_or_else(fail)?;
    if (row_len as u64) * u64::from(height) > crate::document::MAX_PIXELS * 4 {
        return Err("PCX 행 데이터가 처리 예산을 초과합니다.".into());
    }
    let mut palette = [[0u8; 3]; 256];
    let mut end = bytes.len();
    if bits == 8 && planes == 1 {
        if bytes.len() >= 128 + 769 && bytes[bytes.len() - 769] == 12 {
            end = bytes.len() - 769;
            for (index, color) in palette.iter_mut().enumerate() {
                color.copy_from_slice(&bytes[end + 1 + index * 3..end + 4 + index * 3]);
            }
        } else if word(68) == 2 {
            for (index, color) in palette.iter_mut().enumerate() {
                *color = [index as u8; 3];
            }
        } else {
            return Err("PCX의 256색 팔레트가 없습니다.".into());
        }
    } else if indexed {
        if bits == 2 && planes == 1 && bytes[1] <= 3 {
            let background = (bytes[16] >> 4) as usize;
            let flags = bytes[19];
            let mut indices = if flags & 0x80 != 0 {
                [3, 4, 7]
            } else if flags & 0x40 != 0 {
                [3, 5, 7]
            } else {
                [2, 4, 6]
            };
            if flags & 0x20 != 0 {
                for index in &mut indices {
                    *index += 8;
                }
            }
            palette[0] = EGA[background];
            for (i, index) in indices.into_iter().enumerate() {
                palette[i + 1] = EGA[index];
            }
        } else if bytes[1] == 0
            || bytes[1] == 3
            || (bits == 1 && planes == 1 && bytes[16..22].iter().all(|v| *v == 0))
        {
            palette[..16].copy_from_slice(&EGA);
            if bits == 1 && planes == 1 {
                palette[1] = [255; 3];
            }
        } else {
            for (index, color) in palette[..16].iter_mut().enumerate() {
                color.copy_from_slice(&bytes[16 + index * 3..19 + index * 3]);
            }
        }
    }
    let mut position = 128;
    let mut row = vec![0; row_len];
    let mut image = RgbaImage::new(width, height);
    for y in 0..height {
        let mut filled = 0;
        while filled < row_len {
            let value = *bytes
                .get(position)
                .filter(|_| position < end)
                .ok_or_else(fail)?;
            position += 1;
            let (count, value) = if bytes[2] == 1 && value & 0xC0 == 0xC0 {
                let count = (value & 63) as usize;
                let value = *bytes
                    .get(position)
                    .filter(|_| position < end)
                    .ok_or_else(fail)?;
                position += 1;
                (count, value)
            } else {
                (1, value)
            };
            if count == 0 || count > row_len - filled {
                return Err(fail());
            }
            row[filled..filled + count].fill(value);
            filled += count;
        }
        for x in 0..width as usize {
            let color = if rgb {
                [row[x], row[stride + x], row[2 * stride + x]]
            } else {
                let mut index = 0;
                for plane in 0..planes {
                    let value = row[plane * stride + (x * bits) / 8];
                    let shift = 8 - bits - (x * bits) % 8;
                    index |= ((value >> shift) as usize & ((1 << bits) - 1)) << (plane * bits);
                }
                palette[index]
            };
            image.put_pixel(x as u32, y, Rgba([color[0], color[1], color[2], 255]));
        }
    }
    Ok(image)
}
pub fn write(image: &RgbaImage, out: &mut impl Write) -> Result<(), String> {
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 || width > 65534 || height > 65536 {
        return Err("PCX 출력 크기는 가로 65534px·세로 65536px 이하여야 합니다.".into());
    }
    let stride = (width + 1) & !1;
    let mut header = [0u8; 128];
    header[0] = 10;
    header[1] = 5;
    header[2] = 1;
    header[3] = 8;
    header[65] = 3;
    for (offset, value) in [
        (8, width - 1),
        (10, height - 1),
        (12, 72),
        (14, 72),
        (66, stride),
        (68, 1),
    ] {
        header[offset..offset + 2].copy_from_slice(&(value as u16).to_le_bytes());
    }
    out.write_all(&header).map_err(|e| e.to_string())?;
    let mut row = vec![0; stride as usize * 3];
    let mut encoded = Vec::with_capacity(row.len() * 2);
    for y in 0..height {
        row.fill(0);
        for x in 0..width {
            let pixel = image.get_pixel(x, y).0;
            let alpha = u16::from(pixel[3]);
            for channel in 0..3 {
                row[channel * stride as usize + x as usize] =
                    ((u16::from(pixel[channel]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
            }
        }
        encoded.clear();
        let mut at = 0;
        while at < row.len() {
            let value = row[at];
            let mut count = 1;
            while count < 63 && at + count < row.len() && row[at + count] == value {
                count += 1;
            }
            if count > 1 || value >= 192 {
                encoded.push(0xC0 | count as u8);
            }
            encoded.push(value);
            at += count;
        }
        out.write_all(&encoded).map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn save(document: &crate::document::Document, path: &std::path::Path) -> Result<(), String> {
    let image = document.composite();
    crate::files::atomic_write(path, |file| write(&image, file))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn header(bits: u8, planes: u8, width: u16, stride: u16) -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[0] = 10;
        bytes[1] = 5;
        bytes[2] = 1;
        bytes[3] = bits;
        bytes[65] = planes;
        bytes[8..10].copy_from_slice(&(width - 1).to_le_bytes());
        bytes[66..68].copy_from_slice(&stride.to_le_bytes());
        for (i, color) in EGA.into_iter().enumerate() {
            bytes[16 + i * 3..19 + i * 3].copy_from_slice(&color);
        }
        bytes
    }
    #[test]
    fn planes_packed_palettes_padding_and_malformed_runs_use_correct_rows() {
        for bits in [1, 2, 4] {
            let mut bytes = header(bits, 1, 3, 2);
            let packed = match bits {
                1 => 0b01000000,
                2 => 0b00011000,
                _ => 0x01,
            };
            bytes.extend([packed, if bits == 4 { 0x20 } else { 0 }]);
            let image = read(&bytes).unwrap();
            assert_eq!(image.get_pixel(1, 0).0[..3], EGA[1]);
            assert_eq!(
                image.get_pixel(2, 0).0[..3],
                EGA[if bits == 1 { 0 } else { 2 }]
            );
        }
        let mut bytes = header(1, 4, 3, 2);
        bytes.extend([0x40, 0, 0x20, 0, 0, 0, 0, 0]);
        let image = read(&bytes).unwrap();
        assert_eq!(image.get_pixel(1, 0).0[..3], EGA[1]);
        assert_eq!(image.get_pixel(2, 0).0[..3], EGA[2]);
        let mut bytes = header(8, 1, 2, 2);
        bytes.extend([3, 8, 12]);
        let mut palette = vec![0u8; 768];
        palette[9..12].copy_from_slice(&[9, 40, 80]);
        palette[24..27].copy_from_slice(&[90, 70, 10]);
        bytes.extend(palette);
        let image = read(&bytes).unwrap();
        assert_eq!(image.get_pixel(0, 0).0, [9, 40, 80, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [90, 70, 10, 255]);
        for bad in [vec![0xC0, 4], vec![0xC3, 4], vec![0xC1]] {
            let mut bytes = header(8, 3, 1, 2);
            bytes.extend(bad);
            assert!(read(&bytes).is_err());
        }
        let mut bytes = header(8, 3, 1, 2);
        bytes.extend([0xC7, 2]);
        assert!(read(&bytes).is_err());
    }
    #[test]
    fn independent_pillow_files_and_rgb_export_preserve_colors() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/pcx");
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/pcx");
        std::fs::create_dir_all(&dir).unwrap();
        for mode in ["rgb", "palette", "mono", "gray"] {
            let path = fixtures.join(format!("pillow-{mode}.pcx"));
            let decoded = read(&std::fs::read(&path).unwrap()).unwrap();
            let expected = image::open(fixtures.join(format!("pillow-{mode}.png")))
                .unwrap()
                .to_rgba8();
            assert_eq!(decoded, expected);
            assert_eq!(
                crate::files::open(&path).unwrap().document.composite(),
                expected
            );
        }
        let image = RgbaImage::from_fn(5, 3, |x, y| {
            Rgba([
                x as u8 * 49,
                y as u8 * 80,
                207,
                if x == 0 { 128 } else { 255 },
            ])
        });
        let path = dir.join("rust-export.pcx");
        let document = crate::document::Document::from_image("pcx".into(), image.clone());
        save(&document, &path).unwrap();
        let decoded = read(&std::fs::read(&path).unwrap()).unwrap();
        for (p, q) in image.pixels().zip(decoded.pixels()) {
            let alpha = u16::from(p[3]);
            for c in 0..3 {
                assert_eq!(
                    q[c],
                    ((u16::from(p[c]) * alpha + 255 * (255 - alpha) + 127) / 255) as u8
                );
            }
            assert_eq!(q[3], 255);
        }
        decoded.save(dir.join("rust-export-expected.png")).unwrap();
        let before = std::fs::read(&path).unwrap();
        let invalid = RgbaImage::new(65535, 1);
        let doc = crate::document::Document::from_image("bad".into(), invalid);
        assert!(save(&doc, &path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}
