#[test]
fn jpeg2000_icc_output_rgb_gray_alpha_lossy_lossless_native_profile_project_and_source() {
    let root = output().join("icc");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for (label, profile) in [
        ("rgb", crate::color_management::srgb_profile()),
        ("gray", crate::color_management::gray_profile()),
    ] {
        std::fs::write(root.join(format!("{label}.icc")), profile).unwrap();
    }
    for gray in [false, true] {
        for depth in [8, 16] {
            for alpha in [false, true] {
                for lossless in [false, true] {
                    let samples = image::Rgba32FImage::from_fn(17, 9, |x, y| {
                        let i = x + y * 17;
                        image::Rgba([
                            (i * 7193 % 65536) as f32 / 65535.,
                            (i * 11317 % 65536) as f32 / 65535.,
                            (i * 17293 % 65536) as f32 / 65535.,
                            if !alpha || i % 7 == 1 {
                                1.
                            } else if i.is_multiple_of(7) {
                                0.
                            } else {
                                0.25
                            },
                        ])
                    });
                    let doc = crate::precision::document(
                        "JP2 ICC".into(),
                        if depth == 8 {
                            image::DynamicImage::ImageRgba8(crate::precision::preview(&samples))
                        } else {
                            image::DynamicImage::ImageRgba16(crate::precision::image16(&samples))
                        },
                    )
                    .unwrap();
                    let before = doc.layers[0].pixels.clone();
                    let original = doc.layers[0].precision.clone();
                    let mut reference = None;
                    for include in [false, true] {
                        let name = format!("g{gray}-d{depth}-a{alpha}-l{lossless}-i{include}.jp2");
                        let path = root.join(&name);
                        let options = ExportOptions {
                            jp2_icc: include,
                            jp2_depth: depth,
                            jp2_grayscale: gray,
                            jp2_lossless: lossless,
                            jp2_ratio: 2,
                            ..Default::default()
                        };
                        save(&doc, &path, false, &options).unwrap();
                        let bytes = std::fs::read(&path).unwrap();
                        let profile = crate::jpeg2000_icc::profile(&bytes).unwrap();
                        assert_eq!(
                            profile,
                            include.then_some(if gray {
                                crate::color_management::gray_profile()
                            } else {
                                crate::color_management::srgb_profile()
                            })
                        );
                        let native = jpeg2k::Image::from_bytes(&bytes).unwrap();
                        assert_eq!(native.has_icc_profile(), include);
                        if alpha {
                            assert!(native.components().last().unwrap().is_alpha());
                        }
                        let opened = crate::files::open(&path).unwrap();
                        if include {
                            assert!(
                                opened.notes.iter().any(|n| n.contains("적용해")),
                                "{name}: {:?}",
                                opened.notes
                            );
                        }
                        let decoded = crate::precision::composite(&opened.document);
                        if let Some(reference) = &reference {
                            assert_eq!(&decoded, reference, "{name}");
                        } else {
                            reference = Some(decoded.clone());
                        }
                        let raw = format!("{name}.f32");
                        std::fs::write(
                            root.join(&raw),
                            decoded
                                .as_raw()
                                .iter()
                                .flat_map(|v| v.to_le_bytes())
                                .collect::<Vec<_>>(),
                        )
                        .unwrap();
                        let project = root.join(format!("{name}.rphoto"));
                        crate::files::save_project(&opened.document, &project).unwrap();
                        assert_eq!(
                            crate::precision::composite(
                                &crate::files::load_project(&project).unwrap()
                            ),
                            decoded
                        );
                        assert!(Arc::ptr_eq(&before, &doc.layers[0].pixels));
                        if let Some(original) = &original {
                            assert!(Arc::ptr_eq(
                                original,
                                doc.layers[0].precision.as_ref().unwrap()
                            ));
                        }
                        assert_eq!(std::fs::read(&path).unwrap(), bytes);
                        cases.push(serde_json::json!({"file":name,"decoded":raw,"gray":gray,"depth":depth,"alpha":alpha,"lossless":lossless,"include":include}));
                    }
                }
            }
        }
    }
    assert_eq!(cases.len(), 32);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
    let doc = crate::document::Document::from_image(
        "raw".into(),
        image::RgbaImage::from_pixel(7, 5, image::Rgba([20, 50, 70, 100])),
    );
    let path = root.join("profile-ignored.j2k");
    save(
        &doc,
        &path,
        true,
        &ExportOptions {
            jp2_icc: true,
            ..Default::default()
        },
    )
    .unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(crate::jpeg2000_icc::profile(&bytes).unwrap(), None);
    assert!(!jpeg2k::Image::from_bytes(&bytes).unwrap().has_icc_profile());
}

#[test]
fn jpeg2000_icc_multitile_progressive_layers_exact_codestream_and_source() {
    let root = output().join("icc-tiles");
    std::fs::create_dir_all(&root).unwrap();
    let doc = crate::precision::document(
        "tiles".into(),
        image::DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(129, 129, |x, y| {
            image::Rgba([
                (x * 397 + y * 719) as u16,
                (x * 907 + y * 367) as u16,
                (x * 131 + y * 277) as u16,
                if x % 7 == 0 { 0 } else { 32768 },
            ])
        })),
    )
    .unwrap();
    let source = doc.layers[0].precision.clone();
    for gray in [false, true] {
        for lossless in [false, true] {
            let mut reference = None;
            for include in [false, true] {
                let path = root.join(format!("g{gray}-l{lossless}-i{include}.jp2"));
                let options = ExportOptions {
                    jp2_icc: include,
                    jp2_grayscale: gray,
                    jp2_depth: 16,
                    jp2_tile: 128,
                    jp2_layers: 4,
                    jp2_progression: 2,
                    jp2_lossless: lossless,
                    jp2_ratio: 2,
                    ..Default::default()
                };
                save(&doc, &path, false, &options).unwrap();
                let bytes = std::fs::read(&path).unwrap();
                let at = bytes.windows(4).position(|b| b == b"jp2c").unwrap();
                let length = u32::from_be_bytes(bytes[at - 4..at].try_into().unwrap()) as usize;
                let stream = bytes[at + 4..at - 4 + length].to_vec();
                let decoded =
                    crate::precision::composite(&crate::files::open(&path).unwrap().document);
                if let Some((before, pixels)) = &reference {
                    assert_eq!(&stream, before);
                    assert_eq!(&decoded, pixels);
                } else {
                    reference = Some((stream, decoded));
                }
                assert!(Arc::ptr_eq(
                    source.as_ref().unwrap(),
                    doc.layers[0].precision.as_ref().unwrap()
                ));
            }
        }
    }
}
