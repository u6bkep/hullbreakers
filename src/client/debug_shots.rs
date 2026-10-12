//! Scripted screenshots for checking visuals without a human at the controls.
//!
//! `HULLBREAKERS_SHOTS=<dir> cargo run` teleports the player through a few
//! vantage points, saves a screenshot at each, then exits.
//! `HULLBREAKERS_SHOTS_SHIP=hud` puts the HUD through the states it has to
//! show (armed, hurt, critical, by an objective, dying, a lost fight and
//! its debrief); start it in the boarding scenario. `=fauna` visits the
//! field's life: the nearest herd of grazers and the nearest limpet.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::sim::SimTick;
use crate::sim::mission::{Mission, Phase};
use crate::sim::player::{Grapple, Mode, Player};
use crate::sim::vitals::Vitals;
use crate::sim::devices::Devices;
use crate::sim::scenarios::boarding::CARRIER;
use crate::sim::scenarios::dead_ship::DERELICT;
use crate::sim::scenarios::recovery::Pod;
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
    /// EVA `offset` (world axes) from the recovery's pod `index`, looking at
    /// it.
    NearPod {
        index: u8,
        offset: Vec3,
    },
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
    /// EVA `offset` (world axes) from the grazer (or limpet) nearest the
    /// ship, drifting with it, looking at it.
    NearFauna {
        limpet: bool,
        offset: Vec3,
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
        Ok("recovery") => "recovery",
        Ok("freighter") => "freighter",
        Ok("power") => "power",
        Ok("hud") => "hud",
        Ok("fauna") => "fauna",
        _ => HOME,
    }
}

