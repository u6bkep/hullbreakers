//! HUD readouts and the live tuning panel.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use avian3d::prelude::{ColliderAabb, SpatialQuery, SpatialQueryFilter};

use super::camera::{CameraFx, PlayerCamera};
use super::LocalPlayer;
use super::events::EventCursor;
use super::bindings::{Action, Bindings};
use super::input::MouseCaptured;
use crate::sim::vitals::Vitals;
use crate::sim::weapons::{Hand, Slot};
use crate::sim::combat::{SHIELD_CENTER, SHIELD_RADII};
use crate::sim::bodies::Bodies;
use crate::sim::combat::Combatant;
use crate::sim::mission::{Mission, Phase, ScenarioKind};
use crate::sim::player::{CONTROL_REACH, Mode, Player};
use crate::sim::ship::{Control, HomeShip, HullKind, ShipCommand, ShipHull};
use crate::sim::{Layer, SimTick, TICK_HZ};
use crate::sim::tuning::{TUNING_PATH, Tuning};

#[derive(Resource, Default)]
pub struct PanelState {
    status: String,
}

pub fn hud(
    mut contexts: EguiContexts,
    mut tuning: ResMut<Tuning>,
    mut panel: ResMut<PanelState>,
    captured: Res<MouseCaptured>,
    bindings: Res<Bindings>,
    player: Single<(Entity, &Player), With<LocalPlayer>>,
    bodies: Bodies,
    names: Query<&Name>,
    events: Res<EventCursor>,
    mut ship_orders: MessageWriter<ShipCommand>,
    camera: Single<&Transform, With<PlayerCamera>>,
    spatial: SpatialQuery,
    controls: Query<(&Control, &Transform, &ChildOf)>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let (own, player) = *player;

    // Crosshair.
    let center = ctx.content_rect().center();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("crosshair"),
    ));
    let stroke = egui::Stroke::new(1.5, egui::Color32::from_white_alpha(180));
    painter.line_segment(
        [center - egui::vec2(6.0, 0.0), center + egui::vec2(6.0, 0.0)],
        stroke,
    );
    painter.line_segment(
        [center - egui::vec2(0.0, 6.0), center + egui::vec2(0.0, 6.0)],
        stroke,
    );

    // Grapple target: a ring around the crosshair when something is in range.
    let in_range = Dir3::new(camera.rotation * Vec3::NEG_Z)
        .ok()
        .and_then(|dir| {
            spatial.cast_ray(
                camera.translation,
                dir,
                tuning.grapple_range,
                true,
                // Other players can be grappled; we can't.
                &SpatialQueryFilter::default().with_excluded_entities([own]),
            )
        });
    // A button under the crosshair wins over the grapple readout.
    let prompt = control_prompt(&camera, own, &spatial, &bodies, &controls);
    if let Some(label) = prompt {
        let cyan = INK;
        painter.circle_stroke(center, 11.0, egui::Stroke::new(1.5, cyan));
        let galley = painter.layout_no_wrap(
            format!("[{}] {label}", bindings.label(Action::Interact)),
            egui::FontId::monospace(15.0),
            cyan,
        );
        let at = center + egui::vec2(18.0, -galley.size().y / 2.0);
        painter.rect_filled(
            egui::Rect::from_min_size(
                at - egui::vec2(4.0, 2.0),
                galley.size() + egui::vec2(8.0, 4.0),
            ),
            3.0,
            egui::Color32::from_black_alpha(150),
        );
        painter.galley(at, galley, cyan);
    } else if let Some(hit) = in_range {
        let amber = egui::Color32::from_rgb(255, 190, 70);
        painter.circle_stroke(center, 11.0, egui::Stroke::new(1.5, amber));
        painter.text(
            center + egui::vec2(16.0, -16.0),
            egui::Align2::LEFT_BOTTOM,
            format!("{:.0} m", hit.distance),
            egui::FontId::monospace(12.0),
            amber,
        );
    }

    // Rope load gauge under the crosshair.
    if let Some(g) = player.grapple {
        let load = (g.tension / tuning.reel_force).clamp(0.0, 1.0);
        let bar =
            egui::Rect::from_min_size(center + egui::vec2(-30.0, 20.0), egui::vec2(60.0, 4.0));
        painter.rect_stroke(
            bar,
            1.0,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(90)),
            egui::StrokeKind::Outside,
        );
        let fill = egui::Rect::from_min_size(bar.min, egui::vec2(60.0 * load, 4.0));
        let color = egui::Color32::from_rgb(
            255,
            (165.0 - 125.0 * load) as u8,
            (40.0 - 25.0 * load) as u8,
        );
        painter.rect_filled(fill, 1.0, color);
    }

    // Readouts.
    let name_of = |e: Option<Entity>| {
        e.and_then(|e| names.get(e).ok())
            .map(|n| n.as_str().to_owned())
            .unwrap_or_else(|| "—".into())
    };
    let mode = match player.mode {
        Mode::Booted { .. } => "MAG BOOTS",
        Mode::Eva { .. } => "EVA",
    };
    let relative_speed = relative_motion(&player, &bodies, &tuning)
        .map(|(_, v)| v.length())
        .unwrap_or(0.0);
    egui::Area::new(egui::Id::new("readouts"))
        .anchor(egui::Align2::LEFT_BOTTOM, [16.0, -16.0])
        .show(ctx, |ui| {
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            let text = |s: String| {
                egui::RichText::new(s)
                    .monospace()
                    .size(15.0)
                    .color(egui::Color32::from_rgb(170, 230, 255))
            };
            ui.label(text(format!(
                "{mode}   boots {}   assist {}",
                if player.boots_on { "ON" } else { "off" },
                if player.flight_assist { "ON" } else { "off" }
            )));
            ui.label(text(format!(
                "rel {:>6.2} m/s  vs {}",
                relative_speed,
                name_of(player.reference_body)
            )));
            if let Some(g) = player.grapple {
                ui.label(text(format!(
                    "rope {:>5.1} m  tension {:>6.0} N  → {}",
                    g.length,
                    g.tension,
                    name_of(Some(g.target))
                )));
            }
            ui.label(text(events.last_text.clone()));
            if captured.0 {
                ui.label(
                    egui::RichText::new(format!("{}: tuning panel", bindings.label(Action::TuningPanel)))
                        .size(12.0)
                        .color(egui::Color32::GRAY),
                );
            }
        });

    if captured.0 {
        return Ok(());
    }
    egui::Window::new("Tuning")
        .default_width(320.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    panel.status = match tuning.save(std::path::Path::new(TUNING_PATH)) {
                        Ok(()) => format!("saved {TUNING_PATH}"),
                        Err(e) => format!("save failed: {e}"),
                    };
                }
                if ui.button("Defaults").clicked() {
                    *tuning = Tuning::default();
                    panel.status = "reset to defaults".into();
                }
                if ui.button("Ship lurch (L)").clicked() {
                    ship_orders.write(ShipCommand::Lurch);
                }
            });
            ui.label(&panel.status);
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut changed = false;
                let values = tuning.bypass_change_detection();
                let mut section = "";
                for field in values.float_fields() {
                    if field.section != section {
                        section = field.section;
                        ui.separator();
                        ui.strong(section);
                    }
                    changed |= ui
                        .add(egui::Slider::new(field.value, field.min..=field.max).text(field.name))
                        .changed();
                }
                ui.separator();
                for field in values.bool_fields() {
                    changed |= ui
                        .checkbox(field.value, format!("{} / {}", field.section, field.name))
                        .changed();
                }
                if changed {
                    tuning.set_changed();
                }
            });
        });
    Ok(())
}

