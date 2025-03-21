use crate::render::{ONE, ZERO};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgb {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl Rgb {
    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0);

    pub const fn new(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b }
    }

    pub const fn from_u8(r: u8, g: u8, b: u8) -> Self {
        Self {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
        }
    }

    pub fn lerp(&self, other: Self, a: f32) -> Self {
        let a = a.clamp(0.0, 1.0);
        Self {
            r: self.r * (1.0 - a) + other.r * a,
            g: self.g * (1.0 - a) + other.g * a,
            b: self.b * (1.0 - a) + other.b * a,
        }
    }

    pub fn clamp(&self) -> Self {
        Self {
            r: self.r.clamp(0.0, 1.0),
            g: self.g.clamp(0.0, 1.0),
            b: self.b.clamp(0.0, 1.0),
        }
    }

    pub fn quantize_u8(&self) -> (u8, u8, u8) {
        (
            (self.r * 255.0) as u8,
            (self.g * 255.0) as u8,
            (self.b * 255.0) as u8,
        )
    }

    pub fn write_pulses(&self, pulses: &mut [u32]) {
        fn write_u8(value: u8, buffer: &mut [u32]) {
            let mut mask = 0x80;
            for pulse in buffer.iter_mut().take(8) {
                if value & mask != 0 {
                    *pulse = ONE;
                } else {
                    *pulse = ZERO;
                }
                mask >>= 1;
            }
        }

        let (r, g, b) = self.quantize_u8();
        write_u8(g, &mut pulses[0..8]);
        write_u8(r, &mut pulses[8..16]);
        write_u8(b, &mut pulses[16..24]);
    }
}
