use crate::{
    blend::BlendMode,
    document::{Document, Layer},
};
use base64::Engine;
use image::RgbaImage;
use std::{
    io::{Cursor, Write},
    path::Path,
};

fn flatten(document: &Document) -> bool {
    document.layers.iter().any(|l| {
        l.visible
            && (l.adjustment_layer
                || l.mask_placement.is_some()
                || l.clipped
                || l.blend != BlendMode::Normal
                || l.effects.enabled()
                || l.adjustments != Default::default()
                || l.precision.is_some()
                || l.transform.warp.is_some_and(|m| !crate::matrix::affine(m)))
    })
}

pub fn report(document: &Document) -> String {
    if flatten(document) {
        "독립 마스크·원근 왜곡·블렌드·클리핑·보정·효과 또는 고정밀 픽셀이 있는 문서는 외관을 유지한 한 장의 PNG를 SVG 안에 포함합니다. 편집 설정은 프로젝트 저장에 보존하세요.".into()
    } else {
        let raster = document
            .layers
            .iter()
            .filter(|l| l.visible && !l.group && l.content.is_none())
            .count();
        format!(
            "도형·경로·그라디언트·텍스트는 벡터로 유지합니다. 앱에서 만든 선형 그라디언트는 SVG 표준 linearRGB와 원래 색상점으로 저장합니다. 가져온 SVG의 원래 요소·그룹·CSS·defs/참조·주석을 유지하고 레이어별 ID와 CSS 범위를 분리합니다. 사진 레이어 {raster}개와 마스크는 PNG로 포함하며, 위치·회전·불투명도와 그룹을 유지합니다."
        )
    }
}

fn png(image: &RgbaImage) -> Result<String, String> {
    let mut bytes = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
    ))
}

