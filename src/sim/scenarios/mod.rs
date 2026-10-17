//! The scenarios, one module each, with a common shape (a [`Scenario`]):
//!
//! - **build** sets up its world (ships, sites, rocks, its own state as
//!   [`Staged`](super::mission::Staged) entities) and returns the crew's
//!   ship, where everyone comes aboard;
//! - **judge** is its own systems, which run only while it's on (see
//!   [`judge`]): they decide it ([`Mission::decide`]) and keep the
//!   [`Mission`]'s objective, status, markers and report up to date;
//! - **return_after** says whether the crew goes back to the field by itself
//!   once it's decided, and when.
//!
//! The debrief is built from the mission's report and the crew's tally,
//! the same way for every scenario (see [`super::mission`]). Adding a
//! scenario is a module here, a [`ScenarioKind`] for it, a line in
//! [`scenario`], and a button on the mission wall (`assets/ships/home`).
//!
//! [`Mission`]: super::mission::Mission
//! [`Mission::decide`]: super::mission::Mission::decide

use bevy::ecs::schedule::ScheduleConfigs;
use bevy::ecs::system::ScheduleSystem;
use bevy::prelude::*;

use super::mission::{Mission, MissionSystems, Phase, ScenarioKind};
use super::player::Player;
use super::tuning::Tuning;

pub mod boarding;
pub mod dead_ship;
pub mod escort;
pub mod field;
pub mod gallery;
pub mod grapple_run;
pub mod recovery;
pub mod sky;

/// What a scenario's world is built with.
pub struct Stage<'a, 'w, 's> {
    pub commands: &'a mut Commands<'w, 's>,
    pub tuning: &'a Tuning,
    /// The tick it starts on.
    pub tick: u32,
    /// Whether to fill the world with rocks, and crew enemy ships (tests
    /// turn these off).
    pub asteroids: bool,
    pub robots: bool,
    /// How many players there are (at least one): a scenario may size its
    /// opposition to the crew.
    pub players: usize,
}

pub struct Scenario {
    pub kind: ScenarioKind,
    /// What `HULLBREAKERS_SCENARIO` calls it.
    pub id: &'static str,
    /// What its button on the mission wall says.
    pub label: &'static str,
    /// Builds its world; returns the crew's ship.
    pub build: fn(&mut Stage) -> Entity,
    /// Where a player starts, if not on the deck (they're put on the deck
    /// first): at the start, and on joining.
    pub arrive: Option<fn(&mut Player, u32)>,
    /// Adds its systems (see [`judge`]).
    pub systems: fn(&mut App),
    /// How long after it's decided (with that phase) the crew goes back to
    /// the field, if it doesn't wait for them to choose.
    pub return_after: fn(Phase, &Tuning) -> Option<f32>,
    /// Whether getting home is part of it: the debrief then says who made it
    /// home and who was left drifting.
    pub homecoming: bool,
    /// The sky its world is under (see [`crate::sim::ambience`]).
    pub ambience: crate::sim::ambience::Choice,
}

impl super::asteroids::FieldLayout {
    /// Where a field of `count` rocks in this layout has them.
    pub fn places(self, count: usize) -> Vec<super::asteroids::RockPlace> {
        match self {
            Self::GrappleRun => grapple_run::rock_places(count),
        }
    }
}

/// The scenario of a kind.
pub fn scenario(kind: ScenarioKind) -> &'static Scenario {
    match kind {
        ScenarioKind::Field => &field::SCENARIO,
        ScenarioKind::Boarding => &boarding::SCENARIO,
        ScenarioKind::GrappleRun => &grapple_run::SCENARIO,
        ScenarioKind::DeadShip => &dead_ship::SCENARIO,
        ScenarioKind::Recovery => &recovery::SCENARIO,
        ScenarioKind::Escort => &escort::SCENARIO,
        ScenarioKind::Gallery => &gallery::SCENARIO,
        ScenarioKind::Sky => &sky::SCENARIO,
    }
}

/// Adds every scenario's systems.
pub fn add_scenarios(app: &mut App) {
    for kind in ScenarioKind::ALL {
        (scenario(kind).systems)(app);
    }
}

/// Adds a scenario's judging systems: run in the mission tick (after
/// presses and devices, before a switch), only while it's on.
pub fn judge<M>(app: &mut App, kind: ScenarioKind, systems: impl IntoScheduleConfigs<ScheduleSystem, M>) {
    let systems: ScheduleConfigs<ScheduleSystem> = systems.into_configs();
    app.add_systems(
        FixedPostUpdate,
        systems
            .run_if(move |mission: Option<Single<&Mission>>| mission.is_some_and(|m| m.kind == kind))
            .in_set(MissionSystems::Judge),
    );
}

/// Never goes back by itself.
pub fn stay(_: Phase, _: &Tuning) -> Option<f32> {
    None
}

/// Seconds a decided scenario shows its banner before the crew is back in
/// the field (for scenarios with nothing to do once it's over).
pub const BANNER_SECONDS: f32 = 8.0;

pub fn after_banner(_: Phase, _: &Tuning) -> Option<f32> {
    Some(BANNER_SECONDS)
}

#[cfg(test)]
#[test]
fn every_scenario_names_a_shipped_sky() {
    for kind in ScenarioKind::ALL {
        if let crate::sim::ambience::Choice::Named(name) = &scenario(kind).ambience {
            crate::sim::ambience::Ambience::named(name).unwrap_or_else(|e| panic!("{kind:?}: {e}"));
        }
    }
}
