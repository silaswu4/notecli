use ratatui::style::Color;

pub const BG: Color = Color::Rgb(14, 12, 11);
pub const SURFACE: Color = Color::Rgb(22, 20, 18);
pub const SURFACE_HI: Color = Color::Rgb(32, 28, 24);
pub const TEXT: Color = Color::Rgb(232, 224, 208);
pub const TEXT_DIM: Color = Color::Rgb(170, 158, 140);
pub const MUTED: Color = Color::Rgb(118, 108, 94);
pub const DIM: Color = Color::Rgb(68, 62, 54);
pub const BORDER: Color = Color::Rgb(52, 46, 40);

/// warm amber accent. used for active steps, focused elements, brand bits.
pub const ACCENT: Color = Color::Rgb(246, 138, 47);
pub const ACCENT_HI: Color = Color::Rgb(255, 174, 90);
pub const ACCENT_DIM: Color = Color::Rgb(112, 64, 22);

/// cooler accent for selection / cursor / piano-roll notes.
pub const COOL: Color = Color::Rgb(92, 196, 232);
pub const COOL_DIM: Color = Color::Rgb(38, 92, 118);

pub const HOT: Color = Color::Rgb(236, 86, 86);
pub const GREEN: Color = Color::Rgb(130, 196, 134);
pub const PLAYHEAD: Color = Color::Rgb(255, 200, 80);