fn namespaced(source: &str, prefix: &str) -> Result<String, String> {
    let document = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let root_range = document.root_element().range();
    let ids: std::collections::HashSet<&str> = document
        .descendants()
        .filter_map(|n| n.attribute("id"))
        .collect();
    let scope = format!("photo-editor-{prefix}");
    let mut root_marker = format!("data-photo-editor-root-{prefix}");
    while document
        .descendants()
        .any(|n| n.attributes().any(|a| a.name() == root_marker))
    {
        root_marker.push('_');
    }
    let mut replacements = Vec::new();
    for node in document.descendants().filter(|n| n.is_element()) {
        for attr in node.attributes() {
            let mut value = if attr.name() == "id" {
                format!("{prefix}-{}", attr.value())
            } else if attr.name() == "href"
                && attr
                    .value()
                    .strip_prefix('#')
                    .is_some_and(|id| ids.contains(id))
            {
                format!("#{prefix}-{}", &attr.value()[1..])
            } else {
                crate::svg_namespace::urls(attr.value(), &ids, prefix)
            };
            if matches!(attr.name(), "aria-labelledby" | "aria-describedby") {
                value = attr
                    .value()
                    .split_whitespace()
                    .map(|id| {
                        if ids.contains(id) {
                            format!("{prefix}-{id}")
                        } else {
                            id.to_owned()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
            }
            if value != attr.value() {
                replacements.push((attr.range_value(), crate::vector::escape(&value)));
            }
        }
        if node.tag_name().name() == "style" {
            let css = node.children().filter_map(|n| n.text()).collect::<String>();
            let css = crate::svg_namespace::stylesheet(&css, &ids, prefix, &scope, &root_marker)?;
            // Rebuild only stylesheet text; SVG geometry, hierarchy and attributes stay native.
            let start = node.range().start
                + source[node.range()]
                    .find('>')
                    .ok_or("SVG style 태그가 잘못되었습니다.")?
                + 1;
            let end = node.range().start
                + source[node.range()]
                    .rfind("</")
                    .ok_or("SVG style 태그가 잘못되었습니다.")?;
            replacements.push((start..end, crate::vector::escape(&css)));
        }
    }
    let start = root_range.start
        + 1
        + source[root_range.start + 1..]
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '>'))
            .ok_or("SVG 루트가 잘못되었습니다.")?;
    replacements.push((start..start, format!(" {root_marker}=\"\"")));
    let mut root = source[root_range.clone()].to_string();
    replacements.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    for (range, value) in replacements {
        root.replace_range(
            range.start - root_range.start..range.end - root_range.start,
            &value,
        );
    }
    Ok(root)
}

fn mask(layer: &Layer, defs: &mut String) -> Result<String, String> {
    if !layer.mask_enabled {
        return Ok(String::new());
    }
    let Some(mask) = &layer.mask else {
        return Ok(String::new());
    };
    let pixels = RgbaImage::from_fn(mask.width(), mask.height(), |x, y| {
        image::Rgba([255, 255, 255, mask.get_pixel(x, y)[0]])
    });
    let data = png(&pixels)?;
    let size = [mask.width(), mask.height()];
    let (min, max) = if layer.group {
        layer.transform.bounds(layer.offset, size)
    } else {
        ([0, 0], [size[0] as i32, size[1] as i32])
    };
    let transform = if layer.group {
        format!(" transform='{}'", matrix(layer, size))
    } else {
        String::new()
    };
    defs.push_str(&format!("<mask id='mask-{}' maskUnits='userSpaceOnUse' maskContentUnits='userSpaceOnUse' x='{}' y='{}' width='{}' height='{}'><image x='0' y='0' width='{}' height='{}'{transform} href='{data}'/></mask>",layer.id,min[0],min[1],i64::from(max[0])-i64::from(min[0]),i64::from(max[1])-i64::from(min[1]),size[0],size[1]));
    Ok(format!(" mask='url(#mask-{})'", layer.id))
}

fn matrix(layer: &Layer, size: [u32; 2]) -> String {
    let m = crate::matrix::normalized(layer.transform.mapping(layer.offset, size));
    format!(
        "matrix({} {} {} {} {} {})",
        m[0], m[3], m[1], m[4], m[2], m[5]
    )
}

fn children(
    document: &Document,
    parent: Option<u64>,
    defs: &mut String,
    depth: usize,
) -> Result<String, String> {
    if depth >= 64 {
        return Err("SVG 그룹 깊이가 너무 큽니다.".into());
    }
    let mut source = String::new();
    for layer in document
        .layers
        .iter()
        .filter(|l| l.parent == parent && l.visible && l.opacity > 0.0)
    {
        let mask = mask(layer, defs)?;
        if layer.group {
            let body = children(document, Some(layer.id), defs, depth + 1)?;
            source.push_str(&format!("<g opacity='{}'{mask}>{body}</g>", layer.opacity));
            continue;
        }
        let size = [layer.pixels.width(), layer.pixels.height()];
        let matrix = matrix(layer, size);
        let body = if let Some(content) = layer.content.as_ref() {
            let source = if let crate::vector::Content::Svg { source } = content {
                source.clone()
            } else {
                content.svg()
            };
            namespaced(&source, &format!("layer-{}", layer.id))?
        } else {
            format!(
                "<image width='{}' height='{}' href='{}'/>",
                size[0],
                size[1],
                png(&layer.pixels)?
            )
        };
        source.push_str(&format!(
            "<g id='photo-editor-layer-{}' transform='{matrix}' opacity='{}'{mask}>{body}</g>",
            layer.id, layer.opacity
        ));
    }
    Ok(source)
}

pub fn source(document: &Document) -> Result<String, String> {
    crate::document::validate_size(document.width, document.height)?;
    if !document.hierarchy_valid() {
        return Err("레이어 그룹 구조를 확인하세요.".into());
    }
    let mut defs = String::new();
    let body = if flatten(document) {
        format!(
            "<image width='{}' height='{}' href='{}'/>",
            document.width,
            document.height,
            png(&document.composite())?
        )
    } else {
        children(document, None, &mut defs, 0)?
    };
    Ok(format!(
        "<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}' viewBox='0 0 {} {}'><defs>{defs}</defs>{body}</svg>",
        document.width, document.height, document.width, document.height
    ))
}

pub fn save(document: &Document, path: &Path) -> Result<(), String> {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("svgz"))
    {
        return save_compressed(document, path);
    }
    let source = source(document)?;
    crate::files::atomic_write(path, |file| {
        file.write_all(source.as_bytes()).map_err(|e| e.to_string())
    })
}

