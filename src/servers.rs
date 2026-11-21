//! The default servers shipped with CyberCord, from the concept document §7.
//! Each is a (slug, display name, accent colour, channels). Accents use the
//! GNOME palette so the rail reads as a colour code.

pub struct Server {
    pub slug: &'static str,
    pub name: &'static str,
    /// GNOME accent, as (r, g, b).
    pub accent: (u8, u8, u8),
    pub channels: &'static [&'static str],
    /// Voice channels, Discord-style: three per server, by topic.
    pub voice: &'static [&'static str],
    /// PNG icon (GNOME-style app tile, rendered from `assets/servers/<slug>.svg`).
    pub icon: &'static [u8],
}

pub const SERVERS: &[Server] = &[
    Server {
        slug: "sce",
        name: "SocioCyberneering Comms",
        accent: (0x1C, 0x71, 0xD8), // blue
        channels: &[
            "welcome", "announcements", "rules-and-method",
            "resource-based-economy", "cybernated-society", "circular-cities",
            "sustainable-design", "scientific-method", "energy-abundance",
        ],
        voice: &["lounge", "venus-project-talks", "city-design-meetup"],
        icon: include_bytes!("../assets/servers/sce.png"),
    },
    Server {
        slug: "atom-generator",
        name: "Atom-Generator",
        accent: (0x81, 0x3D, 0x9C), // purple
        channels: &["concept-overview", "feynman-plenty-of-room", "molecular-assembly", "energy-budget", "latex-docs"],
        voice: &["lab-talk", "feynman-reading", "office-hours"],
        icon: include_bytes!("../assets/servers/atom-generator.png"),
    },
    Server {
        slug: "automation",
        name: "Automation",
        accent: (0x2E, 0xC2, 0x7E), // green
        channels: &["kicad-pcb", "power-electronics", "sensors-iot", "embedded-rust", "orange-pi", "hydroponics-automation"],
        voice: &["pcb-review", "embedded-hangout", "hydroponics-lab"],
        icon: include_bytes!("../assets/servers/automation.png"),
    },
    Server {
        slug: "ai-design",
        name: "AI Design",
        accent: (0x21, 0x90, 0xA4), // teal
        channels: &["architecture", "city-simulations", "generative-cad", "claude-news", "prompts-and-tools"],
        voice: &["design-studio", "simulation-jam", "claude-news-live"],
        icon: include_bytes!("../assets/servers/ai-design.png"),
    },
    Server {
        slug: "art",
        name: "ART",
        accent: (0xE6, 0x61, 0x00), // orange
        channels: &["gallery", "digital-art", "generative-art", "wip", "critique"],
        voice: &["studio", "critique-live", "chill-and-draw"],
        icon: include_bytes!("../assets/servers/art.png"),
    },
    Server {
        slug: "creativity",
        name: "Creativity",
        accent: (0xE5, 0xA5, 0x0A), // yellow
        channels: &["ideas-board", "writing", "music", "weekly-challenge", "find-collaborators"],
        voice: &["brainstorm", "writers-room", "jam-session"],
        icon: include_bytes!("../assets/servers/creativity.png"),
    },
    Server {
        slug: "general-semantics",
        name: "General-Semantics",
        accent: (0xC0, 0x1C, 0x28), // red
        channels: &["map-is-not-the-territory", "extensional-devices", "structural-differential", "e-prime", "reading-club"],
        voice: &["discussion", "reading-club-live", "e-prime-practice"],
        icon: include_bytes!("../assets/servers/general-semantics.png"),
    },
    Server {
        slug: "archives",
        name: "Archives",
        accent: (0x86, 0x5E, 0x3C), // brown
        channels: &["physics", "mathematics", "computer-science", "electronics", "paper-requests"],
        voice: &["reading-room", "paper-discussion", "study-together"],
        icon: include_bytes!("../assets/servers/archives.png"),
    },
    Server {
        slug: "help",
        name: "Help",
        accent: (0x5D, 0x73, 0x85), // slate
        channels: &["start-here", "faq", "bug-reports", "feature-requests", "protocol-dev"],
        voice: &["live-help", "onboarding", "dev-sync"],
        icon: include_bytes!("../assets/servers/help.png"),
    },
];

/// Every (guild, channel) pair across all default servers — the set of topics to
/// subscribe to at startup.
pub fn all_channels() -> Vec<crate::types::ChannelId> {
    SERVERS
        .iter()
        .flat_map(|s| s.channels.iter().map(move |c| crate::types::ChannelId::new(s.slug, *c)))
        .collect()
}
