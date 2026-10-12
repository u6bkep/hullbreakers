//! The centre of the screen: the crosshair, the grapple's range ring and
//! rope gauge, the interact prompt, and hit markers. Nothing else lives
//! here (see the style guide's layout).

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use egui::{Align2, Color32, Stroke, pos2, vec2};

use super::style::{self, BOLD, INK, INK_DIM, LINE, SMALL, SUIT, WARNING, HOSTILE, WEAK};
use super::view;
use crate::client::LocalPlayer;
use crate::client::bindings::{Action, Bindings};
use crate::client::camera::PlayerCamera;
use crate::client::input::MouseCaptured;
use crate::sim::{Layer, SimTick};
use crate::sim::bodies::Bodies;
use crate::sim::pickups::Pickup;
use crate::sim::player::{CONTROL_REACH, Player};
use crate::sim::ship::Control;
use crate::sim::tuning::Tuning;
use crate::sim::vitals::{Hit, HitLog};

pub fn reticle(
    mut contexts: EguiContexts,
    tuning: Res<Tuning>,
    bindings: Res<Bindings>,
    player: Single<(Entity, &Player), With<LocalPlayer>>,
    bodies: Bodies,
    camera: Single<&Transform, With<PlayerCamera>>,
    spatial: SpatialQuery,
    controls: Query<(&Control, &Transform, &ChildOf)>,
    pickups: Query<&Pickup>,
    names: Query<&Name>,
    tick: Res<SimTick>,
    captured: Res<MouseCaptured>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    // With the menu or the tuning panel open (the mouse free), the world's
    // prompts aren't for you: the centre stays clear for the window.
    if !captured.0 {
        return Ok(());
    }
    let (own, player) = *player;
    let c = ctx.content_rect().center();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("reticle")));

    // The crosshair: four ticks round a dot, outlined dark so it holds on a
    // bright hull.
    let (gap, len) = (4.0, 7.0);
    for d in [vec2(1.0, 0.0), vec2(-1.0, 0.0), vec2(0.0, 1.0), vec2(0.0, -1.0)] {
        let seg = [c + d * gap, c + d * (gap + len)];
        painter.line_segment(seg, Stroke::new(LINE + 2.0, Color32::from_black_alpha(110)));
        painter.line_segment(seg, Stroke::new(LINE, style::fade(INK, 0.9)));
    }
    painter.circle_filled(c, 1.6, Color32::from_black_alpha(110));
    painter.circle_filled(c, 1.0, INK);

    // A button or a case under the crosshair wins over the grapple readout.
    let prompt = control_prompt(player, own, &tuning, tick.0, &spatial, &bodies, &controls, &pickups, &names);
    if let Some((label, false)) = &prompt {
        // Something here that isn't ours to take: say so, without a key.
        let at = c + vec2(26.0, 0.0);
        let label_g = style::galley(&painter, label.clone(), style::ui(style::BODY), INK_DIM, 0.0);
        let plate = Align2::LEFT_CENTER.anchor_size(at, label_g.size() + vec2(16.0, 10.0));
        style::plate(&painter, plate, INK_DIM, 1.0);
        painter.galley(plate.min + vec2(8.0, 5.0), label_g, INK_DIM);
    } else if let Some((label, _)) = prompt {
        style::broken_ring(&painter, c, 15.0, SUIT);
        let at = c + vec2(26.0, 0.0);
        let label_g = style::galley(&painter, label, style::ui(style::BODY), INK, 0.0);
        let key = bindings.label(Action::Interact);
        // The plate goes down first: measure the key cap from its text.
        let key_w = (style::galley(&painter, key.clone(), style::num(SMALL), INK, 0.0).size().x + 10.0).max(22.0);
        let size = vec2(key_w + 8.0 + label_g.size().x, label_g.size().y.max(20.0)) + vec2(16.0, 10.0);
        let plate = Align2::LEFT_CENTER.anchor_size(at, size);
        style::plate(&painter, plate, SUIT, 1.0);
        let cap = style::key_cap(&painter, pos2(plate.min.x + 8.0, plate.center().y), &key, SUIT);
        painter.galley(pos2(cap.max.x + 8.0, plate.center().y - label_g.size().y / 2.0), label_g, INK);
    } else if let Some(hit) = Dir3::new(camera.rotation * Vec3::NEG_Z).ok().and_then(|dir| {
        spatial.cast_ray(
            camera.translation,
            dir,
            tuning.grapple_range,
            true,
            // Other players can be grappled; we can't.
            &SpatialQueryFilter::default().with_excluded_entities([own]),
        )
    }) {
        // In grapple range: the ring, and how far.
        style::broken_ring(&painter, c, 15.0, style::fade(SUIT, 0.85));
        let g = style::galley(&painter, format!("{:.0}", hit.distance), style::num(SMALL), SUIT, 0.0);
        let r = style::shadowed(&painter, c + vec2(19.0, 12.0), Align2::LEFT_TOP, g, SUIT);
        style::shadowed(
            &painter,
            pos2(r.max.x + 2.0, r.max.y - 1.0),
            Align2::LEFT_BOTTOM,
            style::galley(&painter, "m", style::ui(style::MICRO), INK_DIM, 0.0),
            INK_DIM,
        );
    }

    // Rope load under the crosshair: eight cells, cool to hot.
    if let Some(g) = player.grapple {
        let load = (g.tension / tuning.reel_force).clamp(0.0, 1.0);
        let color = if load > 0.85 {
            HOSTILE
        } else if load > 0.5 {
            WARNING
        } else {
            SUIT
        };
        let bar = egui::Rect::from_center_size(c + vec2(0.0, 34.0), vec2(64.0, 4.0));
        painter.rect_filled(bar.expand(2.0), 0.0, Color32::from_black_alpha(100));
        style::seg_bar(&painter, bar, 8, load, 0.0, color);
    }
    Ok(())
}

