use image::{DynamicImage, ImageBuffer};

pub(crate) fn next_box(bytes: &[u8], at: usize) -> Result<([u8; 4], &[u8], usize), String> {
    let fail = || "JP2 박스 길이·색상 정보가 손상되었습니다.".to_string();
    let header = bytes
        .get(at..at.checked_add(8).ok_or_else(fail)?)
        .ok_or_else(fail)?;
    let size = u32::from_be_bytes(header[..4].try_into().unwrap());
    let (size, header_size) = match size {
        0 => (bytes.len() - at, 8),
        1 => (
            usize::try_from(u64::from_be_bytes(
                bytes
                    .get(at + 8..at + 16)
                    .ok_or_else(fail)?
                    .try_into()
                    .unwrap(),
            ))
            .map_err(|_| fail())?,
            16,
        ),
        size => (size as usize, 8),
    };
    if size < header_size {
        return Err(fail());
    }
    let end = at
        .checked_add(size)
        .filter(|end| *end <= bytes.len())
        .ok_or_else(fail)?;
    Ok((
        header[4..8].try_into().unwrap(),
        &bytes[at + header_size..end],
        end,
    ))
}

pub(crate) fn profile(bytes: &[u8]) -> Result<Option<&[u8]>, String> {
    if bytes.starts_with(&[0xff, 0x4f]) {
        return Ok(None);
    }
    let mut at = 0;
    while at < bytes.len() {
        let (kind, body, next) = next_box(bytes, at)?;
        at = next;
        if &kind != b"jp2h" {
            continue;
        }
        let mut child = 0;
        while child < body.len() {
            let (kind, payload, next) = next_box(body, child)?;
            child = next;
            if &kind != b"colr" {
                continue;
            }
            if payload.len() < 3 {
                return Err("JP2 색상 지정 박스가 손상되었습니다.".into());
            }
            // Match the decoder: the first supported colour specification wins.
            // Unsupported methods do not override a later supported COLR.
            match payload[0] {
                1 => return Ok(None),
                2 => return Ok(Some(&payload[3..])),
                _ => {}
            }
        }
        return Ok(None);
    }
    Ok(None)
}

pub fn apply(
    image: DynamicImage,
    components: &[jpeg2k::ImageComponent],
    bytes: &[u8],
    notes: &mut Vec<String>,
) -> DynamicImage {
    let profile = match profile(bytes) {
        Ok(Some(profile)) => profile,
        Ok(None) => return image,
        Err(error) => {
            notes.push(format!(
                "JPEG2000 ICC 정보를 읽지 못했습니다: {error} 원래 픽셀을 유지합니다."
            ));
            return image;
        }
    };
    let gray = profile.get(16..20) == Some(b"GRAY");
    let color_count = if gray { 1 } else { 3 };
    if components.len() < color_count
        || components.len() > color_count + 1
        || components[..color_count].iter().any(|c| c.is_alpha())
        || (components.len() == color_count + 1 && !components[color_count].is_alpha())
    {
        notes.push(
            "JPEG2000 ICC 색/알파 채널 정의를 아직 변환할 수 없습니다. 원래 픽셀을 유지했습니다."
                .into(),
        );
        return image;
    }
    let gray_image = if gray {
        let (w, h) = (image.width(), image.height());
        match &image {
            DynamicImage::ImageRgba8(pixels) => Some(DynamicImage::ImageLumaA8(
                ImageBuffer::from_raw(w, h, pixels.pixels().flat_map(|p| [p[0], p[3]]).collect())
                    .unwrap(),
            )),
            DynamicImage::ImageRgba16(pixels) => Some(DynamicImage::ImageLumaA16(
                ImageBuffer::from_raw(w, h, pixels.pixels().flat_map(|p| [p[0], p[3]]).collect())
                    .unwrap(),
            )),
            _ => None,
        }
    } else {
        None
    };
    match crate::color_management::convert(gray_image.as_ref().unwrap_or(&image), profile) {
        Ok(converted) => {
            notes.push("내장 ICC 색상 프로파일을 적용해 작업 sRGB로 변환했습니다.".into());
            converted
        }
        Err(error) => {
            notes.push(format!(
                "내장 ICC 색상 프로파일을 적용하지 못했습니다: {error}. 원래 픽셀을 유지했습니다."
            ));
            image
        }
    }
}
