//! The scenario, top centre: its objective and status lines, the clock,
//! and in a ship fight both ships' hulls and the enemy's shield; a banner
//! when it's decided; the debrief (with the crew's score cards) back in the
//! field; and the fade after a hard cut. Everything comes from the
//! replicated [`Mission`] and ships, so hosts and clients agree.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use egui::{Align2, Color32, Rect, Stroke, pos2, vec2};

use super::style::{self, BODY, DISPLAY, EDGE, FRIENDLY, HAIR, HOSTILE, INK, INK_DIM, MICRO, OBJECTIVE, SHIELD, SMALL, TITLE, WARNING};
use crate::client::camera::CameraFx;
use crate::sim::SimTick;
use crate::sim::combat::Combatant;
use crate::sim::mission::{CrewCard, Debrief, Mission, Phase, ScenarioKind};
use crate::sim::ship::HomeShip;

fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Whether a line shows a time of its own ("0:42"): then the scenario keeps
/// its own clock, and the HUD's would be a second one.
fn has_clock(line: &str) -> bool {
    let b = line.as_bytes();
    b.windows(4).any(|w| w[0].is_ascii_digit() && w[1] == b':' && w[2].is_ascii_digit() && w[3].is_ascii_digit())
}

/// Bar width in the strip (pt).
const BAR: f32 = 230.0;
/// How long the debrief shows once the crew is back in the field (s).
const DEBRIEF_SECONDS: f32 = 20.0;

pub fn mission_hud(
    mut contexts: EguiContexts,
    time: Res<Time>,
    tick: Res<SimTick>,
    fx: Res<CameraFx>,
    missions: Query<&Mission>,
    ships: Query<(&Combatant, Has<HomeShip>)>,
    mut memory: ResMut<super::HudMemory>,
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
    // The clock stops when the mission is decided.
    let elapsed = mission.elapsed(mission.ended.unwrap_or(tick.0));
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("mission")));

    // A ship fight: both hulls, and the enemy's shield.
    let ours = ships.iter().find(|(_, home)| *home).map(|(c, _)| c);
    let enemy = ships.iter().find(|(_, home)| !*home).map(|(c, _)| c);
    let fight = mission.kind == ScenarioKind::Boarding;

    if !mission.objective.is_empty() || fight {
        let objective_color = match mission.phase {
            Phase::Underway => OBJECTIVE,
            Phase::Won => FRIENDLY,
            Phase::Lost => HOSTILE,
        };
        let own_clock = has_clock(&mission.objective) || mission.status.iter().any(|l| has_clock(l));
        let objective_g = style::galley(&painter, mission.objective.clone(), style::ui(BODY), objective_color, 0.0);
        let status_g: Vec<_> = mission.status.iter().map(|l| style::galley(&painter, l.clone(), style::num(SMALL), INK, 0.0)).collect();
        let clock_g = (!own_clock).then(|| style::galley(&painter, clock(elapsed), style::num(TITLE), INK, 0.0));

        // Rows: [our hull  clock  their hull] in a fight; the objective
        // (with the clock beside it, outside a fight); the status lines.
        let bars_h = if fight { 34.0 } else { 0.0 };
        let objective_w = objective_g.size().x + 40.0 + if fight { 0.0 } else { clock_g.as_ref().map_or(0.0, |g| g.size().x + 24.0) };
        let status_w = status_g.iter().map(|g| g.size().x).fold(0.0, f32::max);
        let status_h = status_g.iter().map(|g| g.size().y + 2.0).sum::<f32>();
        let width = objective_w.max(status_w + 40.0).max(if fight { 2.0 * BAR + 150.0 } else { 320.0 });
        let height = 12.0 + bars_h + 26.0 + status_h + if status_g.is_empty() { 4.0 } else { 10.0 };
        let plate = Rect::from_min_size(pos2(screen.center().x - width / 2.0, screen.min.y + EDGE * 0.6), vec2(width, height));
        style::plate(&painter, plate, OBJECTIVE, 1.0);
        memory.mission_strip = Some(plate);
        let mut y = plate.min.y + 10.0;

        if fight {
            let ours_hull = ours.map_or(0.0, |c| c.integrity);
            // A wrecked ship's entity is gone: no ship, no hull.
            let enemy_hull = enemy.map_or(0.0, |c| c.integrity);
            let dt = time.delta_secs();
            shown.0 = style::ease(shown.0, ours_hull, dt, style::EASE * 3.0);
            shown.1 = style::ease(shown.1, enemy_hull, dt, style::EASE * 3.0);
            let shield_up = enemy.is_some_and(|c| c.shield);
            let row = y + 16.0;
            let (left, right) = (plate.min.x + 14.0, plate.max.x - 14.0);
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
            if let Some(g) = &clock_g {
                painter.galley(Align2::CENTER_CENTER.anchor_size(pos2(plate.center().x, row), g.size()).min, g.clone(), INK);
            }
            y += bars_h;
        }

        // The objective, in chevrons: this is the thing to do.
        let at = pos2(plate.center().x, y + 13.0);
        let clock_beside = if fight { None } else { clock_g };
        let total = objective_g.size().x + clock_beside.as_ref().map_or(0.0, |g| g.size().x + 24.0);
        let r = Align2::LEFT_CENTER.anchor_size(pos2(at.x - total / 2.0, at.y), objective_g.size());
        painter.galley(r.min, objective_g, objective_color);
        if let Some(g) = clock_beside {
            painter.galley(Align2::LEFT_CENTER.anchor_size(pos2(r.max.x + 24.0, at.y), g.size()).min, g, INK);
        }
        if !mission.objective.is_empty() {
            let right = r.min.x + total;
            for (x, s) in [(r.min.x - 10.0, 1.0), (right + 10.0, -1.0)] {
                painter.line_segment([pos2(x - s * 4.0, at.y - 5.0), pos2(x, at.y)], Stroke::new(1.5, objective_color));
                painter.line_segment([pos2(x - s * 4.0, at.y + 5.0), pos2(x, at.y)], Stroke::new(1.5, objective_color));
            }
        }
        y += 26.0;

        // The scenario's board, left-aligned as a block (its columns line up
        // in the mono face).
        if !status_g.is_empty() {
            painter.line_segment(
                [pos2(plate.min.x + 14.0, y + 2.0), pos2(plate.max.x - 14.0, y + 2.0)],
                Stroke::new(HAIR, style::INK_FAINT),
            );
            y += 6.0;
            let x = plate.center().x - status_w / 2.0;
            for g in status_g {
                let h = g.size().y;
                painter.galley(pos2(x, y), g, INK);
                y += h + 2.0;
            }
        }
    }

    // Banner for a few seconds after the mission is decided.
    if let Some(ended) = mission.ended {
        let since = mission.elapsed(tick.0) - mission.elapsed(ended);
        if (0.0..5.0).contains(&since) {
            let color = match mission.phase {
                Phase::Won => FRIENDLY,
                _ => HOSTILE,
            };
            let alpha = style::appear(since, 5.0);
            let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("banner")));
            banner(&p, pos2(screen.center().x, screen.min.y + screen.height() * 0.3), &mission.outcome, color, alpha);
        }
    }

    // Debrief for the mission before this one, for a while after the field starts.
    let since_start = mission.elapsed(tick.0);
    if let Some(debrief) = &mission.debrief
        && mission.kind == ScenarioKind::Field
        && since_start < DEBRIEF_SECONDS
    {
        debrief_card(&painter, screen, debrief, style::appear(since_start, DEBRIEF_SECONDS));
    }
    Ok(())
}

