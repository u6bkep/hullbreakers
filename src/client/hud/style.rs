//! The HUD's look, as code: the tokens of `docs/hud-style.md` (colours,
//! type, line weights, motion) and the painters every element draws with
//! (plates, bars, brackets, arrows, labels). Elements use these, never a
//! raw colour or size.

use std::f32::consts::{FRAC_PI_4, TAU};
use std::sync::Arc;

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::{EguiContexts, egui};
use egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};

// Colour. Roles are in the style guide; red is rare on purpose.

pub const INK: Color32 = Color32::from_rgb(0xe2, 0xf2, 0xf4);
pub const INK_DIM: Color32 = Color32::from_rgb(0x8e, 0xa6, 0xae);
/// INK at 22%, premultiplied.
pub const INK_FAINT: Color32 = Color32::from_rgba_premultiplied(50, 53, 54, 56);
/// The plate behind every block of readouts.
pub const SCRIM: Color32 = Color32::from_rgba_premultiplied(3, 5, 7, 176);
pub const SHADOW: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 170);
pub const SUIT: Color32 = Color32::from_rgb(0x4f, 0xd6, 0xff);
pub const FRIENDLY: Color32 = Color32::from_rgb(0x66, 0xe3, 0x9e);
pub const HOSTILE: Color32 = Color32::from_rgb(0xff, 0x55, 0x44);
pub const WARNING: Color32 = Color32::from_rgb(0xff, 0xa1, 0x2e);
pub const OBJECTIVE: Color32 = Color32::from_rgb(0xff, 0xd8, 0x4a);
pub const SHIELD: Color32 = Color32::from_rgb(0x6e, 0xa8, 0xff);
/// Weak-spot hits, and nothing else.
pub const WEAK: Color32 = Color32::from_rgb(0xff, 0x40, 0xc8);

// Type: sizes in points at 1080 lines (the HUD zooms with the window).

pub const MICRO: f32 = 13.0;
pub const SMALL: f32 = 15.0;
pub const BODY: f32 = 18.0;
pub const TITLE: f32 = 22.0;
pub const NUMERAL: f32 = 34.0;
pub const DISPLAY: f32 = 52.0;

/// Words: labels, names, objectives.
pub fn ui(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Titles and banners.
pub fn bold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("ui_bold".into()))
}

/// Numbers that change.
pub fn num(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

// Lines and shapes.

pub const HAIR: f32 = 1.0;
pub const LINE: f32 = 1.5;
pub const BOLD: f32 = 2.5;
/// The cut corner of a plate.
pub const CUT: f32 = 6.0;
/// Margin from the screen's edge.
pub const EDGE: f32 = 24.0;

// Motion.

/// Seconds to appear, and to leave.
pub const FADE_IN: f32 = 0.12;
pub const FADE_OUT: f32 = 0.4;
/// Time constant (s) a bar follows its value with.
pub const EASE: f32 = 0.1;

/// Moves `value` toward `target` with time constant `tau`.
pub fn ease(value: f32, target: f32, dt: f32, tau: f32) -> f32 {
    value + (target - value) * (1.0 - (-dt / tau).exp())
}

/// A blink for a state that must be fixed now: 1 to 0.35 and back, 1.6 Hz.
pub fn pulse(seconds: f32) -> f32 {
    0.675 + 0.325 * (seconds * 1.6 * TAU).cos()
}

/// Opacity of something that appeared `age` seconds ago and lasts `life`:
/// in fast, out slow.
pub fn appear(age: f32, life: f32) -> f32 {
    (age / FADE_IN).min(1.0) * ((life - age) / FADE_OUT).clamp(0.0, 1.0)
}

/// `color` at `alpha` of its own opacity.
pub fn fade(color: Color32, alpha: f32) -> Color32 {
    color.gamma_multiply(alpha.clamp(0.0, 1.0))
}

// Rarity accents for weapon cards, in order; none near WEAK.

pub const RARITY: [Color32; 5] = [
    INK_DIM,
    Color32::from_rgb(0x7e, 0xe0, 0x8a),
    Color32::from_rgb(0x5a, 0xa9, 0xff),
    Color32::from_rgb(0xa9, 0x8b, 0xff),
    Color32::from_rgb(0xff, 0xb0, 0x3a),
];

// Painters.

/// Text laid out on one line; `caps` adds the letter spacing labels get.
pub fn galley(painter: &Painter, text: impl Into<String>, font: FontId, color: Color32, spacing: f32) -> Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(
        text.into(),
        egui::TextFormat {
            font_id: font,
            color,
            extra_letter_spacing: spacing,
            ..default()
        },
    );
    job.wrap.max_width = f32::INFINITY;
    painter.layout_job(job)
}

