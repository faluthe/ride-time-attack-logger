//! Ride Time Attack live logger.

mod platform;
mod theme;

use theme::Theme;

struct Logger {
    theme: Theme,
}

impl platform::Frame for Logger {
    fn setup(&mut self, ctx: &mut imgui::Context, hidpi: f32) {
        ctx.fonts().add_font(&[imgui::FontSource::DefaultFontData {
            config: Some(imgui::FontConfig { size_pixels: self.theme.font_size * hidpi, ..Default::default() }),
        }]);
        ctx.io_mut().font_global_scale = 1.0 / hidpi;
        self.theme.apply(ctx.style_mut());
    }

    fn frame(&mut self, ui: &imgui::Ui) {
        let size = ui.io().display_size;
        ui.window("dashboard")
            .position([0.0, 0.0], imgui::Condition::Always)
            .size(size, imgui::Condition::Always)
            .flags(imgui::WindowFlags::NO_DECORATION | imgui::WindowFlags::NO_MOVE)
            .build(|| {
                ui.text("Ride Time Attack");
                ui.text_disabled("waiting for Ride 4");
            });
    }
}

fn main() {
    let theme = Theme::load("dark");
    let clear = theme.c("bg");
    platform::run("Ride Time Attack", (1280, 800), clear, Logger { theme });
}
