use std::collections::HashSet;
fn string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\a ")
        .replace('\r', "\\d ")
}
fn identifier(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '_' | '-') || !c.is_ascii() {
                c.to_string()
            } else {
                format!("\\{c}")
            }
        })
        .collect()
}
fn ident(source: &str, mut at: usize) -> (usize, String) {
    let mut value = String::new();
    while at < source.len() {
        let c = source[at..].chars().next().unwrap();
        if c == '\\' {
            at += 1;
            if at == source.len() {
                break;
            }
            let start = at;
            while at < source.len() && at - start < 6 && source.as_bytes()[at].is_ascii_hexdigit() {
                at += 1;
            }
            if at > start {
                let code = u32::from_str_radix(&source[start..at], 16).unwrap();
                value.push(
                    char::from_u32(code)
                        .filter(|c| *c != '\0')
                        .unwrap_or('\u{fffd}'),
                );
                if at < source.len() && source.as_bytes()[at].is_ascii_whitespace() {
                    at += 1;
                }
            } else {
                let c = source[at..].chars().next().unwrap();
                value.push(c);
                at += c.len_utf8();
            }
        } else if c.is_alphanumeric() || matches!(c, '_' | '-') || !c.is_ascii() {
            value.push(c);
            at += c.len_utf8();
        } else {
            break;
        }
    }
    (at, value)
}
fn unescape(value: &str) -> String {
    let mut output = String::new();
    let mut at = 0;
    while at < value.len() {
        if value.as_bytes()[at] == b'\\' {
            let (end, text) = ident(value, at);
            output += &text;
            at = end;
        } else {
            let c = value[at..].chars().next().unwrap();
            output.push(c);
            at += c.len_utf8();
        }
    }
    output
}
fn quote_end(source: &str, start: usize) -> usize {
    let quote = source.as_bytes()[start];
    let mut at = start + 1;
    while at < source.len() {
        if source.as_bytes()[at] == b'\\' {
            at = (at + 2).min(source.len());
        } else if source.as_bytes()[at] == quote {
            return at + 1;
        } else {
            at += 1;
        }
    }
    source.len()
}
fn comment_end(source: &str, at: usize) -> usize {
    source[at + 2..]
        .find("*/")
        .map_or(source.len(), |n| at + 2 + n + 2)
}

