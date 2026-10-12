//! The HUD: what the crew's suit projects on the visor, drawn with egui's
//! painter over the finished picture. `docs/hud-style.md` is the design;
//! [`style`] holds its tokens and painters. One file per element:
//!
//! - [`reticle`]: the crosshair, grapple ring, rope gauge, interact prompt
//!   and hit markers (the centre).
//! - [`markers`]: things in the world: ships, the reference body,
//!   objectives (see [`markers::Objectives`]), your dropped kit.
//! - [`flight`]: the suit's nav readouts and the event feed.
//! - [`mission`]: the scenario strip, banners, the debrief.
//! - [`vitals`]: health, regen, low-health and damage-direction cues, and
//!   dying.
//! - [`hands`]: a card per hand and the stances, from [`view`].
//! - [`progress`]: salvage and experience earned, and a new rank.
//! - [`tuning`]: the F1 panel (a debug panel, left as one).

pub mod flight;
pub mod hands;
pub mod markers;
pub mod mission;
pub mod progress;
pub mod reticle;
pub mod style;
pub mod tuning;
pub mod view;
pub mod vitals;

use bevy::prelude::*;
use bevy_egui::{EguiContexts, EguiPrimaryContextPass, egui};

use super::camera::PlayerCamera;

/// Every HUD system, in drawing order. Other egui (the menu) goes after.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct HudSet;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<tuning::PanelState>()
            .init_resource::<HudMemory>()
            .add_systems(
                EguiPrimaryContextPass,
                (
                    style::setup_egui,
                    begin_frame,
                    vitals::track_threats,
                    // Most important first: their labels keep their places.
                    markers::objective_markers,
                    markers::reference_overlay,
                    markers::ship_markers,
                    reticle::reticle,
                    crate::client::items_hud::pickup_card,
                    flight::flight_hud,
                    flight::event_feed,
                    mission::mission_hud,
                    vitals::vitals_hud,
                    hands::hands_hud,
                    progress::progress_hud,
                    reticle::hit_markers,
                    vitals::death_overlay,
                    tuning::tuning_panel,
                )
                    .chain()
                    .in_set(HudSet),
            );
    }
}

/// What the HUD remembers between frames that more than one element uses.
#[derive(Resource, Default)]
pub struct HudMemory {
    /// Seconds of HUD time (real time, so it runs while the game does).
    pub now: f32,
    /// Where the eye was last frame, in the world.
    pub last_eye: Option<Vec3>,
    /// Your last death.
    pub died: Option<vitals::Death>,
    /// Whether you carry anything a death would leave behind (as of last
    /// frame).
    pub carrying: bool,
    /// The case your kit was left in, once it's been seen.
    pub kit: Option<Entity>,
    /// World direction toward each recent hit's source, and when.
    pub arcs: Vec<(Vec3, f32)>,
    /// Where world markers' labels went this frame.
    pub labels: markers::Labels,
}

fn begin_frame(mut contexts: EguiContexts, time: Res<Time>, mut memory: ResMut<HudMemory>) -> Result {
    memory.now = time.elapsed_secs();
    memory.labels.clear();
    // World markers' labels keep off the HUD's own blocks.
    let screen = contexts.ctx_mut()?.content_rect();
    memory.labels.reserve(screen);
    Ok(())
}

/// World to HUD points through the player's camera.
pub struct Projector<'a> {
    camera: &'a Camera,
    global: GlobalTransform,
    pub eye: Transform,
    /// egui points per logical pixel.
    zoom: f32,
}

impl<'a> Projector<'a> {
    pub fn new(camera: &'a Camera, eye: &Transform, ctx: &egui::Context) -> Projector<'a> {
        Projector {
            camera,
            global: GlobalTransform::from(*eye),
            eye: *eye,
            zoom: ctx.zoom_factor(),
        }
    }

    /// `world` in view axes: x right, y up, z back.
    pub fn local(&self, world: Vec3) -> Vec3 {
        self.eye.rotation.inverse() * (world - self.eye.translation)
    }

    /// Where `world` is on screen, if it's in front of the eye.
    pub fn to_screen(&self, world: Vec3) -> Option<egui::Pos2> {
        if self.local(world).z > -0.1 {
            return None;
        }
        let p = self.camera.world_to_viewport(&self.global, world).ok()?;
        Some(egui::pos2(p.x / self.zoom, p.y / self.zoom))
    }

    /// The screen direction toward `world` (x right, y down), for an arrow
    /// at the edge when it's off screen.
    pub fn edge_dir(&self, world: Vec3) -> egui::Vec2 {
        let local = self.local(world);
        let dir = egui::vec2(local.x, -local.y);
        if dir.length() < 1e-4 { egui::vec2(0.0, 1.0) } else { dir.normalized() }
    }
}

/// The player's camera, for projecting.
pub type CameraView<'w, 's> = Single<'w, 's, (&'static Camera, &'static Transform), With<PlayerCamera>>;