/// The label of the button the crosshair is on, by the sim's own rule for
/// whether a click would press it: the ray enters the button's box within
/// reach with no world geometry in front. Buttons stand proud of their
/// panel, so a hit just behind the button is the panel itself.
fn control_prompt(
    camera: &Transform,
    own: Entity,
    spatial: &SpatialQuery,
    bodies: &Bodies,
    controls: &Query<(&Control, &Transform, &ChildOf)>,
) -> Option<&'static str> {
    let dir = Dir3::new(camera.rotation * Vec3::NEG_Z).ok()?;
    let eye = camera.translation;
    let blocked = spatial
        .cast_ray(
            eye,
            dir,
            CONTROL_REACH,
            true,
            &SpatialQueryFilter::from_mask(Layer::World).with_excluded_entities([own]),
        )
        .map_or(f32::MAX, |hit| hit.distance + 0.15);
    controls
        .iter()
        .filter_map(|(control, transform, ship)| {
            let frame = bodies.frame(ship.parent())?;
            let t = crate::sim::player::ray_box(
                frame.point_to_local(eye) - transform.translation,
                frame.dir_to_local(*dir),
                control.half_size,
            )?;
            (t <= CONTROL_REACH && t <= blocked).then_some((t, control.action.label()))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, label)| label)
}

