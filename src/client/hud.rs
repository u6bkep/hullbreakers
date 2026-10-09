//! HUD readouts and the live tuning panel.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use avian3d::prelude::ColliderAabb;

use super::camera::PlayerCamera;
use super::input::MouseCaptured;
use crate::sim::bodies::Bodies;
use crate::sim::player::{Mode, Player};
use crate::sim::ship::Ship;
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
    player: Single<&Player>,
    bodies: Bodies,
    names: Query<&Name>,
    mut ships: Query<&mut Ship>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

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
                    name_of(Some(g.body))
                )));
            }
            ui.label(text(player.last_event.clone()));
            if captured.0 {
                ui.label(
                    egui::RichText::new("Tab: tuning panel")
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
                    for mut ship in &mut ships {
                        ship.lurch();
                    }
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

/// Player position and velocity relative to the reference body, in world axes.
fn relative_motion(player: &Player, bodies: &Bodies, tuning: &Tuning) -> Option<(Vec3, Vec3)> {
    let reference = bodies.frame(player.reference_body?)?;
    let own = player.mode.body().and_then(|b| bodies.frame(b));
    let (position, velocity) = player.mode.world_kinematics(tuning, own.as_ref());
    Some((position, velocity - reference.point_velocity(position)))
}

const REFERENCE_COLOR: egui::Color32 = egui::Color32::from_rgb(255, 190, 70);

/// Brackets the reference body (or points at it from the screen edge) and
/// marks the prograde/retrograde directions of motion relative to it.
pub fn reference_overlay(
    mut contexts: EguiContexts,
    tuning: Res<Tuning>,
    player: Single<&Player>,
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