/// Draws `text` anchored at `pos`; returns where it went.
pub fn text(painter: &Painter, pos: Pos2, align: Align2, text: impl Into<String>, font: FontId, color: Color32) -> Rect {
    let g = galley(painter, text, font, color, 0.0);
    let rect = align.anchor_size(pos, g.size());
    painter.galley(rect.min, g, color);
    rect
}

/// An uppercase label, letter-spaced.
pub fn caps(painter: &Painter, pos: Pos2, align: Align2, label: &str, size: f32, color: Color32) -> Rect {
    let g = galley(painter, label.to_uppercase(), ui(size), color, size * 0.08);
    let rect = align.anchor_size(pos, g.size());
    painter.galley(rect.min, g, color);
    rect
}

/// A unit after a number (m, m/s, kN): as written, small and dim.
pub fn unit(painter: &Painter, pos: Pos2, align: Align2, unit: &str, color: Color32) -> Rect {
    text(painter, pos, align, unit, ui(MICRO), color)
}

/// Text with a dark shadow, for loose text over the world.
pub fn shadowed(painter: &Painter, pos: Pos2, align: Align2, g: Arc<egui::Galley>, color: Color32) -> Rect {
    let rect = align.anchor_size(pos, g.size());
    let shadow = Color32::from_black_alpha((color.a() as f32 * 0.7) as u8);
    painter.galley_with_override_text_color(rect.min + vec2(1.0, 1.0), g.clone(), shadow);
    painter.galley(rect.min, g, color);
    rect
}

/// The outline of a plate: a rectangle with its top-left and bottom-right
/// corners cut.
pub fn cut_rect(rect: Rect, cut: f32) -> Vec<Pos2> {
    vec![
        pos2(rect.min.x + cut, rect.min.y),
        rect.right_top(),
        pos2(rect.max.x, rect.max.y - cut),
        pos2(rect.max.x - cut, rect.max.y),
        rect.left_bottom(),
        pos2(rect.min.x, rect.min.y + cut),
    ]
}

/// The backing plate for a block of readouts: scrim, hairline, and an
/// accent tick along the top edge from the cut.
pub fn plate(painter: &Painter, rect: Rect, accent: Color32, alpha: f32) {
    let points = cut_rect(rect, CUT);
    painter.add(Shape::convex_polygon(points.clone(), fade(SCRIM, alpha), Stroke::NONE));
    painter.add(Shape::closed_line(points, Stroke::new(HAIR, fade(INK_FAINT, alpha))));
    let tick = (rect.width() * 0.18).clamp(12.0, 40.0);
    painter.line_segment(
        [pos2(rect.min.x + CUT, rect.min.y), pos2(rect.min.x + CUT + tick, rect.min.y)],
        Stroke::new(2.0, fade(accent, alpha)),
    );
}

/// A line with a glow under it: a wider, faint stroke of its colour.
pub fn glow_line(painter: &Painter, points: Vec<Pos2>, color: Color32, width: f32, closed: bool) {
    let glow = Stroke::new(width * 3.0, color.gamma_multiply(0.18));
    let core = Stroke::new(width, color);
    if closed {
        painter.add(Shape::closed_line(points.clone(), glow));
        painter.add(Shape::closed_line(points, core));
    } else {
        painter.add(Shape::line(points.clone(), glow));
        painter.add(Shape::line(points, core));
    }
}