/// Player position and velocity relative to the reference body, in world axes.
fn relative_motion(player: &Player, bodies: &Bodies, tuning: &Tuning) -> Option<(Vec3, Vec3)> {
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

/// Every ship, always marked: a diamond with its name and range, or an arrow
/// at the screen edge when it's off screen, so nobody loses the fight (or
/// the way home) among the rocks. The ship you're standing on isn't marked.
pub fn ship_markers(
    mut contexts: EguiContexts,
    player: Single<&Player, With<LocalPlayer>>,
    ships: Query<(Entity, &ShipHull, &Transform, &Visibility)>,
    camera: Single<(&Camera, &Transform), With<PlayerCamera>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let (camera, cam) = *camera;
    let global = GlobalTransform::from(*cam);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("ship markers"),
    ));
    let screen = ctx.content_rect();
    let font = egui::FontId::monospace(13.0);
    for (entity, hull, transform, visibility) in &ships {
        // Not the ship you're on or in.
        let inside = ((transform.rotation.inverse() * (cam.translation - transform.translation) - SHIELD_CENTER)
            / SHIELD_RADII)
            .length()
            < 1.0;
        if player.mode.body() == Some(entity) || inside || *visibility == Visibility::Hidden {
            continue;
        }
        let (name, color) = match hull.0 {
            HullKind::Home => ("SHIP", egui::Color32::from_rgb(110, 230, 160)),
            HullKind::Raider => ("RAIDER", egui::Color32::from_rgb(255, 95, 80)),
        };
        // The middle of the hull.
        let center = transform.translation + transform.rotation * Vec3::new(0.0, 1.5, 0.0);
        let offset = center - cam.translation;
        let label = format!("{name}  {:.0} m", offset.length());
        let local = cam.rotation.inverse() * offset;
        let on_screen = (local.z < -0.1)
            .then(|| camera.world_to_viewport(&global, center).ok())
            .flatten()
            .map(|p| egui::pos2(p.x, p.y))
            .filter(|p| screen.shrink(24.0).contains(*p));
        if let Some(p) = on_screen {
            let r = 7.0;
            painter.add(egui::Shape::closed_line(
                vec![
                    p + egui::vec2(0.0, -r),
                    p + egui::vec2(r, 0.0),
                    p + egui::vec2(0.0, r),
                    p + egui::vec2(-r, 0.0),
                ],
                egui::Stroke::new(2.0, color),
            ));
            painter.text(p + egui::vec2(0.0, -r - 3.0), egui::Align2::CENTER_BOTTOM, label, font.clone(), color);
        } else {
            // At the edge of the screen, toward it.
            let dir = egui::vec2(local.x, -local.y).normalized();
            let half = screen.size() / 2.0 - egui::vec2(28.0, 28.0);
            let scale = (half.x / dir.x.abs().max(1e-3)).min(half.y / dir.y.abs().max(1e-3));
            let tip = screen.center() + dir * scale;
            let side = egui::vec2(-dir.y, dir.x);
            painter.add(egui::Shape::convex_polygon(
                vec![tip, tip - dir * 12.0 + side * 7.0, tip - dir * 12.0 - side * 7.0],
                color,
                egui::Stroke::NONE,
            ));
            let along = |v: f32| match v {
                v if v > 0.3 => egui::Align::Max,
                v if v < -0.3 => egui::Align::Min,
                _ => egui::Align::Center,
            };
            painter.text(tip - dir * 18.0, egui::Align2([along(dir.x), along(dir.y)]), label, font.clone(), color);
        }
    }
    Ok(())
}

const REFERENCE_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 190, 70);

