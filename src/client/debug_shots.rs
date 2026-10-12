//! Scripted screenshots for checking visuals without a human at the controls.
//!
//! `HULLBREAKERS_SHOTS=<dir> cargo run` teleports the player through a few
//! vantage points, saves a screenshot at each, then exits.
//! `HULLBREAKERS_SHOTS_SHIP=carrier` tours the enemy carrier instead, and
//! `HULLBREAKERS_SHOTS_SHIP=hud` puts the HUD through the states it has to
//! show (armed, hurt, critical, by an objective, dying); both start in the
//! boarding scenario.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::sim::player::{Grapple, Mode, Player};
use crate::sim::vitals::Vitals;
use crate::sim::SimTick;
use crate::sim::mission::{CARRIER, Mission, Phase};
use crate::sim::ship::{HOME, ShipHull};

#[derive(Resource)]
pub struct ShotScript {
    dir: std::path::PathBuf,
    next: usize,
    timer: f32,
    /// Screenshot taken for the current vantage; teleport on the next frame
    /// so the capture isn't of the transition.
    captured: bool,
}

pub fn shot_script_from_env() -> Option<ShotScript> {
    let dir = std::env::var_os("HULLBREAKERS_SHOTS")?;
    Some(ShotScript {
        dir: dir.into(),
        next: 0,
        timer: 0.0,
        captured: false,
    })
}

/// Vantage points: (name, booted feet + up + facing on the ship, or EVA position relative to the ship).
enum Vantage {
    Booted {
        feet: Vec3,
        up: Vec3,
        forward: Vec3,
        pitch: f32,
    },
    Eva {
        offset: Vec3,
        look_at: Vec3,
        /// Drift relative to the ship, in ship axes.
        drift: Vec3,
    },
    /// EVA at `offset`, grappled to `anchor` (ship local) with `slack` meters
    /// of spare rope, drifting away along the rope at `drift` m/s.
    Rope {
        offset: Vec3,
        anchor: Vec3,
        slack: f32,
        drift: f32,
    },
}

/// `HULLBREAKERS_SHOTS_SHIP=carrier`: tour the enemy carrier instead
/// (start in the boarding scenario).
fn carrier_tour() -> bool {
    std::env::var("HULLBREAKERS_SHOTS_SHIP").is_ok_and(|v| v == "carrier")
}

/// `HULLBREAKERS_SHOTS_SHIP=hud`.
fn hud_tour() -> bool {
    std::env::var("HULLBREAKERS_SHOTS_SHIP").is_ok_and(|v| v == "hud")
}

/// Which ship a vantage is relative to: the carrier's tour, and the HUD
/// tour's `hud_carrier_*` shots, are on the carrier.
fn ship_of(name: &str) -> &'static str {
    if carrier_tour() || (hud_tour() && name.starts_with("hud_carrier")) { CARRIER } else { HOME }
}

fn vantages() -> Vec<(&'static str, Vantage)> {
    if carrier_tour() {
        return carrier_vantages();
    }
    if hud_tour() {
        return hud_vantages();
    }
    vec![
        (
            "interior",
            Vantage::Booted {
                feet: Vec3::new(1.5, 0.01, 12.0),
                up: Vec3::Y,
                forward: Vec3::NEG_Z,
                pitch: 0.0,
            },
        ),
        (
            "roof",
            Vantage::Booted {
                feet: Vec3::new(2.5, 3.31, -12.0),
                up: Vec3::Y,
                forward: Vec3::new(0.2, 0.0, 1.0),
                pitch: 0.05,
            },
        ),
        (
            "hull_side",
            Vantage::Booted {
                feet: Vec3::new(4.01, 1.5, -5.0),
                up: Vec3::X,
                forward: Vec3::Z,
                pitch: 0.1,
            },
        ),
        (
            "eva_stern",
            Vantage::Eva {
                offset: Vec3::new(6.0, 5.0, 35.0),
                look_at: Vec3::new(0.0, 1.5, 0.0),
                drift: Vec3::new(-1.5, 0.8, -2.5),
            },
        ),
        (
            "eva_field",
            Vantage::Eva {
                offset: Vec3::new(0.0, 8.0, -25.0),
                look_at: Vec3::new(0.0, 8.0, -200.0),
                drift: Vec3::ZERO,
            },
        ),
        (
            "eva_away",
            Vantage::Eva {
                offset: Vec3::new(-10.0, 4.0, 20.0),
                look_at: Vec3::new(-60.0, 4.0, -20.0),
                drift: Vec3::new(0.0, 0.0, 3.0),
            },
        ),
        (
            "rope_slack",
            Vantage::Rope {
                offset: Vec3::new(-8.0, 9.0, 28.0),
                anchor: Vec3::new(0.0, 4.1, 6.0),
                slack: 6.0,
                drift: 0.0,
            },
        ),
        (
            "rope_taut",
            Vantage::Rope {
                offset: Vec3::new(-8.0, 9.0, 28.0),
                anchor: Vec3::new(0.0, 4.1, 6.0),
                slack: 0.0,
                drift: 2.0,
            },
        ),
    ]
}

