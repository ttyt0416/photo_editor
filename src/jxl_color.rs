use crate::icc_float::convert;
use image::Rgba32FImage;

// JPEG XL's decoder may return pixels in a different profile from original_icc.
// Convert its actual rendered profile after unassociating alpha. This avoids
// the decoder's automatic HDR-to-SDR tone mapping and keeps float excursions.
pub fn apply(
    pixels: Rgba32FImage,
    bytes: &[u8],
    gray: bool,
    notes: &mut Vec<String>,
) -> Rgba32FImage {
    match convert(&pixels, bytes, gray) {
        Ok(converted) => {
            notes.push("JPEG XL 복원 색상 프로파일을 적용해 작업 sRGB로 변환했습니다.".into());
            converted
        }
        Err(error) => {
            notes.push(format!("JPEG XL 복원 색상 프로파일을 적용하지 못했습니다: {error}. 원래 픽셀과 원본 ICC를 유지합니다."));
            pixels
        }
    }
}
#[cfg(test)]
#[path = "jxl_color_tests.rs"]
mod tests;
