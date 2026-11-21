//! Colour themes. The interface reads every colour through [`p()`], so a theme
//! switch repaints the whole app on the next frame.
//!
//! "Discord-style" and "Kali-style" are colour schemes inspired by those
//! products' dark themes — CyberCord's own look, no logos or artwork.

use std::cell::Cell;

use eframe::egui::Color32;

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub dark: bool,
    /// Text in a monospace terminal face.
    pub mono: bool,
    /// Chat area.
    pub window: Color32,
    /// Side panels.
    pub surface: Color32,
    /// Server rail.
    pub rail: Color32,
    /// Buttons, inputs, cards inside panels.
    pub card: Color32,
    pub line: Color32,
    pub ink: Color32,
    pub ink2: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    /// Text / glyphs drawn on accent, green or red fills.
    pub on_accent: Color32,
    /// Light accent: waveform bars not yet played, soft highlights.
    pub peri: Color32,
    /// Selected channel / row background.
    pub sel: Color32,
    pub green: Color32,
    /// Green that is readable as text on `window`.
    pub green_text: Color32,
    pub red: Color32,
}

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub struct Theme {
    pub id: &'static str,
    /// i18n key of the display name.
    pub name_key: &'static str,
    pub pal: Palette,
}

pub const THEMES: &[Theme] = &[
    Theme {
        id: "gnome-light",
        name_key: "theme_gnome_light",
        pal: Palette {
            dark: false,
            mono: false,
            window: hex(0xFFFFFF),
            surface: hex(0xF6F5F4),
            rail: hex(0xECEBE9),
            card: hex(0xF2F1EF),
            line: hex(0xDEDDDA),
            ink: hex(0x241F31),
            ink2: hex(0x5E5C64),
            accent: hex(0x3584E4),
            accent_hover: hex(0x1C71D8),
            on_accent: hex(0xFFFFFF),
            peri: hex(0x99C1F1),
            sel: hex(0xD6E6FB),
            green: hex(0x2EC27E),
            green_text: hex(0x26A269),
            red: hex(0xE01B24),
        },
    },
    Theme {
        id: "gnome-dark",
        name_key: "theme_gnome_dark",
        pal: Palette {
            dark: true,
            mono: false,
            window: hex(0x242424),
            surface: hex(0x2E2E32),
            rail: hex(0x1E1E20),
            card: hex(0x3A3A3E),
            line: hex(0x45454A),
            ink: hex(0xFFFFFF),
            ink2: hex(0xC0BFBC),
            accent: hex(0x3584E4),
            accent_hover: hex(0x62A0EA),
            on_accent: hex(0xFFFFFF),
            peri: hex(0x4A5D78),
            sel: hex(0x2F4B70),
            green: hex(0x2EC27E),
            green_text: hex(0x57E389),
            red: hex(0xE01B24),
        },
    },
    Theme {
        id: "discord-dark",
        name_key: "theme_discord_dark",
        pal: Palette {
            dark: true,
            mono: false,
            window: hex(0x313338),
            surface: hex(0x2B2D31),
            rail: hex(0x1E1F22),
            card: hex(0x383A40),
            line: hex(0x3F4147),
            ink: hex(0xF2F3F5),
            ink2: hex(0xB5BAC1),
            accent: hex(0x5865F2),
            accent_hover: hex(0x4752C4),
            on_accent: hex(0xFFFFFF),
            peri: hex(0x4E5058),
            sel: hex(0x404249),
            green: hex(0x23A55A),
            green_text: hex(0x23A55A),
            red: hex(0xDA373C),
        },
    },
    Theme {
        id: "discord-light",
        name_key: "theme_discord_light",
        pal: Palette {
            dark: false,
            mono: false,
            window: hex(0xFFFFFF),
            surface: hex(0xF2F3F5),
            rail: hex(0xE3E5E8),
            card: hex(0xEBEDEF),
            line: hex(0xE1E2E4),
            ink: hex(0x060607),
            ink2: hex(0x4E5058),
            accent: hex(0x5865F2),
            accent_hover: hex(0x4752C4),
            on_accent: hex(0xFFFFFF),
            peri: hex(0xC9CDFB),
            sel: hex(0xD7D9DC),
            green: hex(0x23A55A),
            green_text: hex(0x1A7F45),
            red: hex(0xDA373C),
        },
    },
    Theme {
        id: "discord-midnight",
        name_key: "theme_discord_midnight",
        pal: Palette {
            dark: true,
            mono: false,
            window: hex(0x000000),
            surface: hex(0x0E0E10),
            rail: hex(0x000000),
            card: hex(0x1A1A1E),
            line: hex(0x26262B),
            ink: hex(0xEDEDEF),
            ink2: hex(0x9A9DA6),
            accent: hex(0x5865F2),
            accent_hover: hex(0x4752C4),
            on_accent: hex(0xFFFFFF),
            peri: hex(0x34363D),
            sel: hex(0x232328),
            green: hex(0x23A55A),
            green_text: hex(0x2DC26B),
            red: hex(0xDA373C),
        },
    },
    Theme {
        id: "kali-dark",
        name_key: "theme_kali_dark",
        pal: Palette {
            dark: true,
            mono: false,
            window: hex(0x1F2129),
            surface: hex(0x23252E),
            rail: hex(0x16171D),
            card: hex(0x2C2F3A),
            line: hex(0x353846),
            ink: hex(0xE7E8EC),
            ink2: hex(0x9EA4B3),
            accent: hex(0x367BF0),
            accent_hover: hex(0x2568DB),
            on_accent: hex(0xFFFFFF),
            peri: hex(0x3B4664),
            sel: hex(0x2C3A5A),
            green: hex(0x27C46B),
            green_text: hex(0x3DDC84),
            red: hex(0xE5484D),
        },
    },
    Theme {
        id: "kali-purple",
        name_key: "theme_kali_purple",
        pal: Palette {
            dark: true,
            mono: false,
            window: hex(0x1A1524),
            surface: hex(0x211A2E),
            rail: hex(0x130F1B),
            card: hex(0x2C2340),
            line: hex(0x3A2E54),
            ink: hex(0xEFE9FA),
            ink2: hex(0xAEA2C7),
            accent: hex(0x9B5CFF),
            accent_hover: hex(0x8243F0),
            on_accent: hex(0xFFFFFF),
            peri: hex(0x4A3A6E),
            sel: hex(0x3B2B60),
            green: hex(0x2EC27E),
            green_text: hex(0x57E389),
            red: hex(0xF0506E),
        },
    },
    Theme {
        id: "kali-terminal",
        name_key: "theme_kali_terminal",
        pal: Palette {
            dark: true,
            mono: true,
            window: hex(0x0B0F0B),
            surface: hex(0x0E140F),
            rail: hex(0x060906),
            card: hex(0x132017),
            line: hex(0x1E3524),
            ink: hex(0xC4F5CC),
            ink2: hex(0x6FAF7B),
            accent: hex(0x1ED760),
            accent_hover: hex(0x17B14E),
            on_accent: hex(0x04140A),
            peri: hex(0x1F4A2B),
            sel: hex(0x163A22),
            green: hex(0x1ED760),
            green_text: hex(0x1ED760),
            red: hex(0xFF4D4D),
        },
    },
];