fn carrier_vantages() -> Vec<(&'static str, Vantage)> {
    let upper = ShipHull::new(CARRIER).design().marker("upper_deck").y;
    let booted = |feet: Vec3, forward: Vec3, pitch: f32| Vantage::Booted {
        feet,
        up: Vec3::Y,
        forward,
        pitch,
    };
    vec![
        (
            "carrier_outside",
            Vantage::Eva {
                offset: Vec3::new(35.0, 18.0, 70.0),
                look_at: Vec3::new(0.0, 3.0, 0.0),
                drift: Vec3::ZERO,
            },
        ),
        ("carrier_door", booted(Vec3::new(0.0, 0.01, 29.0), Vec3::NEG_Z, 0.15)),
        ("carrier_ramp", booted(Vec3::new(3.0, 0.01, 25.0), Vec3::new(-1.0, 0.0, -0.6), 0.3)),
        ("carrier_lower", booted(Vec3::new(0.0, 0.01, 12.0), Vec3::NEG_Z, 0.0)),
        ("carrier_upper_room", booted(Vec3::new(-4.8, upper + 0.01, 12.5), Vec3::NEG_Z, 0.0)),
        ("carrier_upper", booted(Vec3::new(0.0, upper + 0.01, 10.0), Vec3::NEG_Z, 0.0)),
        ("carrier_bridge", booted(Vec3::new(0.0, upper + 0.01, -21.0), Vec3::NEG_Z, -0.2)),
    ]
}

fn hud_vantages() -> Vec<(&'static str, Vantage)> {
    let upper = ShipHull::new(CARRIER).design().marker("upper_deck").y;
    let booted = |feet: Vec3, forward: Vec3, pitch: f32| Vantage::Booted {
        feet,
        up: Vec3::Y,
        forward,
        pitch,
    };
    vec![
        // At the mission wall: the interact prompt.
        ("hud_deck", booted(Vec3::new(-2.2, 0.01, -7.3), Vec3::NEG_X, 0.08)),
        ("hud_hurt", booted(Vec3::new(0.0, 0.01, 10.0), Vec3::NEG_Z, 0.0)),
        ("hud_critical", booted(Vec3::new(1.5, 0.01, 12.0), Vec3::NEG_Z, 0.0)),
        (
            "hud_carrier_eva",
            Vantage::Eva {
                offset: Vec3::new(30.0, 14.0, 75.0),
                look_at: Vec3::new(0.0, 3.0, 0.0),
                drift: Vec3::ZERO,
            },
        ),
        ("hud_carrier_console", booted(Vec3::new(0.0, upper + 0.01, -25.6), Vec3::NEG_Z, -0.38)),
        ("hud_carrier_dead", booted(Vec3::new(0.0, upper + 0.01, 10.0), Vec3::NEG_Z, 0.0)),
        ("hud_respawned", booted(Vec3::new(0.0, 0.01, 5.0), Vec3::NEG_Z, 0.0)),
        // The fight lost (by fiat): the banner, then back to the field
        // after the drift, and the debrief.
        ("hud_lost", booted(Vec3::new(0.0, 0.01, 5.0), Vec3::NEG_Z, 0.0)),
        ("hud_lost_2", booted(Vec3::new(0.0, 0.01, 5.0), Vec3::NEG_Z, 0.0)),
        ("hud_lost_3", booted(Vec3::new(0.0, 0.01, 5.0), Vec3::NEG_Z, 0.0)),
        ("hud_lost_4", booted(Vec3::new(0.0, 0.01, 5.0), Vec3::NEG_Z, 0.0)),
        ("hud_debrief", booted(Vec3::new(0.0, 0.01, 5.0), Vec3::NEG_Z, 0.0)),
    ]
}

