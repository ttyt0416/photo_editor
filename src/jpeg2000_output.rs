use crate::{document::Document, files::ExportOptions};
use openjp2::openjpeg::{CODEC_FORMAT, COLOR_SPACE, opj_cparameters_t};
use openjp2::{Codec, Stream, opj_image, opj_image_comptparm};
use std::path::Path;
pub const PROGRESSION_LABELS: &[&str] = &[
    "화질 우선",
    "해상도 우선",
    "해상도·부분 영역 우선",
    "부분 영역 우선",
    "채널 우선",
];
pub const TILE_SIZES: &[u32] = &[0, 128, 256, 512, 1024, 2048, 4096];
pub const FORMATS: &[(&str, &str)] = &[("JPEG2000 JP2", "jp2"), ("JPEG2000 J2K", "j2k")];
pub fn depth(doc: &Document, options: &ExportOptions) -> u8 {
    if options.jp2_depth == 0 {
        if doc.layers.iter().any(|l| l.bit_depth > 8) {
            16
        } else {
            8
        }
    } else {
        options.jp2_depth
    }
}
pub fn save(
    doc: &Document,
    path: &Path,
    codestream: bool,
    options: &ExportOptions,
) -> Result<(), String> {
    if !TILE_SIZES.contains(&options.jp2_tile)
        || ![0, 8, 16].contains(&options.jp2_depth)
        || !(2..=256).contains(&options.jp2_ratio)
        || options.jp2_progression >= PROGRESSION_LABELS.len()
        || !(1..=100).contains(&options.jp2_layers)
    {
        return Err("JPEG2000 정밀도·압축 배율을 확인하세요.".into());
    }
    if !codestream && options.jp2_resolution_enabled && options.jp2_resolution == 0 {
        return Err("JP2 출력 해상도는1ppi 이상이어야 합니다.".into());
    }
    let xmp = if codestream {
        None
    } else {
        crate::xmp_output::prepare(doc, options.include_xmp)?
    };
    crate::document::validate_size(doc.width, doc.height)?;
    if options.jp2_tile != 0
        && u64::from(doc.width.div_ceil(options.jp2_tile))
            * u64::from(doc.height.div_ceil(options.jp2_tile))
            > 65535
    {
        return Err(
            "JPEG2000 타일 수가 형식의 한도를 초과합니다. 더 큰 타일 또는 분할 없음을 선택하세요."
                .into(),
        );
    }
    let pixels = crate::precision::composite(doc);
    if pixels.as_raw().iter().any(|v| !v.is_finite()) {
        return Err("JPEG2000 출력에 유효하지 않은 픽셀이 있습니다.".into());
    }
    let depth = depth(doc, options);
    let maximum = if depth == 16 { 65535. } else { 255. };
    let alpha = pixels.pixels().any(|p| p[3] < 1.);
    let component = opj_image_comptparm {
        dx: 1,
        dy: 1,
        w: doc.width,
        h: doc.height,
        prec: u32::from(depth),
        bpp: u32::from(depth),
        ..Default::default()
    };
    let mut image = opj_image::create(
        &vec![
            component;
            if options.jp2_grayscale {
                if alpha { 2 } else { 1 }
            } else if alpha {
                4
            } else {
                3
            }
        ],
        if options.jp2_grayscale {
            COLOR_SPACE::OPJ_CLRSPC_GRAY
        } else {
            COLOR_SPACE::OPJ_CLRSPC_SRGB
        },
    )
    .ok_or("JPEG2000 픽셀 버퍼를 만들 수 없습니다.")?;
    image.x1 = doc.width;
    image.y1 = doc.height;
    if !codestream && options.jp2_icc {
        let profile = if options.jp2_grayscale {
            crate::color_management::gray_profile()
        } else {
            crate::color_management::srgb_profile()
        };
        if !image.copy_icc_profile(openjp2::ICCProfileRef::ICC(profile)) {
            return Err("JP2 작업 색상 프로파일을 준비할 수 없습니다.".into());
        }
    }
    for (c, channel) in image
        .comps_mut()
        .ok_or("JPEG2000 채널이 없습니다.")?
        .iter_mut()
        .enumerate()
    {
        channel.alpha = u16::from(c == if options.jp2_grayscale { 1 } else { 3 });
        for (value, pixel) in channel
            .data_mut()
            .ok_or("JPEG2000 채널 버퍼가 없습니다.")?
            .iter_mut()
            .zip(pixels.pixels())
        {
            let mut pixel = *pixel;
            let value_channel = if options.jp2_grayscale {
                if c == 0 {
                    crate::psd::grayscale(&mut pixel);
                    0
                } else {
                    3
                }
            } else {
                c
            };
            *value = (pixel[value_channel].clamp(0., 1.) * maximum).round() as i32;
        }
    }
    // A single-row/column or small image must not request too many wavelet levels.
    // Bound levels by the smallest edge tile as well as the full image. An
    // otherwise valid 1-pixel edge tile must not fail the whole output.
    let smallest_edge = |length: u32| {
        if options.jp2_tile == 0 {
            length
        } else {
            let remainder = length % options.jp2_tile;
            if remainder == 0 {
                length.min(options.jp2_tile)
            } else {
                remainder
            }
        }
    };
    let numresolution = smallest_edge(doc.width)
        .min(smallest_edge(doc.height))
        .ilog2()
        .saturating_add(1)
        .min(6) as i32;
    let mut parameters = opj_cparameters_t {
        tile_size_on: i32::from(options.jp2_tile != 0),
        cp_tdx: options.jp2_tile as i32,
        cp_tdy: options.jp2_tile as i32,
        tcp_numlayers: i32::from(options.jp2_layers),
        prog_order: options.jp2_progression as i32,
        cp_disto_alloc: 1,
        numresolution,
        irreversible: i32::from(!options.jp2_lossless),
        tcp_mct: if options.jp2_grayscale { 0 } else { 1 },
        ..Default::default()
    };
    for i in 0..usize::from(options.jp2_layers) {
        let remaining = i32::from(options.jp2_layers) - 1 - i as i32;
        parameters.tcp_rates[i] = if options.jp2_lossless && remaining == 0 {
            0.
        } else {
            (if options.jp2_lossless {
                1.
            } else {
                f32::from(options.jp2_ratio)
            }) * 2f32.powi(remaining)
        };
    }
    let mut codec = Codec::new_encoder(if codestream {
        CODEC_FORMAT::OPJ_CODEC_J2K
    } else {
        CODEC_FORMAT::OPJ_CODEC_JP2
    })
    .ok_or("JPEG2000 인코더를 만들 수 없습니다.")?;
    if codec.setup_encoder(&mut parameters, &mut image) != 1 {
        return Err("JPEG2000 압축 설정을 적용할 수 없습니다.".into());
    }
    if !codestream
        && options.jp2_resolution_enabled
        && !codec.set_jp2_display_resolution_ppi(Some(options.jp2_resolution))
    {
        return Err("JP2 출력 해상도 정보를 설정할 수 없습니다.".into());
    }
    crate::files::atomic_write(path, |file| {
        let mut stream = Stream::new_writer(65536, file.try_clone().map_err(|e| e.to_string())?);
        if codec.start_compress(&mut image, &mut stream) != 1
            || codec.encode(&mut stream) != 1
            || codec.end_compress(&mut stream) != 1
        {
            return Err("JPEG2000 인코딩에 실패했습니다.".into());
        }
        stream.flush().map_err(|e| e.to_string())?;
        drop(stream);
        crate::jpeg2000_xmp::append(file, xmp.as_deref())
    })
}
#[cfg(test)]
#[path = "jpeg2000_output_tests.rs"]
mod tests;
