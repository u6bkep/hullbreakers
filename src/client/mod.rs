//! Client layer: input, camera, rendering, debug UI. Reads sim state, writes
//! only [`LocalInput`] and [`crate::sim::ship::ShipCommand`] (plus debug
//! actions). Which player is local is the network layer's business.

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

pub use crate::net::{LocalInput, LocalPlayer};

/// A jump in the local player's view that the camera should blend over
/// rather than show: the network replacing a mispredicted state. Whoever
/// causes one adds to it; the camera takes it.
#[derive(Resource, Default)]
pub struct ViewCorrection {
    /// World-space eye displacement (old minus new).
    pub position: Vec3,
    /// Rotation taking the new view to the old.
    pub rotation: Quat,
}

/// `PostUpdate` systems that write body transforms for this frame (when
/// Avian's interpolation doesn't). The camera follows them.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct BodyPoses;

/// `Update` systems that present the local player's state. Whatever delivers
/// that state (a network's interpolation) must run before this set.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Present;

pub struct ClientPlugin;

impl Plugin for ClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            .insert_resource(input::MouseCaptured(true))
            .init_resource::<camera::CameraFx>()
            .insert_resource(ViewCorrection {
                rotation: Quat::IDENTITY,
                ..default()
            })
            .init_resource::<events::EventCursor>()
            .add_message::<events::Presented>()
            .init_resource::<hud::PanelState>()
            .add_observer(render::attach_meshes)
            .add_observer(render::add_ship_lights)
            .add_systems(PreStartup, render::setup_materials)
            .add_systems(
                Startup,
                (
                    camera::spawn_camera,
                    render::setup_lighting,
                    render::setup_starfield,
                    render::setup_dust,
                    sfx::setup_sounds,
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
                    render::update_avatars,
                    camera::update_camera,
                    render::follow_starfield,
                    render::update_dust,
                )
                    .chain()
                    // After body poses are written to their transforms.
                    .after(BodyPoses)
                    .before(TransformSystems::Propagate),
            )
            .add_systems(
                Update,
                (
                    render::draw_gizmos,
                    sfx::play_continuous_sounds,
                    events::present_events,
                    sfx::play_remote_sounds,
                    (sfx::play_event_sounds, camera::camera_fx_events)
                        .after(events::present_events),
                    sfx::play_combat_sounds.after(sfx::play_event_sounds),
                )
                    .in_set(Present),
            )
            .add_systems(EguiPrimaryContextPass, (hud::reference_overlay, hud::hud));
        if let Some(script) = debug_shots::shot_script_from_env() {
            app.insert_resource(script)
                .add_systems(Update, debug_shots::run_shot_script);
        }
    }
}
