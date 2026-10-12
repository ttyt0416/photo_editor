use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ShapeKind {
    Rectangle,
    Rounded,
    Ellipse,
    Line,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GradientStop {
    pub position: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub enum GradientInterpolation {
    #[default]
    Srgb,
    LinearRgb,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PathNode {
    pub anchor: [f32; 2],
    pub incoming: [f32; 2],
    pub outgoing: [f32; 2],
}
impl PathNode {
    pub fn corner(anchor: [f32; 2]) -> Self {
        Self {
            anchor,
            incoming: anchor,
            outgoing: anchor,
        }
    }
    pub fn move_anchor(&mut self, point: [f32; 2]) {
        for (i, value) in point.iter().enumerate() {
            let delta = value - self.anchor[i];
            self.incoming[i] += delta;
            self.outgoing[i] += delta;
        }
        self.anchor = point;
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Content {
    Solid {
        width: u32,
        height: u32,
        color: [u8; 4],
    },
    Shape {
        kind: ShapeKind,
        width: u32,
        height: u32,
        fill: [u8; 4],
        stroke: [u8; 4],
        thickness: f32,
    },
    Gradient {
        width: u32,
        height: u32,
        start: [f32; 2],
        end: [f32; 2],
        colors: [[u8; 4]; 2],
        #[serde(default)]
        stops: Vec<GradientStop>,
        #[serde(default)]
        interpolation: GradientInterpolation,
    },
    Text {
        text: String,
        font: String,
        size: f32,
        width: u32,
        height: u32,
        color: [u8; 4],
        line_height: f32,
        spacing: f32,
        align: u8,
        #[serde(default)]
        wrap: bool,
        #[serde(default)]
        spans: Vec<crate::rich_text::Span>,
    },
    Svg {
        source: String,
    },
    Path {
        width: u32,
        height: u32,
        nodes: Vec<PathNode>,
        closed: bool,
        fill: [u8; 4],
        stroke: [u8; 4],
        thickness: f32,
    },
}

pub(crate) fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
pub(crate) fn color(c: [u8; 4]) -> String {
    format!("rgba({},{},{},{})", c[0], c[1], c[2], c[3] as f32 / 255.0)
}

impl Content {
    pub fn resize(&mut self, old: [u32; 2], new: [u32; 2]) {
        let sx = new[0] as f32 / old[0] as f32;
        let sy = new[1] as f32 / old[1] as f32;
        match self {
            Self::Solid { width, height, .. } => {
                *width = new[0];
                *height = new[1];
            }
            Self::Path {
                width,
                height,
                nodes,
                thickness,
                ..
            } => {
                *width = new[0];
                *height = new[1];
                *thickness *= (sx * sy).sqrt();
                for node in nodes {
                    for point in [&mut node.anchor, &mut node.incoming, &mut node.outgoing] {
                        point[0] *= sx;
                        point[1] *= sy;
                    }
                }
            }
            Self::Shape {
                width,
                height,
                thickness,
                ..
            } => {
                *width = new[0];
                *height = new[1];
                *thickness *= (sx * sy).sqrt();
            }
            Self::Gradient {
                width,
                height,
                start,
                end,
                ..
            } => {
                *width = new[0];
                *height = new[1];
                for p in [start, end] {
                    p[0] *= sx;
                    p[1] *= sy;
                }
            }
            Self::Text {
                width,
                height,
                size,
                spacing,
                spans,
                ..
            } => {
                *width = new[0];
                *height = new[1];
                *size *= sy;
                *spacing *= sx;
                for span in spans {
                    span.style.size *= sy;
                }
            }
            Self::Svg { source } => {
                if let Some(mut pattern) = crate::pattern::Pattern::read(source) {
                    pattern.resize(new);
                    if let Ok(next) = pattern.source() {
                        *source = next;
                    }
                    return;
                }
                let text = source.trim_start_matches('\u{feff}').trim_start();
                let body = if text.starts_with("<?xml") {
                    text.split_once("?>").map_or(text, |(_, s)| s)
                } else {
                    text
                };
                *source = format!(
                    "<svg xmlns='http://www.w3.org/2000/svg' width='{}' height='{}'><g transform='scale({sx},{sy})'>{body}</g></svg>",
                    new[0], new[1]
                );
            }
        }
    }
    pub fn svg(&self) -> String {
        match self {
            Self::Solid {
                width,
                height,
                color: rgba,
            } => format!(
                "<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='{height}'><rect width='{width}' height='{height}' fill='{}'/></svg>",
                color(*rgba)
            ),
            Self::Path {
                width,
                height,
                nodes,
                closed,
                fill,
                stroke,
                thickness,
            } => {
                let data = path_data(nodes, *closed);
                format!(
                    "<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='{height}'><path d='{data}' fill='{}' stroke='{}' stroke-width='{thickness}' stroke-linecap='round' stroke-linejoin='round'/></svg>",
                    if *closed { color(*fill) } else { "none".into() },
                    color(*stroke)
                )
            }
            Self::Svg { source } => source.clone(),
            Self::Shape {
                kind,
                width,
                height,
                fill,
                stroke,
                thickness,
            } => {
                let t = *thickness;
                let x = t / 2.0;
                let w = (*width as f32 - t).max(0.0);
                let h = (*height as f32 - t).max(0.0);
                let element = match kind {
                    ShapeKind::Rectangle | ShapeKind::Rounded => format!(
                        "<rect x='{x}' y='{x}' width='{w}' height='{h}' rx='{}'/>",
                        if *kind == ShapeKind::Rounded {
                            w.min(h) * 0.15
                        } else {
                            0.0
                        }
                    ),
                    ShapeKind::Ellipse => format!(
                        "<ellipse cx='{}' cy='{}' rx='{}' ry='{}'/>",
                        *width as f32 / 2.0,
                        *height as f32 / 2.0,
                        w / 2.0,
                        h / 2.0
                    ),
                    ShapeKind::Line => format!(
                        "<line x1='{x}' y1='{x}' x2='{}' y2='{}'/>",
                        *width as f32 - x,
                        *height as f32 - x
                    ),
                };
                format!(
                    "<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='{height}'><g fill='{}' stroke='{}' stroke-width='{t}'>{element}</g></svg>",
                    color(*fill),
                    color(*stroke)
                )
            }
            Self::Gradient {
                width,
                height,
                start,
                end,
                colors,
                stops,
                interpolation,
            } => {
                let stops = if stops.is_empty() {
                    vec![
                        GradientStop {
                            position: 0.0,
                            color: colors[0],
                        },
                        GradientStop {
                            position: 1.0,
                            color: colors[1],
                        },
                    ]
                } else {
                    stops.clone()
                };
                let elements = stops
                    .iter()
                    .map(|s| {
                        format!(
                            "<stop offset='{}' stop-color='{}'/>",
                            s.position,
                            color(s.color)
                        )
                    })
                    .collect::<String>();
                format!(
                    "<svg xmlns='http://www.w3.org/2000/svg' width='{width}' height='{height}'><defs><linearGradient id='g' color-interpolation='{}' gradientUnits='userSpaceOnUse' x1='{}' y1='{}' x2='{}' y2='{}'>{elements}</linearGradient></defs><rect width='{width}' height='{height}' fill='url(#g)'/></svg>",
                    if *interpolation == GradientInterpolation::LinearRgb {
                        "linearRGB"
                    } else {
                        "sRGB"
                    },
                    start[0],
                    start[1],
                    end[0],
                    end[1],
                )
            }
            Self::Text { .. } => crate::rich_text::svg(self),
        }
    }
    pub fn validate(&self) -> Result<(), String> {
        crate::rich_text::validate(self)?;
        match self {
            Self::Solid { width, height, .. }
            | Self::Shape { width, height, .. }
            | Self::Gradient { width, height, .. }
            | Self::Text { width, height, .. }
            | Self::Path { width, height, .. } => crate::document::validate_size(*width, *height)?,
            Self::Svg { source } if source.len() > 64 * 1024 * 1024 => {
                return Err("SVG 원본이 저장 한도를 초과합니다.".into());
            }
            Self::Svg { .. } => {}
        }
        match self {
            Self::Text {
                size,
                spacing,
                line_height,
                align,
                ..
            } if !size.is_finite()
                || *size <= 0.0
                || !spacing.is_finite()
                || !line_height.is_finite()
                || *line_height <= 0.0
                || *align > 2 =>
            {
                return Err("텍스트 크기·간격·정렬을 확인하세요.".into());
            }
            Self::Shape { thickness, .. } if !thickness.is_finite() || *thickness < 0.0 => {
                return Err("도형의 선 두께가 유효하지 않습니다.".into());
            }
            Self::Gradient { start, end, .. }
                if start
                    .iter()
                    .chain(end)
                    .any(|v| !v.is_finite() || v.abs() > 1e9) =>
            {
                return Err("그라디언트 좌표가 유효하지 않습니다.".into());
            }
            Self::Path {
                nodes,
                thickness,
                closed,
                ..
            } if nodes.len() < if *closed { 3 } else { 2 }
                || nodes.len() > 16384
                || !thickness.is_finite()
                || !(0.0..=1000.0).contains(thickness)
                || nodes.iter().any(|n| {
                    [n.anchor, n.incoming, n.outgoing]
                        .into_iter()
                        .flatten()
                        .any(|v| !v.is_finite() || v.abs() > 1e9)
                }) =>
            {
                return Err("경로의 점 또는 선 두께가 유효하지 않습니다.".into());
            }
            Self::Gradient { stops, .. }
                if !stops.is_empty()
                    && (stops.len() < 2
                        || stops.len() > 256
                        || stops.iter().any(|s| {
                            !s.position.is_finite() || !(0.0..=1.0).contains(&s.position)
                        })
                        || stops.windows(2).any(|p| p[0].position > p[1].position)) =>
            {
                return Err("그라디언트 색상점의 위치를 확인하세요.".into());
            }
            _ => {}
        }
        Ok(())
    }
    pub fn render(&self) -> Result<RgbaImage, String> {
        self.validate()?;
        if let Self::Solid {
            width,
            height,
            color,
        } = self
        {
            return Ok(RgbaImage::from_pixel(*width, *height, Rgba(*color)));
        }
        if let Self::Gradient {
            width,
            height,
            start,
            end,
            colors,
            stops,
            interpolation: GradientInterpolation::LinearRgb,
        } = self
        {
            use resvg::tiny_skia as skia;
            let stops = gradient_stops(*colors, stops);
            let shader = skia::LinearGradient::new(
                skia::Point::from_xy(start[0], start[1]),
                skia::Point::from_xy(end[0], end[1]),
                stops
                    .iter()
                    .map(|s| {
                        skia::GradientStop::new(
                            s.position,
                            skia::Color::from_rgba8(s.color[0], s.color[1], s.color[2], 255),
                        )
                    })
                    .collect(),
                skia::SpreadMode::Pad,
                skia::Transform::identity(),
            )
            .ok_or("그라디언트 방향을 확인하세요.")?;
            let mut pixmap =
                skia::Pixmap::new(*width, *height).ok_or("그라디언트 메모리가 부족합니다.")?;
            let paint = skia::Paint {
                shader,
                colorspace: skia::ColorSpace::FullSRGBGamma,
                ..Default::default()
            };
            pixmap.fill_rect(
                skia::Rect::from_xywh(0.0, 0.0, *width as f32, *height as f32).unwrap(),
                &paint,
                skia::Transform::identity(),
                None,
            );
            let mut pixels = pixmap_image(&pixmap);
            let direction = [end[0] - start[0], end[1] - start[1]];
            let length = direction[0] * direction[0] + direction[1] * direction[1];
            // Gamma compression must operate on straight RGB, before applying alpha.
            // Compressing premultiplied linear light would brighten translucent colors.
            for (x, y, pixel) in pixels.enumerate_pixels_mut() {
                let t = ((x as f32 + 0.5 - start[0]) * direction[0]
                    + (y as f32 + 0.5 - start[1]) * direction[1])
                    / length;
                let index = stops.partition_point(|s| s.position <= t);
                let alpha = if index == 0 {
                    stops[0].color[3] as f32
                } else if index == stops.len() {
                    stops[index - 1].color[3] as f32
                } else {
                    let a = &stops[index - 1];
                    let b = &stops[index];
                    let mix = (t - a.position) / (b.position - a.position);
                    a.color[3] as f32 + (b.color[3] as f32 - a.color[3] as f32) * mix
                };
                pixel[3] = alpha.round().clamp(0.0, 255.0) as u8;
            }
            return Ok(pixels);
        }
        render_svg(&self.svg())
    }
    pub fn reverse_gradient(&mut self) {
        if let Self::Gradient { colors, stops, .. } = self {
            colors.swap(0, 1);
            for stop in stops.iter_mut() {
                stop.position = 1.0 - stop.position;
            }
            stops.reverse();
        }
    }
}

fn gradient_stops(colors: [[u8; 4]; 2], stops: &[GradientStop]) -> Vec<GradientStop> {
    if stops.is_empty() {
        vec![
            GradientStop {
                position: 0.0,
                color: colors[0],
            },
            GradientStop {
                position: 1.0,
                color: colors[1],
            },
        ]
    } else {
        stops.to_vec()
    }
}

fn pixmap_image(pixmap: &resvg::tiny_skia::Pixmap) -> RgbaImage {
    RgbaImage::from_fn(pixmap.width(), pixmap.height(), |x, y| {
        let p = pixmap.pixel(x, y).unwrap().demultiply();
        Rgba([p.red(), p.green(), p.blue(), p.alpha()])
    })
}

pub fn path_data(nodes: &[PathNode], closed: bool) -> String {
    let Some(first) = nodes.first() else {
        return String::new();
    };
    let mut data = format!("M {} {}", first.anchor[0], first.anchor[1]);
    for (a, b) in nodes
        .iter()
        .zip(nodes.iter().skip(1).chain(closed.then_some(first)))
    {
        data.push_str(&format!(
            " C {} {} {} {} {} {}",
            a.outgoing[0], a.outgoing[1], b.incoming[0], b.incoming[1], b.anchor[0], b.anchor[1]
        ));
    }
    if closed {
        data.push_str(" Z");
    }
    data
}

pub(crate) fn fonts() -> &'static Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut database = resvg::usvg::fontdb::Database::new();
        database.load_system_fonts();
        database.set_sans_serif_family("Malgun Gothic");
        Arc::new(database)
    })
}

pub fn text_lines(
    text: &str,
    font: &str,
    size: f32,
    spacing: f32,
    width: f32,
    wrap: bool,
) -> Vec<String> {
    if !wrap {
        return text.split('\n').map(str::to_string).collect();
    }
    let database = fonts();
    let id = crate::font_picker::face_id(font, false, false);
    id.and_then(|id| {
        database.with_face_data(id, |data, index| {
            rustybuzz::Face::from_slice(data, index).map(|face| {
                wrap_lines(text, width, |line| {
                    let mut buffer = rustybuzz::UnicodeBuffer::new();
                    buffer.push_str(line);
                    buffer.guess_segment_properties();
                    let glyphs = rustybuzz::shape(&face, &[], buffer);
                    glyphs
                        .glyph_positions()
                        .iter()
                        .map(|p| p.x_advance as f32)
                        .sum::<f32>()
                        * size
                        / face.units_per_em() as f32
                        + spacing * line.chars().count().saturating_sub(1) as f32
                })
            })
        })
    })
    .flatten()
    .unwrap_or_else(|| wrap_lines(text, width, |s| s.chars().count() as f32 * (size + spacing)))
}

fn wrap_lines(text: &str, width: f32, measure: impl Fn(&str) -> f32) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let paragraph = paragraph.replace('\t', "    ");
        let mut line = String::new();
        for token in paragraph.split_inclusive(char::is_whitespace) {
            if !line.is_empty() && measure(format!("{line}{token}").trim_end()) > width {
                lines.push(line.trim_end().to_string());
                line.clear();
            }
            let token = if line.is_empty() {
                token.trim_start()
            } else {
                token
            };
            if measure(token.trim_end()) <= width {
                line.push_str(token);
            } else {
                for c in token.chars() {
                    if !line.is_empty() && measure(format!("{line}{c}").trim_end()) > width {
                        lines.push(line.trim_end().to_string());
                        line.clear();
                    }
                    line.push(c);
                }
            }
        }
        lines.push(line.trim_end().to_string());
    }
    lines
}

