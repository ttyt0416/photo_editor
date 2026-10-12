use crate::files::{ExportOptions, export, load_project, open, save_project};
use image::ImageDecoder;
use std::sync::Arc;

#[test]
fn webp_icc_lossless_lossy_opaque_alpha_exact_pair_pixels_profile_and_source() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/webp-icc");
    std::fs::create_dir_all(&root).unwrap();
    let mut cases = vec![];
    for (width, height) in [(1, 9), (17, 11)] {
        for alpha in [false, true] {
            let doc = crate::precision::document(
                "WebP profile".into(),
                image::DynamicImage::ImageRgba16(image::ImageBuffer::from_fn(
                    width,
                    height,
                    |x, y| {
                        let i = x + y * width;
                        image::Rgba([
                            (i * 7193 % 65536) as u16,
                            (i * 11317 % 65536) as u16,
                            (i * 17293 % 65536) as u16,
                            if !alpha || i % 7 == 1 {
                                65535
                            } else if i.is_multiple_of(7) {
                                0
                            } else {
                                32768
                            },
                        ])
                    },
                )),
            )
            .unwrap();
            let before = doc.layers[0].precision.clone().unwrap();
            let pixels = doc.layers[0].pixels.clone();
            let source = doc.composite();
            let source_name = format!("{width}x{height}-a{alpha}-source.png");
            source.save(root.join(&source_name)).unwrap();
            for (lossless, quality, method) in [
                (true, 90, 4),
                (false, 0, 0),
                (false, 90, 4),
                (false, 100, 6),
            ] {
                let mut baseline = None;
                for include in [false, true] {
                    let name = format!(
                        "{width}x{height}-a{alpha}-l{lossless}-q{quality}-m{method}-i{include}.webp"
                    );
                    let path = root.join(&name);
                    let options = ExportOptions {
                        include_icc: include,
                        webp_lossless: lossless,
                        webp_quality: quality,
                        webp_method: method,
                        ..Default::default()
                    };
                    export(&doc, &path, image::ImageFormat::WebP, &options).unwrap();
                    let bytes = std::fs::read(&path).unwrap();
                    let mut decoder =
                        image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(&bytes))
                            .unwrap();
                    assert_eq!(
                        decoder.icc_profile().unwrap().as_deref(),
                        include.then_some(crate::color_management::srgb_profile())
                    );
                    let opened = open(&path).unwrap();
                    assert_eq!(opened.document.layers[0].bit_depth, 8);
                    let decoded = opened.document.composite();
                    if lossless {
                        assert_eq!(decoded, source);
                    }
                    for (a, b) in decoded.pixels().zip(source.pixels()) {
                        assert_eq!(a[3], b[3]);
                    }
                    if let Some(original) = &baseline {
                        assert_eq!(&decoded, original);
                    } else {
                        baseline = Some(decoded.clone());
                    }
                    let raw = format!("{name}.rgba8");
                    std::fs::write(root.join(&raw), decoded.as_raw()).unwrap();
                    let project = root.join(format!("{name}.rphoto"));
                    save_project(&opened.document, &project).unwrap();
                    assert_eq!(load_project(&project).unwrap().composite(), decoded);
                    assert!(Arc::ptr_eq(
                        &before,
                        doc.layers[0].precision.as_ref().unwrap()
                    ));
                    assert!(Arc::ptr_eq(&pixels, &doc.layers[0].pixels));
                    assert_eq!(std::fs::read(&path).unwrap(), bytes);
                    cases.push(serde_json::json!({"file":name,"raw":raw,"source":source_name,"width":width,"height":height,"alpha":alpha,"lossless":lossless,"quality":quality,"method":method,"include":include}));
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
}