thread_local! {
    static CURRENT: Cell<Palette> = const { Cell::new(THEMES[0].pal) };
}

/// The active palette.
pub fn p() -> Palette {
    CURRENT.with(|c| c.get())
}

/// Activate a theme by id (unknown or empty → GNOME light); returns its palette.
pub fn set(id: &str) -> Palette {
    let pal = THEMES.iter().find(|t| t.id == id).map(|t| t.pal).unwrap_or(THEMES[0].pal);
    CURRENT.with(|c| c.set(pal));
    pal
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Relative luminance contrast (WCAG) between two colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let l = |c: Color32| {
            let f = |v: u8| {
                let x = v as f32 / 255.0;
                if x <= 0.03928 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        };
        let (x, y) = (l(a), l(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn every_theme_is_readable() {
        for t in THEMES {
            let p = t.pal;
            assert!(contrast(p.ink, p.window) >= 7.0, "{} ink", t.id);
            assert!(contrast(p.ink, p.surface) >= 7.0, "{} ink/surface", t.id);
            assert!(contrast(p.ink2, p.window) >= 4.5, "{} ink2", t.id);
            assert!(contrast(p.ink2, p.surface) >= 4.0, "{} ink2/surface", t.id);
            assert!(contrast(p.on_accent, p.accent) >= 3.0, "{} button text", t.id);
            assert!(contrast(p.green_text, p.window) >= 3.0, "{} green text", t.id);
        }
    }
}
