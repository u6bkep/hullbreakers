//! The suit's nav readouts (bottom left, over the health block) and the
//! event feed (top left).

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use egui::{Align2, Rect, pos2, vec2};

use super::style::{self, EDGE, INK, INK_DIM, MICRO, SMALL, SUIT, WARNING};
use super::vitals::SUIT_BLOCK;
use crate::client::LocalPlayer;
use crate::client::bindings::{Action, Bindings};
use crate::client::events::Presented;
use crate::client::input::MouseCaptured;
use crate::sim::bodies::Bodies;
use crate::sim::events::PlayerEvent;
use crate::sim::ship::Control;
use crate::sim::player::{Mode, Player};
use crate::sim::tuning::Tuning;

/// Player position and velocity relative to the reference body, in world axes.
pub fn relative_motion(player: &Player, bodies: &Bodies, tuning: &Tuning) -> Option<(Vec3, Vec3)> {
    let own = player.mode.body().and_then(|b| bodies.frame(b));
    let (position, velocity) = player.mode.world_kinematics(tuning, own.as_ref());
    // The same velocity flight assist holds, so readouts show what it's doing.
    let matched = bodies.matched_velocity(
        player.reference_body?,
        position,
        tuning.match_surface_near,
        tuning.match_surface_far,
    )?;
    Some((position, velocity - matched))
}

/// A small state chip: lit (SUIT outline and text) or not.
fn chip(painter: &egui::Painter, left_center: egui::Pos2, label: &str, lit: bool) -> Rect {
    let color = if lit { SUIT } else { INK_DIM };
    let g = style::galley(painter, label.to_uppercase(), style::ui(MICRO), color, 1.0);
    let rect = Align2::LEFT_CENTER.anchor_size(left_center, g.size() + vec2(12.0, 4.0));
    if lit {
        painter.rect_filled(rect, 0.0, style::fade(SUIT, 0.12));
    }
    painter.add(egui::Shape::closed_line(style::cut_rect(rect, 3.0), egui::Stroke::new(style::HAIR, style::fade(color, 0.7))));
    painter.galley(rect.center() - g.size() / 2.0, g, color);
    rect
}

/// Mode, boots and assist as chips; your speed against the reference body,
/// and the rope when it's out. Sits on the suit block.
pub fn flight_hud(
    mut contexts: EguiContexts,
    tuning: Res<Tuning>,
    player: Single<&Player, With<LocalPlayer>>,
    bodies: Bodies,
    names: Query<&Name>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("flight")));
    let name_of = |e: Option<Entity>| e.and_then(|e| names.get(e).ok()).map_or_else(|| "—".to_owned(), |n| n.as_str().to_owned());
    let relative = relative_motion(&player, &bodies, &tuning).map(|(_, v)| v.length());
    let rows = 2 + player.grapple.is_some() as usize;
    let height = 14.0 + rows as f32 * 22.0;
    let plate = Rect::from_min_size(
        pos2(screen.min.x + EDGE, screen.max.y - EDGE - SUIT_BLOCK.y - 8.0 - height),
        vec2(SUIT_BLOCK.x, height),
    );
    style::plate(&painter, plate, SUIT, 1.0);
    let left = plate.min.x + 12.0;
    let mut y = plate.min.y + 7.0 + 11.0;

    // Chips: what's holding you, and the assist.
    let booted = matches!(player.mode, Mode::Booted { .. });
    let r = chip(&painter, pos2(left, y), if booted { "Mag boots" } else { "EVA" }, true);
    let r = chip(&painter, pos2(r.max.x + 6.0, y), if player.boots_on { "Boots on" } else { "Boots off" }, player.boots_on);
    chip(&painter, pos2(r.max.x + 6.0, y), "Assist", player.flight_assist);
    y += 22.0;

    // Speed against the reference.
    let value_x = left + 46.0;
    style::caps(&painter, pos2(left, y), Align2::LEFT_CENTER, "Rel", MICRO, INK_DIM);
    let speed = relative.map_or("--.--".into(), |v| format!("{v:>5.2}"));
    let r = style::text(&painter, pos2(value_x, y), Align2::LEFT_CENTER, speed, style::num(SMALL + 2.0), INK);
    let r = style::unit(&painter, pos2(r.max.x + 3.0, y + 1.0), Align2::LEFT_CENTER, "m/s", INK_DIM);
    style::caps(&painter, pos2(r.max.x + 10.0, y), Align2::LEFT_CENTER, &format!("vs {}", name_of(player.reference_body)), MICRO, SUIT);
    y += 22.0;

    if let Some(g) = player.grapple {
        style::caps(&painter, pos2(left, y), Align2::LEFT_CENTER, "Rope", MICRO, INK_DIM);
        let r = style::text(&painter, pos2(value_x, y), Align2::LEFT_CENTER, format!("{:>5.1}", g.length), style::num(SMALL + 2.0), INK);
        let r = style::unit(&painter, pos2(r.max.x + 3.0, y + 1.0), Align2::LEFT_CENTER, "m", INK_DIM);
        let load = g.tension / tuning.reel_force;
        let color = if load > 0.5 { WARNING } else { INK };
        let r = style::text(&painter, pos2(r.max.x + 10.0, y), Align2::LEFT_CENTER, format!("{:.1}", g.tension / 1000.0), style::num(SMALL), color);
        let r = style::unit(&painter, pos2(r.max.x + 3.0, y + 1.0), Align2::LEFT_CENTER, "kN", INK_DIM);
        style::caps(&painter, pos2(r.max.x + 10.0, y), Align2::LEFT_CENTER, &format!("to {}", name_of(Some(g.target))), MICRO, SUIT);
    }
    Ok(())
}