/// Brackets the reference body (or points at it from the screen edge) and
/// marks the prograde/retrograde directions of motion relative to it.
pub fn reference_overlay(
    mut contexts: EguiContexts,
    tuning: Res<Tuning>,
    player: Single<&Player, With<LocalPlayer>>,
    bodies: Bodies,
    names: Query<&Name>,
    aabbs: Query<&ColliderAabb>,
    camera: Single<(&Camera, &Transform), With<PlayerCamera>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Some(reference) = player.reference_body else {
        return Ok(());
    };
    let Some((position, relative_velocity)) = relative_motion(&player, &bodies, &tuning) else {
        return Ok(());
    };
    let (camera, cam) = *camera;
    let global = GlobalTransform::from(*cam);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("reference"),
    ));
    let stroke = egui::Stroke::new(1.5, REFERENCE_COLOR);
    let to_screen = |world: Vec3| -> Option<egui::Pos2> {
        let local = cam.rotation.inverse() * (world - cam.translation);
        if local.z > -0.1 {
            return None;
        }
        camera
            .world_to_viewport(&global, world)
            .ok()
            .map(|p| egui::pos2(p.x, p.y))
    };

    // Prograde (circle with three ticks) and retrograde (circle with a cross).
    if relative_velocity.length() > 0.2 {
        let dir = relative_velocity.normalize();
        if let Some(p) = to_screen(cam.translation + dir * 100.0) {
            painter.circle_stroke(p, 7.0, stroke);
            for tick in [
                egui::vec2(0.0, -1.0),
                egui::vec2(-1.0, 0.0),
                egui::vec2(1.0, 0.0),
            ] {
                painter.line_segment([p + tick * 7.0, p + tick * 12.0], stroke);
            }
        }
        if let Some(p) = to_screen(cam.translation - dir * 100.0) {
            painter.circle_stroke(p, 7.0, stroke);
            let d = 7.0 * std::f32::consts::FRAC_1_SQRT_2;
            painter.line_segment([p + egui::vec2(-d, -d), p + egui::vec2(d, d)], stroke);
            painter.line_segment([p + egui::vec2(-d, d), p + egui::vec2(d, -d)], stroke);
        }
    }

    // Standing on the reference body: no need to point it out.
    if player.mode.body() == Some(reference) {
        return Ok(());
    }
    let Ok(aabb) = aabbs.get(reference) else {
        return Ok(());
    };
    let (min, max) = (aabb.min, aabb.max);
    let center = (min + max) / 2.0;
    let offset = center - position;
    let distance = offset.length();
    // Positive when approaching.
    let closing = relative_velocity.dot(offset / distance.max(1e-3));
    let name = names
        .get(reference)
        .map(|n| n.as_str().to_owned())
        .unwrap_or_default();
    let label = format!("{name}  {distance:.0} m  {closing:+.1} m/s");
    let font = egui::FontId::monospace(13.0);

    let corners: Vec<Option<egui::Pos2>> = (0..8)
        .map(|i| {
            let pick = |bit: usize, lo: f32, hi: f32| if i & bit == 0 { lo } else { hi };
            to_screen(Vec3::new(
                pick(1, min.x, max.x),
                pick(2, min.y, max.y),
                pick(4, min.z, max.z),
            ))
        })
        .collect();
    if corners.iter().all(Option::is_some) {
        let mut rect = egui::Rect::NOTHING;
        for p in corners.into_iter().flatten() {
            rect.extend_with(p);
        }
        let rect = egui::Rect::from_center_size(
            rect.center(),
            rect.size().max(egui::vec2(24.0, 24.0)) + egui::vec2(8.0, 8.0),
        );
        let arm = (rect.width().min(rect.height()) * 0.25).clamp(5.0, 16.0);
        for (corner, sx, sy) in [
            (rect.left_top(), 1.0, 1.0),
            (rect.right_top(), -1.0, 1.0),
            (rect.left_bottom(), 1.0, -1.0),
            (rect.right_bottom(), -1.0, -1.0),
        ] {
            painter.line_segment([corner, corner + egui::vec2(arm * sx, 0.0)], stroke);
            painter.line_segment([corner, corner + egui::vec2(0.0, arm * sy)], stroke);
        }
        painter.text(
            rect.center_bottom() + egui::vec2(0.0, 4.0),
            egui::Align2::CENTER_TOP,
            label,
            font,
            REFERENCE_COLOR,
        );
    } else {
        // Off screen (or partly behind us): an arrow at the screen edge.
        let local = cam.rotation.inverse() * offset;
        let dir = egui::vec2(local.x, -local.y).normalized();
        let screen = ctx.content_rect();
        let radius = screen.width().min(screen.height()) / 2.0 - 48.0;
        let tip = screen.center() + dir * radius;
        let side = egui::vec2(-dir.y, dir.x);
        painter.add(egui::Shape::convex_polygon(
            vec![
                tip,
                tip - dir * 14.0 + side * 8.0,
                tip - dir * 14.0 - side * 8.0,
            ],
            REFERENCE_COLOR,
            egui::Stroke::NONE,
        ));
        // Label sits inward of the arrow, anchored on the side facing it.
        let along = |v: f32| match v {
            v if v > 0.3 => egui::Align::Max,
            v if v < -0.3 => egui::Align::Min,
            _ => egui::Align::Center,
        };
        painter.text(
            tip - dir * 22.0,
            egui::Align2([along(dir.x), along(dir.y)]),
            label,
            font,
            REFERENCE_COLOR,
        );
    }
    Ok(())
}

