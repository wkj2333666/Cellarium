pub const OUTSIDE_DOMAIN: Rgb8 = Rgb8::new(8, 12, 24);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Rgb8 {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}
impl Rgb8 {
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self { red, green, blue }
    }
}

pub fn automatic_palette(count: usize) -> Vec<Rgb8> {
    match count {
        0 => Vec::new(),
        1 => vec![Rgb8::new(245, 245, 245)],
        3 => vec![
            Rgb8::new(255, 0, 0),
            Rgb8::new(0, 255, 0),
            Rgb8::new(0, 0, 255),
        ],
        _ => (0..count)
            .map(|i| {
                let hue = i as f32 / count as f32;
                hsv(hue, 0.72, 1.0)
            })
            .collect(),
    }
}
pub fn composite_pixel(values: &[f32], colors: &[Rgb8]) -> Rgb8 {
    fn component(values: &[f32], colors: &[Rgb8], get: impl Fn(Rgb8) -> u8) -> u8 {
        let remaining = values
            .iter()
            .zip(colors)
            .fold(1.0_f32, |product, (value, color)| {
                product * (1.0 - value.clamp(0.0, 1.0) * f32::from(get(*color)) / 255.0)
            });
        ((1.0 - remaining) * 255.0).round() as u8
    }
    Rgb8::new(
        component(values, colors, |c| c.red),
        component(values, colors, |c| c.green),
        component(values, colors, |c| c.blue),
    )
}
fn hsv(h: f32, s: f32, v: f32) -> Rgb8 {
    let i = (h * 6.0).floor() as i32;
    let f = h * 6.0 - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - f * s);
    let t = v * (1.0 - (1.0 - f) * s);
    let (r, g, b) = match i.rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    Rgb8::new(
        (r * 255.0).round() as u8,
        (g * 255.0).round() as u8,
        (b * 255.0).round() as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn palettes_and_black_are_exact() {
        assert_eq!(automatic_palette(1), vec![Rgb8::new(245, 245, 245)]);
        assert_eq!(
            automatic_palette(3),
            vec![
                Rgb8::new(255, 0, 0),
                Rgb8::new(0, 255, 0),
                Rgb8::new(0, 0, 255)
            ]
        );
        assert_eq!(
            composite_pixel(&[0.0], &[Rgb8::new(245, 245, 245)]),
            Rgb8::new(0, 0, 0)
        );
        assert_eq!(OUTSIDE_DOMAIN, Rgb8::new(8, 12, 24));
    }
}