/// How long a line stays in the feed (s).
const FEED_SECONDS: f32 = 4.0;
const FEED_LINES: usize = 4;

/// What just happened to you (your boots, your rope), newest at the top,
/// each line fading after a few seconds; and, while you're playing, where
/// the menu is.
pub fn event_feed(
    mut contexts: EguiContexts,
    time: Res<Time>,
    captured: Res<MouseCaptured>,
    bindings: Res<Bindings>,
    mut presented: MessageReader<Presented>,
    controls: Query<(&Control, &ChildOf)>,
    mut lines: Local<Vec<(String, f32)>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let now = time.elapsed_secs();
    for Presented(event) in presented.read() {
        // A press is named by what was pressed: its own label (a device's
        // name, not "Device").
        if let PlayerEvent::Pressed { body, action } = event
            && let Some((control, _)) = controls.iter().find(|(c, parent)| parent.parent() == *body && c.action == *action)
        {
            lines.insert(0, (format!("pressed: {}", control.label), now));
            continue;
        }
        if let Some(text) = event.caption() {
            lines.insert(0, (text, now));
        }
    }
    lines.retain(|(_, at)| now - at < FEED_SECONDS);
    lines.truncate(FEED_LINES);
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("feed")));
    let mut y = screen.min.y + EDGE;
    if captured.0 {
        // Where the menus are: key caps and what they open.
        let mut x = screen.min.x + EDGE;
        let dim = style::fade(INK_DIM, 0.85);
        // The actions' own names and bindings, so renames and rebinds show.
        for action in [Action::Menu, Action::Loadout, Action::TuningPanel] {
            let cap = style::key_cap(&painter, pos2(x, y + 10.0), &bindings.label(action), dim);
            let r = style::caps(&painter, pos2(cap.max.x + 6.0, y + 10.0), Align2::LEFT_CENTER, action.name(), MICRO, dim);
            x = r.max.x + 16.0;
        }
        y += 30.0;
    }
    for (i, (text, at)) in lines.iter().enumerate() {
        let alpha = style::appear(now - at, FEED_SECONDS) * if i == 0 { 1.0 } else { 0.7 };
        let x = screen.min.x + EDGE;
        // A tick before each line, SUIT for the newest.
        painter.line_segment(
            [pos2(x, y + 3.0), pos2(x, y + 15.0)],
            egui::Stroke::new(2.0, style::fade(if i == 0 { SUIT } else { INK_DIM }, alpha)),
        );
        let g = style::galley(&painter, text.clone(), style::ui(SMALL + 1.0), style::fade(INK, alpha), 0.0);
        style::shadowed(&painter, pos2(x + 8.0, y), Align2::LEFT_TOP, g, style::fade(INK, alpha));
        y += 20.0;
    }
    Ok(())
}