const HULL_COLOR: egui::Color32 = egui::Color32::from_rgb(120, 220, 140);
const FOE_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 110, 90);
const SHIELD_COLOR: egui::Color32 = egui::Color32::from_rgb(110, 170, 255);
const INK: egui::Color32 = egui::Color32::from_rgb(170, 230, 255);

/// Seconds between two ticks of the (replicated) sim clock.
fn seconds_since(now: u32, then: u32) -> f32 {
    now.wrapping_sub(then) as f32 / TICK_HZ as f32
}

fn clock(seconds: f32) -> String {
    let s = seconds as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// A labelled bar with `fraction` of its width filled and a note beside it.
fn bar(ui: &mut egui::Ui, label: &str, fraction: f32, color: egui::Color32, note: (&str, egui::Color32)) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("{label:<6}")).monospace().size(13.0).color(INK));
        let (rect, _) = ui.allocate_exact_size(egui::vec2(220.0, 12.0), egui::Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, 2.0, egui::Color32::from_black_alpha(140));
        let mut fill = rect;
        fill.set_width(rect.width() * fraction.clamp(0.0, 1.0));
        painter.rect_filled(fill, 2.0, color);
        painter.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(1.0, egui::Color32::from_white_alpha(90)),
            egui::StrokeKind::Outside,
        );
        ui.label(egui::RichText::new(note.0).monospace().size(13.0).color(note.1));
    });
}