pub fn urls(value: &str, ids: &HashSet<&str>, prefix: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let mut result = String::new();
    let mut at = 0;
    while at < value.len() {
        if value[at..].starts_with("/*") {
            let end = comment_end(value, at);
            result += &value[at..end];
            at = end;
            continue;
        }
        if matches!(value.as_bytes()[at], b'\'' | b'"') {
            let end = quote_end(value, at);
            result += &value[at..end];
            at = end;
            continue;
        }
        if lower[at..].starts_with("url(") {
            let mut end = at + 4;
            while end < value.len() && value.as_bytes()[end] != b')' {
                if matches!(value.as_bytes()[end], b'\'' | b'"') {
                    end = quote_end(value, end);
                } else {
                    end += 1;
                }
            }
            if end < value.len() {
                let inner = value[at + 4..end].trim();
                let inner = if inner.len() >= 2 && matches!(inner.as_bytes()[0], b'\'' | b'"') {
                    &inner[1..inner.len() - 1]
                } else {
                    inner
                };
                if let Some(raw) = inner.strip_prefix('#')
                    && let id = unescape(raw)
                    && ids.contains(id.as_str())
                {
                    result += &format!("url(\"#{}\")", string(&format!("{prefix}-{id}")));
                    at = end + 1;
                    continue;
                }
            }
        }
        let c = value[at..].chars().next().unwrap();
        result.push(c);
        at += c.len_utf8();
    }
    result
}
fn selector(value: &str, ids: &HashSet<&str>, prefix: &str, root: &str) -> String {
    let mut result = String::new();
    let mut at = 0;
    let mut bracket = None;
    while at < value.len() {
        if value[at..].starts_with("/*") {
            let end = comment_end(value, at);
            result += &value[at..end];
            at = end;
            continue;
        }
        if matches!(value.as_bytes()[at], b'\'' | b'"') {
            let end = quote_end(value, at);
            let token = unescape(&value[at + 1..end.saturating_sub(1)]);
            let before = bracket.map_or("", |start| &value[start..at]);
            let before = before.split_whitespace().collect::<String>();
            let renamed = if let Some(id) = token.strip_prefix('#')
                && ids.contains(id)
            {
                Some(format!("#{prefix}-{id}"))
            } else if matches!(before.as_str(), "id=" | "id~=" | "id|=")
                && ids.contains(token.as_str())
            {
                Some(format!("{prefix}-{token}"))
            } else {
                None
            };
            if let Some(token) = renamed {
                result += &format!("\"{}\"", string(&token));
            } else {
                result += &value[at..end];
            }
            at = end;
            continue;
        }
        if value[at..].starts_with(":root")
            && value[at + 5..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric() && c != '-' && c != '_')
        {
            result += &format!("[{}]", identifier(root));
            at += 5;
            continue;
        }
        if value.as_bytes()[at] == b'#' {
            let (end, id) = ident(value, at + 1);
            if ids.contains(id.as_str()) {
                result += &format!("#{}", identifier(&format!("{prefix}-{id}")));
                at = end;
                continue;
            }
        }
        let c = value[at..].chars().next().unwrap();
        if c == '[' {
            bracket = Some(at + 1);
        } else if c == ']' {
            bracket = None;
        }
        result.push(c);
        at += c.len_utf8();
    }
    result
}
fn delimiter(source: &str, start: usize, targets: &[u8]) -> Option<usize> {
    let mut at = start;
    let mut parentheses = 0;
    let mut brackets = 0;
    while at < source.len() {
        if source[at..].starts_with("/*") {
            at = comment_end(source, at);
            continue;
        }
        if matches!(source.as_bytes()[at], b'\'' | b'"') {
            at = quote_end(source, at);
            continue;
        }
        if source.as_bytes()[at] == b'\\' {
            at += 1;
            if at < source.len() {
                at += source[at..].chars().next().unwrap().len_utf8();
            }
            continue;
        }
        let c = source.as_bytes()[at];
        if parentheses == 0 && brackets == 0 && targets.contains(&c) {
            return Some(at);
        }
        match c {
            b'(' => parentheses += 1,
            b')' => parentheses = (parentheses - 1).max(0),
            b'[' => brackets += 1,
            b']' => brackets = (brackets - 1).max(0),
            _ => {}
        }
        at += source[at..].chars().next().unwrap().len_utf8();
    }
    None
}
fn leading(source: &str) -> usize {
    let mut at = 0;
    loop {
        while at < source.len() && source.as_bytes()[at].is_ascii_whitespace() {
            at += 1;
        }
        if source[at..].starts_with("/*") {
            at = comment_end(source, at);
        } else {
            return at;
        }
    }
}
pub fn stylesheet(
    source: &str,
    ids: &HashSet<&str>,
    prefix: &str,
    scope: &str,
    root: &str,
) -> Result<String, String> {
    rules(source, ids, prefix, scope, root, 0)
}
fn rules(
    source: &str,
    ids: &HashSet<&str>,
    prefix: &str,
    scope: &str,
    root: &str,
    depth: usize,
) -> Result<String, String> {
    if depth > 64 {
        return Err("SVG CSS 중첩이 너무 깊습니다.".into());
    }
    let mut result = String::new();
    let mut at = 0;
    while let Some(open) = delimiter(source, at, b"{;") {
        let raw = &source[at..open];
        let skip = leading(raw);
        result += &raw[..skip];
        let header = &raw[skip..];
        if source.as_bytes()[open] == b';' {
            result += &urls(header, ids, prefix);
            result.push(';');
            at = open + 1;
            continue;
        }
        let mut close = open + 1;
        let mut level = 1;
        loop {
            let found =
                delimiter(source, close, b"{}").ok_or("SVG CSS 괄호가 닫히지 않았습니다.")?;
            if source.as_bytes()[found] == b'{' {
                level += 1;
            } else {
                level -= 1;
            }
            close = found + 1;
            if level == 0 {
                break;
            }
        }
        let body = &source[open + 1..close - 1];
        if header.starts_with('@') {
            result += header;
            result.push('{');
            if ["@media", "@supports", "@layer", "@container", "@document"]
                .iter()
                .any(|name| header.starts_with(name))
            {
                result += &rules(body, ids, prefix, scope, root, depth + 1)?;
            } else {
                result += &urls(body, ids, prefix);
            }
        } else {
            let mut begin = 0;
            loop {
                let end = delimiter(header, begin, b",").unwrap_or(header.len());
                if !scope.is_empty() {
                    result += &format!("#{} ", identifier(scope));
                }
                result += &selector(header[begin..end].trim(), ids, prefix, root);
                if end == header.len() {
                    break;
                }
                result += ", ";
                begin = end + 1;
            }
            result.push('{');
            result += &urls(body, ids, prefix);
        }
        result.push('}');
        at = close;
    }
    result += &source[at..];
    Ok(result)
}

