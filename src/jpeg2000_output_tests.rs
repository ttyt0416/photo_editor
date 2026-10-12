use super::*;
use std::sync::Arc;
fn output() -> std::path::PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jpeg2000-output");
    std::fs::create_dir_all(&path).unwrap();
    path
}
#[test]
fn jpeg2000_jp2_j2k_lossless_lossy_depth_alpha_small_images_hdr_source_and_projects() {
    let mut cases = vec![];
    for (width, height) in [(1, 1), (1, 9), (7, 5), (257, 65)] {
        for source_depth in [8, 16, 32] {
            let samples = image::Rgba32FImage::from_fn(width, height, |x, y| {
                let mut value = [
                    ((x * 73 + y * 19 + 11) % 256) as f32 / 255.,
                    ((x * 29 + y * 41 + 17) % 256) as f32 / 255.,
                    ((x * 31 + y * 13 + 101) % 256) as f32 / 255.,
                    if x % 7 == 0 {
                        0.
                    } else if x % 7 == 1 {
                        0.5
                    } else {
                        1.
                    },
                ];
                if source_depth == 16 {
                    value[0] = (x * 397 + y * 719 + 193) as f32 % 65536. / 65535.;
                    value[1] = (x * 907 + y * 367 + 89) as f32 % 65536. / 65535.;
                }
                if source_depth == 32 && x % 11 == 2 {
                    value[0] = 2.5;
                    value[1] = -0.125;
                }
                image::Rgba(value)
            });
            let mut doc = crate::precision::document(
                "JP2 고정밀".into(),
                match source_depth {
                    8 => image::DynamicImage::ImageRgba8(crate::precision::preview(&samples)),
                    16 => image::DynamicImage::ImageRgba16(crate::precision::image16(&samples)),
                    _ => image::DynamicImage::ImageRgba32F(samples),
                },
            )
            .unwrap();
            // Single-column cases exercise RGB as well as RGBA components.
            if width == 1 {
                Arc::make_mut(&mut doc.layers[0].pixels)
                    .pixels_mut()
                    .for_each(|p| p[3] = 255);
                if let Some(high) = &mut doc.layers[0].precision {
                    Arc::make_mut(high).pixels_mut().for_each(|p| p[3] = 1.);
                }
            }
            let source = doc.layers[0].precision.clone();
            let preview = doc.layers[0].pixels.clone();
            let composite = crate::precision::composite(&doc);
            let original =
                output().join(format!("source-{width}x{height}-{source_depth}.rgba32.le"));
            std::fs::write(
                &original,
                composite
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            for requested in [0, 8, 16] {
                for codestream in [false, true] {
                    for (lossless, ratio) in [(true, 8), (false, 2), (false, 8)] {
                        let options = ExportOptions {
                            jp2_depth: requested,
                            jp2_lossless: lossless,
                            jp2_ratio: ratio,
                            ..Default::default()
                        };
                        let suffix = if codestream { "j2k" } else { "jp2" };
                        let name = format!(
                            "{width}x{height}-src{source_depth}-depth{requested}-{}-{ratio}.{suffix}",
                            if lossless { "lossless" } else { "lossy" }
                        );
                        let path = output().join(&name);
                        let index =
                            crate::files::EXPORT_FORMATS.len() + if codestream { 4 } else { 3 };
                        crate::files::export_by_index(&doc, &path, index, &options)
                            .unwrap_or_else(|e| panic!("{name}: {e}"));
                        let decoded =
                            jpeg2k::Image::from_bytes(&std::fs::read(&path).unwrap()).unwrap();
                        let depth = depth(&doc, &options);
                        let max = if depth == 16 { 65535. } else { 255. };
                        assert!(
                            decoded
                                .components()
                                .iter()
                                .all(|c| c.precision() == u32::from(depth))
                        );
                        assert_eq!(decoded.num_components(), if width == 1 { 3 } else { 4 });
                        for (c, component) in decoded.components().iter().enumerate() {
                            assert_eq!((component.width(), component.height()), (width, height));
                            if lossless {
                                for (actual, p) in component.data().iter().zip(composite.pixels()) {
                                    assert_eq!(
                                        *actual,
                                        (p[c].clamp(0., 1.) * max).round() as i32,
                                        "{name}/{c}"
                                    );
                                }
                            }
                        }
                        let restored = crate::files::open(&path).unwrap().document;
                        assert_eq!(restored.layers[0].bit_depth, depth);
                        let floats = crate::precision::composite(&restored);
                        assert!(floats.as_raw().iter().all(|v| v.is_finite()));
                        let component_file = format!("{name}.components.le");
                        let comps = decoded.components();
                        let values: Vec<u8> = (0..(width * height) as usize)
                            .flat_map(|i| comps.iter().flat_map(move |c| c.data()[i].to_le_bytes()))
                            .collect();
                        std::fs::write(output().join(&component_file), values).unwrap();
                        cases.push(serde_json::json!({"file":name,"decoded":component_file,"source":original.file_name().unwrap().to_string_lossy(),"size":[width,height],"depth":depth,"lossless":lossless,"ratio":ratio,"channels":decoded.num_components()}));
                    }
                }
            }
            let project = output().join(format!("source-{width}x{height}-{source_depth}.rphoto"));
            crate::files::save_project(&doc, &project).unwrap();
            assert_eq!(
                crate::precision::composite(&crate::files::load_project(&project).unwrap()),
                composite
            );
            assert!(Arc::ptr_eq(&preview, &doc.layers[0].pixels));
            if let Some(source) = source {
                assert!(Arc::ptr_eq(
                    &source,
                    doc.layers[0].precision.as_ref().unwrap()
                ));
            }
        }
    }
    std::fs::write(
        output().join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn jpeg2000_invalid_options_nonfinite_source_and_failed_output_preserve_existing_files() {
    let mut doc = Document::new(2, 2, false).unwrap();
    let directory = output().join("errors");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("existing.jp2");
    std::fs::write(&path, b"original destination").unwrap();
    for options in [
        ExportOptions {
            jp2_depth: 32,
            ..Default::default()
        },
        ExportOptions {
            jp2_ratio: 1,
            ..Default::default()
        },
        ExportOptions {
            jp2_ratio: 257,
            ..Default::default()
        },
    ] {
        assert!(save(&doc, &path, false, &options).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original destination");
    }
    doc.layers[0].bit_depth = 32;
    doc.layers[0].precision = Some(Arc::new(image::Rgba32FImage::from_pixel(
        2,
        2,
        image::Rgba([f32::NAN, 0., 0., 1.]),
    )));
    assert!(save(&doc, &path, false, &Default::default()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"original destination");
    assert!(std::fs::read_dir(&directory).unwrap().all(|e| {
        !e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".photo-editor")
    }));
    assert!(
        crate::files::export_by_index(
            &doc,
            &path,
            crate::files::export_count(),
            &Default::default()
        )
        .is_err()
    );
}

#[test]
fn jpeg2000_progressive_packet_orders_quality_layers_full_and_partial_decode_source_unchanged() {
    let directory = output().join("progressive");
    std::fs::create_dir_all(&directory).unwrap();
    let image = image::RgbaImage::from_fn(64, 48, |x, y| {
        image::Rgba([
            ((x * 37 + y * 19 + 11) % 256) as u8,
            ((x * 23 + y * 71 + 13) % 256) as u8,
            ((x * 43 + y * 7 + 79) % 256) as u8,
            if x % 5 == 0 {
                0
            } else if x % 5 == 1 {
                127
            } else {
                255
            },
        ])
    });
    let doc = Document::from_image("quality layers".into(), image);
    let source = doc.layers[0].pixels.clone();
    let reference = doc.composite();
    reference.save(directory.join("source.png")).unwrap();
    let mut cases = vec![];
    for progression in 0..PROGRESSION_LABELS.len() {
        for layers in [1, 4, 100] {
            for codestream in [false, true] {
                for lossless in [false, true] {
                    let options = ExportOptions {
                        jp2_progression: progression,
                        jp2_layers: layers,
                        jp2_lossless: lossless,
                        jp2_depth: 8,
                        ..Default::default()
                    };
                    let name = format!(
                        "order{progression}-layers{layers}-{}-{}.{}",
                        if lossless { "lossless" } else { "lossy" },
                        8,
                        if codestream { "j2k" } else { "jp2" }
                    );
                    let path = directory.join(&name);
                    save(&doc, &path, codestream, &options)
                        .unwrap_or_else(|e| panic!("{name}: {e}"));
                    let data = std::fs::read(&path).unwrap();
                    let mut limits = vec![1, u32::from(layers).div_ceil(2), u32::from(layers)];
                    limits.sort_unstable();
                    limits.dedup();
                    for limit in limits {
                        let decoded = jpeg2k::Image::from_bytes_with(
                            &data,
                            jpeg2k::DecodeParameters::new().layers(limit),
                        )
                        .unwrap();
                        let comps = decoded.components();
                        assert_eq!(comps.len(), 4);
                        let bytes: Vec<u8> = (0..(64 * 48) as usize)
                            .flat_map(|i| {
                                comps.iter().map(move |c| c.data()[i].clamp(0, 255) as u8)
                            })
                            .collect();
                        let file = format!("{name}-quality{limit}.rgba8");
                        std::fs::write(directory.join(&file), &bytes).unwrap();
                        if limit == u32::from(layers) && lossless {
                            assert_eq!(&bytes, reference.as_raw(), "{name}");
                        }
                        cases.push(serde_json::json!({"file":name,"output":file,"progression":progression,"layers":layers,"limit":limit,"lossless":lossless}));
                    }
                }
            }
        }
    }
    assert!(Arc::ptr_eq(&source, &doc.layers[0].pixels));
    let independent =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg2000-progressive");
    let entries: serde_json::Value =
        serde_json::from_slice(&std::fs::read(independent.join("manifest.json")).unwrap()).unwrap();
    for case in entries.as_array().unwrap() {
        let input = independent.join(case["file"].as_str().unwrap());
        let raw = std::fs::read(&input).unwrap();
        let decoded = jpeg2k::Image::from_bytes_with(
            &raw,
            jpeg2k::DecodeParameters::new().layers(case["limit"].as_u64().unwrap() as u32),
        )
        .unwrap();
        let comps = decoded.components();
        assert_eq!(comps.len(), 4);
        let actual: Vec<u8> = (0..(65 * 47) as usize)
            .flat_map(|i| comps.iter().map(move |c| c.data()[i].clamp(0, 255) as u8))
            .collect();
        assert_eq!(
            actual,
            std::fs::read(independent.join(case["output"].as_str().unwrap())).unwrap(),
            "{}",
            case
        );
    }
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn jpeg2000_invalid_progression_and_quality_layers_keep_destination() {
    let doc = Document::new(2, 2, false).unwrap();
    let directory = output().join("progressive-errors");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("unchanged.jp2");
    std::fs::write(&path, b"original file").unwrap();
    for options in [
        ExportOptions {
            jp2_progression: 5,
            ..Default::default()
        },
        ExportOptions {
            jp2_layers: 0,
            ..Default::default()
        },
        ExportOptions {
            jp2_layers: 101,
            ..Default::default()
        },
    ] {
        assert!(save(&doc, &path, false, &options).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"original file");
    }
}

#[test]
fn jpeg2000_gray_alpha_precision_modes_lossless_lossy_projects_raw_two_channel_input_and_source() {
    let directory = output().join("gray");
    std::fs::create_dir_all(&directory).unwrap();
    let mut cases = vec![];
    for source_depth in [8, 16, 32] {
        for neutral in [false, true] {
            if neutral && source_depth != 16 {
                continue;
            }
            for alpha in [false, true] {
                let (w, h) = if neutral {
                    (1, 9)
                } else if source_depth == 16 {
                    (65, 47)
                } else if source_depth == 32 {
                    (9, 1)
                } else {
                    (7, 5)
                };
                let pixels = image::Rgba32FImage::from_fn(w, h, |x, y| {
                    let r = ((x * 719 + y * 397 + 193) % 65536) as f32 / 65535.;
                    image::Rgba([
                        if source_depth == 32 && x % 4 == 2 {
                            2.5
                        } else {
                            r
                        },
                        if neutral {
                            r
                        } else if source_depth == 32 && x % 4 == 2 {
                            -0.125
                        } else {
                            ((x * 907 + y * 367 + 89) % 65536) as f32 / 65535.
                        },
                        if neutral {
                            r
                        } else {
                            ((x * 113 + y * 701 + 25173) % 65536) as f32 / 65535.
                        },
                        if !alpha {
                            1.
                        } else if (x + y) % 4 == 0 {
                            0.
                        } else if (x + y) % 4 == 1 {
                            0.5
                        } else {
                            0.875
                        },
                    ])
                });
                let doc = crate::precision::document(
                    "회색조 출력".into(),
                    match source_depth {
                        8 => image::DynamicImage::ImageRgba8(crate::precision::preview(&pixels)),
                        16 => image::DynamicImage::ImageRgba16(crate::precision::image16(&pixels)),
                        _ => image::DynamicImage::ImageRgba32F(pixels),
                    },
                )
                .unwrap();
                let original = doc.layers[0].precision.clone();
                let before = doc.layers[0].pixels.clone();
                let source = crate::precision::composite(&doc);
                let stem = format!("src{source_depth}-neutral{neutral}-alpha{alpha}");
                let raw = stem.clone() + ".rgba32.le";
                std::fs::write(
                    directory.join(&raw),
                    source
                        .as_raw()
                        .iter()
                        .flat_map(|v| v.to_le_bytes())
                        .collect::<Vec<_>>(),
                )
                .unwrap();
                for requested in [0, 8, 16] {
                    for codestream in [false, true] {
                        for lossless in [false, true] {
                            let options = ExportOptions {
                                jp2_grayscale: true,
                                jp2_depth: requested,
                                jp2_lossless: lossless,
                                jp2_layers: 4,
                                jp2_progression: 2,
                                ..Default::default()
                            };
                            let name = format!(
                                "{stem}-depth{requested}-lossless{lossless}.{}",
                                if codestream { "j2k" } else { "jp2" }
                            );
                            let path = directory.join(&name);
                            save(&doc, &path, codestream, &options).unwrap();
                            let bytes = std::fs::read(&path).unwrap();
                            let decoded = jpeg2k::Image::from_bytes(&bytes).unwrap();
                            let comps = decoded.components();
                            assert_eq!(comps.len(), if alpha { 2 } else { 1 });
                            let depth = depth(&doc, &options);
                            let maximum = if depth == 16 { 65535. } else { 255. };
                            let mut values = vec![];
                            for (i, pixel) in source.pixels().enumerate() {
                                let mut gray = *pixel;
                                crate::psd::grayscale(&mut gray);
                                for (c, component) in comps.iter().enumerate() {
                                    let expected =
                                        (gray[if c == 0 { 0 } else { 3 }].clamp(0., 1.) * maximum)
                                            .round() as i32;
                                    if lossless {
                                        assert_eq!(component.data()[i], expected, "{name}/{i}/{c}");
                                    }
                                    values.extend(component.data()[i].to_le_bytes());
                                }
                            }
                            let components = name.clone() + ".components.le";
                            std::fs::write(directory.join(&components), values).unwrap();
                            let opened = crate::files::open(&path).unwrap();
                            assert_eq!(opened.document.layers[0].bit_depth, depth);
                            if alpha && codestream {
                                assert!(opened.notes.iter().any(|n| n.contains("밝기·투명도")));
                            }
                            let restored = crate::precision::composite(&opened.document);
                            assert!(restored.pixels().all(|p| p[0].to_bits() == p[1].to_bits()
                                && p[1].to_bits() == p[2].to_bits()));
                            if lossless {
                                for (p, gray) in restored.pixels().zip(source.pixels()) {
                                    assert!(p.0.iter().all(|v| v.is_finite()));
                                    assert!((p[3] - gray[3].clamp(0., 1.)).abs() <= 1. / maximum);
                                }
                            }
                            cases.push(serde_json::json!({"file":name,"decoded":components,"source":raw,"size":[w,h],"depth":depth,"lossless":lossless,"channels":comps.len()}));
                        }
                    }
                }
                let path = directory.join(format!("{stem}.rphoto"));
                crate::files::save_project(&doc, &path).unwrap();
                assert_eq!(
                    crate::precision::composite(&crate::files::load_project(&path).unwrap()),
                    source
                );
                assert!(Arc::ptr_eq(&before, &doc.layers[0].pixels));
                if let Some(original) = original {
                    assert!(Arc::ptr_eq(
                        &original,
                        doc.layers[0].precision.as_ref().unwrap()
                    ));
                }
            }
        }
    }
    std::fs::write(
        directory.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

#[test]
fn jpeg2000_tiled_edges_depth_color_alpha_progression_layers_decoded_pixels_and_source() {
    let root = output().join("tiled");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for (w, h, tile) in [
        (257, 259, 128),
        (256, 256, 128),
        (259, 131, 256),
        (1, 129, 128),
        (129, 1, 512),
        (7, 5, 4096),
        (3, 5, 1024),
        (5, 3, 2048),
    ] {
        for depth in [8, 16] {
            let pixels = image::Rgba32FImage::from_fn(w, h, |x, y| {
                image::Rgba([
                    ((x * 719 + y * 397 + 193) % 65536) as f32 / 65535.,
                    ((x * 907 + y * 367 + 89) % 65536) as f32 / 65535.,
                    ((x * 113 + y * 701 + 25173) % 65536) as f32 / 65535.,
                    [0., 0.5, 0.875, 1.][((x + y) % 4) as usize],
                ])
            });
            let doc = crate::precision::document(
                "타일 출력".into(),
                if depth == 8 {
                    image::DynamicImage::ImageRgba8(crate::precision::preview(&pixels))
                } else {
                    image::DynamicImage::ImageRgba16(crate::precision::image16(&pixels))
                },
            )
            .unwrap();
            let before = doc.layers[0].pixels.clone();
            let precision = doc.layers[0].precision.clone();
            let source = crate::precision::composite(&doc);
            let stem = format!("{w}x{h}-depth{depth}");
            let raw = stem.clone() + ".rgba32.le";
            std::fs::write(
                root.join(&raw),
                source
                    .as_raw()
                    .iter()
                    .flat_map(|v| v.to_le_bytes())
                    .collect::<Vec<_>>(),
            )
            .unwrap();
            for gray in [false, true] {
                for codestream in [false, true] {
                    for lossless in [false, true] {
                        let options = ExportOptions {
                            jp2_tile: tile,
                            jp2_depth: depth,
                            jp2_grayscale: gray,
                            jp2_lossless: lossless,
                            jp2_layers: 4,
                            jp2_progression: 3,
                            ..Default::default()
                        };
                        let name = format!(
                            "{stem}-tile{tile}-gray{gray}-lossless{lossless}.{}",
                            if codestream { "j2k" } else { "jp2" }
                        );
                        let path = root.join(&name);
                        save(&doc, &path, codestream, &options).unwrap();
                        let bytes = std::fs::read(&path).unwrap();
                        let mut decoded_names = vec![];
                        for layers in [1, 0] {
                            let decoded = jpeg2k::Image::from_bytes_with(
                                &bytes,
                                jpeg2k::DecodeParameters::new().layers(layers),
                            )
                            .unwrap();
                            let components = decoded.components();
                            assert_eq!(components.len(), if gray { 2 } else { 4 });
                            let maximum = if depth == 16 { 65535. } else { 255. };
                            let mut values = vec![];
                            for (index, pixel) in source.pixels().enumerate() {
                                let mut pixel = *pixel;
                                if gray {
                                    crate::psd::grayscale(&mut pixel);
                                }
                                for (c, component) in components.iter().enumerate() {
                                    let value = component.data()[index];
                                    if layers == 0 && lossless {
                                        assert_eq!(
                                            value,
                                            (pixel[if gray && c == 1 { 3 } else { c }]
                                                .clamp(0., 1.)
                                                * maximum)
                                                .round()
                                                as i32,
                                            "{name}/{index}/{c}"
                                        );
                                    }
                                    values.extend(value.to_le_bytes());
                                }
                            }
                            let raw = name.clone() + &format!(".layers{layers}.components.le");
                            std::fs::write(root.join(&raw), values).unwrap();
                            decoded_names.push(serde_json::json!({"layers":layers,"file":raw}));
                        }
                        let opened = crate::files::open(&path).unwrap();
                        assert_eq!(opened.document.layers[0].bit_depth, depth);
                        assert_eq!((opened.document.width, opened.document.height), (w, h));
                        cases.push(serde_json::json!({"file":name,"decoded":decoded_names,"source":raw,"size":[w,h],"tile":tile,"depth":depth,"lossless":lossless,"gray":gray,"channels":if gray{2}else{4}}));
                    }
                }
            }
            assert!(Arc::ptr_eq(&before, &doc.layers[0].pixels));
            if let Some(precision) = precision {
                assert!(Arc::ptr_eq(
                    &precision,
                    doc.layers[0].precision.as_ref().unwrap()
                ));
            }
            let project = root.join(format!("{stem}.rphoto"));
            crate::files::save_project(&doc, &project).unwrap();
            assert_eq!(
                crate::precision::composite(&crate::files::load_project(&project).unwrap()),
                source
            );
        }
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn jpeg2000_invalid_tile_and_tile_count_preserve_destination_before_compositing() {
    let root = output().join("tile-errors");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("unchanged.jp2");
    std::fs::write(&path, b"original").unwrap();
    let mut doc = Document::new(2, 2, false).unwrap();
    for tile in [1, 127, 129, 8192, u32::MAX] {
        assert!(
            save(
                &doc,
                &path,
                false,
                &ExportOptions {
                    jp2_tile: tile,
                    ..Default::default()
                }
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"original");
    }
    doc.width = 65535 * 128 + 1;
    doc.height = 1;
    assert!(
        save(
            &doc,
            &path,
            false,
            &ExportOptions {
                jp2_tile: 128,
                ..Default::default()
            }
        )
        .unwrap_err()
        .contains("타일 수")
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
}

#[test]
fn jpeg2000_display_resolution_metadata_options_codestream_unchanged_source_and_atomic_errors() {
    let root = output().join("resolution");
    std::fs::create_dir_all(&root).unwrap();
    let doc = crate::precision::document(
        "출력 해상도".into(),
        image::DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(17, 11, |x, y| {
            image::Rgba([
                (x * 1987 + y * 131) as u16,
                (x * 271 + y * 4091) as u16,
                (x * 991 + y * 1877) as u16,
                if x % 3 == 0 { 32768 } else { 65535 },
            ])
        })),
    )
    .unwrap();
    let before = doc.layers[0].precision.clone().unwrap();
    let mut cases = vec![];
    for gray in [false, true] {
        for lossless in [false, true] {
            let base = ExportOptions {
                jp2_grayscale: gray,
                jp2_lossless: lossless,
                jp2_layers: 4,
                jp2_tile: 128,
                ..Default::default()
            };
            let original = root.join(format!("base-gray{gray}-lossless{lossless}.j2k"));
            save(&doc, &original, true, &base).unwrap();
            let baseline = std::fs::read(&original).unwrap();
            for ppi in [1, 72, 300, 9600, 65535] {
                for enabled in [false, true] {
                    let options = ExportOptions {
                        jp2_resolution_enabled: enabled,
                        jp2_resolution: ppi,
                        ..base.clone()
                    };
                    let name =
                        format!("ppi{ppi}-enabled{enabled}-gray{gray}-lossless{lossless}.jp2");
                    let path = root.join(&name);
                    save(&doc, &path, false, &options).unwrap();
                    let bytes = std::fs::read(&path).unwrap();
                    let native = jpeg2k::Image::from_bytes(&bytes).unwrap();
                    let mut data = vec![];
                    for index in 0..(17 * 11) {
                        for c in native.components() {
                            data.extend(c.data()[index].to_le_bytes());
                        }
                    }
                    let components = name.clone() + ".components.le";
                    std::fs::write(root.join(&components), data).unwrap();
                    let opened = crate::files::open(&path).unwrap();
                    assert_eq!(opened.document.layers[0].bit_depth, 16);
                    assert_eq!((opened.document.width, opened.document.height), (17, 11));
                    let raw = root.join(name.clone() + ".j2k");
                    save(&doc, &raw, true, &options).unwrap();
                    assert_eq!(std::fs::read(&raw).unwrap(), baseline);
                    cases.push(serde_json::json!({"file":name,"decoded":components,"ppi":ppi,"enabled":enabled,"gray":gray,"lossless":lossless,"channels":if gray{2}else{4}}));
                }
            }
        }
    }
    let destination = root.join("invalid.jp2");
    std::fs::write(&destination, b"original").unwrap();
    assert!(
        save(
            &doc,
            &destination,
            false,
            &ExportOptions {
                jp2_resolution_enabled: true,
                jp2_resolution: 0,
                ..Default::default()
            }
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&destination).unwrap(), b"original");
    assert!(Arc::ptr_eq(
        &before,
        doc.layers[0].precision.as_ref().unwrap()
    ));
    let project = root.join("source.rphoto");
    crate::files::save_project(&doc, &project).unwrap();
    assert_eq!(
        crate::precision::composite(&crate::files::load_project(&project).unwrap()),
        crate::precision::composite(&doc)
    );
    let mut encoder = Codec::new_encoder(CODEC_FORMAT::OPJ_CODEC_JP2).unwrap();
    assert!(encoder.set_jp2_display_resolution_ppi(Some(72)));
    assert!(!encoder.set_jp2_display_resolution_ppi(Some(0)));
    assert!(encoder.set_jp2_display_resolution_ppi(None));
    assert!(
        !Codec::new_encoder(CODEC_FORMAT::OPJ_CODEC_J2K)
            .unwrap()
            .set_jp2_display_resolution_ppi(Some(72))
    );
    assert!(
        !Codec::new_decoder(CODEC_FORMAT::OPJ_CODEC_JP2)
            .unwrap()
            .set_jp2_display_resolution_ppi(Some(72))
    );
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

include!("jpeg2000_icc_output_tests.rs");

include!("jpeg2000_metadata_rate_tests.rs");