/// Score-card columns: (heading, width).
const COLUMNS: [(&str, f32); 5] = [("Crew", 150.0), ("EVA", 64.0), ("Swing", 64.0), ("Presses", 70.0), ("Deaths", 60.0)];

fn card_row(card: &CrewCard) -> [String; 5] {
    [
        card.name.clone(),
        clock(card.vacuum_seconds),
        format!("{:.0} m", card.longest_swing),
        card.presses.to_string(),
        card.deaths.to_string(),
    ]
}

/// The last mission: how it ended, the scenario's report, a score card per
/// crew member, and the awards.
fn debrief_card(painter: &egui::Painter, screen: Rect, debrief: &Debrief, alpha: f32) {
    let won = debrief.phase == Phase::Won;
    let color = style::fade(if won { FRIENDLY } else { HOSTILE }, alpha);
    let ink = style::fade(INK, alpha);
    let dim = style::fade(INK_DIM, alpha);
    let lines: Vec<_> = debrief.lines.iter().map(|l| style::galley(painter, l.clone(), style::ui(BODY), ink, 0.0)).collect();
    let awards: Vec<_> = debrief
        .awards
        .iter()
        .map(|l| style::galley(painter, l.clone(), style::ui(BODY), style::fade(OBJECTIVE, alpha), 0.0))
        .collect();
    let table_w: f32 = COLUMNS.iter().map(|c| c.1).sum::<f32>() + 70.0;
    let credit_rows = debrief.crew.iter().filter(|c| !c.credits.is_empty()).count();
    let width = lines.iter().chain(&awards).map(|g| g.size().x + 14.0).fold(table_w.max(440.0), f32::max) + 40.0;
    let crew_h = if debrief.crew.is_empty() { 0.0 } else { 30.0 + debrief.crew.len() as f32 * 22.0 + credit_rows as f32 * 18.0 };
    let height = 84.0 + lines.len() as f32 * 24.0 + crew_h + awards.len() as f32 * 24.0 + 10.0;
    let plate = Rect::from_min_size(pos2(screen.center().x - width / 2.0, screen.min.y + screen.height() * 0.14), vec2(width, height));
    style::plate(painter, plate, color, alpha);
    let (x, right) = (plate.min.x + 20.0, plate.max.x - 20.0);
    style::caps(painter, pos2(x, plate.min.y + 14.0), Align2::LEFT_TOP, "Debrief", MICRO, dim);
    style::caps(
        painter,
        pos2(right, plate.min.y + 14.0),
        Align2::RIGHT_TOP,
        &format!("{}  {}", debrief.scenario.button_label(), clock(debrief.seconds)),
        MICRO,
        dim,
    );
    let outcome = match (debrief.outcome.is_empty(), won) {
        (false, _) => debrief.outcome.to_uppercase(),
        (true, true) => "MISSION COMPLETE".into(),
        (true, false) => "MISSION FAILED".into(),
    };
    let g = style::galley(painter, outcome, style::bold(TITLE + 4.0), color, 2.0);
    painter.galley(pos2(x, plate.min.y + 32.0), g, color);
    let rule = |y: f32| painter.line_segment([pos2(x, y), pos2(right, y)], Stroke::new(HAIR, style::fade(style::INK_FAINT, alpha)));
    let mut y = plate.min.y + 72.0;
    rule(y);
    y += 10.0;
    for g in lines {
        painter.galley(pos2(x, y), g, ink);
        y += 24.0;
    }
    if !debrief.crew.is_empty() {
        y += 4.0;
        let mut cx = x;
        for (heading, w) in COLUMNS {
            style::caps(painter, pos2(cx, y), Align2::LEFT_TOP, heading, MICRO, dim);
            cx += w;
        }
        y += 20.0;
        for card in &debrief.crew {
            let mut cx = x;
            for (i, (cell, (_, w))) in card_row(card).into_iter().zip(COLUMNS).enumerate() {
                let font = if i == 0 { style::ui(BODY) } else { style::num(SMALL) };
                let c = if i == 4 && card.deaths > 0 { style::fade(WARNING, alpha) } else { ink };
                style::text(painter, pos2(cx, y + 9.0), Align2::LEFT_CENTER, cell, font, c);
                cx += w;
            }
            let (word, c) = if card.aboard { ("Home", FRIENDLY) } else { ("Adrift", WARNING) };
            style::caps(painter, pos2(right, y + 9.0), Align2::RIGHT_CENTER, word, MICRO, style::fade(c, alpha));
            y += 22.0;
            if !card.credits.is_empty() {
                style::text(painter, pos2(x + 12.0, y + 7.0), Align2::LEFT_CENTER, card.credits.join(", "), style::ui(SMALL), dim);
                y += 18.0;
            }
        }
        y += 6.0;
    }
    for g in awards {
        // A small diamond: an award.
        painter.add(egui::Shape::convex_polygon(style::diamond(pos2(x + 4.0, y + 11.0), 4.0), style::fade(OBJECTIVE, alpha), Stroke::NONE));
        painter.galley(pos2(x + 14.0, y), g, ink);
        y += 24.0;
    }
}

