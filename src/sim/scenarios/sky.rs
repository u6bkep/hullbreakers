//! The sky preview (debug): our ship drifting in open space, a test stand
//! of things to judge the light on (a hull section, a rock, a dormant
//! Gunner), and the sky console beside it, worked like the mission wall.
//! Nothing to win; it's for looking at ambiences (see `sim::ambience`).
//!
//! The console (`assets/ships/sky_console`) has buttons that step through
//! the skies (every named one, then every archetype) and a generated sky's
//! seed; a lever per layer of the sky (stars, band, nebulae, bodies, the
//! sun's disc), so each can be seen alone or taken away; and a junction for
//! where the sun is: the sky's own place, high, low, behind the stand,
//! edge-on, or turning slowly round. The authority builds the sky from the
//! console's state and replicates it; a named sky's file is read from disk
//! and read again whenever it's saved, so a file can be edited with the
//! preview open. The mission's status board shows which sky it is and
//! anything the validator says about it. (The console sets the sky when
//! it's worked; a sky set another way, by the screenshot tour
//! `HULLBREAKERS_SHOTS_SHIP=sky`, stays until it is.)

use std::time::SystemTime;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{Scenario, Stage, stay};
use crate::sim::SimTick;
use crate::sim::ambience::{self, Ambience, Archetype, Choice};
use crate::sim::devices::Devices;
use crate::sim::mission::{Mission, ScenarioKind, Staged, presses};
use crate::sim::player::{Mode, Player};
use crate::sim::ship::{self, ControlAction, ShipHull};

pub static SCENARIO: Scenario = Scenario {
    kind: ScenarioKind::Sky,
    id: "sky",
    label: "Sky preview",
    build,
    arrive: Some(to_stand),
    systems: |app| super::judge(app, ScenarioKind::Sky, run_preview),
    return_after: stay,
    homecoming: false,
    ambience: Choice::named(ambience::REFERENCE),
};

/// The console's design.
pub const CONSOLE: &str = "sky_console";
/// Where the test stand is: behind our ship's stern, as the gallery is.
pub const STAND_AT: Vec3 = Vec3::new(0.0, 1.5, 26.0);
/// Where the console is, from the stand: to the visitors' left, nearer
/// them, facing them (+Z).
const CONSOLE_OFFSET: Vec3 = Vec3::new(-5.0, -0.8, 4.0);
/// Where the visitors float, from the stand.
const VISITORS_OFFSET: Vec3 = Vec3::new(0.0, 0.0, 9.0);

/// A button on the sky console.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkyControl {
    PreviousSky,
    NextSky,
    PreviousSeed,
    NextSeed,
    RandomSeed,
}

impl SkyControl {
    pub fn label(self) -> &'static str {
        match self {
            SkyControl::PreviousSky => "Previous sky",
            SkyControl::NextSky => "Next sky",
            SkyControl::PreviousSeed => "Previous seed",
            SkyControl::NextSeed => "Next seed",
            SkyControl::RandomSeed => "Random seed",
        }
    }
}

/// One of the skies the console steps through.
#[derive(Clone, Debug, PartialEq)]
enum Entry {
    Named(String),
    Generated(Archetype),
}

/// Where the sun can be put, by the console's junction (its ways in order).
#[derive(Clone, Copy, Debug, PartialEq)]
enum SunPlace {
    Own,
    High,
    Low,
    /// Behind the stand, from where the visitors float: everything on it in
    /// silhouette.
    Behind,
    /// Square to the view: everything on the stand half lit.
    EdgeOn,
    Turning,
}

impl SunPlace {
    const WAYS: [SunPlace; 6] = [SunPlace::Own, SunPlace::High, SunPlace::Low, SunPlace::Behind, SunPlace::EdgeOn, SunPlace::Turning];

    fn label(self) -> &'static str {
        match self {
            SunPlace::Own => "its own place",
            SunPlace::High => "high",
            SunPlace::Low => "low",
            SunPlace::Behind => "behind the stand",
            SunPlace::EdgeOn => "edge-on",
            SunPlace::Turning => "turning",
        }
    }

    /// Toward the sun, at `seconds` into the preview (visitors look along
    /// −Z at the stand).
    fn direction(self, own: Vec3, seconds: f32) -> Vec3 {
        match self {
            SunPlace::Own => own,
            SunPlace::High => Vec3::new(0.25, 1.0, 0.35),
            SunPlace::Low => Vec3::new(0.5, 0.08, 0.85),
            SunPlace::Behind => Vec3::new(0.15, 0.2, -1.0),
            SunPlace::EdgeOn => Vec3::new(1.0, 0.1, 0.0),
            SunPlace::Turning => {
                let a = seconds * TURN_RATE;
                Vec3::new(a.cos(), 0.35, a.sin())
            }
        }
    }
}

