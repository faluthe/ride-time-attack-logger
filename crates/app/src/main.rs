//! Ride Time Attack live logger.
//!
//! ```text
//! rta-logger [--sessions DIR] [--no-record] [--no-upload] [--light] [--screenshot FILE.png [--shot-delay S]]
//! ```

mod fonts;
mod panels;
mod platform;
mod reader;
mod theme;
mod uploader;
mod widgets;

use glow::HasContext;
use reader::{Link, Live, Reader};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use panels::Panels;
use theme::Theme;
use uploader::{Status, Uploader};
use widgets::Series;

struct Args {
    sessions: PathBuf,
    record: bool,
    upload: bool,
    mode: &'static str,
    screenshot: Option<PathBuf>,
    shot_delay: f32,
}

const USAGE: &str = "rta-logger [--sessions DIR] [--no-record] [--no-upload] [--light] [--screenshot FILE.png [--shot-delay S]]";

fn default_sessions() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
    });
    base.join("ride-time-attack/sessions")
}

fn args() -> Args {
    let mut a = Args { sessions: default_sessions(), record: true, upload: true, mode: "dark", screenshot: None, shot_delay: 8.0 };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--sessions" => a.sessions = it.next().expect("--sessions needs a directory").into(),
            "--no-record" => a.record = false,
            "--no-upload" => a.upload = false,
            "--light" => a.mode = "light",
            "--screenshot" => a.screenshot = Some(it.next().expect("--screenshot needs a file").into()),
            "--shot-delay" => a.shot_delay = it.next().and_then(|s| s.parse().ok()).expect("--shot-delay needs seconds"),
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => panic!("unknown argument {other}"),
        }
    }
    a
}

struct Logger {
    theme: Theme,
    fonts: Option<fonts::Fonts>,
    reader: Reader,
    uploader: Uploader,
    panels: Panels,
    screenshot: Option<PathBuf>,
    shot_delay: f32,
    started: std::time::Instant,
    done: bool,
}

impl Logger {
    fn header(&mut self, ui: &imgui::Ui, live: &Live) {
        let th = &self.theme;
        let (role, text) = match &live.link {
            Link::NoGame => ("text-muted", "Ride 4 not running".to_string()),
            Link::Attaching(e) => ("warn", format!("attaching: {e}")),
            Link::NoBike => ("warn", "in menus / loading".to_string()),
            Link::Riding => ("good", "on track".to_string()),
        };
        let p = ui.cursor_screen_pos();
        let r = ui.text_line_height() * 0.3;
        ui.get_window_draw_list().add_circle([p[0] + r, p[1] + ui.text_line_height() / 2.0], r, th.c(role)).filled(true).build();
        ui.set_cursor_screen_pos([p[0] + r * 2.0 + th.s("sm"), p[1]]);
        ui.text_colored(th.c(role), &text);
        let mono = ui.push_font(self.fonts.as_ref().unwrap().mono);
        if let Some(pid) = live.pid {
            ui.same_line();
            ui.text_disabled(format!("pid {pid}"));
        }
        if let Some(c) = &live.controller {
            ui.same_line();
            ui.text_disabled(c);
        }
        if live.link == Link::Riding {
            ui.same_line();
            ui.text_disabled(format!("{:.0} fps · {:.0} Hz", live.fps, live.rate));
        }
        mono.pop();

        // upload status, from the last lap
        let ago = |at: std::time::Instant| {
            let s = at.elapsed().as_secs();
            if s < 60 { format!("{s}s ago") } else { format!("{}m ago", s / 60) }
        };
        let (role, text, tip) = match self.uploader.status() {
            Status::Off(why) => ("text-muted", "upload off".to_string(), why),
            Status::Idle => ("text-muted", "uploads after each lap".to_string(), String::new()),
            Status::Working => ("warn", "uploading…".to_string(), String::new()),
            Status::Done { at, uploaded, bytes } => {
                ("good", format!("uploaded {uploaded} files ({} KB) {}", bytes / 1024, ago(at)), String::new())
            }
            Status::Failed { at, error } => ("bad", format!("upload failed {}", ago(at)), error),
        };
        ui.same_line_with_spacing(0.0, th.s("lg"));
        ui.text_colored(th.c(role), &text);
        if ui.is_item_hovered() {
            ui.tooltip_text(format!("{tip}{}exported to {}", if tip.is_empty() { "" } else { "\n" }, self.uploader.out.display()));
        }

        // right side: recording toggle and file
        let mut rec = self.reader.record.load(Ordering::Relaxed);
        let label = match &live.recording {
            Some(r) => {
                let s = r.started.elapsed().as_secs();
                let name = r.path.file_name().unwrap_or_default().to_string_lossy();
                format!("{name}  {}:{:02}  {} rows", s / 60, s % 60, r.rows)
            }
            None if rec => "records when on track".into(),
            None => "not recording".into(),
        };
        let w = ui.calc_text_size(&label)[0] + 120.0 + ui.calc_text_size("panels")[0] + 3.0 * th.s("md");
        ui.same_line_with_pos(ui.window_content_region_max()[0] - w);
        if live.recording.is_some() {
            ui.text_colored(th.c("bad"), "\u{25CF} REC");
            ui.same_line();
        }
        ui.text_disabled(&label);
        ui.same_line();
        if ui.button("panels") {
            ui.open_popup("panels");
        }
        ui.popup("panels", || self.panels.menu(ui));
        ui.same_line();
        if ui.checkbox("record", &mut rec) {
            self.reader.record.store(rec, Ordering::Relaxed);
        }
        if ui.is_item_hovered() {
            ui.tooltip_text(format!("sessions are saved to {}", self.reader.sessions_dir.display()));
        }
    }