fn vantages() -> Vec<(&'static str, Vantage)> {
    match tour() {
        CARRIER => return carrier_vantages(),
        DERELICT => return derelict_vantages(),
        "course" => return course_vantages(),
        "recovery" => return recovery_vantages(),
        "freighter" => return freighter_vantages(),
        "power" => return power_vantages(),
        "hud" => return hud_vantages(),
        "fauna" => return fauna_vantages(),
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

/// The recovery's tour, from our ship: the wreck's crew out among the
/// debris (survivor A, near its starting place; the shots run at the start).
fn recovery_vantages() -> Vec<(&'static str, Vantage)> {
    vec![
        (
            "recovery_stern",
            Vantage::Eva {
                offset: Vec3::new(8.0, 4.0, 22.0),
                look_at: Vec3::new(-90.0, -25.0, 100.0),
                drift: Vec3::ZERO,
            },
        ),
        (
            "recovery_survivor",
            Vantage::NearPod {
                index: 0,
                offset: Vec3::new(-5.0, 2.0, 4.0),
            },
        ),
        (
            "recovery_lifeboat",
            Vantage::NearPod {
                index: 4,
                offset: Vec3::new(-7.0, 3.0, 6.0),
            },
        ),
    ]
}

/// The field's life (start in the field): a herd from off to its side and
/// from below (their hearts), then a limpet on its rock, from out of its
/// reach and then within it.
fn fauna_vantages() -> Vec<(&'static str, Vantage)> {
    let near = |limpet: bool, offset: Vec3| Vantage::NearFauna { limpet, offset };
    vec![
        ("fauna_herd", near(false, Vec3::new(14.0, 5.0, 10.0))),
        ("fauna_grazer_below", near(false, Vec3::new(3.0, -6.0, 2.5))),
        ("fauna_limpet_rock", near(true, Vec3::new(5.0, 3.0, 22.0))),
        ("fauna_limpet_strike", near(true, Vec3::new(4.0, 2.0, 9.0))),
    ]
}

/// The HUD tour, on our ship and (the `hud_carrier_*` shots) the carrier.
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
        ("hud_cargo", booted(Vec3::new(1.5, 0.01, 12.0), Vec3::NEG_Z, 0.0)),
        (
            "hud_carrier_eva",
            Vantage::Eva {
                offset: Vec3::new(30.0, 14.0, 75.0),
                look_at: Vec3::new(0.0, 3.0, 0.0),
                drift: Vec3::ZERO,
            },
        ),
        ("hud_carrier_console", booted(Vec3::new(0.0, upper + 0.01, -25.6), Vec3::NEG_Z, -0.38)),
        // By a loot crate: the pickup card.
        ("hud_carrier_crate", booted(Vec3::new(3.4, upper + 0.01, -27.4), Vec3::new(1.0, 0.0, -1.0), -0.7)),
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

/// The generated weapon of `frame` and `rarity` with the longest name of
/// the first few hundred seeds: the hand cards' worst case.
fn long_named(frame: crate::sim::weapons::GunKind, rarity: crate::sim::items::Rarity) -> crate::sim::items::Weapon {
    let mut weapon = (0..400u32)
        .map(|seed| crate::sim::items::Weapon::new(frame, seed, rarity))
        .max_by_key(|w| w.name().chars().count())
        .expect("seeds");
    weapon.mag = weapon.stats().mag;
    weapon
}

/// The state each HUD shot is in, set as the player arrives: generated
/// guns with long names, then low ammo, a dry gun, a reload, cargo, low
/// health, dying.
fn hud_state(name: &str, player: &mut Player, vitals: Option<&mut Vitals>) {
    use crate::sim::items::{AmmoKind, CargoKind, Rarity};
    use crate::sim::weapons::GunKind;
    if name.starts_with("hud_respawned") || name.starts_with("hud_lost") || name == "hud_debrief" {
        return;
    }
    let loadout = &mut player.loadout;
    loadout.restock();
    loadout.inventory.weapons[0] = Some(long_named(GunKind::Smg, Rarity::Exotic));
    loadout.inventory.weapons[1] = Some(long_named(GunKind::Rifle, Rarity::Rare));
    loadout.inventory.carrying = None;
    loadout.switching = 0.0;
    let Some(vitals) = vitals else { return };
    match name {
        "hud_hurt" => {
            // The rifle braced and low; hurt a moment ago.
            loadout.current = 2;
            if let Some(rifle) = &mut loadout.inventory.weapons[1] {
                rifle.mag = 6;
            }
            vitals.health = 55.0;
            vitals.since_hurt = 0.0;
        }
        "hud_critical" => {
            // The SMG dry; nearly dead, healing.
            loadout.current = 0;
            if let Some(smg) = &mut loadout.inventory.weapons[0] {
                smg.mag = 0;
            }
            *loadout.inventory.ammo_mut(AmmoKind::Light) = 0;
            vitals.health = 14.0;
            vitals.since_hurt = 4.55;
        }
        "hud_cargo" => {
            loadout.current = 0;
            loadout.inventory.carrying = Some(CargoKind::PowerCell);
        }
        "hud_carrier_console" => loadout.current = 1,
        "hud_carrier_dead" => {
            vitals.health = 0.0;
            vitals.since_hurt = 0.0;
        }
        _ => {}
    }
}

/// The escort's freighter (start in the escort).
fn freighter_vantages() -> Vec<(&'static str, Vantage)> {
    let booted = |feet: Vec3, forward: Vec3, pitch: f32| Vantage::Booted {
        feet,
        up: Vec3::Y,
        forward,
        pitch,
    };
    vec![
        (
            "freighter_outside",
            Vantage::Eva {
                offset: Vec3::new(16.0, 8.0, 34.0),
                look_at: Vec3::new(0.0, 1.5, -2.0),
                drift: Vec3::ZERO,
            },
        ),
        ("freighter_breach", booted(Vec3::new(0.5, 0.01, 4.0), Vec3::NEG_Z, -0.15)),
        ("freighter_gangway_lever", booted(Vec3::new(1.0, 0.01, -8.0), Vec3::new(0.6, 0.0, 1.0), 0.0)),
        ("freighter_bay_door", booted(Vec3::new(-1.0, 0.01, -9.0), Vec3::new(0.15, 0.0, -1.0), 0.0)),
        ("freighter_bay", booted(Vec3::new(0.0, 0.01, -14.8), Vec3::NEG_Z, -0.3)),
    ]
}

/// Our ship's power network (start in the boarding action): the board on
/// the bridge, and the junction and breakers in the hold.
fn power_vantages() -> Vec<(&'static str, Vantage)> {
    let booted = |feet: Vec3, forward: Vec3, pitch: f32| Vantage::Booted {
        feet,
        up: Vec3::Y,
        forward,
        pitch,
    };
    vec![
        ("power_board", booted(Vec3::new(1.4, 0.01, -9.0), Vec3::X, 0.0)),
        ("power_hold", booted(Vec3::new(-1.0, 0.01, 1.5), Vec3::new(-1.0, 0.0, 0.7), -0.05)),
        ("power_spawn", booted(Vec3::new(0.0, 0.01, 5.0), Vec3::NEG_Z, 0.0)),
        ("power_spawn_up", booted(Vec3::new(0.0, 0.01, 5.0), Vec3::NEG_Z, 0.7)),
        ("power_hold_aft", booted(Vec3::new(1.5, 0.01, 2.0), Vec3::new(-0.4, 0.0, 1.0), 0.35)),
        ("power_bridge", booted(Vec3::new(1.0, 0.01, -2.0), Vec3::new(-0.3, 0.0, -1.0), 0.25)),
    ]
}

/// Debug: routes the derelict's power to its vault, as a crew would have,
/// straight into the authority's state (the shots run on a host).
pub(crate) fn power_up(hull: &ShipHull, devices: &mut Devices) {
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
    mut vitals: Option<Single<&mut Vitals, With<super::LocalPlayer>>>,
    ships: Query<(Entity, &Transform, &ShipHull)>,
    mut devices: Query<(&ShipHull, &mut Devices)>,
    pods: Query<(&Pod, &Transform)>,
    fauna: (
        Query<(Entity, &Transform, &avian3d::prelude::LinearVelocity), With<crate::sim::fauna::Grazer>>,
        Query<(&crate::sim::bots::Robot, &Player), Without<super::LocalPlayer>>,
        Query<(&Transform, &avian3d::prelude::LinearVelocity), Without<crate::sim::fauna::Grazer>>,
    ),
    mut missions: Query<&mut Mission>,
    tick: Res<SimTick>,
    mut exit: MessageWriter<AppExit>,
) {
    let shots = vantages();
    let want = match tour() {
        "course" | "recovery" | "power" | "fauna" => HOME,
        // The HUD tour's `hud_carrier_*` shots are on the carrier.
        "hud" => match shots.get(script.next) {
            Some((name, _)) if name.starts_with("hud_carrier") => CARRIER,
            _ => HOME,
        },
        ship => ship,
    };
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
    if tour() == DERELICT && script.next == DERELICT_POWERED {
        for (hull, mut devices) in &mut devices {
            if hull.0 == DERELICT {
                power_up(hull, &mut devices);
            }
        }
    }
    // Who a vantage drifts with (flight assist holds still relative to it).
    let mut reference = None;
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
        Vantage::NearFauna { limpet, offset } => {
            let (grazers, robots, bodies) = &fauna;
            let home = ship_transform.translation;
            // Where it is and how it drifts: a grazer's own, a limpet's
            // rock's.
            let found = if limpet {
                robots
                    .iter()
                    .filter(|(k, _)| **k == crate::sim::bots::Robot::Limpet)
                    .filter_map(|(_, p)| match p.mode {
                        Mode::Booted { body, feet, .. } => {
                            let (t, v) = bodies.get(body).ok()?;
                            Some((t.transform_point(feet), v.0, body))
                        }
                        _ => None,
                    })
                    .min_by(|a, b| a.0.distance(home).total_cmp(&b.0.distance(home)))
            } else {
                grazers
                    .iter()
                    .map(|(e, t, v)| (t.translation, v.0, e))
                    .min_by(|a, b| a.0.distance(home).total_cmp(&b.0.distance(home)))
            };
            let Some((target, velocity, body)) = found else { return };
            reference = Some(body);
            let position = target + offset;
            Mode::Eva {
                position,
                velocity,
                orientation: Transform::from_translation(position).looking_at(target, Vec3::Y).rotation,
                pitch: 0.0,
            }
        }
        Vantage::NearPod { index, offset } => {
            let Some((_, pod)) = pods.iter().find(|(p, _)| p.index == index) else { return };
            let position = pod.translation + offset;
            Mode::Eva {
                position,
                velocity: Vec3::ZERO,
                orientation: Transform::from_translation(position).looking_at(pod.translation, Vec3::Y).rotation,
                pitch: 0.0,
            }
        }
    };
    if let Some(body) = reference {
        player.reference_body = Some(body);
    }
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
    if tour() == "hud" {
        let name = shots[script.next].0;
        hud_state(name, &mut player, vitals.as_mut().map(|v| &mut ***v));
        if name == "hud_lost"
            && let Ok(mut mission) = missions.single_mut()
        {
            mission.decide(Phase::Lost, tick.0, "SHIP LOST");
        }
    }
    script.next += 1;
}
