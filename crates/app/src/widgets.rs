//! Custom-drawn instruments. Everything is drawn into the current window's draw list inside a
//! reserved rectangle, using theme roles for colour.

use crate::theme::{Rgba, Theme};
use imgui::{DrawListMut, Ui};
use std::f32::consts::PI;

type V2 = [f32; 2];

fn add(a: V2, b: V2) -> V2 {
    [a[0] + b[0], a[1] + b[1]]
}

/// Point on a circle; angle 0 = up, clockwise positive (screen y points down).
fn polar(c: V2, r: f32, a: f32) -> V2 {
    [c[0] + r * a.sin(), c[1] - r * a.cos()]
}

fn arc(dl: &DrawListMut, c: V2, r: f32, a0: f32, a1: f32, col: Rgba, thick: f32) {
    let n = ((a1 - a0).abs() / (PI / 48.0)).ceil().max(2.0) as usize;
    let pts: Vec<V2> = (0..=n).map(|i| polar(c, r, a0 + (a1 - a0) * i as f32 / n as f32)).collect();
    dl.add_polyline(pts, col).thickness(thick).build();
}

/// Reserve `size` at the cursor and return its top-left corner.
fn reserve(ui: &Ui, size: V2) -> V2 {
    let p = ui.cursor_screen_pos();
    ui.dummy(size);
    p
}

pub fn centered_text(ui: &Ui, dl: &DrawListMut, center: V2, col: Rgba, text: &str) {
    let s = ui.calc_text_size(text);
    dl.add_text([center[0] - s[0] / 2.0, center[1] - s[1] / 2.0], col, text);
}

/// Tachometer: 0..1 engine speed over a 240° arc with a redline zone, gear and speed in the middle.
pub fn tach(ui: &Ui, th: &Theme, fonts: &crate::fonts::Fonts, size: f32, rpm: f32, gear: i32, speed: f32, live: bool) {
    let p = reserve(ui, [size, size]);
    let dl = ui.get_window_draw_list();
    let c = [p[0] + size / 2.0, p[1] + size / 2.0 + size * 0.04];
    let r = size * 0.42;
    let (a0, a1) = (-2.0 * PI / 3.0, 2.0 * PI / 3.0);
    let at = |f: f32| a0 + (a1 - a0) * f.clamp(0.0, 1.0);
    let thick = size * 0.045;
    arc(&dl, c, r, a0, a1, th.c("surface-2"), thick);
    arc(&dl, c, r, at(0.9), a1, th.ca("redline", 0.35), thick);
    if live {
        let col = if rpm >= 0.9 { th.c("redline") } else { th.c("rpm") };
        arc(&dl, c, r, a0, at(rpm), col, thick);
    }
    // ticks every 10%
    for i in 0..=10 {
        let a = at(i as f32 / 10.0);
        let (r0, r1) = (r - thick * 0.9, r - thick * (if i % 5 == 0 { 1.9 } else { 1.4 }));
        dl.add_line(polar(c, r0, a), polar(c, r1, a), th.c("text-muted")).thickness(1.5).build();
    }
    if live {
        let a = at(rpm);
        dl.add_line(polar(c, r * 0.25, a), polar(c, r + thick * 0.6, a), th.c("text")).thickness(2.5).build();
    }
    {
        let _f = ui.push_font(fonts.huge);
        let g = if !live { "-".to_string() } else if gear == 0 { "N".into() } else { gear.to_string() };
        centered_text(ui, &dl, [c[0], c[1] - size * 0.05], th.c("gear"), &g);
    }
    {
        let _f = ui.push_font(fonts.large);
        let s = if live { format!("{speed:.0}") } else { "-".into() };
        centered_text(ui, &dl, [c[0], c[1] + size * 0.2], th.c("speed"), &s);
    }
    let _f = ui.push_font(fonts.small);
    centered_text(ui, &dl, [c[0], c[1] + size * 0.31], th.c("text-muted"), "km/h");
    centered_text(ui, &dl, [c[0], c[1] + size * 0.42], th.c("text-muted"), &format!("{:.0}% rpm", rpm * 100.0));
}

/// Vertical input bar 0..1 with a label underneath.
pub fn bar(ui: &Ui, th: &Theme, size: V2, value: f32, role: &str, label: &str) {
    let p = reserve(ui, size);
    let dl = ui.get_window_draw_list();
    let label_h = ui.text_line_height() + th.s("xs");
    let bot = [p[0] + size[0], p[1] + size[1] - label_h];
    dl.add_rect(p, bot, th.c("surface-2")).filled(true).rounding(th.r("md")).build();
    let h = (bot[1] - p[1]) * value.clamp(0.0, 1.0);
    if h > 1.0 {
        dl.add_rect([p[0], bot[1] - h], bot, th.c(role)).filled(true).rounding(th.r("md")).build();
    }
    centered_text(ui, &dl, [p[0] + size[0] / 2.0, bot[1] + label_h / 2.0 + 1.0], th.c("text-muted"), label);
    let pct = format!("{:.0}", value * 100.0);
    centered_text(ui, &dl, [p[0] + size[0] / 2.0, p[1] + ui.text_line_height()], th.c("text"), &pct);
}

/// Round indicator for on/off inputs (rear brake button).
pub fn lamp(ui: &Ui, th: &Theme, size: V2, on: bool, role: &str, label: &str) {
    let p = reserve(ui, size);
    let dl = ui.get_window_draw_list();
    let r = size[0].min(size[1]) * 0.3;
    let c = [p[0] + size[0] / 2.0, p[1] + r + 2.0];
    dl.add_circle(c, r, if on { th.c(role) } else { th.c("surface-2") }).filled(true).build();
    centered_text(ui, &dl, [c[0], c[1] + r + ui.text_line_height() * 0.8], th.c("text-muted"), label);
}

