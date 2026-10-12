#[test]
fn jpeg2000_eycc_signed_chroma_alpha_precision_edit_history_project_png_and_source() {
    let root = root().join("eycc");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for precision in [1, 4, 8, 12, 16] {
        for alpha in [None, Some((8, false)), Some((16, true))] {
            let mut reference = None;
            for signed_chroma in [[false, false], [true, false], [false, true], [true, true]] {
                let mut precisions = vec![precision; 3];
                let mut signed = vec![false, signed_chroma[0], signed_chroma[1]];
                if let Some((depth, sign)) = alpha {
                    precisions.push(depth);
                    signed.push(sign);
                }
                let name = format!("mode{}.jp2", cases.len());
                let path = root.join(&name);
                let source = encode(
                    &path,
                    &precisions,
                    &signed,
                    false,
                    COLOR_SPACE::OPJ_CLRSPC_EYCC,
                );
                let before = std::fs::read(&path).unwrap();
                let native = jpeg2k::Image::from_bytes(&before).unwrap();
                assert!(matches!(native.color_space(), jpeg2k::ColorSpace::EYCC));
                if alpha.is_some() {
                    assert!(native.components()[3].is_alpha());
                }
                let opened = crate::files::open(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert!(
                    opened
                        .notes
                        .iter()
                        .any(|n| n.contains("eYCC") && n.contains("RGB"))
                );
                let layer = &opened.document.layers[0];
                let depth = if *precisions.iter().max().unwrap() > 8 {
                    16
                } else {
                    8
                };
                assert_eq!(layer.bit_depth, depth);
                let pixels = layer.precision.as_deref().cloned().unwrap_or_else(|| {
                    DynamicImage::ImageRgba8((*layer.pixels).clone()).into_rgba32f()
                });
                if let Some(reference) = &reference {
                    assert_eq!(&pixels, reference);
                } else {
                    reference = Some(pixels.clone());
                }
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
                let pixel_arc = layer.pixels.clone();
                let precision_arc = layer.precision.clone();
                let mut editor = crate::document::Editor::new(opened.document, false);
                editor.edit("eYCC opacity", |doc| doc.layers[0].opacity = 0.5);
                assert_eq!(editor.history().count(), 1);
                assert!(
                    crate::precision::composite(&editor.document)
                        .pixels()
                        .any(|p| p[3] > 0. && p[3] < 1.)
                );
                editor.undo();
                assert_eq!(crate::precision::composite(&editor.document), expected);
                assert!(Arc::ptr_eq(&pixel_arc, &editor.document.layers[0].pixels));
                if let Some(precision_arc) = precision_arc {
                    assert!(Arc::ptr_eq(
                        &precision_arc,
                        editor.document.layers[0].precision.as_ref().unwrap()
                    ));
                }
                editor.redo();
                assert_eq!(editor.document.layers[0].opacity, 0.5);
                editor.undo();
                let project = root.join(name.clone() + ".rphoto");
                crate::files::save_project(&editor.document, &project).unwrap();
                assert_eq!(
                    crate::precision::composite(&crate::files::load_project(&project).unwrap()),
                    expected
                );
                let png = root.join(name.clone() + ".png");
                crate::files::export(
                    &editor.document,
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
                cases.push(serde_json::json!({"file":name,"decoded":raw,"precisions":precisions,"signed":signed,"depth":depth,"size":[7,5],"source_planes":source}));
            }
        }
    }
    assert_eq!(cases.len(), 60);
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}

#[test]
fn jpeg2000_eycc_invalid_structure_alpha_sampling_truncation_preserves_source() {
    let root = root().join("eycc-errors");
    std::fs::create_dir_all(&root).unwrap();
    for (name, precisions, signed, subsample) in [
        ("two", vec![8; 2], vec![false; 2], false),
        ("five", vec![8; 5], vec![false; 5], false),
        ("signed-y", vec![8; 3], vec![true, false, false], false),
        ("mixed-color", vec![8, 12, 8], vec![false; 3], false),
        ("sampling", vec![8; 3], vec![false; 3], true),
        ("precision17", vec![17; 3], vec![false; 3], false),
    ] {
        let path = root.join(format!("{name}.jp2"));
        encode(
            &path,
            &precisions,
            &signed,
            subsample,
            COLOR_SPACE::OPJ_CLRSPC_EYCC,
        );
        let before = std::fs::read(&path).unwrap();
        assert!(crate::files::open(&path).is_err(), "{name}");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }
    let path = root.join("undefined-alpha.jp2");
    encode(
        &path,
        &[8; 4],
        &[false; 4],
        false,
        COLOR_SPACE::OPJ_CLRSPC_EYCC,
    );
    let mut before = std::fs::read(&path).unwrap();
    let at = before.windows(4).position(|v| v == b"cdef").unwrap() + 4;
    before[at + 2 + 3 * 6 + 2..at + 2 + 3 * 6 + 4].copy_from_slice(&0u16.to_be_bytes());
    std::fs::write(&path, &before).unwrap();
    assert!(crate::files::open(&path).err().unwrap().contains("투명도"));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    let valid = root.join("valid.jp2");
    encode(
        &valid,
        &[16; 3],
        &[false, true, true],
        false,
        COLOR_SPACE::OPJ_CLRSPC_EYCC,
    );
    let bytes = std::fs::read(&valid).unwrap();
    for length in [0, 12, bytes.len() / 3, bytes.len() / 2, bytes.len() - 12] {
        let truncated = root.join(format!("truncated{length}.jp2"));
        std::fs::write(&truncated, &bytes[..length]).unwrap();
        assert!(crate::files::open(&truncated).is_err(), "length{length}");
        assert_eq!(std::fs::read(&truncated).unwrap(), bytes[..length]);
    }
    assert_eq!(eycc_rgb(0, 0, 0, 16, [true, true]), [0; 3]);
    for precision in [1, 4, 8, 12, 16] {
        let limit = (1 << precision) - 1;
        for y in [i32::MIN, 0, limit, i32::MAX] {
            for chroma in [i32::MIN, 0, i32::MAX] {
                assert!(
                    eycc_rgb(y, chroma, chroma, precision, [true, true])
                        .iter()
                        .all(|c| (0..=limit).contains(c))
                );
            }
        }
    }
}
