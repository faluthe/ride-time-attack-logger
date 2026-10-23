//! Shared theme tokens (`assets/theme.json`, synced from `../theme/tokens.json`) applied to ImGui.
//!
//! UI code asks for colours by role (`theme.c("throttle")`) and never hard-codes values.

use serde::Deserialize;
use std::collections::HashMap;

const TOKENS: &str = include_str!("../assets/theme.json");

pub type Rgba = [f32; 4];

#[derive(Deserialize)]
struct Tokens {
    modes: HashMap<String, HashMap<String, String>>,
    radius: HashMap<String, f32>,
    space: HashMap<String, f32>,
    font: FontTokens,
}

#[derive(Deserialize)]
struct FontTokens {
    ui: String,
    mono: String,
    size: f32,
    #[serde(rename = "size-small")]
    size_small: f32,
    #[serde(rename = "size-large")]
    size_large: f32,
}

pub struct Theme {
    colors: HashMap<String, Rgba>,
    pub radius: HashMap<String, f32>,
    pub space: HashMap<String, f32>,
    pub font_ui: String,
    pub font_mono: String,
    pub font_size: f32,
    pub font_size_small: f32,
    pub font_size_large: f32,
}

fn parse_hex(s: &str) -> Rgba {
    let h = s.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("ff"), 16).unwrap_or(255) as f32 / 255.0;
    [byte(0), byte(2), byte(4), if h.len() >= 8 { byte(6) } else { 1.0 }]
}

impl Theme {
    pub fn load(mode: &str) -> Self {
        let t: Tokens = serde_json::from_str(TOKENS).expect("assets/theme.json is invalid");
        let colors = t.modes.get(mode).or_else(|| t.modes.values().next()).expect("theme has no modes");
        Self {
            colors: colors.iter().map(|(k, v)| (k.clone(), parse_hex(v))).collect(),
            radius: t.radius,
            space: t.space,
            font_ui: t.font.ui,
            font_mono: t.font.mono,
            font_size: t.font.size,
            font_size_small: t.font.size_small,
            font_size_large: t.font.size_large,
        }
    }

    /// Colour for a role. Unknown roles show magenta so they're easy to spot.
    pub fn c(&self, role: &str) -> Rgba {
        self.colors.get(role).copied().unwrap_or([1.0, 0.0, 1.0, 1.0])
    }

    /// Same colour with a different alpha.
    pub fn ca(&self, role: &str, a: f32) -> Rgba {
        let [r, g, b, _] = self.c(role);
        [r, g, b, a]
    }

    pub fn r(&self, key: &str) -> f32 {
        self.radius.get(key).copied().unwrap_or(4.0)
    }

    pub fn s(&self, key: &str) -> f32 {
        self.space.get(key).copied().unwrap_or(8.0)
    }

    /// Map tokens onto ImGui's style.
    pub fn apply(&self, style: &mut imgui::Style) {
        use imgui::StyleColor as C;
        let mix = |a: Rgba, b: Rgba, t: f32| [0, 1, 2, 3].map(|i| a[i] + (b[i] - a[i]) * t);
        let (bg, surface, surface2, border, text, muted, accent) = (
            self.c("bg"), self.c("surface"), self.c("surface-2"), self.c("border"),
            self.c("text"), self.c("text-muted"), self.c("accent"),
        );
        style.window_rounding = self.r("lg");
        style.child_rounding = self.r("md");
        style.frame_rounding = self.r("sm");
        style.popup_rounding = self.r("md");
        style.grab_rounding = self.r("sm");
        style.tab_rounding = self.r("sm");
        style.scrollbar_rounding = self.r("sm");
        style.window_padding = [self.s("md"), self.s("md")];
        style.frame_padding = [self.s("sm"), self.s("xs")];
        style.item_spacing = [self.s("sm"), self.s("sm")];
        style.window_border_size = 1.0;
        style.child_border_size = 1.0;
        style.frame_border_size = 0.0;

        style[C::Text] = text;
        style[C::TextDisabled] = muted;
        style[C::WindowBg] = bg;
        style[C::ChildBg] = surface;
        style[C::PopupBg] = surface;
        style[C::Border] = border;
        style[C::BorderShadow] = [0.0; 4];
        style[C::FrameBg] = surface2;
        style[C::FrameBgHovered] = mix(surface2, accent, 0.15);
        style[C::FrameBgActive] = mix(surface2, accent, 0.3);
        style[C::TitleBg] = surface;
        style[C::TitleBgActive] = surface;
        style[C::TitleBgCollapsed] = surface;
        style[C::MenuBarBg] = surface;
        style[C::ScrollbarBg] = bg;
        style[C::ScrollbarGrab] = border;
        style[C::ScrollbarGrabHovered] = muted;
        style[C::ScrollbarGrabActive] = accent;
        style[C::CheckMark] = accent;
        style[C::SliderGrab] = accent;
        style[C::SliderGrabActive] = accent;
        style[C::Button] = surface2;
        style[C::ButtonHovered] = mix(surface2, accent, 0.25);
        style[C::ButtonActive] = mix(surface2, accent, 0.45);
        style[C::Header] = surface2;
        style[C::HeaderHovered] = mix(surface2, accent, 0.25);
        style[C::HeaderActive] = mix(surface2, accent, 0.45);
        style[C::Separator] = border;
        style[C::Tab] = surface;
        style[C::TabHovered] = mix(surface2, accent, 0.25);
        style[C::TabActive] = surface2;
        style[C::PlotLines] = accent;
        style[C::PlotHistogram] = accent;
        style[C::TableHeaderBg] = surface2;
        style[C::TableBorderStrong] = border;
        style[C::TableBorderLight] = border;
        style[C::TextSelectedBg] = mix(bg, accent, 0.35);
    }
}
