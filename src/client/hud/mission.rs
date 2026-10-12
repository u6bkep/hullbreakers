//! The scenario, top centre: both ships' hulls, the carrier's shield, the
//! objective and the clock during a fight; a banner when it's decided; the
//! debrief back in the field; and the fade after a hard cut. Nothing here
//! reads authority-only state, so hosts and clients agree.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use egui::{Align2, Color32, Rect, Stroke, pos2, vec2};

use super::style::{self, BODY, DISPLAY, EDGE, FRIENDLY, HAIR, HOSTILE, INK, INK_DIM, MICRO, OBJECTIVE, SHIELD, SMALL, TITLE, WARNING};
use crate::client::camera::CameraFx;
use crate::sim::combat::Combatant;
use crate::sim::mission::{Mission, Phase, ScenarioKind};
use crate::sim::ship::HomeShip;
use crate::sim::{SimTick, TICK_HZ};

/// Seconds between two ticks of the (replicated) sim clock.
fn seconds_since(now: u32, then: u32) -> f32 {
    now.wrapping_sub(then) as f32 / TICK_HZ as f32
}

fn clock(seconds: f32) -> String {
    let s = seconds as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Bar width in the strip (pt).
const BAR: f32 = 230.0;

pub fn mission_hud(
    mut contexts: EguiContexts,
    time: Res<Time>,
    tick: Res<SimTick>,
    fx: Res<CameraFx>,
    missions: Query<&Mission>,
    ships: Query<(&Combatant, Has<HomeShip>)>,
    // Eased hull bars: ours, theirs.
    mut shown: Local<(f32, f32)>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect();
    let now = time.elapsed_secs();

    if fx.fade() > 0.0 {
        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("fade")))
            .rect_filled(screen, 0.0, Color32::from_black_alpha((fx.fade() * 255.0) as u8));
    }

    let Ok(mission) = missions.single() else {
        return Ok(());
    };
    let elapsed = seconds_since(tick.0, mission.started);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("mission")));

    if mission.kind == ScenarioKind::Boarding {
        let ours = ships.iter().find(|(_, home)| *home).map(|(c, _)| c);
        let enemy = ships.iter().find(|(_, home)| !*home).map(|(c, _)| c);
        // A wrecked ship's entity is gone: no ship, no hull.
        let ours_hull = ours.map_or(0.0, |c| c.integrity);
        let enemy_hull = enemy.map_or(0.0, |c| c.integrity);
        let dt = time.delta_secs();
        shown.0 = style::ease(shown.0, ours_hull, dt, style::EASE * 3.0);
        shown.1 = style::ease(shown.1, enemy_hull, dt, style::EASE * 3.0);
        let shield_up = enemy.is_some_and(|c| c.shield);
        let (objective, objective_color) = match mission.phase {
            Phase::Won => ("Carrier destroyed. Return to the mission wall", OBJECTIVE),
            Phase::Lost => ("Hull breached. Abandon ship", HOSTILE),
            Phase::Underway if shield_up => ("Board the carrier and disable its shield on the bridge", OBJECTIVE),
            Phase::Underway => ("Shield down. Get clear!", style::fade(WARNING, style::pulse(now))),
        };

        // The strip: [our hull] clock [their hull], objective under it.
        let objective_g = style::galley(&painter, objective, style::ui(BODY), objective_color, 0.0);
        let width = (2.0 * BAR + 150.0).max(objective_g.size().x + 60.0);
        let plate = Rect::from_min_size(pos2(screen.center().x - width / 2.0, screen.min.y + EDGE * 0.6), vec2(width, 74.0));
        style::plate(&painter, plate, OBJECTIVE, 1.0);
        let row = plate.min.y + 24.0;
        let left = plate.min.x + 14.0;
        let right = plate.max.x - 14.0;

        let ours_color = if ours_hull < 0.3 { style::fade(HOSTILE, style::pulse(now)) } else { FRIENDLY };
        style::caps(&painter, pos2(left, row - 8.0), Align2::LEFT_BOTTOM, "Our hull", MICRO, INK_DIM);
        style::text(&painter, pos2(left + BAR, row - 7.0), Align2::RIGHT_BOTTOM, format!("{:.0}%", ours_hull * 100.0), style::num(SMALL), ours_color);
        style::seg_bar(&painter, Rect::from_min_size(pos2(left, row - 4.0), vec2(BAR, 8.0)), 20, shown.0, 0.0, ours_color);

        let (state, state_color) = match (enemy.is_some(), shield_up) {
            (false, _) => ("Destroyed", INK_DIM),
            (true, true) => ("Shielded", SHIELD),
            (true, false) => ("Shield down", WARNING),
        };
        style::caps(&painter, pos2(right - BAR, row - 8.0), Align2::LEFT_BOTTOM, "Carrier", MICRO, INK_DIM);
        style::caps(&painter, pos2(right, row - 8.0), Align2::RIGHT_BOTTOM, state, MICRO, state_color);
        let theirs = Rect::from_min_size(pos2(right - BAR, row - 4.0), vec2(BAR, 8.0));
        style::seg_bar(&painter, theirs, 20, shown.1, 0.0, HOSTILE);
        if shield_up {
            // The shield as a frame round their bar.
            painter.rect_stroke(theirs.expand(2.5), 0.0, Stroke::new(HAIR, SHIELD), egui::StrokeKind::Middle);
        }

        style::text(&painter, pos2(plate.center().x, row), Align2::CENTER_CENTER, clock(elapsed), style::num(TITLE), INK);
        let at = pos2(plate.center().x, plate.max.y - 16.0);
        let r = Align2::CENTER_CENTER.anchor_size(at, objective_g.size());
        painter.galley(r.min, objective_g, objective_color);
        // Chevrons either side: this is the thing to do.
        for (x, s) in [(r.min.x - 10.0, 1.0), (r.max.x + 10.0, -1.0)] {
            painter.line_segment([pos2(x - s * 4.0, at.y - 5.0), pos2(x, at.y)], Stroke::new(1.5, objective_color));
            painter.line_segment([pos2(x - s * 4.0, at.y + 5.0), pos2(x, at.y)], Stroke::new(1.5, objective_color));
        }
    }

    // Banner for a few seconds after the fight is decided.
    if let Some(ended) = mission.ended
        && mission.kind == ScenarioKind::Boarding
    {
        let since = seconds_since(tick.0, ended);
        if (0.0..5.0).contains(&since) {
            let (text, color) = match mission.phase {
                Phase::Won => ("CARRIER DESTROYED", FRIENDLY),
                Phase::Lost => ("SHIP LOST", HOSTILE),
                Phase::Underway => ("", INK),
            };
            let alpha = style::appear(since, 5.0);
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("banner")));
            banner(&p, pos2(screen.center().x, screen.min.y + screen.height() * 0.3), text, color, alpha);
        }
    }

    // Debrief for the mission before this one, for a while after the field starts.
    if let Some(debrief) = &mission.debrief
        && mission.kind == ScenarioKind::Field
        && elapsed < 15.0
    {
        let alpha = style::appear(elapsed, 15.0);
        let won = debrief.phase == Phase::Won;
        let (title, color) = if won { ("Carrier destroyed", FRIENDLY) } else { ("Ship lost", HOSTILE) };
        let mut lines = vec![match (&debrief.shield_breaker, won) {
            (Some(who), _) => format!("Shield dropped by {who}. Remember that name."),
            (None, true) => "Nobody dropped the shield; it fell on its own. Suspicious.".into(),
            (None, false) => "Nobody ever dropped the shield. Bold strategy.".into(),
        }];
        if !debrief.aboard.is_empty() {
            lines.push(format!("Made it home: {}", debrief.aboard.join(", ")));
        }
        if !debrief.adrift.is_empty() {
            lines.push(format!("Left drifting: {}. Send a postcard.", debrief.adrift.join(", ")));
        }
        let ink = style::fade(INK, alpha);
        let galleys: Vec<_> = lines.into_iter().map(|l| style::galley(&painter, l, style::ui(BODY), ink, 0.0)).collect();
        let width = galleys.iter().map(|g| g.size().x).fold(420.0, f32::max) + 40.0;
        let height = 92.0 + galleys.len() as f32 * 24.0;
        let plate = Rect::from_min_size(pos2(screen.center().x - width / 2.0, screen.min.y + screen.height() * 0.14), vec2(width, height));
        style::plate(&painter, plate, color, alpha);
        let x = plate.min.x + 20.0;
        style::caps(&painter, pos2(x, plate.min.y + 14.0), Align2::LEFT_TOP, "Debrief", MICRO, style::fade(INK_DIM, alpha));
        style::caps(
            &painter,
            pos2(plate.max.x - 20.0, plate.min.y + 14.0),
            Align2::RIGHT_TOP,
            &format!("{}  {}", debrief.scenario.button_label(), clock(debrief.seconds)),
            MICRO,
            style::fade(INK_DIM, alpha),
        );
        let g = style::galley(&painter, title.to_uppercase(), style::bold(TITLE + 4.0), style::fade(color, alpha), 2.0);
        painter.galley(pos2(x, plate.min.y + 32.0), g, style::fade(color, alpha));
        let rule = plate.min.y + 72.0;
        painter.line_segment([pos2(x, rule), pos2(plate.max.x - 20.0, rule)], Stroke::new(HAIR, style::fade(style::INK_FAINT, alpha)));
        let mut y = rule + 10.0;
        for g in galleys {
            painter.galley(pos2(x, y), g, ink);
            y += 24.0;
        }
    }
    Ok(())
}

/// A big centred word with rules either side.
pub fn banner(painter: &egui::Painter, at: egui::Pos2, text: &str, color: Color32, alpha: f32) {
    if text.is_empty() {
        return;
    }
    let color = style::fade(color, alpha);
    let g = style::galley(painter, text, style::bold(DISPLAY), color, DISPLAY * 0.12);
    let r = style::shadowed(painter, at, Align2::CENTER_CENTER, g, color);
    for s in [-1.0, 1.0] {
        let x0 = if s < 0.0 { r.min.x - 18.0 } else { r.max.x + 18.0 };
        painter.line_segment([pos2(x0, at.y), pos2(x0 + s * 120.0, at.y)], Stroke::new(HAIR, color));
        painter.line_segment([pos2(x0, at.y - 6.0), pos2(x0, at.y + 6.0)], Stroke::new(style::LINE, color));
    }
}
