//! Load the theme's font families from the system (via fontconfig), falling back to ImGui's
//! built-in font when a family isn't installed.

use imgui::{FontConfig, FontId, FontSource};
use std::process::Command;

pub struct Fonts {
    pub ui: FontId,
    pub small: FontId,
    pub large: FontId,
    /// Big numeric readouts (gear, speed).
    pub huge: FontId,
    pub mono: FontId,
}

/// Path of the best installed match for `family`, if fontconfig is available.
fn locate(family: &str) -> Option<String> {
    let out = Command::new("fc-match").args(["-f", "%{file}", family]).output().ok()?;
    let path = String::from_utf8(out.stdout).ok()?;
    (path.ends_with(".ttf") || path.ends_with(".otf")).then_some(path)
}

fn load(family: &str) -> Option<&'static [u8]> {
    // ImGui keeps referencing font data for the atlas's lifetime; leak it once at startup.
    std::fs::read(locate(family)?).ok().map(|d| &*Box::leak(d.into_boxed_slice()))
}

impl Fonts {
    pub fn build(ctx: &mut imgui::Context, theme: &crate::theme::Theme, hidpi: f32) -> Self {
        let ui_data = load(&theme.font_ui);
        let mono_data = load(&theme.font_mono);
        let mut add = |data: Option<&'static [u8]>, px: f32| {
            let px = (px * hidpi).round();
            match data {
                Some(data) => ctx.fonts().add_font(&[FontSource::TtfData {
                    data,
                    size_pixels: px,
                    config: Some(FontConfig { oversample_h: 2, ..Default::default() }),
                }]),
                None => ctx.fonts().add_font(&[FontSource::DefaultFontData {
                    config: Some(FontConfig { size_pixels: px, ..Default::default() }),
                }]),
            }
        };
        let fonts = Self {
            ui: add(ui_data, theme.font_size),
            small: add(ui_data, theme.font_size_small),
            large: add(ui_data, theme.font_size_large),
            huge: add(ui_data, theme.font_size_large * 2.6),
            mono: add(mono_data, theme.font_size_small),
        };
        ctx.io_mut().font_global_scale = 1.0 / hidpi;
        fonts
    }
}