/// Scenario status from the replicated [`Mission`] and the two ships'
/// [`Combatant`]s: hull bars and objective during a fight, banners when it
/// ends, the debrief back in the field, and the fade after a hard cut.
/// Nothing here reads authority-only state, so hosts and clients agree.
pub fn mission_hud(
    mut contexts: EguiContexts,
    tick: Res<SimTick>,
    fx: Res<CameraFx>,
    missions: Query<&Mission>,
    ships: Query<(&Combatant, Has<HomeShip>)>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect();

    if fx.fade() > 0.0 {
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("fade"),
        ));
        painter.rect_filled(
            screen,
            0.0,
            egui::Color32::from_black_alpha((fx.fade() * 255.0) as u8),
        );
    }

    let Ok(mission) = missions.single() else {
        return Ok(());
    };
    let elapsed = seconds_since(tick.0, mission.started);

    if mission.kind == ScenarioKind::Boarding {
        let ours = ships.iter().find(|(_, home)| *home).map(|(c, _)| c);
        let raider = ships.iter().find(|(_, home)| !*home).map(|(c, _)| c);
        // A wrecked ship's entity is gone: no ship, no hull.
        let ours_integrity = ours.map_or(0.0, |c| c.integrity);
        let raider_integrity = raider.map_or(0.0, |c| c.integrity);
        let shield_up = raider.is_some_and(|c| c.shield);
        let objective = match mission.phase {
            Phase::Won => "Raider destroyed — return to the mission wall",
            Phase::Lost => "Hull breached — abandon ship",
            Phase::Underway if shield_up => "Cross to the raider and disable its shield",
            Phase::Underway => "Shield down — get clear!",
        };
        egui::Area::new(egui::Id::new("mission"))
            .anchor(egui::Align2::CENTER_TOP, [0.0, 16.0])
            .show(ctx, |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.vertical_centered(|ui| {
                    let ours_color = if ours_integrity < 0.3 { FOE_COLOR } else { HULL_COLOR };
                    let pct = format!("{:>3.0}%", ours_integrity * 100.0);
                    bar(ui, "HULL", ours_integrity, ours_color, (&pct, ours_color));
                    let note = match (raider.is_some(), shield_up) {
                        (false, _) => ("DESTROYED", FOE_COLOR),
                        (true, true) => ("SHIELDED", SHIELD_COLOR),
                        (true, false) => ("SHIELD DOWN", egui::Color32::from_rgb(255, 190, 70)),
                    };
                    bar(ui, "RAIDER", raider_integrity, FOE_COLOR, note);
                    ui.label(
                        egui::RichText::new(format!("{objective}   {}", clock(elapsed)))
                            .monospace()
                            .size(15.0)
                            .color(INK),
                    );
                });
            });
    }

    // Banner for a few seconds after the fight is decided.
    if let Some(ended) = mission.ended
        && mission.kind == ScenarioKind::Boarding
    {
        let since = seconds_since(tick.0, ended);
        if (0.0..5.0).contains(&since) {
            let (text, color) = match mission.phase {
                Phase::Won => ("RAIDER DESTROYED", HULL_COLOR),
                Phase::Lost => ("SHIP LOST", FOE_COLOR),
                Phase::Underway => ("", INK),
            };
            // Fades in fast, out slowly.
            let alpha = (since * 6.0).min(1.0) * (5.0 - since).min(1.0);
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("banner"),
            ));
            painter.text(
                screen.center() - egui::vec2(0.0, screen.height() * 0.2),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::monospace(64.0),
                color.gamma_multiply(alpha),
            );
        }
    }

    // Debrief for the mission before this one, for a while after the field starts.
    if let Some(debrief) = &mission.debrief
        && mission.kind == ScenarioKind::Field
        && elapsed < 15.0
    {
        let won = debrief.phase == Phase::Won;
        let (title, color) = if won {
            ("RAIDER DESTROYED", HULL_COLOR)
        } else {
            ("SHIP LOST", FOE_COLOR)
        };
        egui::Area::new(egui::Id::new("debrief"))
            .anchor(egui::Align2::CENTER_TOP, [0.0, 120.0])
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(egui::Color32::from_black_alpha(190))
                    .stroke(egui::Stroke::new(1.0, color))
                    .corner_radius(4.0)
                    .inner_margin(16.0)
                    .show(ui, |ui| {
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                        let line = |ui: &mut egui::Ui, s: String| {
                            ui.label(egui::RichText::new(s).monospace().size(15.0).color(INK));
                        };
                        ui.label(egui::RichText::new("DEBRIEF").monospace().size(12.0).color(egui::Color32::GRAY));
                        ui.label(egui::RichText::new(title).monospace().size(28.0).color(color));
                        line(ui, format!("{}  —  {}", debrief.scenario.button_label(), clock(debrief.seconds)));
                        ui.separator();
                        line(
                            ui,
                            match (&debrief.shield_breaker, won) {
                                (Some(who), _) => format!("Shield dropped by {who}. Remember that name."),
                                (None, true) => "Nobody dropped the shield; it fell on its own. Suspicious.".into(),
                                (None, false) => "Nobody ever dropped the shield. Bold strategy.".into(),
                            },
                        );
                        if !debrief.aboard.is_empty() {
                            line(ui, format!("Made it home: {}", debrief.aboard.join(", ")));
                        }
                        if !debrief.adrift.is_empty() {
                            line(ui, format!("Left drifting: {}. Send a postcard.", debrief.adrift.join(", ")));
                        }
                    });
            });
    }
    Ok(())
}