pub fn preview_roots(source: &str) -> Result<String, String> {
    if !source.contains(":root") {
        return Ok(source.to_owned());
    }
    let document = roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let mut marker = "data-photo-editor-preview-root".to_owned();
    while document
        .descendants()
        .any(|n| n.attributes().any(|a| a.name() == marker))
    {
        marker.push('_');
    }
    let root = document.root_element().range();
    let start = root.start
        + 1
        + source[root.start + 1..]
            .find(|c: char| c.is_whitespace() || matches!(c, '/' | '>'))
            .ok_or("SVG 루트가 잘못되었습니다.")?;
    let mut replacements = vec![(start..start, format!(" {marker}=\"\""))];
    for node in document.descendants().filter(|n| n.has_tag_name("style")) {
        let css = node.children().filter_map(|n| n.text()).collect::<String>();
        let css = stylesheet(&css, &HashSet::new(), "", "", &marker)?;
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
    replacements.sort_by_key(|(r, _)| std::cmp::Reverse(r.start));
    let mut output = source.to_owned();
    for (range, value) in replacements {
        output.replace_range(range, &value);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn css_selector_url_root_nested_rules_comments_strings_and_unicode_are_preserved() {
        let ids = HashSet::from(["paint", "shape", "a:b", "12", "한글"]);
        let css = r##"/* keep */ @import url('other.css'); #shape, [href="#shape"], [id='shape'], :root > rect {fill:url( '#paint' );stroke:#fff;content:"url(#paint), #shape { }"} @media screen { #a\:b, #\31 2, #\한글 { fill: URL(#paint) } }"##;
        let output = stylesheet(css, &ids, "layer-2", "scope-2", "root-2").unwrap();
        assert!(output.contains("/* keep */ @import url('other.css');"));
        for expected in [
            "#scope-2 #layer-2-shape",
            "[href=\"#layer-2-shape\"]",
            "[id=\"layer-2-shape\"]",
            "#scope-2 [root-2] > rect",
            "stroke:#fff",
            "content:\"url(#paint), #shape { }\"",
            "#scope-2 #layer-2-a\\:b",
            "#scope-2 #layer-2-12",
            "#scope-2 #layer-2-한글",
            "url(\"#layer-2-paint\")",
        ] {
            assert!(output.contains(expected), "missing {expected}: {output}");
        }
        assert_eq!(urls(r##"url("#a\:b")"##, &ids, "p"), r##"url("#p-a:b")"##);
        assert!(stylesheet(".a {fill:red", &ids, "p", "scope", "root").is_err());
    }
}
