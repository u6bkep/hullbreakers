//! Scripted screenshots for checking visuals without a human at the controls.
//!
//! `HULLBREAKERS_SHOTS=<dir> cargo run` teleports the player through a few
//! vantage points, saves a screenshot at each, then exits.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::sim::player::{Grapple, Mode, Player};
use crate::sim::devices::Devices;
use crate::sim::scenarios::boarding::CARRIER;
use crate::sim::scenarios::dead_ship::DERELICT;
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
/// (start in the boarding scenario); `derelict`, the dead ship's derelict
/// (start in it), dark and then with its power routed; `course`, the
/// grapple run (start in it).
fn tour() -> &'static str {
    match std::env::var("HULLBREAKERS_SHOTS_SHIP").as_deref() {
        Ok("carrier") => CARRIER,
        Ok("derelict") => DERELICT,
        Ok("course") => "course",
        _ => HOME,
    }
}

fn vantages() -> Vec<(&'static str, Vantage)> {
    match tour() {
        CARRIER => return carrier_vantages(),
        DERELICT => return derelict_vantages(),
        "course" => return course_vantages(),
        _ => {}
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
        // Loot crates (the hull's `loot…` markers).
        ("carrier_loot_hangar", booted(Vec3::new(2.4, 0.01, 24.2), Vec3::new(1.0, 0.0, -1.0), -0.45)),
        ("carrier_loot_bridge", booted(Vec3::new(1.8, upper + 0.01, -25.8), Vec3::new(1.0, 0.0, -1.0), -0.45)),
    ]
}

/// The derelict's tour: the first few vantages dark, the rest (from
/// [`DERELICT_POWERED`]) with its power routed to the vault.
fn derelict_vantages() -> Vec<(&'static str, Vantage)> {
    let booted = |feet: Vec3, forward: Vec3, pitch: f32| Vantage::Booted {
        feet,
        up: Vec3::Y,
        forward,
        pitch,
    };
    vec![
        (
            "derelict_outside",
            Vantage::Eva {
                offset: Vec3::new(14.0, 7.0, 42.0),
                look_at: Vec3::new(0.0, 1.5, 0.0),
                drift: Vec3::ZERO,
            },
        ),
        ("derelict_airlock", booted(Vec3::new(0.0, 0.01, 17.4), Vec3::NEG_Z, 0.0)),
        ("derelict_engineering_dark", booted(Vec3::new(0.3, 0.01, 13.2), Vec3::new(-0.3, 0.0, -1.0), -0.1)),
        ("derelict_junction_dark", booted(Vec3::new(4.6, 0.01, 1.0), Vec3::X, -0.05)),
        ("derelict_engineering_lit", booted(Vec3::new(0.3, 0.01, 13.2), Vec3::new(-0.3, 0.0, -1.0), -0.1)),
        ("derelict_board", booted(Vec3::new(-4.6, 0.01, 10.0), Vec3::NEG_X, 0.0)),
        ("derelict_junction", booted(Vec3::new(4.6, 0.01, 1.0), Vec3::X, -0.05)),
        ("derelict_mid_hall", booted(Vec3::new(3.0, 0.01, 5.2), Vec3::new(-0.5, 0.0, -1.0), -0.25)),
        ("derelict_vault_door", booted(Vec3::new(-2.0, 0.01, -8.5), Vec3::new(-0.3, 0.0, -1.0), -0.05)),
        ("derelict_key", booted(Vec3::new(4.5, 0.01, -17.2), Vec3::NEG_Z, -0.5)),
    ]
}

/// From this vantage on, the derelict's power is on and routed.
const DERELICT_POWERED: usize = 4;

/// The grapple run's tour, from our ship (positions are in its frame).
fn course_vantages() -> Vec<(&'static str, Vantage)> {
    vec![
        (
            "run_stern",
            Vantage::Booted {
                feet: Vec3::new(0.0, 0.01, 12.5),
                up: Vec3::Y,
                forward: Vec3::Z,
                pitch: 0.05,
            },
        ),
        (
            "run_gate",
            Vantage::Eva {
                offset: Vec3::new(4.0, 4.0, 22.0),
                look_at: Vec3::new(18.0, 8.0, 75.0),
                drift: Vec3::ZERO,
            },
        ),
        (
            "run_cluster",
            Vantage::Eva {
                offset: Vec3::new(30.0, 45.0, 40.0),
                look_at: Vec3::new(-20.0, 0.0, 90.0),
                drift: Vec3::ZERO,
            },
        ),
    ]
}

/// Debug: routes the derelict's power to its vault, as a crew would have,
/// straight into the authority's state (the shots run on a host).
fn power_up(hull: &ShipHull, devices: &mut Devices) {
    let design = hull.design();
    for (id, set) in [("reactor", 1), ("j1", 2), ("j2", 1), ("j3", 1)] {
        devices.0[design.device(id)].set = set;
    }
    // The doors those junctions opened on the way, latched.
    for id in ["port_door", "bulkhead"] {
        devices.0[design.device(id)].live = true;
    }
}

const SETTLE_SECONDS: f32 = 1.5;

pub fn run_shot_script(
    time: Res<Time>,
    mut script: ResMut<ShotScript>,
    mut commands: Commands,
    mut player: Single<&mut Player, With<super::LocalPlayer>>,
    ships: Query<(Entity, &Transform, &ShipHull)>,
    mut devices: Query<(&ShipHull, &mut Devices)>,
    mut exit: MessageWriter<AppExit>,
) {
    let want = match tour() {
        "course" => HOME,
        ship => ship,
    };
    let Some((ship_entity, ship_transform, _)) = ships.iter().find(|(.., h)| h.0 == want) else {
        return;
    };
    let shots = vantages();
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
    if tour() == DERELICT && script.next == DERELICT_POWERED {
        for (hull, mut devices) in &mut devices {
            if hull.0 == DERELICT {
                power_up(hull, &mut devices);
            }
        }
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
    script.next += 1;
}