/// A bar of `segments` cells, `value` (0..1) of it filled in `color`, and
/// up to `trail` (what was just lost) in white.
pub fn seg_bar(painter: &Painter, rect: Rect, segments: usize, value: f32, trail: f32, color: Color32) {
    // One mesh of plain quads, edges on whole pixels: a bar is up to
    // fifty cells, and as separate rect shapes each is tessellated with
    // anti-aliased (feathered) edges it doesn't need.
    let ppp = painter.pixels_per_point();
    let snap = |x: f32| (x * ppp).round() / ppp;
    let (top, bottom) = (snap(rect.min.y), snap(rect.max.y).max(snap(rect.min.y) + 1.0 / ppp));
    let gap = if segments > 1 { (rect.width() / segments as f32 * 0.18).clamp(1.0, 3.0) } else { 0.0 };
    let cell = (rect.width() - gap * (segments as f32 - 1.0)) / segments as f32;
    let value = value.clamp(0.0, 1.0) * segments as f32;
    let trail = trail.clamp(0.0, 1.0) * segments as f32;
    let lost_color = Color32::from_white_alpha(200);
    let mut mesh = egui::Mesh::default();
    let mut quad = |x0: f32, x1: f32, c: Color32| {
        let (x0, x1) = (snap(x0), snap(x1));
        if x1 > x0 {
            mesh.add_colored_rect(Rect::from_min_max(pos2(x0, top), pos2(x1, bottom)), c);
        }
    };
    for i in 0..segments {
        let x = rect.min.x + i as f32 * (cell + gap);
        let lit = (value - i as f32).clamp(0.0, 1.0);
        let lost = (trail - i as f32).clamp(0.0, 1.0).max(lit);
        quad(x, x + cell * lit, color);
        quad(x + cell * lit, x + cell * lost, lost_color);
        quad(x + cell * lost, x + cell, INK_FAINT);
    }
    painter.add(Shape::mesh(mesh));
}

/// Four L-corners round `rect`.
pub fn brackets(painter: &Painter, rect: Rect, arm: f32, color: Color32) {
    for (corner, sx, sy) in [
        (rect.left_top(), 1.0, 1.0),
        (rect.right_top(), -1.0, 1.0),
        (rect.left_bottom(), 1.0, -1.0),
        (rect.right_bottom(), -1.0, -1.0),
    ] {
        glow_line(
            painter,
            vec![corner + vec2(arm * sx, 0.0), corner, corner + vec2(0.0, arm * sy)],
            color,
            LINE,
            false,
        );
    }
}

/// A diamond of radius `r` at `p`.
pub fn diamond(p: Pos2, r: f32) -> Vec<Pos2> {
    vec![p + vec2(0.0, -r), p + vec2(r, 0.0), p + vec2(0.0, r), p + vec2(-r, 0.0)]
}

/// Where a direction in view space (`x` right, `y` down) meets the edge of
/// `screen` shrunk by `inset`.
pub fn edge_point(screen: Rect, dir: Vec2, inset: f32) -> Pos2 {
    let half = screen.size() / 2.0 - vec2(inset, inset);
    let scale = (half.x / dir.x.abs().max(1e-3)).min(half.y / dir.y.abs().max(1e-3));
    screen.center() + dir * scale
}

/// A notched arrowhead with its tip at `tip`, pointing along `dir`.
pub fn edge_arrow(painter: &Painter, tip: Pos2, dir: Vec2, size: f32, color: Color32) {
    let side = vec2(-dir.y, dir.x);
    let back = tip - dir * size;
    let points = vec![tip, back + side * size * 0.6, back + dir * size * 0.35, back - side * size * 0.6];
    // Not convex (the notch): two triangles.
    painter.add(Shape::convex_polygon(vec![points[0], points[1], points[2]], color, Stroke::NONE));
    painter.add(Shape::convex_polygon(vec![points[0], points[2], points[3]], color, Stroke::NONE));
    painter.add(Shape::closed_line(points, Stroke::new(HAIR, SHADOW)));
}

