//! Client layer: input, camera, rendering, debug UI. Reads sim state, writes
//! only [`crate::sim::input::PlayerInput`] and [`crate::sim::ship::ShipCommand`]
//! (plus debug actions).

pub mod camera;
pub mod debug_shots;
pub mod events;
pub mod hud;
pub mod input;
pub mod render;
pub mod sfx;

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy_egui::{EguiPlugin, EguiPrimaryContextPass};

pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            .insert_resource(input::MouseCaptured(true))
            .init_resource::<camera::CameraFx>()
            .init_resource::<events::EventCursor>()
            .add_message::<events::Presented>()
            .init_resource::<hud::PanelState>()
            .add_observer(render::attach_meshes)
            .add_systems(PreStartup, render::setup_materials)
            .add_systems(
                Startup,
                (
                    camera::spawn_camera,
                    render::setup_lighting,
                    render::setup_starfield,
                    render::setup_dust,
                    sfx::setup_sounds,
                    render::setup_ship_lights.after(crate::sim::ship::spawn_ship),
                ),
            )
            .add_systems(
                PreUpdate,
                (input::toggle_capture, input::read_input)
                    .chain()
                    .after(bevy::input::InputSystems),
            )
            .add_systems(
                PostUpdate,
                (
                    camera::update_camera,
                    render::follow_starfield,
                    render::update_dust,
                )
                    .chain()
                    .before(TransformSystems::Propagate),
            )
            .add_systems(
                Update,
                (
                    render::draw_gizmos,
                    sfx::play_continuous_sounds,
                    events::present_events,
                    (sfx::play_event_sounds, camera::camera_fx_events)
                        .after(events::present_events),
                ),
            )
            .add_systems(EguiPrimaryContextPass, (hud::reference_overlay, hud::hud));
        if let Some(script) = debug_shots::shot_script_from_env() {
            app.insert_resource(script)
                .add_systems(Update, debug_shots::run_shot_script);
        }
    }
}