pub fn save_compressed(document: &Document, path: &Path) -> Result<(), String> {
    let source = source(document)?;
    crate::files::atomic_write(path, |file| {
        let mut encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
        encoder
            .write_all(source.as_bytes())
            .map_err(|e| e.to_string())?;
        encoder.finish().map_err(|e| e.to_string())?;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector::{Content, GradientStop, PathNode};
    use std::sync::Arc;
    #[test]
    fn linear_gradient_keeps_native_stops_and_standard_interpolation() {
        let mut doc = Document::new(256, 16, true).unwrap();
        let gradient = Content::Gradient {
            width: 256,
            height: 16,
            start: [0.0, 0.0],
            end: [256.0, 0.0],
            colors: [[0, 0, 0, 255], [255; 4]],
            stops: vec![],
            interpolation: crate::vector::GradientInterpolation::LinearRgb,
        };
        doc.add_layer("선형 보간".into(), gradient.render().unwrap())
            .unwrap();
        doc.active_layer_mut().unwrap().content = Some(gradient);
        let path = Content::Path {
            width: 16,
            height: 16,
            nodes: vec![PathNode::corner([2.0, 2.0]), PathNode::corner([14.0, 14.0])],
            closed: false,
            fill: [0; 4],
            stroke: [255, 0, 0, 255],
            thickness: 2.0,
        };
        doc.add_layer("벡터 경로".into(), path.render().unwrap())
            .unwrap();
        doc.active_layer_mut().unwrap().content = Some(path);
        let svg = source(&doc).unwrap();
        assert!(svg.contains("<path") && svg.contains("linearRGB"));
        // The transparent background is the only raster layer.
        assert!(report(&doc).contains("사진 레이어 1개"));
        let parsed = roxmltree::Document::parse(&svg).unwrap();
        assert_eq!(
            parsed
                .descendants()
                .filter(|n| n.has_tag_name("stop"))
                .count(),
            2
        );
        let actual = crate::vector::render_svg(&svg).unwrap();
        let expected = doc.composite();
        for (a, b) in actual.pixels().zip(expected.pixels()) {
            for c in 0..4 {
                assert!(a[c].abs_diff(b[c]) <= 2, "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn linear_native_svg_keeps_alpha_hard_stops_and_reopens_with_matching_light() {
        use crate::vector::GradientInterpolation;
        let mut doc = Document::new(256, 16, true).unwrap();
        let stops = vec![
            GradientStop {
                position: 0.0,
                color: [0, 0, 0, 255],
            },
            GradientStop {
                position: 0.4,
                color: [255, 30, 20, 128],
            },
            GradientStop {
                position: 0.4,
                color: [30, 60, 255, 220],
            },
            GradientStop {
                position: 1.0,
                color: [20, 240, 80, 255],
            },
        ];
        let content = Content::Gradient {
            width: 256,
            height: 16,
            start: [0.0; 2],
            end: [256.0, 0.0],
            colors: [[0; 4], [255; 4]],
            stops: stops.clone(),
            interpolation: GradientInterpolation::LinearRgb,
        };
        doc.add_layer("선형 다중 색상점".into(), content.render().unwrap())
            .unwrap();
        doc.active_layer_mut().unwrap().content = Some(content);
        let svg = source(&doc).unwrap();
        let parsed = roxmltree::Document::parse(&svg).unwrap();
        let nodes: Vec<_> = parsed
            .descendants()
            .filter(|n| n.has_tag_name("stop"))
            .collect();
        assert_eq!(nodes.len(), stops.len());
        assert_eq!(nodes[1].attribute("offset"), Some("0.4"));
        assert_eq!(nodes[2].attribute("offset"), Some("0.4"));
        assert!(
            nodes[1]
                .attribute("stop-color")
                .unwrap()
                .contains("0.5019608")
        );
        let actual = crate::vector::render_svg(&svg).unwrap();
        let expected = doc.composite();
        for (a, b) in actual.pixels().zip(expected.pixels()) {
            assert!(a[3].abs_diff(b[3]) <= 2, "{a:?} {b:?}");
            for c in 0..3 {
                let pa = u16::from(a[c]) * u16::from(a[3]) / 255;
                let pb = u16::from(b[c]) * u16::from(b[3]) / 255;
                assert!(pa.abs_diff(pb) <= 3, "{a:?} {b:?}");
            }
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/text-vector");
        std::fs::create_dir_all(&dir).unwrap();
        save(&doc, &dir.join("선형-색상점.svg")).unwrap();
        actual.save(dir.join("선형-다시열기.png")).unwrap();
    }
    #[test]
    fn rich_text_svg_keeps_editable_runs_including_unicode_weight_and_alpha() {
        let mut doc = Document::new(360, 200, true).unwrap();
        let mut session = crate::text_canvas::Session::new(1, [10.25, 8.5], [20, 30, 40, 255]);
        crate::rich_text::replace_text(
            &mut session.content,
            "한글 스타일 <텍스트>\n두 번째 문단".into(),
        );
        let mut style = crate::rich_text::base(&session.content);
        style.bold = true;
        style.italic = true;
        style.underline = true;
        style.color = [200, 40, 80, 180];
        style.size = 40.0;
        crate::rich_text::apply_style(&mut session.content, 3..6, style);
        doc.add_layer("문단".into(), session.content.render().unwrap())
            .unwrap();
        let layer = doc.active_layer_mut().unwrap();
        layer.content = Some(session.content);
        layer.offset = session.offset;
        layer.transform = session.transform;
        let svg = source(&doc).unwrap();
        let parsed = roxmltree::Document::parse(&svg).unwrap();
        assert!(parsed.descendants().any(|n| n.has_tag_name("text")));
        let span = parsed
            .descendants()
            .find(|n| n.attribute("font-weight") == Some("700"))
            .unwrap();
        assert_eq!(span.text(), Some("스타일"));
        assert_eq!(span.attribute("font-style"), Some("italic"));
        assert_eq!(span.attribute("text-decoration"), Some("underline"));
        assert!(svg.contains("&lt;텍스트&gt;"));
        let image = crate::vector::render_svg(&svg).unwrap();
        assert!(image.pixels().any(|p| p[0] > 100 && p[3] > 0));
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/text-vector");
        std::fs::create_dir_all(&dir).unwrap();
        save(&doc, &dir.join("글자별-스타일.svg")).unwrap();
        image.save(dir.join("텍스트-다시열기.png")).unwrap();
    }
    #[test]
    fn malformed_css_preserves_existing_output_and_original_content() {
        let mut doc = Document::new(8, 8, true).unwrap();
        doc.active_layer_mut().unwrap().content=Some(Content::Svg{source:r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><style>.paint {fill:red</style><rect class="paint" width="8" height="8"/></svg>"#.into()});
        let before = doc.active_layer().unwrap().content.clone();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/svg-native");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("malformed.svg");
        std::fs::write(&path, b"preserve").unwrap();
        assert!(save(&doc, &path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"preserve");
        assert_eq!(doc.active_layer().unwrap().content, before);
    }
    #[test]
    fn native_css_root_specificity_and_cdata_are_preserved() {
        let content=Content::Svg{source:r##"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><style><![CDATA[:root rect {fill:red} rect.paint {fill:blue} #box {stroke:#fff;stroke-width:1}]]></style><rect id="box" class="paint" x="1" y="1" width="6" height="6"/></svg>"##.into()};
        let mut doc = Document::new(8, 8, true).unwrap();
        doc.layers.clear();
        doc.add_layer("CSS".into(), content.render().unwrap())
            .unwrap();
        doc.active_layer_mut().unwrap().content = Some(content);
        let output = source(&doc).unwrap();
        let image = crate::vector::render_svg(&output).unwrap();
        assert_eq!(image, doc.composite());
        assert_eq!(image.get_pixel(4, 4).0, [0, 0, 255, 255]);
    }
    #[test]
    fn imported_native_shapes_styles_defs_use_comments_and_layer_scopes_roundtrip() {
        let mut doc = Document::new(80, 40, true).unwrap();
        doc.layers.clear();
        let mut originals = vec![];
        for (offset, color) in [(0, "red"), (40, "blue")] {
            let imported = format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40" viewBox="0 0 20 20"><!-- original comment --><metadata>retain original structure</metadata><style>:root {{fill:{color};}} rect.paint, circle.paint {{fill:url( '#gradient' );stroke:#fff;stroke-width:0.5}} #shape {{opacity:0.8}} [href='#template'] {{opacity:0.9}}</style><defs><linearGradient id="gradient"><stop stop-color="{color}"/><stop offset="1" stop-color="white"/></linearGradient><rect id="template" width="3" height="3"/></defs><g transform="translate(1 1)"><rect id="shape" class="paint" x="1" y="1" width="7" height="10"/><circle class="paint" cx="13" cy="7" r="4"/><use href="#template" x="10" y="14"/><path fill="{color}" d="M1 15L5 18L1 18Z"/></g></svg>"##
            );
            let content = Content::Svg {
                source: imported.clone(),
            };
            doc.add_layer("imported".into(), content.render().unwrap())
                .unwrap();
            let l = doc.active_layer_mut().unwrap();
            l.offset = [offset, 0];
            l.content = Some(content);
            originals.push((l.id, imported, l.pixels.clone()));
        }
        assert!(!flatten(&doc));
        let output = source(&doc).unwrap();
        let xml = roxmltree::Document::parse(&output).unwrap();
        assert_eq!(
            xml.descendants().filter(|n| n.has_tag_name("rect")).count(),
            4
        );
        assert_eq!(
            xml.descendants()
                .filter(|n| n.has_tag_name("circle"))
                .count(),
            2
        );
        assert_eq!(
            xml.descendants().filter(|n| n.has_tag_name("use")).count(),
            2
        );
        assert_eq!(
            xml.descendants()
                .filter(|n| n.has_tag_name("metadata"))
                .count(),
            2
        );
        let mut ids = std::collections::HashSet::new();
        for id in xml.descendants().filter_map(|n| n.attribute("id")) {
            assert!(ids.insert(id));
        }
        for href in xml
            .descendants()
            .filter_map(|n| n.attribute("href"))
            .filter(|v| v.starts_with('#'))
        {
            assert!(ids.contains(&href[1..]));
        }
        assert!(output.contains("<!-- original comment -->"));
        assert!(output.contains("stroke:#fff"));
        let rendered = crate::vector::render_svg(&output).unwrap();
        let expected = doc.composite();
        for (index, (a, b)) in rendered.pixels().zip(expected.pixels()).enumerate() {
            for c in 0..4 {
                assert!(
                    a[c].abs_diff(b[c]) <= 2,
                    "pixel {index} c{c}: {:?} != {:?}",
                    a.0,
                    b.0
                );
            }
        }
        for (id, original, pixels) in originals {
            let l = doc.layers.iter().find(|l| l.id == id).unwrap();
            assert_eq!(l.content, Some(Content::Svg { source: original }));
            assert!(Arc::ptr_eq(&pixels, &l.pixels));
        }
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/svg-native");
        std::fs::create_dir_all(&dir).unwrap();
        save(&doc, &dir.join("native.svg")).unwrap();
        crate::files::save_project(&doc, &dir.join("native.rphoto")).unwrap();
        assert!(report(&doc).contains("원래 요소"));
    }
    #[test]
    fn imported_svg_css_use_and_whitespace_ids_stay_distinct_on_export() {
        let mut doc = Document::new(40, 20, true).unwrap();
        for (offset, color) in [(0, "red"), (20, "blue")] {
            let content = Content::Svg {
                source: format!(
                    "<svg xmlns='http://www.w3.org/2000/svg' width='20' height='20'><style>#item {{ fill: url(#gradient); }}</style><defs><linearGradient id = 'gradient'><stop offset='0' stop-color='{color}'/><stop offset='1' stop-color='white'/></linearGradient><rect id = 'item' width='20' height='20'/></defs><use href='#item'/></svg>"
                ),
            };
            doc.add_layer("SVG".into(), content.render().unwrap())
                .unwrap();
            let l = doc.active_layer_mut().unwrap();
            l.content = Some(content);
            l.offset = [offset, 0];
        }
        let output = source(&doc).unwrap();
        assert!(output.contains("<path") || output.contains("<rect"));
        let image = crate::vector::render_svg(&output).unwrap();
        let expected = doc.composite();
        for x in [2, 10, 18, 22, 30, 38] {
            for c in 0..4 {
                assert!(image.get_pixel(x, 10)[c].abs_diff(expected.get_pixel(x, 10)[c]) <= 1);
            }
        }
    }
    #[test]
    fn mixed_svg_keeps_paths_text_gradients_masks_groups_and_distinct_gradient_ids() {
        let mut doc = Document::new(80, 40, true).unwrap();
        for (offset, colors) in [
            ([0, 0], [[255, 0, 0, 255], [0, 255, 0, 255]]),
            ([40, 0], [[0, 0, 255, 255], [255, 255, 0, 255]]),
        ] {
            let content = Content::Gradient {
                width: 40,
                height: 20,
                start: [0.0, 0.0],
                end: [40.0, 0.0],
                colors,
                interpolation: crate::vector::GradientInterpolation::Srgb,
                stops: vec![
                    GradientStop {
                        position: 0.0,
                        color: colors[0],
                    },
                    GradientStop {
                        position: 1.0,
                        color: colors[1],
                    },
                ],
            };
            doc.add_layer("gradient".into(), content.render().unwrap())
                .unwrap();
            let l = doc.active_layer_mut().unwrap();
            l.content = Some(content);
            l.offset = offset;
        }
        doc.active_layer_mut().unwrap().mask = Some(Arc::new(image::GrayImage::from_pixel(
            40,
            20,
            image::Luma([128]),
        )));
        let text = Content::Text {
            text: "한글 & <SVG>".into(),
            font: "Malgun Gothic".into(),
            size: 10.0,
            width: 80,
            height: 20,
            color: [0, 0, 0, 255],
            line_height: 1.2,
            spacing: 0.0,
            align: 0,
            wrap: false,
            spans: vec![],
        };
        doc.add_layer("text".into(), text.render().unwrap())
            .unwrap();
        doc.active_layer_mut().unwrap().content = Some(text);
        doc.active_layer_mut().unwrap().offset = [0, 20];
        let path = Content::Path {
            width: 20,
            height: 20,
            nodes: vec![PathNode::corner([2.0, 2.0]), PathNode::corner([18.0, 18.0])],
            closed: false,
            fill: [0; 4],
            stroke: [255, 0, 0, 255],
            thickness: 2.0,
        };
        doc.add_layer("path".into(), path.render().unwrap())
            .unwrap();
        doc.active_layer_mut().unwrap().content = Some(path);
        doc.group_active().unwrap();
        let source = source(&doc).unwrap();
        assert!(
            source.contains("<path")
                && source.contains("<text")
                && source.contains("<linearGradient")
        );
        assert!(source.contains("layer-2-g") && source.contains("layer-3-g"));
        assert!(source.contains("<mask") && source.contains("data:image/png;base64,"));
        let rendered = crate::vector::render_svg(&source).unwrap();
        let expected = doc.composite();
        for p in [[10, 10], [30, 10], [50, 10], [70, 10]] {
            let a = rendered.get_pixel(p[0], p[1]);
            let b = expected.get_pixel(p[0], p[1]);
            for c in 0..4 {
                assert!(
                    a[c].abs_diff(b[c]) <= 2,
                    "SVG color mismatch {p:?}: {a:?} != {b:?}"
                );
            }
        }
    }
    #[test]
    fn unsupported_blends_flatten_with_a_report_and_svg_reopens() {
        let mut doc = Document::new(16, 16, false).unwrap();
        doc.add_layer(
            "overlay".into(),
            RgbaImage::from_pixel(16, 16, image::Rgba([255, 0, 0, 128])),
        )
        .unwrap();
        doc.active_layer_mut().unwrap().blend = BlendMode::Multiply;
        assert!(report(&doc).contains("한 장"));
        let source = source(&doc).unwrap();
        assert_eq!(crate::vector::render_svg(&source).unwrap(), doc.composite());
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/svg-export");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("한글.svg");
        save(&doc, &path).unwrap();
        assert_eq!(
            crate::files::open(&path).unwrap().document.composite(),
            doc.composite()
        );
    }
}
