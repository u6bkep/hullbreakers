//! Your health: the suit block (bottom left), the regen state, the edge of
//! the screen going red as you get low, arcs round the centre toward
//! whatever hit you, and dying.
//!
//! Where damage came from isn't something the sim records for the victim
//! (its [`HitLog`](crate::sim::vitals::HitLog) is what a player *dealt*).
//! The HUD works it out from what every peer already sees: robots' shots
//! are in their replicated event logs, and beams are traced on every peer.
//! When your health drops, the hostile shot whose line passed closest to
//! you (or the beam you were caught in) is where it came from.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use egui::{Align2, Rect, Stroke, pos2, vec2};

use super::style::{self, BOLD, EDGE, HOSTILE, INK, INK_DIM, MICRO, SUIT, WARNING};
use super::{CameraView, HudMemory, Projector, view};
use crate::client::LocalPlayer;
use crate::sim::bots::Robot;
use crate::sim::combat::Beams;
use crate::sim::events::{EventLog, LOG_TICKS, PlayerEvent};
use crate::sim::pickups::Pickup;
use crate::sim::player::Player;
use crate::sim::tuning::Tuning;
use crate::sim::vitals::Vitals;

/// The suit block's size (pt); the nav readouts stack on it.
pub const SUIT_BLOCK: egui::Vec2 = vec2(340.0, 64.0);

/// Fractions of health: below these, low and critical.
const LOW: f32 = 0.5;
const CRITICAL: f32 = 0.25;
/// How long a damage arc shows (s).
const ARC_SECONDS: f32 = 1.2;
/// How far a shot's line may pass from you and still be the one that hit.
const SHOT_MISS: f32 = 2.5;

/// How long a drop in health waits for the shot that caused it (s): on a
/// client, robots' shots arrive interpolated, after the damage.
const PENDING_SECONDS: f32 = 0.3;

/// A hostile shot seen in flight: where from, which way, when.
struct SeenShot {
    origin: Vec3,
    dir: Vec3,
    at: f32,
}

/// Where recent damage came from, for the arcs.
#[derive(Default)]
pub struct Threats {
    /// Robots' newest read event tick.
    read: HashMap<Entity, u32>,
    /// Your own deaths and beam hits already seen, by tick.
    own_seen: Vec<(u32, bool)>,
    /// Whether your log has been seen at all.
    own_started: bool,
    shots: Vec<SeenShot>,
    /// When a beam last caught you.
    beam_hit: Option<f32>,
    /// Drops in health not yet put down to a source: where you were, when.
    pending: Vec<(Vec3, f32)>,
    /// World direction toward each recent hit's source, and when.
    arcs: Vec<(Vec3, f32)>,
    last_health: Option<f32>,
}

/// The events in `log` after `cursor`, moving the cursor on. A log seen for
/// the first time only sets the cursor: what's already in it is history.
fn read_new(log: &EventLog, cursor: &mut Option<u32>) -> Vec<(u32, PlayerEvent)> {
    let first = cursor.is_none();
    let last = *cursor;
    let mut new = Vec::new();
    for (tick, event) in log.iter() {
        if last.is_some_and(|l| tick.wrapping_sub(l) as i32 <= 0) {
            continue;
        }
        *cursor = Some(cursor.map_or(tick, |n| if tick.wrapping_sub(n) as i32 > 0 { tick } else { n }));
        if !first {
            new.push((tick, event));
        }
    }
    if cursor.is_none() {
        // An empty log: start from where it is.
        *cursor = Some(log.tick());
    }
    new
}

