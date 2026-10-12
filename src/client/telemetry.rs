//! Debug: `HULLBREAKERS_TELEMETRY=<file>` writes the local player's view
//! and the mission's state to `<file>` about thirty times a second, as lines of
//! plain `key=value` fields, replaced whole each time. It's for
//! playing through the headless harness by hand (`tools/hl-aim` reads it to
//! turn a target into mouse motion): read-only, it changes nothing.

use std::path::PathBuf;

use bevy::prelude::*;

use super::LocalPlayer;
use super::camera::PlayerCamera;
use crate::sim::mission::{MarkedBodies, Mission};
use crate::sim::player::{Mode, Player};
use crate::sim::ship::ShipHull;
use crate::sim::vitals::Vitals;

#[derive(Resource)]
pub struct Telemetry {
    path: PathBuf,
    since: f32,
}

pub fn telemetry_from_env() -> Option<Telemetry> {
    let path = std::env::var_os("HULLBREAKERS_TELEMETRY")?;
    Some(Telemetry {
        path: path.into(),
        since: 0.0,
    })
}

fn v(p: Vec3) -> String {
    format!("{:.2},{:.2},{:.2}", p.x, p.y, p.z)
}

#[allow(clippy::too_many_arguments)]
pub fn write_telemetry(
    time: Res<Time>,
    mut telemetry: ResMut<Telemetry>,
    camera: Option<Single<&GlobalTransform, With<PlayerCamera>>>,
    player: Option<Single<(&Player, Option<&Vitals>), With<LocalPlayer>>>,
    mission: Option<Single<&Mission>>,
    hulls: Query<(&ShipHull, &GlobalTransform)>,
    poses: Query<&GlobalTransform>,
    robots: Query<(&crate::sim::bots::Robot, &avian3d::prelude::Position, Option<&Vitals>), With<Player>>,
    marked: MarkedBodies,
) {
    telemetry.since += time.delta_secs();
    if telemetry.since < 0.03 {
        return;
    }
    telemetry.since = 0.0;
    let (Some(camera), Some(player)) = (camera, player) else { return };
    let (player, vitals) = *player;
    let mut out = String::new();
    let (_, rotation, eye) = camera.to_scale_rotation_translation();
    out += &format!("t={:.2}\n", time.elapsed_secs());
    out += &format!("eye={}\n", v(eye));
    out += &format!("forward={}\n", v(rotation * Vec3::NEG_Z));
    out += &format!("up={}\n", v(rotation * Vec3::Y));
    out += &format!("right={}\n", v(rotation * Vec3::X));
    match player.mode {
        Mode::Booted { body, feet, up, .. } => {
            let design = hulls.get(body).map_or("?".to_string(), |(h, _)| h.0.clone());
            out += &format!("mode=booted on={design} feet={} up={}\n", v(feet), v(up));
        }
        Mode::Eva { velocity, .. } => out += &format!("mode=eva velocity={}\n", v(velocity)),
    }
    out += &format!("grapple={}\n", player.grapple.map_or("none".to_string(), |g| format!("{:.1}m", g.length)));
    out += &format!("carrying={:?}\n", player.loadout.inventory.carrying);
    out += &format!(
        "guns={}\n",
        player.loadout.inventory.weapons.iter().flatten().map(|w| w.name()).collect::<Vec<_>>().join("|")
    );
    if let Some(vitals) = vitals {
        out += &format!("health={:.0}\n", vitals.health);
    }
    if let Some(mission) = mission {
        out += &format!("mission={:?} phase={:?}\n", mission.kind, mission.phase);
        out += &format!("objective={}\n", mission.objective);
        for line in &mission.status {
            out += &format!("status={line}\n");
        }
        for m in &mission.markers {
            let world = match m.body {
                Some(id) => marked
                    .entity(id)
                    .and_then(|e| poses.get(e).ok().map(|t| t.transform_point(m.at))),
                None => Some(m.at),
            };
            if let Some(w) = world {
                out += &format!("marker={} @ {} ({:.0} m)\n", m.label, v(w), w.distance(eye));
            }
        }
    }
    for (kind, pose, vitals) in &robots {
        out += &format!(
            "robot={kind:?} @ {} ({:.0} m) health={:.0}\n",
            v(pose.0),
            pose.0.distance(eye),
            vitals.map_or(0.0, |v| v.health)
        );
    }
    for (hull, pose) in &hulls {
        let (_, r, p) = pose.to_scale_rotation_translation();
        out += &format!("ship={} pos={} rot={:.4},{:.4},{:.4},{:.4}\n", hull.0, v(p), r.x, r.y, r.z, r.w);
    }
    // Written beside it and renamed over it, so a reader never sees it half
    // written.
    let partial = telemetry.path.with_extension("partial");
    if std::fs::write(&partial, out).is_ok() {
        let _ = std::fs::rename(&partial, &telemetry.path);
    }
}
