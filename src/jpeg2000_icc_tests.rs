fn replace_jp2_profile(path: &Path, profile: &[u8]) {
    let mut bytes = std::fs::read(path).unwrap();
    let parent = bytes.windows(4).position(|b| b == b"jp2h").unwrap() - 4;
    let parent_len = u32::from_be_bytes(bytes[parent..parent + 4].try_into().unwrap());
    let at = bytes.windows(4).position(|b| b == b"colr").unwrap() - 4;
    let old = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
    let mut box_bytes = (11u32 + profile.len() as u32).to_be_bytes().to_vec();
    box_bytes.extend(b"colr");
    box_bytes.extend([2, 0, 0]);
    box_bytes.extend(profile);
    bytes[parent..parent + 4]
        .copy_from_slice(&(parent_len + box_bytes.len() as u32 - old as u32).to_be_bytes());
    bytes.splice(at..at + old, box_bytes);
    std::fs::write(path, bytes).unwrap();
}
#[test]
fn jpeg2000_icc_rgb_gray_alpha_mixed_precision_projects_png_and_original() {
    use moxcms::ColorProfile;
    let root = root().join("icc");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for (name, profile, gray) in [
        ("srgb", ColorProfile::new_srgb(), false),
        ("p3", ColorProfile::new_display_p3(), false),
        ("adobe", ColorProfile::new_adobe_rgb(), false),
        ("gray1", ColorProfile::new_gray_with_gamma(1.), true),
        ("gray18", ColorProfile::new_gray_with_gamma(1.8), true),
        ("gray22", ColorProfile::new_gray_with_gamma(2.2), true),
    ] {
        let icc = profile.encode().unwrap();
        std::fs::write(root.join(format!("{name}.icc")), &icc).unwrap();
        for precision in [1, 4, 8, 12, 16] {
            for alpha in [false, true] {
                let mut precisions = vec![precision; if gray { 1 } else { 3 }];
                let mut signed = vec![false; precisions.len()];
                if alpha {
                    precisions.push(16);
                    signed.push(true);
                }
                let file = format!("{name}-{precision}-{alpha}.jp2");
                let path = root.join(&file);
                encode(
                    &path,
                    &precisions,
                    &signed,
                    false,
                    if gray {
                        COLOR_SPACE::OPJ_CLRSPC_GRAY
                    } else {
                        COLOR_SPACE::OPJ_CLRSPC_SRGB
                    },
                );
                replace_jp2_profile(&path, &icc);
                let bytes = std::fs::read(&path).unwrap();
                assert_eq!(
                    crate::jpeg2000_icc::profile(&bytes).unwrap(),
                    Some(icc.as_slice())
                );
                let native = jpeg2k::Image::from_bytes(&bytes).unwrap();
                assert!(native.has_icc_profile());
                let original = pixels(&native, &bytes, &mut vec![]).unwrap();
                let source = original.to_rgba32f();
                let original_alpha = original.to_rgba16();
                let opened = crate::files::open(&path).unwrap();
                assert!(
                    opened.notes.iter().any(|n| n.contains("적용해")),
                    "{file}: {:?}",
                    opened.notes
                );
                let layer = &opened.document.layers[0];
                let depth = if alpha || precision > 8 { 16 } else { 8 };
                assert_eq!(layer.bit_depth, depth);
                let decoded = layer.precision.as_deref().cloned().unwrap_or_else(|| {
                    DynamicImage::ImageRgba8((*layer.pixels).clone()).to_rgba32f()
                });
                for (s, d) in original_alpha.pixels().zip(
                    DynamicImage::ImageRgba32F(decoded.clone())
                        .to_rgba16()
                        .pixels(),
                ) {
                    assert_eq!(s[3], d[3]);
                }
                for (label, buffer) in [("src", &source), ("dst", &decoded)] {
                    std::fs::write(
                        root.join(format!("{file}.{label}.f32")),
                        buffer
                            .as_raw()
                            .iter()
                            .flat_map(|v| v.to_le_bytes())
                            .collect::<Vec<_>>(),
                    )
                    .unwrap();
                }
                let expected = crate::precision::composite(&opened.document);
                let project = root.join(format!("{file}.rphoto"));
                crate::files::save_project(&opened.document, &project).unwrap();
                assert_eq!(
                    crate::precision::composite(&crate::files::load_project(&project).unwrap()),
                    expected
                );
                let png = root.join(format!("{file}.png"));
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
                cases.push(serde_json::json!({"file":file,"profile":name,"gray":gray,"precision":precision,"alpha":alpha,"depth":depth}));
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
fn jpeg2000_icc_boxes_priority_extended_eof_invalid_profile_source_pixels() {
    fn box_bytes(kind: &[u8; 4], data: &[u8], mode: u8) -> Vec<u8> {
        let mut b = match mode {
            0 => (8 + data.len() as u32).to_be_bytes().to_vec(),
            1 => {
                let mut b = 1u32.to_be_bytes().to_vec();
                b.extend(kind);
                b.extend((16 + data.len() as u64).to_be_bytes());
                b.extend(data);
                return b;
            }
            _ => 0u32.to_be_bytes().to_vec(),
        };
        b.extend(kind);
        b.extend(data);
        b
    }
    let marker = [2, 0, 0, 9, 8, 7];
    for outer in 0..3 {
        for inner in 0..3 {
            let b = box_bytes(b"jp2h", &box_bytes(b"colr", &marker, inner), outer);
            assert_eq!(
                crate::jpeg2000_icc::profile(&b).unwrap(),
                Some(&marker[3..])
            );
        }
    }
    for first in [1u8, 2, 3] {
        let mut body = box_bytes(b"colr", &[first, 0, 0, 16], 0);
        body.extend(box_bytes(b"colr", &marker, 0));
        let bytes = box_bytes(b"jp2h", &body, 0);
        assert_eq!(
            crate::jpeg2000_icc::profile(&bytes).unwrap(),
            match first {
                1 => None,
                2 => Some(&[16][..]),
                _ => Some(&marker[3..]),
            }
        );
    }
    assert_eq!(
        crate::jpeg2000_icc::profile(&[0xff, 0x4f, 0, 0]).unwrap(),
        None
    );
    for bad in [
        vec![0; 7],
        vec![0, 0, 0, 1, b'j', b'p', b'2', b'h'],
        box_bytes(b"jp2h", &[0; 7], 0),
        box_bytes(b"jp2h", &box_bytes(b"colr", &[2, 0], 0), 0),
    ] {
        assert!(crate::jpeg2000_icc::profile(&bad).is_err());
    }
    let root = root().join("icc-errors");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("invalid.jp2");
    encode(
        &path,
        &[16; 3],
        &[false; 3],
        false,
        COLOR_SPACE::OPJ_CLRSPC_SRGB,
    );
    replace_jp2_profile(&path, &[0; 128]);
    let bytes = std::fs::read(&path).unwrap();
    let native = jpeg2k::Image::from_bytes(&bytes).unwrap();
    let original = pixels(&native, &bytes, &mut vec![]).unwrap();
    let reference = crate::precision::document("original".into(), original).unwrap();
    let opened = crate::files::open(&path).unwrap();
    assert!(opened.notes.iter().any(|n| n.contains("못했습니다")));
    assert_eq!(
        crate::precision::composite(&opened.document),
        crate::precision::composite(&reference)
    );
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
}
