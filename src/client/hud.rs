//! HUD readouts and the live tuning panel.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

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
    let (mode, relative_speed) = match player.mode {
        Mode::Booted { velocity, .. } => ("MAG BOOTS", velocity.length()),
        Mode::Eva {
            position, velocity, ..
        } => {
            let rel = player
                .reference_body
                .and_then(|b| bodies.frame(b))
                .map(|f| (velocity - f.point_velocity(position)).length())
                .unwrap_or(velocity.length());
            ("EVA", rel)
        }
    };
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
                "{mode}   boots {}",
                if player.boots_on { "ON" } else { "off" }
            )));
            ui.label(text(format!(
                "rel {:>6.2} m/s  vs {}",
                relative_speed,
                name_of(player.reference_body)
            )));
            ui.label(text(format!(
                "fuel {:>5.1} / {:.0} m/s Δv",
                player.fuel, tuning.fuel_capacity
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
