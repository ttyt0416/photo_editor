/// Pixel-edge coordinates matching the EXIF transforms already applied during RAW development.
pub(super) fn size(size: [u32; 2], orientation: u8) -> [u32; 2] {
    if orientation >= 5 {
        [size[1], size[0]]
    } else {
        size
    }
}
pub(super) fn forward([x, y]: [f64; 2], [w, h]: [u32; 2], orientation: u8) -> [f64; 2] {
    let (w, h) = (f64::from(w), f64::from(h));
    match orientation {
        2 => [w - x, y],
        3 => [w - x, h - y],
        4 => [x, h - y],
        5 => [y, x],
        6 => [h - y, x],
        7 => [h - y, w - x],
        8 => [y, w - x],
        _ => [x, y],
    }
}
pub(super) fn inverse(point: [f64; 2], oriented_size: [u32; 2], orientation: u8) -> [f64; 2] {
    forward(
        point,
        oriented_size,
        match orientation {
            6 => 8,
            8 => 6,
            value => value,
        },
    )
}
pub(super) const LABELS: [&str; 8] = [
    "기본 방향",
    "좌우 뒤집기",
    "180° 회전",
    "상하 뒤집기",
    "대각선 뒤집기",
    "시계 방향 90°",
    "반대 대각선 뒤집기",
    "반시계 방향 90°",
];

pub(super) fn pixel([x, y]: [u32; 2], [w, h]: [u32; 2], orientation: u8) -> [u32; 2] {
    match orientation {
        2 => [w - 1 - x, y],
        3 => [w - 1 - x, h - 1 - y],
        4 => [x, h - 1 - y],
        5 => [y, x],
        6 => [h - 1 - y, x],
        7 => [h - 1 - y, w - 1 - x],
        8 => [y, w - 1 - x],
        _ => [x, y],
    }
}