/// The state each HUD shot is in, set as the player arrives.
fn hud_state(name: &str, player: &mut Player, vitals: Option<&mut Vitals>) {
    let loadout = &mut player.loadout;
    if !name.starts_with("hud_respawned") && !name.starts_with("hud_lost") && name != "hud_debrief" {
        loadout.arm_fully();
    }
    let Some(vitals) = vitals else { return };
    match name {
        "hud_hurt" => {
            // The rifle braced and low; hurt a moment ago.
            loadout.current = 2;
            loadout.guns[1].mag = 6;
            vitals.health = 55.0;
            vitals.since_hurt = 0.0;
        }
        "hud_critical" => {
            // The SMG dry, the rifle reloading; nearly dead, healing.
            loadout.current = 0;
            loadout.guns[0].mag = 0;
            loadout.guns[0].reserve = 0;
            vitals.health = 14.0;
            vitals.since_hurt = 4.55;
        }
        "hud_carrier_console" => loadout.current = 1,
        "hud_carrier_dead" => {
            vitals.health = 0.0;
            vitals.since_hurt = 0.0;
        }
        _ => {}
    }
}

const SETTLE_SECONDS: f32 = 1.5;

pub fn run_shot_script(
    time: Res<Time>,
    mut script: ResMut<ShotScript>,
    mut commands: Commands,
    mut player: Single<&mut Player, With<super::LocalPlayer>>,
    mut vitals: Option<Single<&mut Vitals, With<super::LocalPlayer>>>,
    ships: Query<(Entity, &Transform, &ShipHull)>,
    mut missions: Query<&mut Mission>,
    tick: Res<SimTick>,
    mut exit: MessageWriter<AppExit>,
) {
    let shots = vantages();
    let want = shots.get(script.next).map_or(HOME, |(name, _)| ship_of(name));
    let Some((ship_entity, ship_transform, _)) = ships.iter().find(|(.., h)| h.0 == want) else {
        return;
    };
    script.timer += time.delta_secs();
    if script.timer < SETTLE_SECONDS {
        return;
    }
    // Screenshot the vantage set up last round, then set up the next one.
    if script.next > 0 && script.next <= shots.len() && !script.captured {
        let name = shots[script.next - 1].0;
        let path = script.dir.join(format!("{name}.png"));
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        script.captured = true;
        return;
    }
    script.timer = 0.0;
    script.captured = false;
    if script.next > shots.len() {
        exit.write(AppExit::Success);
        return;
    }
    if script.next == shots.len() {
        script.next += 1;
        return;
    }
    player.mode = match shots[script.next].1 {
        Vantage::Booted {
            feet,
            up,
            forward,
            pitch,
        } => {
            let right = forward.cross(up).normalize();
            let forward = up.cross(right);
            let orientation = Quat::from_mat3(&Mat3::from_cols(right, up, -forward));
            Mode::Booted {
                body: ship_entity,
                feet,
                up,
                velocity: Vec3::ZERO,
                orientation,
                pitch,
            }
        }
        Vantage::Eva {
            offset,
            look_at,
            drift,
        } => {
            let position = ship_transform.transform_point(offset);
            let target = ship_transform.transform_point(look_at);
            let orientation = Transform::from_translation(position)
                .looking_at(target, ship_transform.up())
                .rotation;
            Mode::Eva {
                position,
                velocity: ship_transform.rotation * drift,
                orientation,
                pitch: 0.0,
            }
        }
        // Set up below, along with the grapple.
        Vantage::Rope { .. } => player.mode,
    };
    if let Vantage::Rope {
        offset,
        anchor,
        slack,
        drift,
    } = shots[script.next].1
    {
        let position = ship_transform.transform_point(offset);
        let anchor_w = ship_transform.transform_point(anchor);
        let away = (position - anchor_w).normalize();
        player.mode = Mode::Eva {
            position,
            velocity: away * drift,
            orientation: Transform::from_translation(position)
                .looking_at(anchor_w, ship_transform.up())
                .rotation,
            pitch: 0.0,
        };
        player.grapple = Some(Grapple {
            target: ship_entity,
            anchor,
            length: position.distance(anchor_w) + slack,
            tension: 0.0,
        });
    } else {
        player.grapple = None;
    }
    // Keep the scripted drift so the prograde marker is visible.
    player.flight_assist = false;
    if hud_tour() {
        hud_state(shots[script.next].0, &mut player, vitals.as_mut().map(|v| &mut ***v));
        if shots[script.next].0 == "hud_lost"
            && let Ok(mut mission) = missions.single_mut()
        {
            mission.phase = Phase::Lost;
            mission.ended = Some(tick.0);
        }
    }
    script.next += 1;
}
