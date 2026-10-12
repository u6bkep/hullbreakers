//! The asteroid field: drifting with the stream. Where the game starts, and
//! where the crew comes back to after a mission. Nothing to win.

use bevy::prelude::*;

use super::{Scenario, Stage, stay};
use crate::sim::asteroids;
use crate::sim::mission::ScenarioKind;
use crate::sim::ship;

pub static SCENARIO: Scenario = Scenario {
    kind: ScenarioKind::Field,
    id: "field",
    label: "Asteroid field",
    build,
    arrive: None,
    systems: |_| {},
    return_after: stay,
};

fn build(stage: &mut Stage) -> Entity {
    let home = stage.commands.spawn(ship::drifting_home_ship()).id();
    if stage.asteroids {
        asteroids::spawn_field(stage.commands, stage.tuning);
    }
    home
}