    fn dashboard(&self, ui: &imgui::Ui, live: &Live) {
        let th = &self.theme;
        let f = self.fonts.as_ref().unwrap();
        let p = &self.panels;
        let on = live.link == Link::Riding && live.sample.is_some();
        let (inp, lean) = match live.sample {
            Some(s) => (s.inputs.unwrap_or_default(), s.lean_deg()),
            None => (Default::default(), 0.0),
        };
        let avail = ui.content_region_avail();
        let gap = th.s("md");
        let any_graph = panels::GRAPHS.iter().any(|g| p.on(g));
        // with no graphs, the widgets get the whole window
        let gauges_h = if any_graph { (avail[1] * 0.48).clamp(240.0, 420.0) } else { (avail[1] - 120.0).max(240.0) };

        // widgets row: tach | rider | bars. The tach is square and the bars a fixed width; the
        // rider takes the rest (or the bars do, without the rider).
        let (tach, rider, bars) = (p.on("tach"), p.on("rider"), p.on("bars"));
        if tach || rider || bars {
            let bars_fixed = 240.0;
            let shown = [tach, rider, bars].iter().filter(|&&x| x).count() as f32;
            let fixed = if tach { gauges_h } else { 0.0 } + if bars && rider { bars_fixed } else { 0.0 };
            let flex = (avail[0] - fixed - gap * (shown - 1.0)).max(200.0);
            let mut first = true;
            let mut next = || {
                if !std::mem::take(&mut first) {
                    ui.same_line_with_spacing(0.0, gap);
                }
            };
            if tach {
                next();
                ui.child_window("tach").size([gauges_h, gauges_h]).build(|| {
                    widgets::tach(ui, th, f, gauges_h - 2.0 * th.s("md"), inp.rpm, inp.gear, live.speed, on);
                });
            }
            if rider {
                next();
                let (max_l, max_r) = live.history.iter().fold((0.0f32, 0.0f32), |(l, r), p| (l.min(p.lean), r.max(p.lean)));
                ui.child_window("rider").size([flex, gauges_h]).build(|| {
                    let s = ui.content_region_avail();
                    widgets::rider(ui, th, f, s, lean, max_l, max_r, on);
                });
            }
            if bars {
                next();
                let w = if rider { bars_fixed } else { flex };
                ui.child_window("bars").size([w, gauges_h]).build(|| {
                    let s = ui.content_region_avail();
                    let bw = (s[0] - 2.0 * th.s("sm")) / 3.0;
                    widgets::bar(ui, th, [bw, s[1]], inp.throttle, "throttle", "throttle");
                    ui.same_line();
                    widgets::bar(ui, th, [bw, s[1]], inp.brake, "brake", "brake");
                    ui.same_line();
                    widgets::lamp(ui, th, [bw, s[1]], inp.rear > 0.5, "rear", "rear");
                });
            }
            ui.dummy([0.0, gap * 0.25]);
        }

        // timer: the lap timing strip (the game's own clock, Time Attack only)
        if p.on("timer") {
            let fmt = |s: f64| if s.is_finite() && s > 0.0 { format!("{}:{:06.3}", (s / 60.0).floor(), s % 60.0) } else { "-:--.---".into() };
            let (current, last, best, laps) = match live.timing {
                Some((g, best)) => (
                    live.current_lap.map_or(f64::NAN, f64::from),
                    g.last_lap_ms as f64 / 1000.0,
                    // before the first completed lap the game holds a huge placeholder here
                    if g.laps > 0 && best < 3600.0 { best as f64 } else { f64::NAN },
                    Some(g.laps),
                ),
                None => (f64::NAN, f64::NAN, f64::NAN, None),
            };
            let _f = ui.push_font(f.large);
            let cell = |label: &str, value: String, role: &str| {
                ui.group(|| {
                    let _s = ui.push_font(f.small);
                    ui.text_disabled(label);
                    drop(_s);
                    ui.text_colored(th.c(role), value);
                });
            };
            cell("CURRENT LAP", fmt(current), "text");
            ui.same_line_with_spacing(0.0, 48.0);
            cell("LAST LAP", fmt(last), "text");
            ui.same_line_with_spacing(0.0, 48.0);
            cell("BEST LAP", fmt(best), "good");
            ui.same_line_with_spacing(0.0, 48.0);
            cell("LAPS", laps.map_or("-".into(), |n| n.to_string()), "text");
            if laps.is_none() && on {
                ui.same_line_with_spacing(0.0, 48.0);
                let _s = ui.push_font(f.small);
                ui.text_disabled("lap timing is read in Time Attack");
            }
            ui.dummy([0.0, gap * 0.5]);
        }

        // rolling graphs: one column for up to three, then two columns
        let graphs: Vec<&str> = panels::GRAPHS.iter().copied().filter(|g| p.on(g)).collect();
        if graphs.is_empty() {
            return;
        }
        let h: Vec<_> = live.history.iter().collect();
        let times: Vec<f64> = h.iter().map(|p| p.t).collect();
        let col = |f: fn(&reader::Point) -> f32| h.iter().map(|p| f(p)).collect::<Vec<f32>>();
        let speed = col(|p| p.speed);
        let cols = if graphs.len() <= 3 { 1 } else { 2 };
        let rows = graphs.len().div_ceil(cols);
        let sm = th.s("sm");
        let w = (ui.content_region_avail()[0] - sm * (cols - 1) as f32) / cols as f32;
        let gh = ((ui.content_region_avail()[1] - sm * (rows - 1) as f32) / rows as f32).max(60.0);
        let win = reader::HISTORY_S;
        for (k, &g) in graphs.iter().enumerate() {
            if k % cols != 0 {
                ui.same_line_with_spacing(0.0, sm);
            }
            let size = [w, gh];
            match g {
                "speed" => {
                    let top = speed.iter().cloned().fold(100.0f32, f32::max) * 1.1;
                    widgets::graph(ui, th, size, "speed  km/h", &times, win, 0.0, top, &[Series { values: &speed, role: "speed", step: false }]);
                }
                "lean" => widgets::graph(ui, th, size, "lean  deg", &times, win, -65.0, 65.0, &[Series { values: &col(|p| p.lean), role: "lean", step: false }]),
                "inputs" => widgets::graph(ui, th, size, "throttle / brake", &times, win, 0.0, 1.0, &[
                    Series { values: &col(|p| p.inputs.throttle), role: "throttle", step: false },
                    Series { values: &col(|p| p.inputs.brake), role: "brake", step: false },
                ]),
                "gear" => widgets::graph(ui, th, size, "gear", &times, win, 0.0, 6.0, &[Series { values: &col(|p| p.inputs.gear as f32), role: "gear", step: true }]),
                "rpm" => widgets::graph(ui, th, size, "rpm  idle to limiter", &times, win, 0.0, 1.0, &[Series { values: &col(|p| p.inputs.rpm), role: "rpm", step: false }]),
                "g" => widgets::graph(ui, th, size, "g  braking / accelerating", &times, win, -1.5, 1.5, &[Series { values: &accel_g(&times, &speed), role: "accent", step: false }]),
                _ => {}
            }
        }
    }
}

