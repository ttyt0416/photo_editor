use super::*;
use crate::{document::Editor, vector::Content};
fn out(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("qa-output/svg-assets")
        .join(name);
    std::fs::create_dir_all(&path).unwrap();
    path
}
fn source(document: &crate::document::Document) -> &str {
    match document.layers[0].content.as_ref().unwrap() {
        Content::Svg { source } => source,
        _ => panic!(),
    }
}
fn svg(body: &str, w: u32, h: u32) -> String {
    format!(
        "<svg xmlns='http://www.w3.org/2000/svg' xmlns:xlink='http://www.w3.org/1999/xlink' width='{w}' height='{h}'>{body}</svg>"
    )
}
fn png(path: &Path, color: [u8; 4]) {
    let pixels =
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(8, 6, image::Rgba(color)));
    let mut bytes = Cursor::new(Vec::new());
    pixels.write_to(&mut bytes, ImageFormat::Png).unwrap();
    std::fs::write(path, bytes.into_inner()).unwrap();
}

#[test]
fn svg_external_native_images_local_xml_base_percent_file_uri_precedence_and_portable_history() {
    let root = out("한글 원본");
    let assets = root.join("사진 폴더");
    std::fs::create_dir_all(&assets).unwrap();
    let mut cases = vec![];
    for (i, extension) in ["png", "jpg", "gif", "webp"].into_iter().enumerate() {
        let image =
            image::RgbaImage::from_pixel(8, 6, image::Rgba([30 + i as u8 * 40, 80, 170, 255]));
        let path = assets.join(format!("이미지 {i}.{extension}"));
        let pixels = image::DynamicImage::ImageRgba8(image);
        if extension == "jpg" {
            pixels.to_rgb8().save(&path).unwrap();
        } else {
            pixels.save(&path).unwrap();
        }
        let original = std::fs::read(&path).unwrap();
        let href = format!("사진%20폴더/이미지%20{i}.{extension}");
        let body = match i {
            0 => {
                "<g xml:base='사진%20폴더/'><image width='8' height='6' href='이미지%200.png'/></g>"
                    .to_string()
            }
            1 => format!("<image width='8' height='6' xlink:href='missing.png' href='{href}'/>"),
            2 => format!("<image width='8' height='6' href='missing.png' xlink:href='{href}'/>"),
            _ => format!(
                "<image width='8' height='6' href='file:///{}'/>",
                path.to_string_lossy()
                    .replace('\\', "/")
                    .replace(' ', "%20")
            ),
        };
        // SVG2 unqualified href takes precedence regardless of attribute order.
        let body = if i == 2 {
            body.replace("href='missing.png'", &format!("href='{href}'"))
        } else {
            body
        };
        let input = root.join(format!("native-{i}.svg"));
        let input_source = svg(&format!("<!-- preserve -->{body}"), 8, 6);
        std::fs::write(&input, &input_source).unwrap();
        let opened = crate::files::open(&input).unwrap();
        assert!(!opened.notes.is_empty());
        let doc = opened.document;
        let xml = roxmltree::Document::parse(source(&doc)).unwrap();
        let node = xml.descendants().find(|n| image_node(*n)).unwrap();
        let (_, bytes) = decode_data(
            node.attribute("data-rphoto-image-original")
                .unwrap_or_else(|| super::href(node).unwrap().value()),
        )
        .unwrap();
        assert_eq!(bytes, original);
        if let Some(legacy) = node.attribute((XLINK, "href")) {
            assert_eq!(legacy, node.attribute("href").unwrap());
        }
        assert!(source(&doc).contains("<!-- preserve -->"));
        assert_eq!(std::fs::read_to_string(&input).unwrap(), input_source);
        let original_pixels = doc.composite();
        let project = root.join(format!("native-{i}.rphoto"));
        crate::files::save_project(&doc, &project).unwrap();
        let exported = root.join(format!("native-{i}-export.svg"));
        crate::svg_export::save(&doc, &exported).unwrap();
        png(&path, [0, 255, 0, 255]); // Change dependencies only after the import; the captured document is stable.
        let restored = crate::files::load_project(&project).unwrap();
        assert_eq!(source(&restored), source(&doc));
        assert_eq!(restored.composite(), original_pixels);
        assert_eq!(
            crate::files::open(&exported).unwrap().document.composite(),
            original_pixels
        );
        let edited =
            crate::svg_edit::attribute(source(&doc), if i == 0 { 2 } else { 1 }, "opacity", "0.5")
                .unwrap();
        let content = Content::Svg { source: edited };
        let pixels = content.render().unwrap();
        let mut editor = Editor::new(doc.clone(), true);
        editor.edit("SVG 이미지", |doc| {
            doc.layers[0].content = Some(content);
            doc.layers[0].pixels = std::sync::Arc::new(pixels)
        });
        editor.undo();
        assert_eq!(editor.document.composite(), original_pixels);
        editor.redo();
        assert!(editor.document.composite().get_pixel(3, 3)[3] < 255);
        std::fs::write(&path, &original).unwrap();
        let pixel = original_pixels.get_pixel(3, 3).0;
        cases.push(serde_json::json!({"file":format!("native-{i}-export.svg"),"original":format!("사진 폴더/이미지 {i}.{extension}"),"pixel":pixel}));
    }
    std::fs::write(
        root.join("native.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn svg_external_other_codecs_preserve_exact_originals_and_png_preview_in_project_and_export() {
    let root = out("other");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut inputs = vec![];
    let rgba = image::RgbaImage::from_fn(8, 6, |x, y| {
        image::Rgba([(x * 27) as u8, (y * 39) as u8, 119, 255])
    });
    for ext in ["bmp", "qoi", "tga", "ppm", "ff", "tiff"] {
        let path = root.join(format!("owned.{ext}"));
        let pixels = image::DynamicImage::ImageRgba8(rgba.clone());
        match ext {
            "ff" | "tiff" => pixels.to_rgba16().save(&path).unwrap(),
            "ppm" => pixels.to_rgb8().save(&path).unwrap(),
            _ => pixels.save(&path).unwrap(),
        };
        inputs.push(path);
    }
    let float = image::Rgba32FImage::from_pixel(8, 6, image::Rgba([2.5, -0.3, 0.4, 0.6]));
    let path = root.join("owned.exr");
    image::DynamicImage::ImageRgba32F(float)
        .save(&path)
        .unwrap();
    inputs.push(path);
    inputs.extend([
        fixtures.join("flat-red.heic"),
        fixtures.join("independent.jp2"),
        fixtures.join("jxl/lossless_pfm/input.jxl"),
        fixtures.join("raw/linear-16-o1.dng"),
        fixtures.join("rgb-psd-order/d16-c0-b0-g0.psd"),
    ]);
    let mut cases = vec![];
    for (index, path) in inputs.iter().enumerate() {
        if !path.exists() {
            panic!("{}", path.display());
        }
        let original = std::fs::read(path).unwrap();
        let expected = crate::files::open(path).unwrap().document.composite();
        let href = format!(
            "file:///{}",
            path.to_string_lossy()
                .replace('\\', "/")
                .replace(' ', "%20")
        );
        let input = root.join(format!("codec-{index}.svg"));
        std::fs::write(
            &input,
            svg(
                &format!(
                    "<image width='{}' height='{}' href='{href}'/>",
                    expected.width(),
                    expected.height()
                ),
                expected.width(),
                expected.height(),
            ),
        )
        .unwrap();
        let opened =
            crate::files::open(&input).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(
            opened
                .notes
                .iter()
                .any(|n| n.contains("原本") || n.contains("원본 바이트"))
        );
        let doc = opened.document;
        let xml = roxmltree::Document::parse(source(&doc)).unwrap();
        let node = xml.descendants().find(|n| image_node(*n)).unwrap();
        let (_, original_copy) =
            decode_data(node.attribute("data-rphoto-image-original").unwrap()).unwrap();
        assert_eq!(original_copy, original);
        let actual = doc.composite();
        for (a, b) in actual.pixels().zip(expected.pixels()) {
            for c in 0..4 {
                assert!(a[c].abs_diff(b[c]) <= 1, "{} {a:?} {b:?}", path.display());
            }
        }
        let project = root.join(format!("codec-{index}.rphoto"));
        crate::files::save_project(&doc, &project).unwrap();
        let restored = crate::files::load_project(&project).unwrap();
        assert_eq!(source(&restored), source(&doc));
        assert_eq!(restored.composite(), actual);
        let exported = root.join(format!("codec-{index}-export.svg"));
        crate::svg_export::save(&doc, &exported).unwrap();
        assert_eq!(
            crate::files::open(&exported).unwrap().document.composite(),
            actual
        );
        expected
            .save(root.join(format!("codec-{index}-expected.png")))
            .unwrap();
        actual
            .save(root.join(format!("codec-{index}-actual.png")))
            .unwrap();
        assert_eq!(std::fs::read(path).unwrap(), original);
        cases.push(serde_json::json!({"file":format!("codec-{index}-export.svg"),"input":path,"bytes":original.len(),"name":path.file_name().unwrap().to_string_lossy()}));
    }
    std::fs::write(
        root.join("codecs.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn svg_nested_images_recursively_capture_and_reject_cycles_missing_remote_and_corrupt_sources() {
    let root = out("nested");
    png(&root.join("image.png"), [91, 32, 175, 255]);
    let nested = svg("<image width='8' height='6' href='image.png'/>", 8, 6);
    std::fs::write(root.join("nested.svg"), &nested).unwrap();
    let main = svg("<image width='8' height='6' href='nested.svg'/>", 8, 6);
    std::fs::write(root.join("main.svg"), &main).unwrap();
    let doc = crate::files::open(&root.join("main.svg")).unwrap().document;
    assert_eq!(doc.composite().get_pixel(4, 3).0, [91, 32, 175, 255]);
    let parsed = roxmltree::Document::parse(source(&doc)).unwrap();
    let image = parsed.descendants().find(|n| image_node(*n)).unwrap();
    let (_, bytes) = decode_data(image.attribute("data-rphoto-image-original").unwrap()).unwrap();
    assert_eq!(bytes, nested.as_bytes());
    std::fs::write(
        root.join("nested.svg"),
        svg("<image href='main.svg'/>", 8, 6),
    )
    .unwrap();
    assert!(load(&root.join("main.svg")).unwrap_err().contains("순환"));
    for name in ["missing.png", "https://example.com/image.png", "bad.png"] {
        std::fs::write(root.join("bad.png"), b"not an image").unwrap();
        let input = svg(&format!("<image href='{name}'/>"), 8, 6);
        std::fs::write(root.join("error.svg"), &input).unwrap();
        assert!(crate::files::open(&root.join("error.svg")).is_err());
        assert_eq!(
            std::fs::read_to_string(root.join("error.svg")).unwrap(),
            input
        );
    }
    for i in 0..=DEPTH + 1 {
        std::fs::write(
            root.join(format!("deep-{i}.svg")),
            svg(&format!("<image href='deep-{}.svg'/>", i + 1), 8, 6),
        )
        .unwrap();
    }
    assert!(load(&root.join("deep-0.svg")).unwrap_err().contains("예산"));
    let external = svg("<image href='image.png'/>", 8, 6);
    assert!(crate::vector::render_svg(&external).is_err());
}
#[test]
fn svg_data_images_feimage_internal_references_and_memory_budget_are_checked() {
    let root = out("inline");
    let bitmap = root.join("native.png");
    png(&bitmap, [190, 70, 30, 255]);
    let bytes = std::fs::read(&bitmap).unwrap();
    let input = svg(
        "<defs><filter id='filter'><feImage href='native.png'/></filter><rect id='shape' width='8' height='6' fill='white'/><filter id='internal'><feImage href='#shape'/></filter></defs><rect width='8' height='6' filter='url(#filter)'/>",
        8,
        6,
    );
    std::fs::write(root.join("filter.svg"), input).unwrap();
    let doc = crate::files::open(&root.join("filter.svg"))
        .unwrap()
        .document;
    assert_eq!(doc.composite().get_pixel(4, 3).0, [190, 70, 30, 255]);
    assert!(source(&doc).contains("href='#shape'"));
    let inline = svg(
        &format!(
            "<image width='8' height='6' href='{}'/>",
            data("image/png", &bytes)
        ),
        8,
        6,
    );
    assert!(crate::vector::render_svg(&inline).is_ok());
    assert!(
        validate(&svg(
            &format!("<image href='{}'/>", data("image/jpeg", &bytes)),
            8,
            6
        ))
        .is_err()
    );
    let embedded = svg(
        &format!(
            "<image width='8' height='6' href='{}'/>",
            data("image/svg+xml", inline.as_bytes())
        ),
        8,
        6,
    );
    assert!(validate(&embedded).is_ok());
    let broken = svg("<image href='data:image/png;base64,broken!'/>", 8, 6);
    assert!(validate(&broken).is_err());
    let mut used = LIMIT;
    assert!(charge(&mut used, 1).is_err());
    let mut used = usize::MAX;
    assert!(charge(&mut used, 1).is_err());
    let big = root.join("over-budget.svg");
    std::fs::File::create(&big)
        .unwrap()
        .set_len((LIMIT + 1) as u64)
        .unwrap();
    assert!(read(&big).is_err());
    assert_eq!(
        resolve(&root.join("main.svg"), "写真%20image.png").unwrap(),
        root.join("写真 image.png")
    );
    assert!(local("https://example.com/x").is_err());
    assert!(local("foo.svg#shape").is_err());
    assert!(local("%00.png").is_err());
}

#[test]
fn svg_normalized_native_exif_png_high_depth_gif_canvas_animation_originals_and_legacy_sources() {
    let root = out("normalized");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut inputs = Vec::new();
    for o in 1..=8 {
        inputs.push((
            fixtures.join(format!("jpeg-input/exif{o}.jpg")),
            fixtures.join(format!("jpeg-input/exif{o}.png")),
        ));
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixtures.join("svg-images/manifest.json")).unwrap())
            .unwrap();
    for case in manifest.as_array().unwrap() {
        inputs.push((
            fixtures
                .join("svg-images")
                .join(case["file"].as_str().unwrap()),
            fixtures
                .join("svg-images")
                .join(case["expected"].as_str().unwrap()),
        ));
    }
    let high = root.join("high.png");
    let high_pixels = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_fn(8, 6, |x, y| {
        image::Rgba([
            (x * 7137 + 57) as u16,
            (y * 11439 + 29) as u16,
            21677,
            65535,
        ])
    });
    high_pixels.save(&high).unwrap();
    let high_expected = root.join("high-expected.png");
    image::DynamicImage::ImageRgba16(high_pixels)
        .to_rgba8()
        .save(&high_expected)
        .unwrap();
    inputs.push((high, high_expected));
    let gif = root.join("offset.gif");
    {
        let file = std::fs::File::create(&gif).unwrap();
        let mut encoder =
            gif::Encoder::new(file, 8, 6, &[0, 0, 0, 191, 29, 71, 21, 64, 196]).unwrap();
        for color in [1, 2] {
            let frame = gif::Frame {
                left: 2,
                top: 3,
                width: 4,
                height: 2,
                transparent: Some(0),
                buffer: std::borrow::Cow::Owned(vec![color; 8]),
                ..Default::default()
            };
            encoder.write_frame(&frame).unwrap();
        }
    }
    let gif_expected = root.join("offset-expected.png");
    image::RgbaImage::from_fn(8, 6, |x, y| {
        image::Rgba(if (2..6).contains(&x) && (3..5).contains(&y) {
            [191, 29, 71, 255]
        } else {
            [0; 4]
        })
    })
    .save(&gif_expected)
    .unwrap();
    inputs.push((gif, gif_expected));
    let mut cases = vec![];
    for (index, (path, golden)) in inputs.iter().enumerate() {
        let original = std::fs::read(path).unwrap();
        let expected = image::open(golden).unwrap().to_rgba8();
        let href = format!(
            "file:///{}",
            path.to_string_lossy()
                .replace('\\', "/")
                .replace(' ', "%20")
        );
        let input = root.join(format!("sample-{index}.svg"));
        std::fs::write(
            &input,
            svg(
                &format!(
                    "<image width='{}' height='{}' href='{href}'/>",
                    expected.width(),
                    expected.height()
                ),
                expected.width(),
                expected.height(),
            ),
        )
        .unwrap();
        let opened = crate::files::open(&input).unwrap();
        let doc = opened.document;
        let actual = doc.composite();
        assert_eq!(actual.dimensions(), expected.dimensions());
        for (a, b) in actual.pixels().zip(expected.pixels()) {
            for c in 0..4 {
                assert!(a[c].abs_diff(b[c]) <= 2, "{} {a:?} {b:?}", path.display());
            }
        }
        let xml = roxmltree::Document::parse(source(&doc)).unwrap();
        let node = xml.descendants().find(|n| image_node(*n)).unwrap();
        let bytes = decode_data(
            node.attribute("data-rphoto-image-original")
                .unwrap_or_else(|| node.attribute("href").unwrap()),
        )
        .unwrap()
        .1;
        assert_eq!(bytes, original);
        let project = root.join(format!("sample-{index}.rphoto"));
        crate::files::save_project(&doc, &project).unwrap();
        assert_eq!(
            crate::files::load_project(&project).unwrap().composite(),
            actual
        );
        let output = root.join(format!("sample-{index}.svgz"));
        crate::files::export_by_index(
            &doc,
            &output,
            crate::files::EXPORT_FORMATS.len() + 2,
            &Default::default(),
        )
        .unwrap();
        assert_eq!(
            crate::files::open(&output).unwrap().document.composite(),
            actual
        );
        actual
            .save(root.join(format!("sample-{index}-actual.png")))
            .unwrap();
        let mime = match image::guess_format(&original).unwrap() {
            ImageFormat::Jpeg => "image/jpeg",
            ImageFormat::Png => "image/png",
            ImageFormat::Gif => "image/gif",
            ImageFormat::WebP => "image/webp",
            _ => panic!(),
        };
        let legacy = svg(
            &format!(
                "<image width='{}' height='{}' href='{}'/>",
                expected.width(),
                expected.height(),
                data(mime, &original)
            ),
            expected.width(),
            expected.height(),
        );
        assert_eq!(crate::vector::render_svg(&legacy).unwrap(), actual);
        assert_eq!(std::fs::read(path).unwrap(), original);
        cases.push(serde_json::json!({"file":format!("sample-{index}.svgz"),"input":path,"expected":golden,"actual":format!("sample-{index}-actual.png"),"bytes":original.len()}));
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn svgz_gzip_native_structure_nested_assets_truncation_checksum_budget_and_atomic_output() {
    let root = out("compressed");
    png(&root.join("image.png"), [21, 39, 177, 255]);
    let raw = svg(
        "<!-- retained --><style>#shape{fill:red}</style><defs><path id='shape' d='M0 0H2V2H0Z'/></defs><image width='8' height='6' href='image.png'/><use href='#shape'/>",
        8,
        6,
    );
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(raw.as_bytes()).unwrap();
    let bytes = gzip.finish().unwrap();
    let input = root.join("original.svgz");
    std::fs::write(&input, &bytes).unwrap();
    let doc = crate::files::open(&input).unwrap().document;
    assert!(source(&doc).contains("<!-- retained -->"));
    assert!(source(&doc).contains("<style>#shape{fill:red}</style>"));
    assert_eq!(std::fs::read(&input).unwrap(), bytes);
    let native = crate::svg_export::source(&doc).unwrap();
    let output = root.join("output.SVGZ");
    crate::svg_export::save(&doc, &output).unwrap();
    let mut decoded = String::new();
    flate2::read::GzDecoder::new(std::fs::File::open(&output).unwrap())
        .read_to_string(&mut decoded)
        .unwrap();
    assert_eq!(decoded, native);
    assert_eq!(
        crate::files::open(&output).unwrap().document.composite(),
        doc.composite()
    );
    let plain = root.join("gzip.svg");
    std::fs::write(&plain, &bytes).unwrap();
    assert_eq!(
        crate::files::open(&plain).unwrap().document.composite(),
        doc.composite()
    );
    let nested = root.join("nested.svg");
    std::fs::write(
        &nested,
        svg("<image width='8' height='6' href='original.svgz'/>", 8, 6),
    )
    .unwrap();
    assert_eq!(
        crate::files::open(&nested).unwrap().document.composite(),
        doc.composite()
    );
    for (i, bad) in [
        raw.as_bytes().to_vec(),
        bytes[..bytes.len() - 3].to_vec(),
        {
            let mut b = bytes.clone();
            let n = b.len();
            b[n - 8] ^= 1;
            b
        },
    ]
    .into_iter()
    .enumerate()
    {
        let path = root.join(format!("bad-{i}.svgz"));
        std::fs::write(&path, &bad).unwrap();
        assert!(crate::files::open(&path).is_err());
        assert_eq!(std::fs::read(path).unwrap(), bad);
    }
    let mut budget = LIMIT;
    assert!(expanded(&bytes, &mut budget).is_err());
    let previous = std::fs::read(&output).unwrap();
    let mut broken = doc.clone();
    broken.layers[0].parent = Some(999);
    assert!(crate::svg_export::save_compressed(&broken, &output).is_err());
    assert_eq!(std::fs::read(&output).unwrap(), previous);
    assert_eq!(
        crate::files::export_extension(crate::files::EXPORT_FORMATS.len() + 2).unwrap(),
        "svgz"
    );
    assert!(crate::files::IMAGE_EXTENSIONS.contains(&"svgz"));
}

#[test]
fn svg_image_replacement_keeps_other_objects_geometry_metadata_masks_and_undo_projects() {
    let root = out("replace");
    let first = root.join("first.png");
    let next = root.join("다음 사진.bmp");
    png(&first, [191, 29, 71, 255]);
    image::RgbaImage::from_pixel(13, 9, image::Rgba([21, 64, 196, 255]))
        .save(&next)
        .unwrap();
    let old = data("image/png", &std::fs::read(&first).unwrap());
    let raw = svg(
        &format!(
            "<!-- exact --><style>#target{{opacity:0.9}}</style><defs><mask id='mask'><rect width='12' height='8' fill='white'/></mask></defs><g id='group'><image id='target' x='1' y='1' width='8' height='6' transform='translate(1 0)' mask='url(#mask)' style='image-rendering:optimizeQuality' preserveAspectRatio='none' href='{old}' xlink:href='{old}' data-rphoto-image-original='old-original'/><image id='other' x='10' y='1' width='1' height='1' href='{old}'/></g>"
        ),
        12,
        8,
    );
    let doc = roxmltree::Document::parse(&raw).unwrap();
    let (index, target) = doc
        .descendants()
        .filter(|n| n.is_element())
        .enumerate()
        .find(|(_, n)| n.attribute("id") == Some("target"))
        .unwrap();
    let other = doc
        .descendants()
        .find(|n| n.attribute("id") == Some("other"))
        .unwrap();
    let other_source = raw[other.range()].to_owned();
    let changed = replace_image(&raw, index, &next).unwrap();
    let parsed = roxmltree::Document::parse(&changed.source).unwrap();
    let updated = parsed
        .descendants()
        .find(|n| n.attribute("id") == Some("target"))
        .unwrap();
    for attr in [
        "x",
        "y",
        "width",
        "height",
        "transform",
        "mask",
        "style",
        "preserveAspectRatio",
        "id",
    ] {
        assert_eq!(target.attribute(attr), updated.attribute(attr));
    }
    let other = parsed
        .descendants()
        .find(|n| n.attribute("id") == Some("other"))
        .unwrap();
    assert_eq!(&changed.source[other.range()], other_source);
    assert_eq!(
        updated.attribute("href"),
        updated.attribute((XLINK, "href"))
    );
    assert_eq!(
        decode_data(updated.attribute("data-rphoto-image-original").unwrap())
            .unwrap()
            .1,
        std::fs::read(&next).unwrap()
    );
    assert_eq!(
        updated.attribute("data-rphoto-image-name"),
        Some("다음 사진.bmp")
    );
    assert!(changed.source.contains("<!-- exact -->"));
    assert!(
        changed
            .source
            .contains("<style>#target{opacity:0.9}</style>")
    );
    assert_ne!(changed.pixels, crate::vector::render_svg(&raw).unwrap());
    let old = Content::Svg {
        source: raw.clone(),
    };
    let new = Content::Svg {
        source: changed.source.clone(),
    };
    let mut doc = crate::document::Document::from_image("replace".into(), old.render().unwrap());
    doc.layers[0].content = Some(old);
    doc.layers[0].mask = Some(std::sync::Arc::new(image::GrayImage::from_pixel(
        12,
        8,
        image::Luma([217]),
    )));
    doc.layers[0].offset = [2, -1];
    doc.layers[0].transform.angle = 13.;
    let baseline = doc.composite();
    let mask = doc.layers[0].mask.clone().unwrap();
    let mut editor = Editor::new(doc, true);
    editor.edit("SVG 이미지 교체", |doc| {
        doc.layers[0].content = Some(new);
        doc.layers[0].pixels = std::sync::Arc::new(changed.pixels)
    });
    assert!(std::sync::Arc::ptr_eq(
        &mask,
        editor.document.layers[0].mask.as_ref().unwrap()
    ));
    let final_pixels = editor.document.composite();
    editor.undo();
    assert_eq!(editor.document.composite(), baseline);
    editor.redo();
    assert_eq!(editor.document.composite(), final_pixels);
    assert_eq!(editor.history().count(), 1);
    let project = root.join("replaced.rphoto");
    crate::files::save_project(&editor.document, &project).unwrap();
    let restored = crate::files::load_project(&project).unwrap();
    assert_eq!(restored.composite(), final_pixels);
    assert_eq!(source(&restored), changed.source);
    crate::svg_export::save(&editor.document, &root.join("replaced.svgz")).unwrap();
    let plain = replace_image(&changed.source, index, &first).unwrap();
    let parsed = roxmltree::Document::parse(&plain.source).unwrap();
    let target = parsed
        .descendants()
        .find(|n| n.attribute("id") == Some("target"))
        .unwrap();
    assert!(target.attribute("data-rphoto-image-original").is_none());
    assert_eq!(
        decode_data(target.attribute("href").unwrap()).unwrap().1,
        std::fs::read(&first).unwrap()
    );
    assert!(replace_image(&raw, 0, &next).is_err());
    assert!(replace_image(&raw, index, &root.join("missing.jpg")).is_err());
    assert_eq!(source(&editor.document), changed.source);
    std::fs::write(root.join("before.svg"), raw).unwrap();
    std::fs::write(root.join("after.svg"), changed.source).unwrap();
}

#[test]
fn svg_image_file_extraction_exact_originals_precision_animation_display_and_projects() {
    let root = out("extract");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    // Create inputs locally; this test does not depend on another test's output order.
    let exr = root.join("hdr.exr");
    image::DynamicImage::ImageRgba32F(image::Rgba32FImage::from_pixel(
        8,
        6,
        image::Rgba([2.5, -0.3, 0.4, 0.6]),
    ))
    .save(&exr)
    .unwrap();
    let png = root.join("高精度.png");
    image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
        8,
        6,
        image::Rgba([32771, 17777, 65535, 59999]),
    )
    .save(&png)
    .unwrap();
    let tiff = root.join("source.tiff");
    image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_pixel(
        8,
        6,
        image::Rgba([32771, 17777, 65535, 59999]),
    )
    .save(&tiff)
    .unwrap();
    let webp = root.join("plain.webp");
    image::RgbaImage::from_pixel(8, 6, image::Rgba([21, 64, 196, 255]))
        .save(&webp)
        .unwrap();
    let plain = root.join("plain.png");
    image::RgbaImage::from_pixel(8, 6, image::Rgba([21, 64, 196, 255]))
        .save(&plain)
        .unwrap();
    let inputs = vec![
        png,
        tiff,
        exr,
        webp,
        plain,
        fixtures.join("jpeg-input/exif6.jpg"),
        fixtures.join("svg-images/default-image.apng"),
        fixtures.join("svg-images/animated.webp"),
        fixtures.join("raw/linear-32-o6-crop.dng"),
        fixtures.join("raw/cfa-16-o1.dng"),
        fixtures.join("independent.jp2"),
        fixtures.join("jxl/lossless_pfm/input.jxl"),
    ];
    let mut cases = vec![];
    for (i, path) in inputs.iter().enumerate() {
        let encoded = std::fs::read(path).unwrap();
        let uri = format!(
            "file:///{}",
            path.to_string_lossy()
                .replace('\\', "/")
                .replace(' ', "%20")
        );
        let input = root.join(format!("sample-{i}.svg"));
        std::fs::write(
            &input,
            svg(
                &format!("<image id='target' width='8' height='6' href='{uri}'/>"),
                8,
                6,
            ),
        )
        .unwrap();
        let doc = crate::files::open(&input).unwrap().document;
        let raw = source(&doc).to_owned();
        let original = image_file(&raw, 1, ImageFile::Original).unwrap();
        let display = image_file(&raw, 1, ImageFile::Display).unwrap();
        assert_eq!(original.bytes, encoded);
        assert_eq!(original.name, path.file_name().unwrap().to_string_lossy());
        let originals = root.join(format!("original-{i}"));
        let displays = root.join(format!("display-{i}"));
        std::fs::create_dir_all(&originals).unwrap();
        std::fs::create_dir_all(&displays).unwrap();
        let original_path = originals.join(&original.name);
        let display_path = displays.join(&display.name);
        save_image(&original, &original_path, || true).unwrap();
        save_image(&display, &display_path, || true).unwrap();
        assert_eq!(std::fs::read(&original_path).unwrap(), encoded);
        let restored = crate::files::open(&original_path).unwrap().document;
        let baseline = crate::files::open(path).unwrap().document;
        assert_eq!(
            crate::precision::composite(&restored),
            crate::precision::composite(&baseline)
        );
        assert_eq!(restored.layers[0].bit_depth, baseline.layers[0].bit_depth);
        if let Some(raw) = &baseline.layers[0].raw_source {
            assert_eq!(
                restored.layers[0].raw_source.as_ref().unwrap().bytes,
                raw.bytes
            );
        }
        let project = root.join(format!("sample-{i}.rphoto"));
        crate::files::save_project(&doc, &project).unwrap();
        let restored = crate::files::load_project(&project).unwrap();
        assert_eq!(
            image_file(source(&restored), 1, ImageFile::Original)
                .unwrap()
                .bytes,
            encoded
        );
        assert_eq!(source(&restored), raw);
        let exported = root.join(format!("sample-{i}.svgz"));
        crate::svg_export::save(&doc, &exported).unwrap();
        let reimported = crate::files::open(&exported).unwrap().document;
        let xml = roxmltree::Document::parse(source(&reimported)).unwrap();
        let (index, _) = xml
            .descendants()
            .filter(|n| n.is_element())
            .enumerate()
            .find(|(_, n)| image_node(*n))
            .unwrap();
        assert_eq!(
            image_file(source(&reimported), index, ImageFile::Original)
                .unwrap()
                .bytes,
            encoded
        );
        assert_eq!(std::fs::read(path).unwrap(), encoded);
        assert_eq!(source(&doc), raw);
        cases.push(serde_json::json!({"input":path,"original":original_path,"display":display_path,"name":original.name,"bytes":encoded.len(),"depth":baseline.layers[0].bit_depth}));
    }
    std::fs::write(
        root.join("manifest.json"),
        serde_json::to_vec_pretty(&cases).unwrap(),
    )
    .unwrap();
}
#[test]
fn svg_image_file_extraction_nested_portable_svg_names_legacy_orientation_and_invalid_assets_atomicity()
 {
    let root = out("extract-nested");
    png(&root.join("image.png"), [91, 32, 175, 255]);
    let nested = svg("<image width='8' height='6' href='image.png'/>", 8, 6);
    std::fs::write(root.join("nested.svg"), &nested).unwrap();
    let main = svg("<image width='8' height='6' href='nested.svg'/>", 8, 6);
    std::fs::write(root.join("main.svg"), main).unwrap();
    let doc = crate::files::open(&root.join("main.svg")).unwrap().document;
    let original = image_file(source(&doc), 1, ImageFile::Original).unwrap();
    let display = image_file(source(&doc), 1, ImageFile::Display).unwrap();
    assert_eq!(original.bytes, nested.as_bytes());
    assert_eq!(original.name, "nested.svg");
    assert_eq!(display.name, "nested.svg");
    assert!(
        std::str::from_utf8(&display.bytes)
            .unwrap()
            .contains("data:image/png")
    );
    let portable = root.join("portable");
    std::fs::create_dir_all(&portable).unwrap();
    save_image(&display, &portable.join("nested.svg"), || true).unwrap();
    assert_eq!(
        crate::files::open(&portable.join("nested.svg"))
            .unwrap()
            .document
            .composite(),
        doc.composite()
    );
    let jpeg = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jpeg-input/exif6.jpg");
    let raw = std::fs::read(jpeg).unwrap();
    let legacy = svg(
        &format!("<image href='{}'/>", data("image/jpeg", &raw)),
        8,
        12,
    );
    let original = image_file(&legacy, 1, ImageFile::Original).unwrap();
    let display = image_file(&legacy, 1, ImageFile::Display).unwrap();
    assert_eq!(original.bytes, raw);
    assert_eq!(original.name, "image-1.jpg");
    assert_eq!(display.name, "image-1.png");
    assert_eq!(image::load_from_memory(&display.bytes).unwrap().width(), 8);
    assert_eq!(
        image::load_from_memory(&display.bytes).unwrap().height(),
        12
    );
    let file = root.join("previous.bin");
    std::fs::write(&file, b"previous").unwrap();
    assert!(
        save_image(
            &Resource {
                name: "empty.bin".into(),
                bytes: vec![]
            },
            &file,
            || true
        )
        .is_err()
    );
    assert_eq!(std::fs::read(file).unwrap(), b"previous");
    let cancelled = root.join("cancelled.bin");
    std::fs::write(&cancelled, b"previous").unwrap();
    let checks = std::cell::Cell::new(0);
    let payload = Resource {
        name: "cancelled.bin".into(),
        bytes: vec![17; 128 * 1024],
    };
    assert!(
        save_image(&payload, &cancelled, || {
            let n = checks.get();
            checks.set(n + 1);
            n == 0
        })
        .is_err()
    );
    assert_eq!(std::fs::read(cancelled).unwrap(), b"previous");
    for body in [
        "<rect/>",
        "<image/>",
        "<image href='#shape'/>",
        "<image href='external.png'/>",
        "<image href='data:image/png;base64,broken!'/>",
    ] {
        assert!(image_file(&svg(body, 8, 6), 1, ImageFile::Original).is_err());
    }
    for invalid in [
        "../",
        "C:\\path\\NUL.txt",
        "bad:name.png",
        "..",
        "foo?bar.png",
        "COM1.bin",
        "lpt².png",
    ] {
        assert!(filename(invalid).is_none(), "{invalid}");
    }
    assert_eq!(filename("../../folder/okay.png").unwrap(), "okay.png");
    assert_eq!(filename("名前.png").unwrap(), "名前.png");
}
