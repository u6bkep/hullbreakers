//! The model gallery (debug): every model under `assets/` set out in a row
//! behind our ship, for review (the client sets them out; see
//! `client::gallery`). The crew floats in front of it. Below the models, a
//! row of live robots: every kind, with each affix it can have, dormant
//! (see `bots::spawn_zoo`).

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{Scenario, Stage, stay};
use crate::sim::mission::{ScenarioKind, Staged};
use crate::sim::player::{Mode, Player};
use crate::sim::ship;

pub static SCENARIO: Scenario = Scenario {
    kind: ScenarioKind::Gallery,
    id: "gallery",
    label: "Model gallery",
    build,
    arrive: Some(to_gallery),
    systems: |_| {},
    return_after: stay,
    homecoming: false,
};

/// Where the model gallery starts: a row behind our ship's stern, centred
/// on its cargo door, at standing eye height.
pub const GALLERY_AT: Vec3 = Vec3::new(0.0, 1.5, 24.0);

/// Where the gallery's models are set out (by the client, which knows what
/// models there are).
#[derive(Component, Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct GalleryAnchor;

fn build(stage: &mut Stage) -> Entity {
    stage.commands.spawn((
        Name::new("Gallery"),
        GalleryAnchor,
        Staged,
        Transform::from_translation(GALLERY_AT),
    ));
    let home = stage.commands.spawn(ship::drifting_home_ship()).id();
    if stage.robots {
        crate::sim::bots::spawn_zoo(stage.commands, stage.tuning, home, GALLERY_AT + ZOO_OFFSET);
    }
    home
}

/// Where the gallery's robots float, from its row of models: below them,
/// nearer the visitors.
const ZOO_OFFSET: Vec3 = Vec3::new(0.0, -3.0, 3.0);

/// Puts a player in front of the gallery's row, facing it (−Z), floating
/// beside the others.
fn to_gallery(player: &mut Player, slot: u32) {
    player.mode = Mode::Eva {
        position: GALLERY_AT + Vec3::new(slot as f32 * 1.5, 0.0, 7.0),
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    player.boots_on = false;
}
