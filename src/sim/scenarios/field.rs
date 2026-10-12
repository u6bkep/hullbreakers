//! The asteroid field: drifting with the stream. Where the game starts, and
//! where the crew comes back to after a mission. Nothing to win, but it's
//! alive (see `fauna`): limpets on its rocks, herds of grazers.

use bevy::prelude::*;

use super::{Scenario, Stage, stay};
use crate::sim::{asteroids, fauna};
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
    homecoming: false,
};

fn build(stage: &mut Stage) -> Entity {
    let home = stage.commands.spawn(ship::drifting_home_ship()).id();
    if stage.asteroids {
        asteroids::spawn_field(stage.commands, stage.tuning);
        if stage.robots {
            stage.commands.insert_resource(fauna::FaunaField);
        }
    }
    home
}
