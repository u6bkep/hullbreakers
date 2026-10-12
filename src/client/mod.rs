//! Client layer: input, camera, rendering, debug UI. Reads sim state, writes
//! only [`LocalInput`] and [`crate::sim::ship::ShipCommand`] (plus debug
//! actions). Which player is local is the network layer's business.

pub mod animate;
pub mod bindings;
pub mod camera;
pub mod combat_fx;
pub mod crew;
pub mod debug_shots;
pub mod devices;
pub mod events;
pub mod gallery;
pub mod hud;
pub mod input;
pub mod items_hud;
pub mod menu;
pub mod pickup_look;
pub mod profile;
pub mod render;
pub mod save;
pub mod sfx;
pub mod sky;
pub mod stress;
pub mod viewmodel;
pub mod weapons_fx;

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
        app.add_plugins((EguiPlugin::default(), combat_fx::CombatFxPlugin))
            .insert_resource(input::MouseCaptured(true))
            .init_resource::<input::TuningOpen>()
            .init_resource::<menu::Menu>()
            .init_resource::<menu::ItemOps>()
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
            .add_observer(render::add_ship_model)
            .add_observer(devices::build_devices)
            .add_observer(sky::add_sky_to_cameras)
            .add_observer(gallery::set_out_gallery)
            .add_observer(viewmodel::spawn_view_hands)
            .add_observer(camera::light_view_model::<PointLight>)
            .add_observer(camera::light_view_model::<SpotLight>)
            .add_observer(camera::light_view_model::<DirectionalLight>)
            .add_observer(camera::layer_world_mesh)
            .add_systems(
                PreStartup,
                (
                    render::setup_materials,
                    bindings::load_bindings,
                    menu::load_stances,
                    save::load_profile,
                    sky::setup_sky,
                ),
            )
            .add_systems(
                Startup,
                (
                    camera::spawn_camera,
                    render::setup_lighting,
                    render::setup_dust,
                    pickup_look::setup_pickup_look,
                    devices::setup_device_look,
                    weapons_fx::setup_weapon_fx,
                    sfx::setup_sounds,
                ),
            )
            .add_systems(
                PreUpdate,
                (input::toggle_capture, menu::capture_binding, input::read_input)
                    .chain()
                    .after(bevy::input::InputSystems),
            )
            .add_systems(
                PostUpdate,
                (
                    render::update_avatars,
                    pickup_look::draw_pickups,
                    crew::dress_crew,
                    animate::rig_avatars,
                    animate::animate_avatars,
                    camera::update_camera,
                    viewmodel::pose_view_hands,
                    pickup_look::paint_weapons,
                    viewmodel::dress_view_hands,
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
                    (
                        sfx::play_event_sounds,
                        camera::camera_fx_events,
                        camera::beam_shake,
                    )
                        .after(events::present_events),
                    sfx::play_combat_sounds.after(sfx::play_event_sounds),
                    sfx::play_hit_sounds,
                    (devices::show_devices, devices::dim_dark_cabins),
                    (gallery::hide_colliders, gallery::arrange_exhibits, gallery::turn_exhibits, gallery::draw_sockets, gallery::pulse_cores, sky::settle_environment_maps),
                    (
                        weapons_fx::spawn_tracers,
                        weapons_fx::fly_tracers,
                        weapons_fx::fade_sparks,
                    )
                        .chain(),
                )
                    .in_set(Present),
            )
            .add_systems(
                EguiPrimaryContextPass,
                (
                    hud::ship_markers,
                    hud::reference_overlay,
                    hud::hud,
                    hud::mission_hud,
                    hud::vitals_hud,
                    hud::loadout_hud,
                    items_hud::pickup_card,
                    gallery::label_exhibits,
                    hud::hit_markers,
                    menu::menu,
                )
                    .chain(),
            );
        if let Some(script) = debug_shots::shot_script_from_env() {
            app.insert_resource(script)
                .add_systems(Update, debug_shots::run_shot_script);
        }
        // Measurement and stress set-ups for `tools/stress`; off by default.
        if let Some(config) = profile::profile_from_env() {
            app.add_plugins(profile::ProfilePlugin(config));
        }
        if let Some(stress) = stress::stress_from_env() {
            app.add_plugins(stress::StressPlugin(stress));
        }
    }
}
