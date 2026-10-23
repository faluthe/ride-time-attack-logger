//! Ride Time Attack live logger.
//!
//! ```text
//! rta-logger [--sessions DIR] [--no-record] [--light] [--screenshot FILE.png [--shot-delay S]]
//! ```

mod fonts;
mod platform;
mod reader;
mod theme;
mod widgets;

use glow::HasContext;
use reader::{Link, Live, Reader};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use theme::Theme;
use widgets::Series;

struct Args {
    sessions: PathBuf,
    record: bool,
    mode: &'static str,
    screenshot: Option<PathBuf>,
    shot_delay: f32,
}

const USAGE: &str = "rta-logger [--sessions DIR] [--no-record] [--light] [--screenshot FILE.png [--shot-delay S]]";

fn default_sessions() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
    });
    base.join("ride-time-attack/sessions")
}

fn args() -> Args {
    let mut a = Args { sessions: default_sessions(), record: true, mode: "dark", screenshot: None, shot_delay: 8.0 };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--sessions" => a.sessions = it.next().expect("--sessions needs a directory").into(),
            "--no-record" => a.record = false,
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
    screenshot: Option<PathBuf>,
    shot_delay: f32,
    started: std::time::Instant,
    done: bool,
}

impl Logger {
    fn header(&self, ui: &imgui::Ui, live: &Live) {
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
            ui.text_disabled(format!("{:.0} Hz", live.rate));
        }
        mono.pop();

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
        let w = ui.calc_text_size(&label)[0] + 120.0;
        ui.same_line_with_pos(ui.window_content_region_max()[0] - w);
        if live.recording.is_some() {
            ui.text_colored(th.c("bad"), "\u{25CF} REC");
            ui.same_line();
        }
        ui.text_disabled(&label);
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
        let on = live.link == Link::Riding && live.sample.is_some();
        let (inp, lean) = match live.sample {
            Some(s) => (s.inputs.unwrap_or_default(), s.lean_deg()),
            None => (Default::default(), 0.0),
        };
        let avail = ui.content_region_avail();
        let gauges_h = (avail[1] * 0.48).clamp(240.0, 420.0);
        let gap = th.s("md");

        // gauges row: tach | lean | inputs
        ui.child_window("tach").size([gauges_h, gauges_h]).build(|| {
            widgets::tach(ui, th, f, gauges_h - 2.0 * th.s("md"), inp.rpm, inp.gear, live.speed, on);
        });
        ui.same_line_with_spacing(0.0, gap);
        let lean_w = (avail[0] - gauges_h - 2.0 * gap - 240.0).max(260.0);
        let (max_l, max_r) = live.history.iter().fold((0.0f32, 0.0f32), |(l, r), p| (l.min(p.lean), r.max(p.lean)));
        ui.child_window("lean").size([lean_w, gauges_h]).build(|| {
            let s = ui.content_region_avail();
            widgets::lean(ui, th, f, s, lean, max_l, max_r, on);
        });
        ui.same_line_with_spacing(0.0, gap);
        ui.child_window("inputs").size([0.0, gauges_h]).build(|| {
            let s = ui.content_region_avail();
            let bw = (s[0] - 2.0 * th.s("sm")) / 3.0;
            widgets::bar(ui, th, [bw, s[1]], inp.throttle, "throttle", "throttle");
            ui.same_line();
            widgets::bar(ui, th, [bw, s[1]], inp.brake, "brake", "brake");
            ui.same_line();
            widgets::lamp(ui, th, [bw, s[1]], inp.rear > 0.5, "rear", "rear");
        });

        // lap timing strip (the game's own clock, Time Attack only)
        ui.dummy([0.0, gap * 0.25]);
        let fmt = |s: f64| if s.is_finite() && s > 0.0 { format!("{}:{:06.3}", (s / 60.0).floor(), s % 60.0) } else { "-:--.---".into() };
        let (current, last, best, laps) = match live.timing {
            Some((g, best)) => (
                live.lap_start.map_or(f64::NAN, |s| g.clock - s),
                g.last_lap_ms as f64 / 1000.0,
                best as f64,
                Some(g.laps),
            ),
            None => (f64::NAN, f64::NAN, f64::NAN, None),
        };
        {
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
        }

        // rolling graphs
        ui.dummy([0.0, gap * 0.5]);
        let h: Vec<_> = live.history.iter().collect();
        let times: Vec<f64> = h.iter().map(|p| p.t).collect();
        let col = |f: fn(&reader::Point) -> f32| h.iter().map(|p| f(p)).collect::<Vec<f32>>();
        let (speed, thr, brk, rpm, gear, leanv) = (
            col(|p| p.speed), col(|p| p.inputs.throttle), col(|p| p.inputs.brake),
            col(|p| p.inputs.rpm), col(|p| p.inputs.gear as f32), col(|p| p.lean),
        );
        let w = ui.content_region_avail()[0];
        let gh = ((ui.content_region_avail()[1] - 2.0 * th.s("sm")) / 3.0).max(60.0);
        let top = speed.iter().cloned().fold(100.0f32, f32::max) * 1.1;
        let win = reader::HISTORY_S;
        widgets::graph(ui, th, [w, gh], "speed  km/h", &times, win, 0.0, top, &[Series { values: &speed, role: "speed", step: false }]);
        widgets::graph(ui, th, [w, gh], "throttle / brake", &times, win, 0.0, 1.0, &[
            Series { values: &thr, role: "throttle", step: false },
            Series { values: &brk, role: "brake", step: false },
        ]);
        let half = (w - th.s("sm")) / 2.0;
        widgets::graph(ui, th, [half, gh], "rpm / gear", &times, win, 0.0, 1.0, &[
            Series { values: &rpm, role: "rpm", step: false },
            Series { values: &gear.iter().map(|g| g / 6.0).collect::<Vec<_>>(), role: "gear", step: true },
        ]);
        ui.same_line();
        widgets::graph(ui, th, [half, gh], "lean  deg", &times, win, -65.0, 65.0, &[Series { values: &leanv, role: "lean", step: false }]);
    }
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
    let reader = Reader::spawn(a.sessions, a.record && a.screenshot.is_none());
    let app = Logger {
        theme, fonts: None, reader, screenshot: a.screenshot, shot_delay: a.shot_delay,
        started: std::time::Instant::now(), done: false,
    };
    platform::run("Ride Time Attack", (1280, 800), clear, app);
}