/// The label of the button (or case) a click would press just now, by the
/// sim's own rule and from the sim's own view: the ray from the eye
/// enters the button's box within reach with no world geometry in front.
/// (Buttons stand proud of their panel, so a hit just behind the button is
/// the panel itself.) The sim's poses, not the rendered ones: on a ship
/// that's flying, the two are a tick apart, which is a different button.
fn control_prompt(
    player: &Player,
    own: Entity,
    tuning: &Tuning,
    tick: u32,
    spatial: &SpatialQuery,
    bodies: &Bodies,
    controls: &Query<(&Control, &Transform, &ChildOf)>,
    pickups: &Query<&Pickup>,
    names: &Query<&Name>,
) -> Option<(String, bool)> {
    let frame = player.mode.body().and_then(|b| bodies.frame(b));
    let (eye, view) = player.mode.world_view(tuning, frame.as_ref());
    let dir = Dir3::new(view * Vec3::NEG_Z).ok()?;
    let blocked = spatial
        .cast_ray(
            eye,
            dir,
            CONTROL_REACH,
            true,
            &SpatialQueryFilter::from_mask(Layer::World).with_excluded_entities([own]),
        )
        .map_or(f32::MAX, |hit| hit.distance + 0.15);
    let buttons = controls.iter().filter_map(|(control, transform, ship)| {
        let frame = bodies.frame(ship.parent())?;
        let t = crate::sim::player::ray_box(
            frame.point_to_local(eye) - transform.translation,
            frame.dir_to_local(*dir),
            control.half_size,
        )?;
        (t <= CONTROL_REACH && t <= blocked).then(|| (t, (control.label.to_string(), true)))
    });
    // Pickups sit in the open, a little further than a wall behind them.
    let blocked = blocked + 0.1;
    let cases = pickups.iter().filter_map(|pickup| {
        let (at, turn) = pickup.sim_pose(bodies, tick)?;
        let t = crate::sim::player::ray_box(turn.inverse() * (eye - at), turn.inverse() * *dir, pickup.half())?;
        let owner = pickup.owner.and_then(|o| names.get(o).ok()).map(|n| n.as_str());
        (t <= CONTROL_REACH && t <= blocked).then(|| (t, view::pickup_label(pickup, own, owner)))
    });
    buttons
        .chain(cases)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, label)| label)
}

/// Hit markers: ticks round the crosshair when your shots land (white),
/// bigger and magenta on a weak spot, an X on a kill. From what the
/// authority says you hit, so a little after the shot.
pub fn hit_markers(
    mut contexts: EguiContexts,
    time: Res<Time>,
    log: Option<Single<&HitLog, With<LocalPlayer>>>,
    mut seen: Local<Option<u32>>,
    // The newest marker: how it looks, and its age (s).
    mut showing: Local<Option<(Hit, f32)>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Some(log) = log else {
        return Ok(());
    };
    if let Some(count) = *seen {
        // Of several new hits, show the most telling.
        let best = log.since(count).copied().max_by_key(|h| (h.kill, h.weak));
        if let Some(hit) = best {
            *showing = Some((hit, 0.0));
        }
    }
    *seen = Some(log.count);
    let Some((hit, age)) = showing.as_mut() else {
        return Ok(());
    };
    *age += time.delta_secs();
    let life = if hit.kill { 0.5 } else { 0.25 };
    if *age > life {
        *showing = None;
        return Ok(());
    }
    let fade = 1.0 - *age / life;
    let color = if hit.weak { WEAK } else { INK };
    let (inner, outer, width) = match (hit.kill, hit.weak) {
        (true, _) => (5.0, 17.0, BOLD + 0.5),
        (false, true) => (8.0, 16.0, BOLD),
        (false, false) => (8.0, 13.0, LINE + 0.3),
    };
    // A kill pops out a little as it fades.
    let grow = if hit.kill { 1.0 + 0.25 * (1.0 - fade) } else { 1.0 };
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("hits")));
    let c = ctx.content_rect().center();
    for (dx, dy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
        let d = vec2(dx, dy) / std::f32::consts::SQRT_2;
        let seg = [c + d * inner * grow, c + d * outer * grow];
        painter.line_segment(seg, Stroke::new(width + 2.0, Color32::from_black_alpha((120.0 * fade) as u8)));
        painter.line_segment(seg, Stroke::new(width, style::fade(color, fade)));
    }
    Ok(())
}
