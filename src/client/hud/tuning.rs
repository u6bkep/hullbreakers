//! The live tuning panel (F1): a debug panel, and left looking like one.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use crate::client::input::TuningOpen;
use crate::sim::bots::{Robot, SpawnRobots};
use crate::sim::ship::ShipCommand;
use crate::sim::tuning::{TUNING_PATH, Tuning};

#[derive(Resource, Default)]
pub struct PanelState {
    status: String,
}

pub fn tuning_panel(
    mut contexts: EguiContexts,
    mut tuning: ResMut<Tuning>,
    mut panel: ResMut<PanelState>,
    tuning_open: Res<TuningOpen>,
    mut ship_orders: MessageWriter<ShipCommand>,
    mut robot_orders: MessageWriter<SpawnRobots>,
) -> Result {
    if !tuning_open.0 {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    egui::Window::new("Tuning").default_width(320.0).show(ctx, |ui| {
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
        // Host only: the authority reads these. (Turrets need a hull
        // mount, so they aren't offered.)
        ui.horizontal(|ui| {
            ui.label("Spawn:");
            for (label, kind) in [
                ("Gunner on bridge", Robot::Gunner),
                ("Grappler on bridge", Robot::Grappler),
                ("3 Swarmers to port", Robot::Swarmer),
            ] {
                if ui.button(label).clicked() {
                    robot_orders.write(SpawnRobots(kind));
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