/// Reads robots' shots, your own events and your health each frame; a drop
/// in health finds its source. Also notices dying, and the case your kit
/// was left in.
///
/// Your deaths are read straight from your log, not as presented events:
/// presentation drops what arrives late, and a death (which only the
/// authority decides) reaches a laggy client late.
pub fn track_threats(
    mut memory: ResMut<HudMemory>,
    mut threats: Local<Threats>,
    robots: Query<(Entity, &Player), With<Robot>>,
    me: Option<Single<(&Player, &Vitals), With<LocalPlayer>>>,
    beams: Option<Res<Beams>>,
    camera: CameraView,
    cases: Query<(Entity, &Pickup, &Transform)>,
) {
    let now = memory.now;
    let eye = camera.1.translation;
    // Your body is a little below your eye.
    let body = eye - camera.1.rotation * Vec3::Y * 0.5;

    let threats = &mut *threats;
    for (entity, robot) in &robots {
        let mut cursor = threats.read.get(&entity).copied();
        for (_, event) in read_new(&robot.events, &mut cursor) {
            if let PlayerEvent::Fired { origin, velocity, .. } = event
                && let Some(dir) = velocity.try_normalize()
            {
                threats.shots.push(SeenShot { origin, dir, at: now });
            }
        }
        if let Some(c) = cursor {
            threats.read.insert(entity, c);
        }
    }
    threats.read.retain(|e, _| robots.contains(*e));
    threats.shots.retain(|s| now - s.at < 1.5);

    let Some(me) = me else {
        return;
    };
    let (player, vitals) = *me;
    // Your own log is rewritten under prediction: a death the authority
    // decided turns up at a tick already passed. Each is taken once,
    // however late (a re-timed copy within a few ticks is the same one).
    let log_now = player.events.tick();
    threats.own_seen.retain(|(t, _)| log_now.wrapping_sub(*t) <= LOG_TICKS);
    let started = std::mem::replace(&mut threats.own_started, true);
    let mut own = Vec::new();
    for (tick, event) in player.events.iter() {
        let died = match event {
            PlayerEvent::Died => true,
            PlayerEvent::BeamHit => false,
            _ => continue,
        };
        if threats.own_seen.iter().any(|&(t, d)| d == died && (tick.wrapping_sub(t) as i32).unsigned_abs() <= 3) {
            continue;
        }
        threats.own_seen.push((tick, died));
        if started {
            own.push((tick, event));
        }
    }
    for (tick, event) in own {
        match event {
            PlayerEvent::BeamHit => threats.beam_hit = Some(now),
            PlayerEvent::Died => {
                if let Some(place) = memory.last_eye {
                    memory.died = Some(Death {
                        at: now,
                        place,
                        tick,
                        carried: memory.armed,
                    });
                    memory.kit = None;
                }
            }
            _ => {}
        }
    }
    // As of before this frame's events: dying empties your hands.
    memory.armed = view::armed(player);

    // A drop in health waits a moment for the shot that caused it.
    if threats.last_health.is_some_and(|before| vitals.health < before - 0.01) {
        threats.pending.push((body, now));
    }
    threats.last_health = Some(vitals.health);
    let (shots, beam_hit) = (&threats.shots, threats.beam_hit);
    let mut found = Vec::new();
    threats.pending.retain(|&(at, when)| {
        let shot = shots
            .iter()
            .filter_map(|s| {
                let along = (at - s.origin).dot(s.dir);
                let miss = (at - (s.origin + s.dir * along)).length();
                (along > 0.0 && miss < SHOT_MISS).then_some((miss, s.origin))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, origin)| origin);
        let beam = || {
            beam_hit.filter(|hit| (hit - when).abs() < 0.5)?;
            beams
                .as_ref()?
                .0
                .iter()
                .map(|b| {
                    let d = (b.to - b.from).normalize_or_zero();
                    let along = (at - b.from).dot(d).clamp(0.0, b.from.distance(b.to));
                    ((at - (b.from + d * along)).length(), b.from)
                })
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, from)| from)
        };
        match shot.or_else(beam).and_then(|source| (source - at).try_normalize()) {
            Some(dir) => {
                found.push(dir);
                false
            }
            None => now - when < PENDING_SECONDS,
        }
    });
    threats.arcs.extend(found.into_iter().map(|dir| (dir, now)));
    threats.arcs.retain(|(_, at)| now - at < ARC_SECONDS);
    memory.arcs.clone_from(&threats.arcs);

    // Your kit is in the case left on the tick you died, where you fell.
    if let Some(death) = memory.died
        && death.carried
        && memory.kit.is_none()
        && now - death.at < 5.0
    {
        memory.kit = cases
            .iter()
            .filter_map(|(e, case, t)| {
                let d = t.translation.distance(death.place);
                ((case.since.wrapping_sub(death.tick) as i32).abs() <= 2 && d < 8.0).then_some((e, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(e, _)| e);
    }
    memory.last_eye = Some(eye);
}

/// Your last death.
#[derive(Clone, Copy, Debug)]
pub struct Death {
    /// HUD time.
    pub at: f32,
    /// Where your eye was.
    pub place: Vec3,
    /// The sim tick.
    pub tick: u32,
    /// Whether you had anything to leave behind.
    pub carried: bool,
}

/// The health block, its regen, the low-health edge, the flash of being
/// hit, and the damage arcs.
pub fn vitals_hud(
    mut contexts: EguiContexts,
    time: Res<Time>,
    tuning: Res<Tuning>,
    memory: Res<HudMemory>,
    vitals: Option<Single<&Vitals, With<LocalPlayer>>>,
    camera: CameraView,
    // What the bar shows (eased), and the white trail of what was lost.
    mut shown: Local<Option<(f32, f32)>>,
    mut last: Local<Option<f32>>,
    mut trail_hold: Local<f32>,
    mut flash: Local<f32>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Some(vitals) = vitals else {
        return Ok(());
    };
    let dt = time.delta_secs();
    let now = memory.now;
    let fraction = (vitals.health / tuning.player_health).clamp(0.0, 1.0);
    let (bar, trail) = shown.get_or_insert((fraction, fraction));
    if last.is_some_and(|before| fraction < before - 0.001) {
        *flash = 1.0;
        *trail_hold = 0.35;
        *trail = trail.max(*bar);
    }
    *last = Some(fraction);
    *bar = style::ease(*bar, fraction, dt, style::EASE);
    *trail_hold -= dt;
    if *trail_hold <= 0.0 {
        *trail = (*trail - dt * 0.15).max(*bar);
    }
    *flash = (*flash - dt * 2.5).max(0.0);
    let (bar, trail) = (*bar, *trail);

    let screen = ctx.content_rect();
    let edge_painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("vitals edge")));
    // The edge: red as health runs low (beating when critical), and a
    // flash on each hit.
    let low = ((LOW - fraction) / LOW).clamp(0.0, 1.0);
    let beat = if fraction < CRITICAL { 0.75 + 0.25 * (now * 1.2 * std::f32::consts::TAU).sin().abs() } else { 1.0 };
    let edge = (low * 0.55 * beat).max(*flash * 0.45);
    if edge > 0.01 {
        let depth = screen.height() * (0.12 + 0.1 * low);
        style::vignette(&edge_painter, screen, egui::Color32::from_rgba_unmultiplied(200, 24, 14, (edge * 255.0) as u8), depth);
    }

    // Damage arcs round the centre, toward each recent hit's source.
    let (camera, cam) = *camera;
    let view = Projector::new(camera, cam, ctx);
    let ring = screen.height() * 0.26;
    for &(dir, at) in &memory.arcs {
        let age = now - at;
        let alpha = 1.0 - (age / ARC_SECONDS).powi(2);
        // As a compass in the view's own plane: ahead is the top of the
        // ring, behind the bottom. Straight above or below you, it's where
        // it would be on screen.
        let local = view.local(cam.translation + dir);
        let flat = vec2(local.x, -local.z);
        let angle = if flat.length() > 0.2 {
            flat.x.atan2(flat.y)
        } else if local.y > 0.0 {
            0.0
        } else {
            std::f32::consts::PI
        };
        let points = style::arc_points(screen.center(), ring, angle, 0.38);
        edge_painter.add(egui::Shape::line(points.clone(), Stroke::new(BOLD + 8.0, style::fade(HOSTILE, alpha * 0.22))));
        edge_painter.add(egui::Shape::line(points, Stroke::new(BOLD + 1.5, style::fade(HOSTILE, alpha))));
        // A notch pointing outward, at its middle.
        let out = vec2(angle.sin(), -angle.cos());
        let tip = screen.center() + out * (ring + 12.0);
        let side = vec2(-out.y, out.x) * 7.0;
        edge_painter.add(egui::Shape::convex_polygon(
            vec![tip, screen.center() + out * (ring + 3.0) + side, screen.center() + out * (ring + 3.0) - side],
            style::fade(HOSTILE, alpha),
            Stroke::NONE,
        ));
    }

    // The suit block, bottom left.
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("vitals")));
    let plate = Rect::from_min_size(pos2(screen.min.x + EDGE, screen.max.y - EDGE - SUIT_BLOCK.y), SUIT_BLOCK);
    let color = if fraction < CRITICAL {
        style::fade(HOSTILE, style::pulse(now))
    } else if fraction < LOW {
        WARNING
    } else {
        INK
    };
    style::plate(&painter, plate, if fraction < LOW { color } else { SUIT }, 1.0);
    let inner = plate.shrink2(vec2(12.0, 8.0));
    // The number, big, on the left.
    let n = style::text(
        &painter,
        pos2(inner.min.x, inner.center().y + 2.0),
        Align2::LEFT_CENTER,
        format!("{:.0}", vitals.health.ceil()),
        style::num(style::NUMERAL),
        color,
    );
    let x = n.max.x.max(inner.min.x + 58.0) + 10.0;
    let bar_rect = Rect::from_min_max(pos2(x, inner.center().y + 1.0), pos2(inner.max.x, inner.center().y + 11.0));
    style::caps(&painter, pos2(x, bar_rect.min.y - 5.0), Align2::LEFT_BOTTOM, "Suit integrity", MICRO, INK_DIM);
    style::seg_bar(&painter, bar_rect, 10, bar, trail, color);

    // Regen: waiting (a countdown), then on (a sweep over what's missing).
    let hurt = fraction < 0.999;
    if hurt {
        let waited = vitals.since_hurt / tuning.health_regen_delay.max(0.01);
        let status = if waited >= 1.0 {
            let sweep = (now * 0.8).fract();
            let gap = Rect::from_min_max(pos2(bar_rect.min.x + bar_rect.width() * bar, bar_rect.min.y), bar_rect.max);
            if gap.width() > 1.0 {
                let x = gap.min.x + gap.width() * sweep;
                painter.rect_filled(
                    Rect::from_min_max(pos2((x - 14.0).max(gap.min.x), gap.min.y), pos2(x, gap.max.y)),
                    0.0,
                    style::fade(SUIT, 0.55),
                );
            }
            ("Regen", SUIT)
        } else {
            let wait = Rect::from_min_size(pos2(bar_rect.min.x, bar_rect.max.y + 3.0), vec2(bar_rect.width() * waited, 2.0));
            painter.rect_filled(wait, 0.0, style::fade(SUIT, 0.6));
            ("Regen pending", INK_DIM)
        };
        style::caps(&painter, pos2(inner.max.x, bar_rect.min.y - 5.0), Align2::RIGHT_BOTTOM, status.0, MICRO, status.1);
    }
    Ok(())
}