/// How a label next to an edge arrow anchors, so it sits inward of it.
pub fn inward(dir: Vec2) -> Align2 {
    let along = |v: f32| match v {
        v if v > 0.3 => egui::Align::Max,
        v if v < -0.3 => egui::Align::Min,
        _ => egui::Align::Center,
    };
    Align2([along(dir.x), along(dir.y)])
}

/// An arc of the circle at `center`, `radius`, centred on `angle` (radians,
/// clockwise from up) and `half` wide either side.
pub fn arc_points(center: Pos2, radius: f32, angle: f32, half: f32) -> Vec<Pos2> {
    let steps = ((half * 2.0 * radius) / 6.0).ceil().max(4.0) as usize;
    (0..=steps)
        .map(|i| {
            let a = angle - half + 2.0 * half * i as f32 / steps as f32;
            center + vec2(a.sin(), -a.cos()) * radius
        })
        .collect()
}

/// A ring broken into four arcs (gaps on the diagonals).
pub fn broken_ring(painter: &Painter, center: Pos2, radius: f32, color: Color32) {
    for k in 0..4 {
        let a = k as f32 * std::f32::consts::FRAC_PI_2;
        glow_line(painter, arc_points(center, radius, a, FRAC_PI_4 * 0.62), color, LINE, false);
    }
}

/// A glow round the screen's edge, `width` deep, fading inward.
pub fn vignette(painter: &Painter, screen: Rect, color: Color32, width: f32) {
    let inner = screen.shrink(width);
    let mut mesh = egui::Mesh::default();
    let clear = Color32::TRANSPARENT;
    let outer = [screen.left_top(), screen.right_top(), screen.right_bottom(), screen.left_bottom()];
    let inside = [inner.left_top(), inner.right_top(), inner.right_bottom(), inner.left_bottom()];
    for i in 0..4 {
        mesh.colored_vertex(outer[i], color);
        mesh.colored_vertex(inside[i], clear);
    }
    for i in 0..4u32 {
        let (o0, i0, o1, i1) = (2 * i, 2 * i + 1, 2 * ((i + 1) % 4), 2 * ((i + 1) % 4) + 1);
        mesh.add_triangle(o0, o1, i0);
        mesh.add_triangle(i0, o1, i1);
    }
    painter.add(Shape::mesh(mesh));
}

/// A key cap: the binding's label in a small outlined box. Returns its rect.
pub fn key_cap(painter: &Painter, left_center: Pos2, key: &str, color: Color32) -> Rect {
    let g = galley(painter, key, num(SMALL), color, 0.0);
    let size = vec2((g.size().x + 10.0).max(g.size().y + 6.0), g.size().y + 4.0);
    let rect = Align2::LEFT_CENTER.anchor_size(left_center, size);
    painter.add(Shape::closed_line(cut_rect(rect, 3.0), Stroke::new(LINE, color)));
    painter.galley(rect.center() - g.size() / 2.0, g, color);
    rect
}

// Set-up: fonts, the egui theme, and the zoom.

const RAJDHANI_SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Rajdhani-SemiBold.ttf");
const RAJDHANI_BOLD: &[u8] = include_bytes!("../../../assets/fonts/Rajdhani-Bold.ttf");
const SHARE_TECH_MONO: &[u8] = include_bytes!("../../../assets/fonts/ShareTechMono-Regular.ttf");

/// Installs the HUD's fonts and theme on the egui context, once; and keeps
/// its zoom matched to the window's height, so the HUD covers the same
/// share of the screen at any resolution.
pub fn setup_egui(
    mut contexts: EguiContexts,
    window: Option<Single<&Window, With<PrimaryWindow>>>,
    mut done: Local<bool>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    if !*done {
        *done = true;
        ctx.set_fonts(fonts());
        ctx.all_styles_mut(theme);
    }
    if let Some(window) = window {
        let zoom = hud_zoom(window.height());
        if (ctx.zoom_factor() - zoom).abs() > 1e-3 {
            ctx.set_zoom_factor(zoom);
        }
    }
    Ok(())
}

