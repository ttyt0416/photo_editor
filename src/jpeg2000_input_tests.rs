use super::*;
use openjp2::openjpeg::{CODEC_FORMAT, COLOR_SPACE, opj_cparameters_t};
use openjp2::{Codec, Stream, opj_image, opj_image_comptparm};
use std::{path::Path, sync::Arc};
fn root() -> std::path::PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jpeg2000-input");
    std::fs::create_dir_all(&root).unwrap();
    root
}
fn encode(
    path: &Path,
    precisions: &[u32],
    signed: &[bool],
    subsampled: bool,
    space: COLOR_SPACE,
) -> Vec<Vec<i32>> {
    let components = precisions
        .iter()
        .enumerate()
        .map(|(c, precision)| opj_image_comptparm {
            dx: if subsampled && c == 1 { 2 } else { 1 },
            dy: 1,
            w: if subsampled && c == 1 { 4 } else { 7 },
            h: 5,
            prec: *precision,
            bpp: *precision,
            sgnd: u32::from(signed[c]),
            ..Default::default()
        })
        .collect::<Vec<_>>();
    let mut image = opj_image::create(&components, space).unwrap();
    image.x1 = 7;
    image.y1 = 5;
    let mut planes = vec![];
    for (c, component) in image.comps_mut().unwrap().iter_mut().enumerate() {
        component.alpha = u16::from(if matches!(space, COLOR_SPACE::OPJ_CLRSPC_CMYK) {
            c == 4 && precisions.len() == 5
        } else {
            c == if precisions.len() == 2 { 1 } else { 3 }
        });
        let maximum = (1i32 << precisions[c]) - 1;
        let shift = if signed[c] {
            1i32 << (precisions[c] - 1)
        } else {
            0
        };
        let values = component.data_mut().unwrap();
        for (i, value) in values.iter_mut().enumerate() {
            *value = match i % 7 {
                0 => 0,
                1 => maximum,
                _ => ((i as u32 * 397 + c as u32 * 719 + 193) % (maximum as u32 + 1)) as i32,
            } - shift;
        }
        planes.push(values.to_vec());
    }
    let mut parameters = opj_cparameters_t {
        tcp_numlayers: 1,
        cp_disto_alloc: 1,
        numresolution: 1,
        tcp_mct: 0,
        ..Default::default()
    };
    parameters.tcp_rates[0] = 0.;
    let mut codec = Codec::new_encoder(if path.extension().unwrap() == "jp2" {
        CODEC_FORMAT::OPJ_CODEC_JP2
    } else {
        CODEC_FORMAT::OPJ_CODEC_J2K
    })
    .unwrap();
    assert_eq!(codec.setup_encoder(&mut parameters, &mut image), 1);
    crate::files::atomic_write(path, |file| {
        let mut stream = Stream::new_writer(65536, file.try_clone().unwrap());
        assert_eq!(codec.start_compress(&mut image, &mut stream), 1);
        assert_eq!(codec.encode(&mut stream), 1);
        assert_eq!(codec.end_compress(&mut stream), 1);
        stream.flush().map_err(|e| e.to_string())
    })
    .unwrap();
    // The existing native writer has no eYCC enum mapping. Set the own
    // fixture's enumerated COLR field explicitly; coded samples stay intact.
    if matches!(
        space,
        COLOR_SPACE::OPJ_CLRSPC_EYCC | COLOR_SPACE::OPJ_CLRSPC_CMYK
    ) {
        let mut bytes = std::fs::read(path).unwrap();
        let at = bytes.windows(4).position(|v| v == b"colr").unwrap() + 7;
        let enumeration = if matches!(space, COLOR_SPACE::OPJ_CLRSPC_CMYK) {
            12u32
        } else {
            24u32
        };
        bytes[at..at + 4].copy_from_slice(&enumeration.to_be_bytes());
        if (matches!(space, COLOR_SPACE::OPJ_CLRSPC_CMYK) && precisions.len() == 5)
            || (matches!(space, COLOR_SPACE::OPJ_CLRSPC_EYCC) && precisions.len() == 4)
        {
            let count = precisions.len() as u16;
            let at = bytes.windows(4).position(|v| v == b"jp2h").unwrap() - 4;
            let length = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            let mut definition = (8u32 + 2 + u32::from(count) * 6).to_be_bytes().to_vec();
            definition.extend(b"cdef");
            definition.extend(count.to_be_bytes());
            for c in 0..count {
                definition.extend(c.to_be_bytes());
                definition.extend(u16::from(c == count - 1).to_be_bytes());
                definition.extend((if c == count - 1 { 0u16 } else { c + 1 }).to_be_bytes());
            }
            bytes[at..at + 4].copy_from_slice(&((length + definition.len()) as u32).to_be_bytes());
            bytes.splice(at + length..at + length, definition);
        }
        std::fs::write(path, bytes).unwrap();
    }
    planes
}
#[test]
fn jpeg2000_signed_unsigned_mixed_precision_endpoints_projects_output_and_source() {
    let root = root();
    let mut cases = vec![];
    let mut modes = vec![];
    for count in 1..=4 {
        for depth in [1, 4, 8, 12, 16] {
            for signed in [false, true] {
                modes.push((vec![depth; count], vec![signed; count]));
            }
        }
    }
    modes.push((vec![8, 16, 12, 4], vec![true, false, true, false]));
    modes.push((vec![4, 12], vec![false, true]));
    for (mode, (precisions, signed)) in modes.iter().enumerate() {
        for extension in ["jp2", "j2k"] {
            let name = format!("mode{mode}.{extension}");
            let path = root.join(&name);
            let source = encode(
                &path,
                precisions,
                signed,
                false,
                if precisions.len() < 3 {
                    COLOR_SPACE::OPJ_CLRSPC_GRAY
                } else {
                    COLOR_SPACE::OPJ_CLRSPC_SRGB
                },
            );
            let original = std::fs::read(&path).unwrap();
            let opened = crate::files::open(&path).unwrap();
            let depth = if precisions.iter().any(|p| *p > 8) {
                16
            } else {
                8
            };
            assert_eq!(opened.document.layers[0].bit_depth, depth);
            let pixels = crate::precision::composite(&opened.document);
            let imported = opened.document.layers[0]
                .precision
                .as_deref()
                .cloned()
                .unwrap_or_else(|| {
                    image::DynamicImage::ImageRgba8((*opened.document.layers[0].pixels).clone())
                        .into_rgba32f()
                });
            let maximum = if depth == 16 { 65535 } else { 255 };
            let mut expected = vec![];
            for (i, pixel) in imported.pixels().enumerate() {
                for c in 0..4 {
                    let channel = if c == 3 {
                        if precisions.len() == 2 {
                            Some(1)
                        } else if precisions.len() == 4 {
                            Some(3)
                        } else {
                            None
                        }
                    } else if precisions.len() < 3 {
                        Some(0)
                    } else {
                        Some(c)
                    };
                    let value = channel
                        .map(|channel| {
                            let limit = (1i64 << precisions[channel]) - 1;
                            let bias = if signed[channel] {
                                1i64 << (precisions[channel] - 1)
                            } else {
                                0
                            };
                            ((i64::from(source[channel][i]) + bias) * maximum / limit) as f32
                                / maximum as f32
                        })
                        .unwrap_or(1.);
                    assert!(
                        (pixel[c] - value).abs() <= 1e-7,
                        "{name}/{i}/{c}: {} != {value}",
                        pixel[c]
                    );
                    expected.extend(pixel[c].to_le_bytes());
                }
            }
            let raw = name.clone() + ".rgba32.le";
            std::fs::write(root.join(&raw), expected).unwrap();
            let before = opened.document.layers[0].precision.clone();
            let pixels_before = opened.document.layers[0].pixels.clone();
            let project = root.join(name.clone() + ".rphoto");
            crate::files::save_project(&opened.document, &project).unwrap();
            assert_eq!(
                crate::precision::composite(&crate::files::load_project(&project).unwrap()),
                pixels
            );
            let out = root.join(name.clone() + ".png");
            crate::files::export(
                &opened.document,
                &out,
                image::ImageFormat::Png,
                &Default::default(),
            )
            .unwrap();
            assert_eq!(
                crate::precision::composite(&crate::files::open(&out).unwrap().document),
                pixels
            );
            assert!(Arc::ptr_eq(
                &pixels_before,
                &opened.document.layers[0].pixels
            ));
            if let Some(before) = before {
                assert!(Arc::ptr_eq(
                    &before,
                    opened.document.layers[0].precision.as_ref().unwrap()
                ));
            }
            assert_eq!(std::fs::read(&path).unwrap(), original);
            cases.push(serde_json::json!({"file":name,"decoded":raw,"precisions":precisions,"signed":signed,"depth":depth,"size":[7,5]}));
        }
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn jpeg2000_channel_sampling_precision_and_color_errors_do_not_panic_or_replace_source() {
    let root = root().join("errors");
    std::fs::create_dir_all(&root).unwrap();
    for (name, precision, signed, subsample, space, error) in [
        (
            "subsampled",
            vec![8, 8, 8],
            vec![false; 3],
            true,
            COLOR_SPACE::OPJ_CLRSPC_SRGB,
            "샘플링",
        ),
        (
            "precision17",
            vec![17],
            vec![false],
            false,
            COLOR_SPACE::OPJ_CLRSPC_GRAY,
            "정밀도",
        ),
        (
            "five-channels",
            vec![8; 5],
            vec![false; 5],
            false,
            COLOR_SPACE::OPJ_CLRSPC_SRGB,
            "다중 채널",
        ),
        (
            "eycc-mixed-depth",
            vec![8, 12, 8],
            vec![false; 3],
            false,
            COLOR_SPACE::OPJ_CLRSPC_EYCC,
            "정밀도",
        ),
    ] {
        let path = root.join(format!("{name}.jp2"));
        encode(&path, &precision, &signed, subsample, space);
        let before = std::fs::read(&path).unwrap();
        assert!(
            crate::files::open(&path).err().unwrap().contains(error),
            "{name}"
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let truncated = root.join(format!("{name}-truncated.jp2"));
        std::fs::write(&truncated, &before[..before.len() / 2]).unwrap();
        assert!(crate::files::open(&truncated).is_err());
    }
    for precision in [1, 4, 8, 12, 16] {
        for signed in [false, true] {
            let maximum = (1i32 << precision) - 1;
            let minimum = if signed {
                -(1i32 << (precision - 1))
            } else {
                0
            };
            let high = minimum + maximum;
            for target in [255, 65535] {
                assert_eq!(scale(i32::MIN, precision, signed, target), 0);
                assert_eq!(scale(minimum, precision, signed, target), 0);
                assert_eq!(scale(high, precision, signed, target), target);
                assert_eq!(scale(i32::MAX, precision, signed, target), target);
            }
        }
    }
}

#[test]
fn jpeg2000_sycc_full_resolution_precision_alpha_rgb_editing_projects_and_exports() {
    let root = root().join("sycc");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for precision in [8, 12, 16] {
        for alpha in [false, true] {
            let precisions = vec![precision; if alpha { 4 } else { 3 }];
            let name = format!("depth{precision}-alpha{alpha}.jp2");
            let path = root.join(&name);
            encode(
                &path,
                &precisions,
                &vec![false; precisions.len()],
                false,
                COLOR_SPACE::OPJ_CLRSPC_SYCC,
            );
            let bytes = std::fs::read(&path).unwrap();
            let opened = crate::files::open(&path).unwrap();
            assert!(opened.notes.iter().any(|n| n.contains("sYCC")));
            let pixels = opened.document.layers[0]
                .precision
                .as_deref()
                .cloned()
                .unwrap_or_else(|| {
                    image::DynamicImage::ImageRgba8((*opened.document.layers[0].pixels).clone())
                        .into_rgba32f()
                });
            let expected = crate::precision::composite(&opened.document);
            let raw = name.clone() + ".rgba32.le";
            std::fs::write(
                root.join(&raw),
                pixels
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            assert!(
                pixels
                    .as_raw()
                    .iter()
                    .all(|v| v.is_finite() && *v >= 0. && *v <= 1.)
            );
            let depth = if precision > 8 { 16 } else { 8 };
            assert_eq!(opened.document.layers[0].bit_depth, depth);
            let project = root.join(name.clone() + ".rphoto");
            crate::files::save_project(&opened.document, &project).unwrap();
            assert_eq!(
                crate::precision::composite(&crate::files::load_project(&project).unwrap()),
                expected
            );
            let png = root.join(name.clone() + ".png");
            crate::files::export(
                &opened.document,
                &png,
                image::ImageFormat::Png,
                &Default::default(),
            )
            .unwrap();
            assert_eq!(
                crate::precision::composite(&crate::files::open(&png).unwrap().document),
                expected
            );
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
            cases.push(serde_json::json!({"file":name,"decoded":raw,"precision":precision,"depth":depth,"alpha":alpha}));
        }
    }
    for (name, precisions, signed, subsample) in [
        ("signed", vec![8; 3], vec![false, true, false], false),
        ("mixed", vec![8, 12, 8], vec![false; 3], false),
        ("subsampled", vec![8; 3], vec![false; 3], true),
    ] {
        let path = root.join(format!("unsupported-{name}.jp2"));
        encode(
            &path,
            &precisions,
            &signed,
            subsample,
            COLOR_SPACE::OPJ_CLRSPC_SYCC,
        );
        assert!(crate::files::open(&path).is_err());
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

fn encode_sycc_sampling(path: &Path, w: u32, h: u32, dy: u32, precision: u32, alpha: bool) {
    encode_sycc_sampling_origin(path, w, h, dy, precision, alpha, [0, 0]);
}

fn encode_sycc_sampling_origin(
    path: &Path,
    w: u32,
    h: u32,
    dy: u32,
    precision: u32,
    alpha: bool,
    origin: [u32; 2],
) {
    let [x0, y0] = origin;
    let parameters = (0..if alpha { 4 } else { 3 })
        .map(|c| {
            let (dx, dy) = if c == 1 || c == 2 { (2, dy) } else { (1, 1) };
            opj_image_comptparm {
                dx,
                dy,
                w: (x0 + w).div_ceil(dx) - x0.div_ceil(dx),
                h: (y0 + h).div_ceil(dy) - y0.div_ceil(dy),
                x0: x0.div_ceil(dx),
                y0: y0.div_ceil(dy),
                prec: precision,
                bpp: precision,
                ..Default::default()
            }
        })
        .collect::<Vec<_>>();
    let mut image = opj_image::tile_create(&parameters, COLOR_SPACE::OPJ_CLRSPC_SYCC).unwrap();
    image.x0 = x0;
    image.y0 = y0;
    image.x1 = x0 + w;
    image.y1 = y0 + h;
    let maximum = (1i32 << precision) - 1;
    for (c, component) in image.comps_mut().unwrap().iter_mut().enumerate() {
        component.alpha = u16::from(c == 3);
        if component.w == 0 || component.h == 0 {
            continue;
        }
        assert!(component.alloc_data());
        for (i, value) in component.data_mut().unwrap().iter_mut().enumerate() {
            *value = if i % 11 == 0 {
                0
            } else if i % 11 == 1 {
                maximum
            } else {
                ((i as u32 * 397 + c as u32 * 719 + 193) % (maximum as u32 + 1)) as i32
            };
        }
    }
    let mut parameters = opj_cparameters_t {
        tcp_numlayers: 1,
        cp_disto_alloc: 1,
        numresolution: 1,
        tcp_mct: 0,
        ..Default::default()
    };
    parameters.tcp_rates[0] = 0.;
    let mut codec = Codec::new_encoder(CODEC_FORMAT::OPJ_CODEC_JP2).unwrap();
    assert_eq!(codec.setup_encoder(&mut parameters, &mut image), 1);
    crate::files::atomic_write(path, |file| {
        let mut stream = Stream::new_writer(65536, file.try_clone().unwrap());
        assert_eq!(codec.start_compress(&mut image, &mut stream), 1);
        assert_eq!(codec.encode(&mut stream), 1);
        assert_eq!(codec.end_compress(&mut stream), 1);
        stream.flush().map_err(|e| e.to_string())
    })
    .unwrap();
}
#[test]
fn jpeg2000_sycc_422_420_odd_edges_tiny_rows_columns_alpha_precision_projects_and_png() {
    let root = root().join("sampling");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for (w, h) in [(7, 5), (8, 6), (1, 1), (1, 9), (9, 1), (65, 47)] {
        for dy in [1, 2] {
            for precision in [8, 16] {
                for alpha in [false, true] {
                    let name = format!("{w}x{h}-sampling2{dy}-depth{precision}-alpha{alpha}.jp2");
                    let path = root.join(&name);
                    encode_sycc_sampling(&path, w, h, dy, precision, alpha);
                    let bytes = std::fs::read(&path).unwrap();
                    let native = jpeg2k::Image::from_bytes(&bytes).unwrap();
                    assert_eq!(sycc_sampling(&bytes, native.components()).unwrap(), [2, dy]);
                    let opened = crate::files::open(&path).unwrap();
                    assert_eq!((opened.document.width, opened.document.height), (w, h));
                    assert_eq!(opened.document.layers[0].bit_depth, precision as u8);
                    let pixels = opened.document.layers[0]
                        .precision
                        .as_deref()
                        .cloned()
                        .unwrap_or_else(|| {
                            image::DynamicImage::ImageRgba8(
                                (*opened.document.layers[0].pixels).clone(),
                            )
                            .into_rgba32f()
                        });
                    let raw = name.clone() + ".rgba32.le";
                    std::fs::write(
                        root.join(&raw),
                        pixels
                            .as_raw()
                            .iter()
                            .flat_map(|v| v.to_le_bytes())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                    let expected = crate::precision::composite(&opened.document);
                    let project = root.join(name.clone() + ".rphoto");
                    crate::files::save_project(&opened.document, &project).unwrap();
                    assert_eq!(
                        crate::precision::composite(&crate::files::load_project(&project).unwrap()),
                        expected
                    );
                    let png = root.join(name.clone() + ".png");
                    crate::files::export(
                        &opened.document,
                        &png,
                        image::ImageFormat::Png,
                        &Default::default(),
                    )
                    .unwrap();
                    assert_eq!(
                        crate::precision::composite(&crate::files::open(&png).unwrap().document),
                        expected
                    );
                    assert_eq!(std::fs::read(&path).unwrap(), bytes);
                    cases.push(serde_json::json!({"file":name,"decoded":raw,"size":[w,h],"dy":dy,"precision":precision,"alpha":alpha}));
                }
            }
        }
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn jpeg2000_sampling_parser_extended_to_eof_codestream_and_malformed_headers_are_checked() {
    let root = root().join("sampling-boxes");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("normal.jp2");
    encode_sycc_sampling(&path, 7, 5, 2, 16, true);
    let bytes = std::fs::read(&path).unwrap();
    let native = jpeg2k::Image::from_bytes(&bytes).unwrap();
    let mut at = 0;
    while &bytes[at + 4..at + 8] != b"jp2c" {
        at += u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    }
    let raw = &bytes[at + 8..];
    assert_eq!(sycc_sampling(raw, native.components()).unwrap(), [2, 2]);
    let baseline = crate::precision::composite(&crate::files::open(&path).unwrap().document);
    for extended in [false, true] {
        let mut variant = bytes[..at].to_vec();
        variant.extend(if extended { 1u32 } else { 0u32 }.to_be_bytes());
        variant.extend(b"jp2c");
        if extended {
            variant.extend((raw.len() as u64 + 16).to_be_bytes());
        }
        variant.extend(raw);
        assert_eq!(
            sycc_sampling(&variant, native.components()).unwrap(),
            [2, 2]
        );
        let path = root.join(format!("extended{extended}.jp2"));
        std::fs::write(&path, &variant).unwrap();
        assert_eq!(
            crate::precision::composite(&crate::files::open(&path).unwrap().document),
            baseline
        );
    }
    for broken in [
        vec![],
        bytes[..7].to_vec(),
        bytes[..at + 7].to_vec(),
        vec![0xff, 0x4f, 0xff, 0x51, 0, 1],
    ] {
        assert!(sycc_sampling(&broken, native.components()).is_err());
    }
    for offset in [16, 20, 43, 46] {
        let mut broken = raw.to_vec();
        if offset < 40 {
            broken[offset..offset + 4].copy_from_slice(&1u32.to_be_bytes());
        } else {
            broken[offset] = 0;
        }
        assert!(sycc_sampling(&broken, native.components()).is_err());
    }
}

#[test]
fn jpeg2000_independent_ffmpeg_sampling_input_color_alpha_precision_tiles_and_source() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg2000-sampling");
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(manifest.as_array().unwrap().len(), 32);
    for case in manifest.as_array().unwrap() {
        let name = case["file"].as_str().unwrap();
        let path = root.join(name);
        let before = std::fs::read(&path).unwrap();
        let opened = crate::files::open(&path).unwrap_or_else(|error| panic!("{name}: {error}"));
        assert!(
            opened.notes.iter().any(|n| n.contains("sYCC")),
            "{name}: {:?}",
            opened.notes
        );
        assert_eq!(
            opened.document.width,
            case["size"][0].as_u64().unwrap() as u32
        );
        assert_eq!(
            opened.document.height,
            case["size"][1].as_u64().unwrap() as u32
        );
        assert_eq!(
            u64::from(opened.document.layers[0].bit_depth),
            case["precision"].as_u64().unwrap()
        );
        let reference = std::fs::read(root.join(case["reference"].as_str().unwrap())).unwrap();
        let actual = opened.document.layers[0]
            .precision
            .as_deref()
            .cloned()
            .unwrap_or_else(|| {
                image::DynamicImage::ImageRgba8((*opened.document.layers[0].pixels).clone())
                    .into_rgba32f()
            });
        assert_eq!(reference.len(), actual.as_raw().len() * 4);
        for (index, (actual, expected)) in actual
            .as_raw()
            .iter()
            .zip(reference.as_chunks::<4>().0)
            .enumerate()
        {
            let expected = f32::from_le_bytes(*expected);
            assert!(
                (*actual - expected).abs() <= 6e-8,
                "{name}/{index}: {actual} != {expected}"
            );
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
}

#[test]
fn jpeg2000_sycc_sampling_aligned_nonzero_origins_edges_alpha_projects_and_png() {
    let root = root().join("sampling-origins");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for origin in [[2, 2], [4, 6], [16, 12], [2, 1], [4, 7]] {
        for dy in [1, 2] {
            if origin[1] % dy != 0 {
                continue;
            }
            for (w, h) in [(7, 5), (8, 6), (1, 1), (1, 9), (9, 1)] {
                for precision in [8, 16] {
                    for alpha in [false, true] {
                        cases.push(verify_sycc_origin_case(
                            &root, origin, w, h, dy, precision, alpha,
                        ));
                    }
                }
            }
        }
    }
    assert_eq!(cases.len(), 160);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

fn verify_sycc_origin_case(
    root: &Path,
    origin: [u32; 2],
    w: u32,
    h: u32,
    dy: u32,
    precision: u32,
    alpha: bool,
) -> serde_json::Value {
    let name = format!(
        "{}-{}-{w}x{h}-2{dy}-{precision}-{alpha}.jp2",
        origin[0], origin[1]
    );
    let path = root.join(&name);
    encode_sycc_sampling_origin(&path, w, h, dy, precision, alpha, origin);
    let bytes = std::fs::read(&path).unwrap();
    let empty = (origin[0] + w).div_ceil(2) == origin[0].div_ceil(2)
        || (origin[1] + h).div_ceil(dy) == origin[1].div_ceil(dy);
    if empty {
        // Both Rust and the independent C decoder reject these encoded empty
        // components. Preserve this as an explicit codec limit, not RGB output.
        assert!(jpeg2k::Image::from_bytes(&bytes).is_err(), "{name}");
        assert!(crate::files::open(&path).is_err(), "{name}");
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        return serde_json::json!({"file":name,"size":[w,h],"origin":origin,"dy":dy,"precision":precision,"alpha":alpha,"empty":true});
    }
    let native = jpeg2k::Image::from_bytes(&bytes).unwrap();
    assert_eq!([native.x_offset(), native.y_offset()], origin);
    assert_eq!(sycc_sampling(&bytes, native.components()).unwrap(), [2, dy]);
    let opened = crate::files::open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
    assert_eq!(
        opened.notes.iter().any(|note| note.contains("시작 경계")),
        !origin[0].is_multiple_of(2) || !origin[1].is_multiple_of(dy)
    );
    assert_eq!((opened.document.width, opened.document.height), (w, h));
    assert_eq!(opened.document.layers[0].bit_depth, precision as u8);
    let layer = &opened.document.layers[0];
    let pixels =
        layer.precision.as_deref().cloned().unwrap_or_else(|| {
            image::DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f()
        });
    let raw = name.clone() + ".rgba32.le";
    std::fs::write(
        root.join(&raw),
        pixels
            .as_raw()
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let expected = crate::precision::composite(&opened.document);
    let project = root.join(name.clone() + ".rphoto");
    crate::files::save_project(&opened.document, &project).unwrap();
    assert_eq!(
        crate::precision::composite(&crate::files::load_project(&project).unwrap()),
        expected
    );
    let png = root.join(name.clone() + ".png");
    crate::files::export(
        &opened.document,
        &png,
        image::ImageFormat::Png,
        &Default::default(),
    )
    .unwrap();
    assert_eq!(
        crate::precision::composite(&crate::files::open(&png).unwrap().document),
        expected
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    serde_json::json!({"file":name,"decoded":raw,"size":[w,h],"origin":origin,"dy":dy,"precision":precision,"alpha":alpha,"empty":false})
}

#[test]
fn jpeg2000_sycc_sampling_odd_origins_zero_chroma_edges_alpha_projects_and_png() {
    let root = root().join("sampling-odd-origins");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for origin in [[1, 0], [1, 1], [2, 1], [3, 5], [17, 13]] {
        for dy in [1, 2] {
            if origin[0] % 2 == 0 && origin[1] % dy == 0 {
                continue;
            }
            for (w, h) in [(7, 5), (8, 6), (1, 1), (1, 9), (9, 1), (2, 2)] {
                for precision in [8, 16] {
                    for alpha in [false, true] {
                        cases.push(verify_sycc_origin_case(
                            &root, origin, w, h, dy, precision, alpha,
                        ));
                    }
                }
            }
        }
    }
    assert_eq!(cases.len(), 216);
    assert_eq!(
        cases.iter().filter(|case| case["empty"] == true).count(),
        84
    );
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

#[test]
fn jpeg2000_cmyk_mixed_precision_alpha_rgb_editing_project_png_and_source() {
    let root = root().join("cmyk");
    std::fs::create_dir_all(&root).unwrap();
    let mut modes = vec![];
    for precision in [1, 4, 8, 12, 16] {
        modes.push((vec![precision; 4], vec![false; 4]));
        modes.push((vec![precision; 5], vec![false; 5]));
        modes.push((vec![precision; 5], vec![false, false, false, false, true]));
    }
    modes.push((vec![8, 16, 12, 4], vec![false; 4]));
    modes.push((vec![8, 16, 12, 4, 12], vec![false; 5]));
    modes.push((
        vec![8, 16, 12, 4, 12],
        vec![false, false, false, false, true],
    ));
    let mut cases = vec![];
    for (i, (precisions, signed)) in modes.iter().enumerate() {
        let name = format!("mode{i}.jp2");
        let path = root.join(&name);
        encode(
            &path,
            precisions,
            signed,
            false,
            COLOR_SPACE::OPJ_CLRSPC_CMYK,
        );
        let before = std::fs::read(&path).unwrap();
        let native = jpeg2k::Image::from_bytes(&before).unwrap();
        assert!(matches!(native.color_space(), jpeg2k::ColorSpace::CMYK));
        if precisions.len() == 5 {
            assert!(native.components()[4].is_alpha());
        }
        let opened = crate::files::open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(
            opened
                .notes
                .iter()
                .any(|n| n.contains("CMYK") && n.contains("프로파일 없이"))
        );
        let depth = if *precisions.iter().max().unwrap() > 8 {
            16
        } else {
            8
        };
        assert_eq!(opened.document.layers[0].bit_depth, depth);
        let layer = &opened.document.layers[0];
        let pixels = layer.precision.as_deref().cloned().unwrap_or_else(|| {
            image::DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f()
        });
        let raw = name.clone() + ".rgba32.le";
        std::fs::write(
            root.join(&raw),
            pixels
                .as_raw()
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
        let expected = crate::precision::composite(&opened.document);
        let project = root.join(name.clone() + ".rphoto");
        crate::files::save_project(&opened.document, &project).unwrap();
        assert_eq!(
            crate::precision::composite(&crate::files::load_project(&project).unwrap()),
            expected
        );
        let png = root.join(name.clone() + ".png");
        crate::files::export(
            &opened.document,
            &png,
            image::ImageFormat::Png,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(
            crate::precision::composite(&crate::files::open(&png).unwrap().document),
            expected
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        cases.push(serde_json::json!({"file":name,"decoded":raw,"precisions":precisions,"signed":signed,"depth":depth}));
    }
    assert_eq!(cases.len(), 18);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

#[test]
fn jpeg2000_cmyk_invalid_count_signed_color_sampling_extra_alpha_and_source() {
    let root = root().join("cmyk-errors");
    std::fs::create_dir_all(&root).unwrap();
    for (name, count, signed, subsample) in [
        ("three", 3, false, false),
        ("six", 6, false, false),
        ("signed", 4, true, false),
        ("sampling", 4, false, true),
    ] {
        let path = root.join(format!("{name}.jp2"));
        encode(
            &path,
            &vec![8; count],
            &vec![signed; count],
            subsample,
            COLOR_SPACE::OPJ_CLRSPC_CMYK,
        );
        let before = std::fs::read(&path).unwrap();
        assert!(crate::files::open(&path).is_err(), "{name}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    let path = root.join("undefined-fifth.jp2");
    encode(
        &path,
        &[8; 5],
        &[false; 5],
        false,
        COLOR_SPACE::OPJ_CLRSPC_CMYK,
    );
    let mut bytes = std::fs::read(&path).unwrap();
    let at = bytes.windows(4).position(|v| v == b"cdef").unwrap() + 4;
    bytes[at + 2 + 4 * 6 + 2..at + 2 + 4 * 6 + 4].copy_from_slice(&0u16.to_be_bytes());
    std::fs::write(&path, &bytes).unwrap();
    assert!(crate::files::open(&path).err().unwrap().contains("투명도"));
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}

include!("jpeg2000_eycc_tests.rs");

include!("jpeg2000_icc_tests.rs");