/// Longitudinal g from the (smoothed) speed history: the change over the last half second. Zero
/// until the history spans that much, since speed jumps from 0 when the reader picks the bike up.
fn accel_g(times: &[f64], speed: &[f32]) -> Vec<f32> {
    const WINDOW: f64 = 0.5;
    let mut j = 0;
    times.iter().enumerate().map(|(i, &t)| {
        while j + 1 < i && t - times[j + 1] >= WINDOW {
            j += 1;
        }
        let dt = t - times[j];
        if dt < WINDOW || t - times[0] < 2.0 * WINDOW { 0.0 } else { (speed[i] - speed[j]) / 3.6 / dt as f32 / 9.81 }
    }).collect()
}

impl platform::Frame for Logger {
    fn setup(&mut self, ctx: &mut imgui::Context, hidpi: f32) {
        self.fonts = Some(fonts::Fonts::build(ctx, &self.theme, hidpi));
        self.theme.apply(ctx.style_mut());
    }

    fn frame(&mut self, ui: &imgui::Ui) {
        let live = self.reader.snapshot();
        let size = ui.io().display_size;
        let _font = ui.push_font(self.fonts.as_ref().unwrap().ui);
        ui.window("dashboard")
            .position([0.0, 0.0], imgui::Condition::Always)
            .size(size, imgui::Condition::Always)
            .flags(imgui::WindowFlags::NO_DECORATION | imgui::WindowFlags::NO_MOVE | imgui::WindowFlags::NO_BRING_TO_FRONT_ON_FOCUS)
            .build(|| {
                self.header(ui, &live);
                ui.separator();
                self.dashboard(ui, &live);
            });
    }

