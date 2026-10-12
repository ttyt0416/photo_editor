#[test]
fn jpeg2000_resolution_and_icc_metadata_preserve_codestream_pixels_tiles_and_layers() {
    let root = output().join("metadata-rate");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for (width, height) in [(1, 9), (17, 9), (129, 129)] {
        let doc = crate::precision::document(
            "Metadata rate".into(),
            image::DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(width, height, |x, y| {
                let i = x + y * width;
                image::Rgba([
                    (i * 7193 % 65536) as u16,
                    (i * 11317 % 65536) as u16,
                    (i * 17293 % 65536) as u16,
                    if i.is_multiple_of(7) {
                        0
                    } else if i % 7 == 1 {
                        65535
                    } else {
                        32768
                    },
                ])
            })),
        )
        .unwrap();
        let before = doc.layers[0].precision.clone().unwrap();
        let pixels = doc.layers[0].pixels.clone();
        for gray in [false, true] {
            for depth in [8, 16] {
                for lossless in [false, true] {
                    let mut reference = None;
                    for icc in [false, true] {
                        for resolution in [false, true] {
                            let ppi = if width == 1 {
                                1
                            } else if width == 17 {
                                300
                            } else {
                                65535
                            };
                            let name = format!(
                                "{width}x{height}-g{gray}-d{depth}-l{lossless}-i{icc}-r{resolution}.jp2"
                            );
                            let path = root.join(&name);
                            let options = ExportOptions {
                                jp2_icc: icc,
                                jp2_resolution_enabled: resolution,
                                jp2_resolution: ppi,
                                jp2_grayscale: gray,
                                jp2_depth: depth,
                                jp2_tile: 128,
                                jp2_layers: 4,
                                jp2_progression: 2,
                                jp2_lossless: lossless,
                                jp2_ratio: 2,
                                ..Default::default()
                            };
                            save(&doc, &path, false, &options).unwrap();
                            let bytes = std::fs::read(&path).unwrap();
                            let mut at = 0;
                            let stream = loop {
                                assert!(at + 8 <= bytes.len());
                                let length =
                                    u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap())
                                        as usize;
                                assert!(length >= 8 && at + length <= bytes.len());
                                if &bytes[at + 4..at + 8] == b"jp2c" {
                                    break bytes[at + 8..at + length].to_vec();
                                }
                                at += length;
                            };
                            let opened = crate::files::open(&path).unwrap();
                            let decoded = crate::precision::composite(&opened.document);
                            if let Some((baseline, baseline_pixels)) = &reference {
                                assert_eq!(
                                    &stream, baseline,
                                    "metadata changed codestream: {name}"
                                );
                                assert_eq!(
                                    &decoded, baseline_pixels,
                                    "metadata changed pixels: {name}"
                                );
                            } else {
                                reference = Some((stream, decoded.clone()));
                            }
                            let raw = format!("{name}.rgba32f.le");
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
                            assert!(Arc::ptr_eq(
                                &before,
                                doc.layers[0].precision.as_ref().unwrap()
                            ));
                            assert!(Arc::ptr_eq(&pixels, &doc.layers[0].pixels));
                            assert_eq!(std::fs::read(&path).unwrap(), bytes);
                            cases.push(serde_json::json!({"file":name,"raw":raw,"width":width,"height":height,"gray":gray,"depth":depth,"lossless":lossless,"icc":icc,"resolution":resolution,"ppi":ppi}));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cases.len(), 96);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