/// Rear view of bike and rider rolling with lean angle (right positive), over a ±65° scale with
/// the session's max lean marked. A placeholder for a proper rider model.
pub fn rider(ui: &Ui, th: &Theme, fonts: &crate::fonts::Fonts, size: V2, deg: f32, max_l: f32, max_r: f32, live: bool) {
    let p = reserve(ui, size);
    let dl = ui.get_window_draw_list();
    let ground = [p[0] + size[0] / 2.0, p[1] + size[1] * 0.86];
    let r = size[1] * 0.72;
    let rad = |d: f32| d.to_radians();
    // scale
    arc(&dl, ground, r, rad(-65.0), rad(65.0), th.c("surface-2"), 6.0);
    for d in (-60..=60).step_by(15) {
        let a = rad(d as f32);
        dl.add_line(polar(ground, r - 6.0, a), polar(ground, r - if d % 30 == 0 { 16.0 } else { 11.0 }, a), th.c("text-muted")).build();
    }
    for (m, d) in [(max_l, max_l), (max_r, max_r)] {
        if m.abs() > 1.0 {
            let a = rad(d);
            dl.add_line(polar(ground, r - 10.0, a), polar(ground, r + 8.0, a), th.c("lean")).thickness(2.0).build();
        }
    }
    dl.add_line([p[0] + 8.0, ground[1]], [p[0] + size[0] - 8.0, ground[1]], th.c("border")).thickness(2.0).build();

    // body-frame shapes, x right, y up, in units of the bike height; rotated about the contact patch
    let h = size[1] * 0.55;
    let a = if live { rad(deg) } else { 0.0 };
    let (s, co) = a.sin_cos();
    let tf = |x: f32, y: f32| -> V2 { add(ground, [h * (x * co + y * s), -h * (-x * s + y * co)]) };
    let poly = |pts: &[(f32, f32)], col: Rgba| {
        dl.add_polyline(pts.iter().map(|&(x, y)| tf(x, y)).collect::<Vec<_>>(), col).filled(true).build();
    };
    // tyre, bike body, fairing
    poly(&[(-0.07, 0.0), (0.07, 0.0), (0.08, 0.32), (-0.08, 0.32)], th.c("text-muted"));
    poly(&[(-0.2, 0.3), (0.2, 0.3), (0.16, 0.62), (-0.16, 0.62)], th.c("surface-2"));
    poly(&[(-0.12, 0.6), (0.12, 0.6), (0.08, 0.72), (-0.08, 0.72)], th.c("border"));
    // rider hangs off toward the inside of the turn, more as lean grows
    let off = if live { (deg / 55.0).clamp(-1.0, 1.0) * 0.18 } else { 0.0 };
    poly(&[(-0.17 + off, 0.55), (0.17 + off, 0.55), (0.14 + off * 1.6, 0.92), (-0.14 + off * 1.6, 0.92)], th.c("accent"));
    dl.add_circle(tf(off * 2.0, 1.03), h * 0.12, th.c("text")).filled(true).build();

    let _f = ui.push_font(fonts.large);
    let txt = if live { format!("{:.0}° {}", deg.abs(), if deg < -0.5 { "L" } else if deg > 0.5 { "R" } else { "" }) } else { "-".into() };
    centered_text(ui, &dl, [p[0] + size[0] / 2.0, p[1] + ui.text_line_height() * 0.6], th.c("lean"), &txt);
}

/// One series on a time graph.
pub struct Series<'a> {
    pub values: &'a [f32],
    pub role: &'a str,
    /// Draw as steps (gear).
    pub step: bool,
}

/// Rolling time graph: `times` (seconds, increasing) shared by all series, right edge = latest.
pub fn graph(ui: &Ui, th: &Theme, size: V2, label: &str, times: &[f64], window: f64, lo: f32, hi: f32, series: &[Series]) {
    let p = reserve(ui, size);
    let dl = ui.get_window_draw_list();
    let q = add(p, size);
    dl.add_rect(p, q, th.c("surface")).filled(true).rounding(th.r("md")).build();
    dl.add_rect(p, q, th.c("border")).rounding(th.r("md")).build();
    let pad = th.s("sm");
    let (x0, x1, y0, y1) = (p[0] + pad, q[0] - pad, p[1] + pad, q[1] - pad);
    for f in [0.25, 0.5, 0.75] {
        let y = y1 - (y1 - y0) * f;
        dl.add_line([x0, y], [x1, y], th.ca("border", 0.6)).build();
    }
    dl.add_text([x0 + 2.0, y0], th.c("text-muted"), label);
    let Some(&t_end) = times.last() else { return };
    let x = |t: f64| x1 - ((t_end - t) / window) as f32 * (x1 - x0);
    let y = |v: f32| y1 - ((v - lo) / (hi - lo)).clamp(0.0, 1.0) * (y1 - y0);
    dl.with_clip_rect_intersect([x0, y0], [x1, y1 + 1.0], || {
        for s in series {
            let mut pts: Vec<V2> = Vec::with_capacity(times.len() * 2);
            for (i, (&t, &v)) in times.iter().zip(s.values).enumerate() {
                if s.step && i > 0 {
                    pts.push([x(t), y(s.values[i - 1])]);
                }
                pts.push([x(t), y(v)]);
            }
            if pts.len() > 1 {
                dl.add_polyline(pts, th.c(s.role)).thickness(2.0).build();
            }
        }
    });
}