    fn rendered(&mut self, gl: &glow::Context, [w, h]: [u32; 2]) {
        // Wait so the reader has attached and the graphs have some history.
        let Some(path) = &self.screenshot else { return };
        if self.started.elapsed().as_secs_f32() < self.shot_delay || self.done {
            return;
        }
        let mut px = vec![0u8; (w * h * 4) as usize];
        unsafe {
            gl.read_pixels(0, 0, w as i32, h as i32, glow::RGBA, glow::UNSIGNED_BYTE, glow::PixelPackData::Slice(&mut px));
        }
        let row = (w * 4) as usize;
        let flipped: Vec<u8> = px.chunks_exact(row).rev().flatten().copied().collect();
        let file = std::fs::File::create(path).expect("screenshot file");
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.write_header().and_then(|mut wr| wr.write_image_data(&flipped)).expect("write png");
        println!("screenshot saved to {}", path.display());
        self.done = true;
    }

    fn quit(&self) -> bool {
        self.done
    }
}

fn main() {
    let a = args();
    let theme = Theme::load(a.mode);
    let clear = theme.c("bg");
    let uploader = Uploader::spawn(a.sessions.clone(), a.upload);
    let reader = Reader::spawn(a.sessions, a.record && a.screenshot.is_none(), uploader.tx.clone());
    let app = Logger {
        theme, fonts: None, reader, uploader, panels: Panels::load(a.screenshot.is_some()), screenshot: a.screenshot, shot_delay: a.shot_delay,
        started: std::time::Instant::now(), done: false,
    };
    platform::run("Ride Time Attack", (1280, 800), clear, app);
}