/// A big centred word with rules either side.
pub fn banner(painter: &egui::Painter, at: egui::Pos2, text: &str, color: Color32, alpha: f32) {
    if text.is_empty() {
        return;
    }
    let color = style::fade(color, alpha);
    let g = style::galley(painter, text.to_uppercase(), style::bold(DISPLAY), color, DISPLAY * 0.12);
    let r = style::shadowed(painter, at, Align2::CENTER_CENTER, g, color);
    for s in [-1.0, 1.0] {
        let x0 = if s < 0.0 { r.min.x - 18.0 } else { r.max.x + 18.0 };
        painter.line_segment([pos2(x0, at.y), pos2(x0 + s * 120.0, at.y)], Stroke::new(HAIR, color));
        painter.line_segment([pos2(x0, at.y - 6.0), pos2(x0, at.y + 6.0)], Stroke::new(style::LINE, color));
    }
}

#[cfg(test)]
mod tests {
    use super::has_clock;

    #[test]
    fn a_line_with_a_time_keeps_its_own_clock() {
        assert!(has_clock("Fly the gates in order — 1:42 left"));
        assert!(has_clock("Overload in 0:09"));
        assert!(!has_clock("Board the carrier and disable its shield on the bridge"));
        assert!(!has_clock("Aboard: 1 of 2"));
    }
}