/// Points per logical pixel for a window this tall: 1 at 1080 lines.
pub fn hud_zoom(height: f32) -> f32 {
    (height / 1080.0).clamp(0.8, 2.0)
}

fn fonts() -> egui::FontDefinitions {
    let mut fonts = egui::FontDefinitions::default();
    for (name, data) in [
        ("Rajdhani-SemiBold", RAJDHANI_SEMIBOLD),
        ("Rajdhani-Bold", RAJDHANI_BOLD),
        ("ShareTechMono", SHARE_TECH_MONO),
    ] {
        fonts.font_data.insert(name.into(), Arc::new(egui::FontData::from_static(data)));
    }
    // egui's own fonts stay behind ours, for the glyphs ours lack (arrows,
    // symbols).
    let fallback = |first: &str, family: &FontFamily, fonts: &egui::FontDefinitions| {
        let mut list = vec![first.to_string()];
        list.extend(fonts.families.get(family).cloned().unwrap_or_default());
        list
    };
    let proportional = fallback("Rajdhani-SemiBold", &FontFamily::Proportional, &fonts);
    let bold = fallback("Rajdhani-Bold", &FontFamily::Proportional, &fonts);
    let mono = fallback("ShareTechMono", &FontFamily::Monospace, &fonts);
    fonts.families.insert(FontFamily::Proportional, proportional);
    fonts.families.insert(FontFamily::Name("ui_bold".into()), bold);
    fonts.families.insert(FontFamily::Monospace, mono);
    fonts
}

/// egui's widgets (the menu, the tuning panel) in the HUD's colours: dark
/// scrim windows, hairline frames, square corners, SUIT for selection.
fn theme(style: &mut egui::Style) {
    use egui::{CornerRadius, TextStyle};
    // Rajdhani is small for its size: everything a step up.
    for (text_style, size) in [
        (TextStyle::Small, 13.0),
        (TextStyle::Body, 16.0),
        (TextStyle::Button, 16.0),
        (TextStyle::Heading, 22.0),
        (TextStyle::Monospace, 14.0),
    ] {
        if let Some(font) = style.text_styles.get_mut(&text_style) {
            font.size = size;
        }
    }
    let v = &mut style.visuals;
    let square = CornerRadius::ZERO;
    v.window_corner_radius = square;
    v.menu_corner_radius = square;
    v.window_fill = Color32::from_rgba_premultiplied(6, 9, 12, 236);
    v.panel_fill = v.window_fill;
    v.window_stroke = Stroke::new(HAIR, Color32::from_rgba_premultiplied(70, 80, 84, 120));
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.extreme_bg_color = Color32::from_rgb(3, 5, 7);
    v.faint_bg_color = Color32::from_rgb(14, 19, 23);
    v.selection.bg_fill = SUIT.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(HAIR, SUIT);
    v.hyperlink_color = SUIT;
    v.warn_fg_color = WARNING;
    v.error_fg_color = HOSTILE;
    let w = &mut v.widgets;
    for (visuals, fill, stroke, text) in [
        (&mut w.noninteractive, Color32::from_rgb(10, 14, 17), Color32::from_rgb(40, 50, 55), INK_DIM),
        (&mut w.inactive, Color32::from_rgb(18, 25, 30), Color32::from_rgb(44, 56, 62), INK),
        (&mut w.hovered, Color32::from_rgb(24, 38, 46), SUIT.gamma_multiply(0.7), INK),
        (&mut w.active, Color32::from_rgb(20, 52, 66), SUIT, INK),
        (&mut w.open, Color32::from_rgb(18, 30, 38), SUIT.gamma_multiply(0.6), INK),
    ] {
        visuals.bg_fill = fill;
        visuals.weak_bg_fill = fill;
        visuals.bg_stroke = Stroke::new(HAIR, stroke);
        visuals.fg_stroke = Stroke::new(LINE, text);
        visuals.corner_radius = square;
    }
}