/// Your health, at the bottom of the screen, and a red flash when you're
/// hurt.
pub fn vitals_hud(
    mut contexts: EguiContexts,
    time: Res<Time>,
    tuning: Res<Tuning>,
    vitals: Option<Single<&Vitals, With<LocalPlayer>>>,
    mut last: Local<Option<f32>>,
    mut flash: Local<f32>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Some(vitals) = vitals else {
        return Ok(());
    };
    let health = vitals.health;
    if last.is_some_and(|before| health < before) {
        *flash = 1.0;
    }
    *last = Some(health);
    *flash = (*flash - time.delta_secs() * 2.5).max(0.0);
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("vitals")));
    if *flash > 0.0 {
        // A red rim, strongest at the edges.
        let alpha = (*flash * 110.0) as u8;
        let red = egui::Color32::from_rgba_unmultiplied(200, 20, 10, alpha);
        let edge = 60.0;
        for rect in [
            egui::Rect::from_min_max(screen.min, egui::pos2(screen.max.x, screen.min.y + edge)),
            egui::Rect::from_min_max(egui::pos2(screen.min.x, screen.max.y - edge), screen.max),
            egui::Rect::from_min_max(screen.min, egui::pos2(screen.min.x + edge, screen.max.y)),
            egui::Rect::from_min_max(egui::pos2(screen.max.x - edge, screen.min.y), screen.max),
        ] {
            painter.rect_filled(rect, 0.0, red);
        }
    }
    let fraction = (health / tuning.player_health).clamp(0.0, 1.0);
    let size = egui::vec2(240.0, 10.0);
    let at = egui::pos2(screen.center().x - size.x / 2.0, screen.max.y - 34.0);
    let back = egui::Rect::from_min_size(at, size);
    painter.rect_filled(back.expand(2.0), 3.0, egui::Color32::from_black_alpha(150));
    let color = if fraction > 0.35 {
        egui::Color32::from_rgb(120, 220, 160)
    } else {
        egui::Color32::from_rgb(240, 90, 70)
    };
    painter.rect_filled(
        egui::Rect::from_min_size(at, egui::vec2(size.x * fraction, size.y)),
        2.0,
        color,
    );
    painter.text(
        at + egui::vec2(size.x / 2.0, -4.0),
        egui::Align2::CENTER_BOTTOM,
        format!("SUIT {:.0}", health),
        egui::FontId::monospace(12.0),
        color,
    );
    Ok(())
}

/// What's in each hand, ammo and reloads, and the stances, bottom right.
pub fn loadout_hud(
    mut contexts: EguiContexts,
    bindings: Res<Bindings>,
    player: Option<Single<&Player, With<LocalPlayer>>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Some(player) = player else {
        return Ok(());
    };
    let loadout = &player.loadout;
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("loadout")));
    let font = egui::FontId::monospace(13.0);
    let ink = egui::Color32::from_rgb(200, 225, 235);
    let dim = egui::Color32::from_rgb(110, 125, 135);
    let name = |slot: Slot| match slot {
        Slot::Empty => "—".to_string(),
        Slot::Grapple => "Grapple".to_string(),
        Slot::Gun(i) => loadout
            .guns
            .get(i as usize)
            .map_or("?".into(), |g| g.kind.stats().name.to_string()),
    };
    let mut y = screen.max.y - 24.0;
    let x = screen.max.x - 16.0;
    // Hands, off hand above main.
    for (hand, action) in [(Hand::Main, Action::MainUse), (Hand::Off, Action::OffUse)] {
        let slot = loadout.stance().slot(hand);
        let state = loadout.hands[hand as usize];
        let mut line = format!("[{}] {}", bindings.label(action), name(slot));
        if let Some(gun) = loadout.gun(hand) {
            line += &format!("  {:>2} | {}", gun.mag, gun.reserve);
            if state.reloading.is_some() {
                line += "  reloading";
            } else if state.reload_queued {
                line += "  queued";
            }
            if loadout.braced(hand) {
                line += "  braced";
            }
        }
        painter.text(egui::pos2(x, y), egui::Align2::RIGHT_BOTTOM, line, font.clone(), ink);
        y -= 18.0;
    }
    // Stances.
    y -= 6.0;
    for (i, stance) in loadout.stances.iter().enumerate().rev() {
        let current = i == loadout.current as usize;
        let text = format!("{} {} + {}", i + 1, name(stance.main), name(stance.off));
        let color = if current { ink } else { dim };
        let text = if current && loadout.switching > 0.0 {
            format!("{text}  …")
        } else {
            text
        };
        painter.text(egui::pos2(x, y), egui::Align2::RIGHT_BOTTOM, text, egui::FontId::monospace(11.0), color);
        y -= 14.0;
    }
    Ok(())
}
