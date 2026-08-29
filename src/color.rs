//! Colour arithmetic, kept apart from everything that decides which colours.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    #[must_use]
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parses `#rrggbb`, which is how the palette writes colours.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.strip_prefix('#').unwrap_or(text);
        if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let pair = |at: usize| u8::from_str_radix(hex.get(at..at + 2)?, 16).ok();
        Some(Self::new(pair(0)?, pair(2)?, pair(4)?))
    }

    /// Applies a tint the way the game does: channel by channel.
    #[must_use]
    pub fn multiply(self, tint: Self) -> Self {
        let channel = |a: u8, b: u8| ((u16::from(a) * u16::from(b)) / 255) as u8;
        Self::new(
            channel(self.r, tint.r),
            channel(self.g, tint.g),
            channel(self.b, tint.b),
        )
    }

    /// A step of `weight` from this colour towards another.
    ///
    /// What the game's shader calls `mix`, and it means it literally: the season
    /// tint does not darken the climate tint, it stands in for as much of it as
    /// the season is felt at all.
    #[must_use]
    pub fn mix(self, other: Self, weight: f32) -> Self {
        let weight = weight.clamp(0.0, 1.0);
        let channel = |a: u8, b: u8| {
            (f32::from(a) + (f32::from(b) - f32::from(a)) * weight).clamp(0.0, 255.0) as u8
        };
        Self::new(
            channel(self.r, other.r),
            channel(self.g, other.g),
            channel(self.b, other.b),
        )
    }

    /// Lightens or darkens by a factor, for slope shading.
    #[must_use]
    pub fn scale(self, factor: f32) -> Self {
        let channel = |value: u8| (f32::from(value) * factor).clamp(0.0, 255.0) as u8;
        Self::new(channel(self.r), channel(self.g), channel(self.b))
    }
}