/// How fast a turning sun goes round (rad/s), and how often its sky is
/// sent again (ticks): each change is a re-bake on every peer.
const TURN_RATE: f32 = 0.2;
const TURN_EVERY: u32 = 6;
/// How often a named sky's file is checked for edits (ticks).
const RELOAD_EVERY: u32 = 30;

/// The preview's own state: which sky, which seed, and the file it was
/// read from as it was then.
#[derive(Component)]
struct Preview {
    console: Entity,
    skies: Vec<Entry>,
    at: usize,
    seed: u64,
    /// The sky as chosen (before the console's levers and sun), and when
    /// its file was last changed (a named sky's).
    base: Ambience,
    modified: Option<SystemTime>,
    problems: Vec<String>,
    /// The sky as the console last made it. The console sets the sky only
    /// when it's worked (or the file's edited), so a sky set some other
    /// way (the screenshot tour's) stays until then.
    made: Option<Ambience>,
}

impl Preview {
    fn entry(&self) -> &Entry {
        &self.skies[self.at]
    }

    /// Makes the chosen sky afresh (reading a named one's file again).
    fn remake(&mut self) {
        let (made, when) = match self.entry() {
            Entry::Named(name) => (ambience::read_named(name), modified(name)),
            Entry::Generated(archetype) => (Ok(ambience::generate(*archetype, self.seed)), None),
        };
        self.modified = when;
        match made {
            Ok(sky) => {
                self.problems = ambience::problems(&sky);
                self.base = sky;
            }
            // A file mid-edit that doesn't parse: keep the last good sky,
            // and say why.
            Err(err) => self.problems = vec![err],
        }
    }
}

/// When a named sky's file was last changed, if it's on disk.
fn modified(name: &str) -> Option<SystemTime> {
    std::fs::metadata(ambience::source_dir().join(format!("{name}.ron"))).and_then(|m| m.modified()).ok()
}

/// Every named sky (those shipped, and any new files beside them), then
/// every archetype.
fn skies() -> Vec<Entry> {
    let mut names: Vec<String> = ambience::names().into_iter().map(String::from).collect();
    if let Ok(dir) = std::fs::read_dir(ambience::source_dir()) {
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "ron")
                && let Some(stem) = path.file_stem()
            {
                let stem = stem.to_string_lossy().to_string();
                if !names.contains(&stem) {
                    names.push(stem);
                }
            }
        }
    }
    names.sort();
    names
        .into_iter()
        .map(Entry::Named)
        .chain(Archetype::ALL.into_iter().map(Entry::Generated))
        .collect()
}

fn build(stage: &mut Stage) -> Entity {
    let home = stage.commands.spawn(ship::drifting_home_ship()).id();
    let console = stage
        .commands
        .spawn(ship::site("Sky console", CONSOLE, STAND_AT + CONSOLE_OFFSET, Quat::IDENTITY))
        .id();
    // The test stand: the console's design carries the hull section; a
    // rock and a Gunner float beside it.
    crate::sim::asteroids::spawn_rocks(stage.commands, 17, &[(STAND_AT + Vec3::new(3.6, 0.0, 0.0), 1.4)]);
    if stage.robots {
        crate::sim::bots::spawn_dormant(
            stage.commands,
            stage.tuning,
            home,
            crate::sim::bots::Robot::Gunner,
            STAND_AT + Vec3::new(-3.2, -0.6, 0.0),
            Quat::from_rotation_y(0.5),
        );
    }
    let mut preview = Preview {
        console,
        skies: skies(),
        at: 0,
        seed: 1,
        base: Ambience::reference(),
        modified: None,
        problems: Vec::new(),
        made: None,
    };
    preview.at = preview.skies.iter().position(|e| *e == Entry::Named(ambience::REFERENCE.into())).unwrap_or(0);
    preview.remake();
    stage.commands.spawn((Name::new("Sky preview"), Staged, preview));
    home
}

/// Puts a player in front of the stand, facing it (−Z), floating beside
/// the others.
fn to_stand(player: &mut Player, slot: u32) {
    player.mode = Mode::Eva {
        position: STAND_AT + VISITORS_OFFSET + Vec3::new(slot as f32 * 1.5, 0.0, 0.0),
        velocity: Vec3::ZERO,
        orientation: Quat::IDENTITY,
        pitch: 0.0,
    };
    player.boots_on = false;
}

