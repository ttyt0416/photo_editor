pub type Matrix = [f64; 9];
pub const IDENTITY: Matrix = [1., 0., 0., 0., 1., 0., 0., 0., 1.];
pub fn multiply(a: Matrix, b: Matrix) -> Matrix {
    std::array::from_fn(|i| (0..3).map(|k| a[i / 3 * 3 + k] * b[k * 3 + i % 3]).sum())
}
pub fn translate(x: f64, y: f64) -> Matrix {
    [1., 0., x, 0., 1., y, 0., 0., 1.]
}
pub fn inverse(m: Matrix) -> Option<Matrix> {
    let c = [
        m[4] * m[8] - m[5] * m[7],
        m[2] * m[7] - m[1] * m[8],
        m[1] * m[5] - m[2] * m[4],
        m[5] * m[6] - m[3] * m[8],
        m[0] * m[8] - m[2] * m[6],
        m[2] * m[3] - m[0] * m[5],
        m[3] * m[7] - m[4] * m[6],
        m[1] * m[6] - m[0] * m[7],
        m[0] * m[4] - m[1] * m[3],
    ];
    let det = m[0] * c[0] + m[1] * c[3] + m[2] * c[6];
    if !det.is_finite() || det.abs() < 1e-14 {
        return None;
    }
    Some(c.map(|v| v / det))
}
pub fn point(m: Matrix, p: [f32; 2]) -> [f32; 2] {
    let x = p[0] as f64;
    let y = p[1] as f64;
    let w = m[6] * x + m[7] * y + m[8];
    if !w.is_finite() || w.abs() < 1e-12 {
        return [f32::NAN; 2];
    }
    [
        ((m[0] * x + m[1] * y + m[2]) / w) as f32,
        ((m[3] * x + m[4] * y + m[5]) / w) as f32,
    ]
}
pub fn normalized(m: Matrix) -> Matrix {
    let divisor = if m[8].abs() > 1e-12 {
        m[8]
    } else {
        m.iter().copied().map(f64::abs).fold(0., f64::max)
    };
    m.map(|v| v / divisor)
}
pub fn affine(m: Matrix) -> bool {
    m[6].abs() < 1e-10 && m[7].abs() < 1e-10 && m[8].abs() > 1e-12
}
pub fn quad_valid(q: [[f32; 2]; 4]) -> bool {
    if q.iter().flatten().any(|v| !v.is_finite() || v.abs() > 1e9) {
        return false;
    }
    let cross = std::array::from_fn::<_, 4, _>(|i| {
        let a = q[i];
        let b = q[(i + 1) % 4];
        let c = q[(i + 2) % 4];
        (b[0] - a[0]) as f64 * (c[1] - b[1]) as f64 - (b[1] - a[1]) as f64 * (c[0] - b[0]) as f64
    });
    cross.iter().all(|v| *v > 1e-10) || cross.iter().all(|v| *v < -1e-10)
}
pub fn unit_quad(q: [[f32; 2]; 4]) -> Option<Matrix> {
    if !quad_valid(q) {
        return None;
    }
    let q = q.map(|p| p.map(f64::from));
    let dx1 = q[1][0] - q[2][0];
    let dx2 = q[3][0] - q[2][0];
    let dx3 = q[0][0] - q[1][0] + q[2][0] - q[3][0];
    let dy1 = q[1][1] - q[2][1];
    let dy2 = q[3][1] - q[2][1];
    let dy3 = q[0][1] - q[1][1] + q[2][1] - q[3][1];
    let (g, h) = if dx3.abs() + dy3.abs() < 1e-10 {
        (0., 0.)
    } else {
        let det = dx1 * dy2 - dx2 * dy1;
        if det.abs() < 1e-14 {
            return None;
        }
        ((dx3 * dy2 - dx2 * dy3) / det, (dx1 * dy3 - dx3 * dy1) / det)
    };
    let m = [
        q[1][0] - q[0][0] + g * q[1][0],
        q[3][0] - q[0][0] + h * q[3][0],
        q[0][0],
        q[1][1] - q[0][1] + g * q[1][1],
        q[3][1] - q[0][1] + h * q[3][1],
        q[0][1],
        g,
        h,
        1.,
    ];
    let w = [1., 1. + g, 1. + g + h, 1. + h];
    (w.iter().all(|v| *v > 1e-8) && inverse(m).is_some()).then_some(m)
}