pub fn render_svg(source: &str) -> Result<RgbaImage, String> {
    crate::svg_assets::validate(source)?;
    let roots = crate::svg_namespace::preview_roots(source)?;
    let prepared = crate::svg_edit::linear_preview(&roots)?;
    let source = prepared.as_str();
    let options = resvg::usvg::Options {
        fontdb: fonts().clone(),
        ..crate::svg_assets::options()
    };
    let tree =
        resvg::usvg::Tree::from_data(source.as_bytes(), &options).map_err(|e| e.to_string())?;
    let size = tree.size().to_int_size();
    crate::document::validate_size(size.width(), size.height())?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(size.width(), size.height())
        .ok_or("벡터 이미지 메모리가 부족합니다.")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap_image(&pixmap))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linear_gradient_reverse_and_legacy_defaults_keep_settings_and_alpha() {
        let source = r#"{"Gradient":{"width":256,"height":2,"start":[0.0,0.0],"end":[256.0,0.0],"colors":[[0,0,0,255],[255,255,255,255]]}}"#;
        let mut content: Content = serde_json::from_str(source).unwrap();
        assert!(matches!(
            &content,
            Content::Gradient {
                interpolation: GradientInterpolation::Srgb,
                ..
            }
        ));
        assert!(content.render().unwrap().get_pixel(127, 0)[0].abs_diff(127) <= 1);
        if let Content::Gradient { interpolation, .. } = &mut content {
            *interpolation = GradientInterpolation::LinearRgb;
        }
        let pixels = content.render().unwrap();
        assert!(
            pixels.get_pixel(127, 0)[0].abs_diff(187) <= 1,
            "{:?}",
            pixels.get_pixel(127, 0)
        );
        if let Content::Gradient { stops, .. } = &mut content {
            *stops = vec![
                GradientStop {
                    position: 0.0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    position: 0.25,
                    color: [0, 255, 0, 128],
                },
                GradientStop {
                    position: 1.0,
                    color: [0, 0, 255, 64],
                },
            ];
        }
        let original = content.clone();
        let image = content.render().unwrap();
        content.reverse_gradient();
        let reversed = content.render().unwrap();
        for x in 0..256 {
            for c in 0..4 {
                assert!(image.get_pixel(x, 0)[c].abs_diff(reversed.get_pixel(255 - x, 0)[c]) <= 2);
            }
        }
        content.reverse_gradient();
        assert_eq!(content, original);
        let mut doc = crate::document::Document::new(256, 2, true).unwrap();
        doc.add_layer("선형 그라디언트".into(), image).unwrap();
        doc.active_layer_mut().unwrap().content = Some(content.clone());
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/colors/linear.rphoto");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        crate::files::save_project(&doc, &path).unwrap();
        let reopened = crate::files::load_project(&path).unwrap();
        assert_eq!(
            reopened.active_layer().unwrap().content.as_ref(),
            Some(&content)
        );
        assert_eq!(reopened.composite(), doc.composite());
    }
    #[test]
    fn legacy_gradient_and_text_defaults_and_multi_stop_alpha_survive_project_roundtrip() {
        let legacy = r#"{"Gradient":{"width":80,"height":10,"start":[0.0,0.0],"end":[80.0,0.0],"colors":[[255,0,0,255],[0,0,255,255]]}}"#;
        let old: Content = serde_json::from_str(legacy).unwrap();
        assert!(matches!(&old, Content::Gradient { stops, .. } if stops.is_empty()));
        assert!(old.render().unwrap().get_pixel(1, 5)[0] > 240);
        let gradient = Content::Gradient {
            width: 80,
            height: 10,
            start: [0.0, 0.0],
            end: [80.0, 0.0],
            colors: [[255, 0, 0, 255], [0, 0, 255, 255]],
            interpolation: crate::vector::GradientInterpolation::Srgb,
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: [255, 0, 0, 255],
                },
                GradientStop {
                    position: 0.5,
                    color: [0, 255, 0, 128],
                },
                GradientStop {
                    position: 1.0,
                    color: [0, 0, 255, 255],
                },
            ],
        };
        let pixels = gradient.render().unwrap();
        let center = pixels.get_pixel(40, 5);
        assert!(center[1] > 240 && center[3].abs_diff(128) < 5);
        let mut doc = crate::document::Document::new(80, 50, true).unwrap();
        doc.add_layer("gradient".into(), pixels).unwrap();
        doc.active_layer_mut().unwrap().content = Some(gradient.clone());
        let path = Content::Path {
            width: 40,
            height: 40,
            nodes: vec![
                PathNode::corner([3.0, 3.0]),
                PathNode::corner([35.0, 3.0]),
                PathNode::corner([20.0, 35.0]),
            ],
            closed: true,
            fill: [255, 40, 10, 255],
            stroke: [0, 0, 0, 255],
            thickness: 2.0,
        };
        doc.add_layer("path".into(), path.render().unwrap())
            .unwrap();
        doc.active_layer_mut().unwrap().content = Some(path.clone());
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("qa-output/vector-editing");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("경로와 그라디언트.rphoto");
        crate::files::save_project(&doc, &file).unwrap();
        let restored = crate::files::load_project(&file).unwrap();
        assert_eq!(restored.layers[1].content, Some(gradient));
        assert_eq!(restored.layers[2].content, Some(path));
        assert_eq!(restored.composite(), doc.composite());
        let mut invalid = doc.clone();
        if let Some(Content::Path { nodes, .. }) = &mut invalid.layers[2].content {
            nodes.pop();
        }
        assert!(crate::files::save_project(&invalid, &file).is_err());
        assert_eq!(
            crate::files::load_project(&file).unwrap().composite(),
            doc.composite()
        );
        let old_text: Content = serde_json::from_str(r#"{"Text":{"text":"abc","font":"Malgun Gothic","size":12.0,"width":100,"height":50,"color":[0,0,0,255],"line_height":1.2,"spacing":0.0,"align":0}}"#).unwrap();
        assert!(matches!(old_text, Content::Text { wrap: false, .. }));
    }
    #[test]
    fn paragraph_wrap_keeps_words_explicit_newlines_and_unicode_and_uses_font_advances() {
        let measured = |s: &str| s.chars().count() as f32;
        assert_eq!(
            wrap_lines("alpha beta gamma\n\n한글문단줄바꿈", 10.0, measured),
            ["alpha beta", "gamma", "", "한글문단줄바꿈"]
        );
        assert_eq!(
            wrap_lines("한글문단줄바꿈", 3.0, measured),
            ["한글문", "단줄바", "꿈"]
        );
        assert_eq!(
            text_lines("a\nb\n", "Malgun Gothic", 20.0, 0.0, 10.0, false),
            ["a", "b", ""]
        );
        let narrow = text_lines(
            "한국어 문단을 상자 폭에 맞게 자동으로 줄바꿈합니다",
            "Malgun Gothic",
            20.0,
            0.0,
            100.0,
            true,
        );
        let wide = text_lines(
            "한국어 문단을 상자 폭에 맞게 자동으로 줄바꿈합니다",
            "Malgun Gothic",
            20.0,
            0.0,
            400.0,
            true,
        );
        assert!(narrow.len() > wide.len());
        assert_eq!(
            narrow.join("").replace(' ', ""),
            "한국어문단을상자폭에맞게자동으로줄바꿈합니다"
        );
    }
    #[test]
    fn editable_shapes_gradient_and_korean_text_render_transparency() {
        let shape = Content::Shape {
            kind: ShapeKind::Ellipse,
            width: 40,
            height: 30,
            fill: [230, 40, 10, 255],
            stroke: [0; 4],
            thickness: 0.0,
        };
        let image = shape.render().unwrap();
        assert_eq!(image.get_pixel(20, 15)[0], 230);
        assert_eq!(image.get_pixel(0, 0)[3], 0);
        let gradient = Content::Gradient {
            width: 30,
            height: 2,
            start: [0.0, 0.0],
            end: [30.0, 0.0],
            colors: [[255, 0, 0, 255], [0, 0, 255, 255]],
            stops: vec![],
            interpolation: crate::vector::GradientInterpolation::Srgb,
        };
        let image = gradient.render().unwrap();
        assert!(image.get_pixel(0, 0)[0] > 240);
        assert!(image.get_pixel(29, 0)[2] > 240);
        let text = Content::Text {
            text: "한글 <테스트>\nPhotos".into(),
            font: "Malgun Gothic".into(),
            size: 24.0,
            width: 240,
            height: 90,
            color: [255; 4],
            line_height: 1.3,
            spacing: 0.0,
            align: 0,
            wrap: false,
            spans: vec![],
        };
        assert!(text.svg().contains("&lt;테스트&gt;"));
        let pixels = text.render().unwrap();
        assert!(pixels.pixels().any(|p| p[3] > 0));
        assert!(pixels.pixels().any(|p| p[3] == 0));
    }
}