/// How long the death card shows (s).
const DEATH_SECONDS: f32 = 3.5;

/// Dying: the view goes dark and red, SUIT FAILURE, and where your kit is.
pub fn death_overlay(mut contexts: EguiContexts, memory: Res<HudMemory>) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Some(death) = memory.died else {
        return Ok(());
    };
    let age = memory.now - death.at;
    if age > DEATH_SECONDS {
        return Ok(());
    }
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("death")));
    // Dark at once, clearing over the last second and a half.
    let dark = ((DEATH_SECONDS - age) / 1.5).clamp(0.0, 1.0);
    painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha((dark * 190.0) as u8));
    style::vignette(&painter, screen, egui::Color32::from_rgba_unmultiplied(170, 16, 10, (dark * 140.0) as u8), screen.height() * 0.2);
    let alpha = style::appear(age, DEATH_SECONDS);
    let c = pos2(screen.center().x, screen.min.y + screen.height() * 0.36);
    super::mission::banner(&painter, c, "SUIT FAILURE", HOSTILE, alpha);
    let line = if death.carried {
        "Respawning aboard. Your kit is where you fell."
    } else {
        "Respawning aboard."
    };
    let g = style::galley(&painter, line, style::ui(style::BODY + 2.0), style::fade(INK, alpha), 0.0);
    style::shadowed(&painter, c + vec2(0.0, 44.0), Align2::CENTER_CENTER, g, style::fade(INK, alpha));
    Ok(())
}
