#[derive(Copy, Clone)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub w: u8,
}

impl Color {
    pub const BLACK: Color = Color::from_rgbw(0, 0, 0, 0);

    pub const fn from_rgbw(r: u8, g: u8, b: u8, w: u8) -> Self {
        Self {
            r, g, b, w
        }
    }

    pub fn from_hsv(hue: u16, sat: u8, val: u8) -> Self {
        let hue = (((hue as u32) * 1530 + 32768) >> 16) as u16;

        let (r, g, b) = if hue < 510 {
            let (r, g) = if hue < 255 { (255, hue) } else { (510 - hue, 255) };
            (r, g, 0)
        } else if hue < 1020 {
            let (g, b) = if hue < 765 { (255, hue - 510) } else { (1020 - hue, 255) };
            (0, g, b)
        } else if hue < 1530 {
            let (r, b) = if hue < 1275 { (hue - 1020, 255) } else { (255, 1530 - hue) };
            (r, 0, b)
        } else {
            (255, 0, 0)
        };

        let v1 = 1 + (val as u16);
        let s1 = 1 + (sat as u16);
        let s2 = 255 - (sat as u16);

        let r = ((((r * s1) >> 8) + s2) * v1) >> 8;
        let g = ((((g * s1) >> 8) + s2) * v1) >> 8;
        let b = ((((b * s1) >> 8) + s2) * v1) >> 8;

        Self {
            r: r as _, g: g as _, b: b as _, w: 0
        }
    }

    pub fn with_brightness(&self, brightness: u8) -> Color {
        let brightness = 1 + (brightness as u16);
        let r = (self.r as u16 * brightness) >> 8;
        let g = (self.g as u16 * brightness) >> 8;
        let b = (self.b as u16 * brightness) >> 8;
        let w = (self.w as u16 * brightness) >> 8;
        Self {
            r: r as _, g: g as _, b: b as _, w: w as _
        }
    }

    pub fn encode_for_sk6812(&self) -> u32 {
        ((self.r as u32) << 16) | ((self.g as u32) << 24) | ((self.b as u32) << 8) | (self.w as u32)
    }
}
