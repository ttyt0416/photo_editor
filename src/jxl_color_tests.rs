use super::*;
use std::{io::Write, path::Path};

fn output() -> std::path::PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/jxl-color");
    std::fs::create_dir_all(&root).unwrap();
    root
}
fn floats(path: &Path, pixels: &Rgba32FImage) {
    let mut encoder = flate2::write::ZlibEncoder::new(
        std::fs::File::create(path).unwrap(),
        flate2::Compression::default(),
    );
    for value in pixels.as_raw() {
        encoder.write_all(&value.to_le_bytes()).unwrap();
    }
    encoder.finish().unwrap();
}
#[test]
fn jxl_working_rgb_gray_matrix_hdr_alpha_profiles_project_and_outputs() {
    let root = output();
    let base = Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::write(
        root.join("working.icc"),
        crate::color_management::srgb_profile(),
    )
    .unwrap();
    let mut cases = vec![];
    for (name, category) in [
        ("alpha_triangles", "jxl"),
        ("alpha_nonpremultiplied", "jxl"),
        ("alpha_premultiplied", "jxl"),
        ("sunset_logo", "jxl"),
        ("lossless_pfm", "jxl"),
        ("grayscale_jpeg", "jxl"),
        ("bench_oriented_brg", "jxl-icc"),
        ("grayscale", "jxl-icc"),
    ] {
        let path = base.join(format!("tests/fixtures/{category}/{name}/input.jxl"));
        let (reader, _) = crate::jxl_metadata::open(&path, &mut vec![]).unwrap();
        let image = jxl_oxide::JxlImage::builder().read(reader).unwrap();
        let profile = image.rendered_icc();
        let gray = image.pixel_format().is_grayscale();
        std::fs::write(root.join(format!("{name}.icc")), &profile).unwrap();
        let native = crate::jxl_input::read_native(&path)
            .unwrap()
            .pixels
            .to_rgba32f();
        let working = crate::jxl_input::read(&path).unwrap();
        assert!(
            working.notes.iter().any(|n| n.contains("작업 sRGB로 변환")),
            "{name}: {:?}",
            working.notes
        );
        let pixels = working.pixels.to_rgba32f();
        assert!(
            pixels
                .pixels()
                .zip(native.pixels())
                .all(|(p, s)| p[3].to_bits() == s[3].to_bits()),
            "{name}"
        );
        floats(&root.join(format!("{name}.src.zlib")), &native);
        floats(&root.join(format!("{name}.dst.zlib")), &pixels);
        let opened = crate::files::open(&path).unwrap().document;
        let expected = crate::precision::composite(&opened);
        crate::files::save_project(&opened, &root.join(format!("{name}.rphoto"))).unwrap();
        let restored = crate::files::load_project(&root.join(format!("{name}.rphoto"))).unwrap();
        assert_eq!(crate::precision::composite(&restored), expected);
        assert_eq!(restored.source_icc, opened.source_icc);
        crate::files::export(
            &opened,
            &root.join(format!("{name}.exr")),
            image::ImageFormat::OpenExr,
            &Default::default(),
        )
        .unwrap();
        let exr = crate::files::open(&root.join(format!("{name}.exr")))
            .unwrap()
            .document;
        assert_eq!(crate::precision::composite(&exr), expected);
        cases.push(serde_json::json!({"name":name,"gray":gray,"w":pixels.width(),"h":pixels.height(),"depth":restored.layers[0].bit_depth}));
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn jxl_working_extended_float_and_invalid_profiles_do_not_clip_or_change_source() {
    let root = output();
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jxl/lossless_pfm/input.jxl");
    let (reader, _) = crate::jxl_metadata::open(&path, &mut vec![]).unwrap();
    let image = jxl_oxide::JxlImage::builder().read(reader).unwrap();
    let profile = image.rendered_icc();
    let source = Rgba32FImage::from_fn(17, 3, |x, y| {
        image::Rgba([
            (x as f32 - 8.) / 4.,
            y as f32 + 0.5,
            x as f32 / 8.,
            [0., 0.125, 1.][y as usize],
        ])
    });
    let result = convert(&source, &profile, false).unwrap();
    assert!(result.pixels().any(|p| p[0] < 0.));
    assert!(result.pixels().any(|p| p[1] > 1.));
    assert!(
        result
            .pixels()
            .zip(source.pixels())
            .all(|(p, s)| p[3].to_bits() == s[3].to_bits())
    );
    std::fs::write(root.join("synthetic.icc"), profile).unwrap();
    floats(&root.join("synthetic.src.zlib"), &source);
    floats(&root.join("synthetic.dst.zlib"), &result);
    for profile in [
        b"invalid".as_slice(),
        crate::color_management::gray_profile(),
    ] {
        let mut notes = vec![];
        assert_eq!(apply(source.clone(), profile, false, &mut notes), source);
        assert!(notes[0].contains("원래 픽셀과 원본 ICC"));
    }
}