/// Authority: the console's presses and switches make the sky.
fn run_preview(
    tick: Res<SimTick>,
    preview: Option<Single<&mut Preview>>,
    players: Query<(Entity, &Player)>,
    consoles: Query<(&ShipHull, &Devices)>,
    sky: Option<Single<&mut Ambience>>,
    mission: Single<&mut Mission>,
) {
    let (Some(preview), Some(sky)) = (preview, sky) else {
        return;
    };
    let mut preview = preview.into_inner();
    let mut sky = sky.into_inner();
    let mut mission = mission.into_inner();
    // The first tick: start from the sky the scenario was started under (the
    // reference, or `HULLBREAKERS_AMBIENCE`'s), until the console's worked.
    if preview.made.is_none() {
        preview.base = sky.clone();
        preview.problems = ambience::problems(&sky);
    }
    let mut remake = false;
    for (_, body, action) in presses(tick.0, &players) {
        let ControlAction::Sky(control) = action else { continue };
        if body != preview.console {
            continue;
        }
        let count = preview.skies.len();
        match control {
            SkyControl::NextSky => preview.at = (preview.at + 1) % count,
            SkyControl::PreviousSky => preview.at = (preview.at + count - 1) % count,
            SkyControl::NextSeed => preview.seed = preview.seed.wrapping_add(1),
            SkyControl::PreviousSeed => preview.seed = preview.seed.wrapping_sub(1),
            SkyControl::RandomSeed => preview.seed = random_seed(tick.0, preview.seed),
        }
        remake = true;
    }
    // A named sky's file, saved since it was read.
    if !remake
        && tick.0 % RELOAD_EVERY == 0
        && let Entry::Named(name) = preview.entry()
        && modified(name) != preview.modified
    {
        remake = true;
    }
    if remake {
        preview.remake();
    }

    // The console's switches.
    let Ok((hull, devices)) = consoles.get(preview.console) else { return };
    let console = hull.design();
    let lever = |id: &str| devices.live(console, id);
    let way = console.device_index("sun").map_or(0, |i| devices.0[i].set as usize);
    let place = SunPlace::WAYS[way.min(SunPlace::WAYS.len() - 1)];
    let started = mission.started;
    // A turning sun moves in steps, each a new sky to send and bake.
    let seconds = tick.0.wrapping_sub(started) / TURN_EVERY * TURN_EVERY;
    let seconds = seconds as f32 / crate::sim::TICK_HZ as f32;
    let mut shown = preview.base.clone();
    shown.show = ambience::Layers {
        stars: lever("stars"),
        band: lever("band"),
        nebulae: lever("nebulae"),
        bodies: lever("bodies"),
        sun: lever("sun_disc"),
    };
    shown.sun.direction = place.direction(preview.base.sun.direction, seconds);
    if preview.made.as_ref() != Some(&shown) {
        *sky = shown.clone();
        preview.made = Some(shown);
    }

    // The board: which sky, and what the validator says.
    let which = format!("Sky: {}", sky.name);
    let layers: Vec<&str> = [
        ("stars", sky.show.stars),
        ("band", sky.show.band),
        ("nebulae", sky.show.nebulae),
        ("bodies", sky.show.bodies),
        ("sun", sky.show.sun),
    ]
    .into_iter()
    .filter_map(|(name, on)| on.then_some(name))
    .collect();
    let mut status = vec![
        which,
        format!("Showing: {}", if layers.is_empty() { "nothing".to_string() } else { layers.join(", ") }),
        format!("Sun: {}", place.label()),
    ];
    status.extend(preview.problems.iter().take(3).map(|p| format!("! {p}")));
    let objective = "Work the console to change the sky".to_string();
    if mission.status != status || mission.objective != objective {
        mission.status = status;
        mission.objective = objective;
    }
}

/// A new seed, from the tick it was asked for on (and the last).
fn random_seed(tick: u32, last: u64) -> u64 {
    let mut x = (tick as u64) ^ last.rotate_left(17) ^ 0x9e37_79b9_7f4a_7c15;
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x % 100_000
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::design::design;

    #[test]
    fn the_console_has_its_switches() {
        let console = design(CONSOLE);
        for id in ["stars", "band", "nebulae", "bodies", "sun_disc", "sun"] {
            assert!(console.device_index(id).is_some(), "no {id:?} on the console");
        }
        let sun = &console.devices[console.device_index("sun").unwrap()];
        assert!(matches!(sun.kind, crate::sim::devices::DeviceKind::Junction { ways } if ways as usize == SunPlace::WAYS.len()));
        for control in [SkyControl::PreviousSky, SkyControl::NextSky, SkyControl::PreviousSeed, SkyControl::NextSeed, SkyControl::RandomSeed] {
            assert!(
                console.controls.iter().any(|(a, ..)| *a == ControlAction::Sky(control)),
                "no {control:?} button on the console"
            );
        }
    }

    #[test]
    fn the_skies_start_named_and_end_generated() {
        let skies = skies();
        assert!(skies.contains(&Entry::Named(ambience::REFERENCE.into())));
        assert_eq!(skies.last(), Some(&Entry::Generated(*Archetype::ALL.last().unwrap())));
    }
}
