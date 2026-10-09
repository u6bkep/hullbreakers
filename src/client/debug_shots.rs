//! Scripted screenshots for checking visuals without a human at the controls.
//!
//! `SPACE_GAME_SHOTS=<dir> cargo run` teleports the player through a few
//! vantage points, saves a screenshot at each, then exits.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::sim::player::{Grapple, Mode, Player};
use crate::sim::ship::Ship;

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
    let dir = std::env::var_os("SPACE_GAME_SHOTS")?;
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

fn vantages() -> Vec<(&'static str, Vantage)> {
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

const SETTLE_SECONDS: f32 = 1.5;

pub fn run_shot_script(
    time: Res<Time>,
    mut script: ResMut<ShotScript>,
    mut commands: Commands,
    mut player: Single<&mut Player>,
    ship: Single<(Entity, &Transform), With<Ship>>,
    mut exit: MessageWriter<AppExit>,
) {
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
    let (ship_entity, ship_transform) = *ship;
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
            body: ship_entity,
            anchor,
            length: position.distance(anchor_w) + slack,
            tension: 0.0,
        });
    } else {
        player.grapple = None;
    }
    // Keep the scripted drift so the prograde marker is visible.
    player.flight_assist = false;
    player.discontinuity += 1;
    script.next += 1;
}
