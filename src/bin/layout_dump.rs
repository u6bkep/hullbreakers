//! Writes a hull kind's layout as JSON, the ground truth a model of the
//! ship is built on (its collision stays the code's boxes):
//!
//!   cargo run --bin layout_dump -- carrier > assets/ships/carrier/layout.json
//!
//! Ship coordinates, metres: +Y up, the bow toward −Z.

use bevy::prelude::*;
use hullbreakers::sim::SimMaterial;
use hullbreakers::sim::ship::{HullKind, controls, hull_pieces};

fn v(v: Vec3) -> String {
    format!("[{:.4}, {:.4}, {:.4}]", v.x, v.y, v.z)
}

fn main() {
    let kind = match std::env::args().nth(1).as_deref() {
        Some("carrier") => HullKind::Carrier,
        Some("home") => HullKind::Home,
        other => {
            eprintln!("usage: layout_dump carrier|home (got {other:?})");
            std::process::exit(2);
        }
    };
    let mut out = String::from("{\n");
    out += "  \"doc\": \"Ship coordinates, metres, +Y up, bow toward -Z. boxes: the collision (center, full size, rotation as an [x, y, z, w] quaternion, material: hull = structure, trim = fittings and furniture). Walls are boxes; doorways are the gaps between them. controls: buttons (center, half size). turrets: mount points. shield: ellipsoid center and radii.\",\n";
    out += "  \"boxes\": [\n";
    let boxes: Vec<String> = hull_pieces(kind)
        .into_iter()
        .map(|(center, size, rotation, material)| {
            let material = match material {
                SimMaterial::Trim => "trim",
                SimMaterial::Hull | SimMaterial::HostileHull => "hull",
                SimMaterial::Rock => "rock",
            };
            format!(
                "    {{\"center\": {}, \"size\": {}, \"rotation\": [{:.5}, {:.5}, {:.5}, {:.5}], \"material\": \"{material}\"}}",
                v(center),
                v(size),
                rotation.x,
                rotation.y,
                rotation.z,
                rotation.w
            )
        })
        .collect();
    out += &boxes.join(",\n");
    out += "\n  ],\n  \"controls\": [\n";
    let controls: Vec<String> = controls(kind)
        .into_iter()
        .map(|(action, center, half)| {
            format!("    {{\"label\": \"{}\", \"center\": {}, \"half_size\": {}}}", action.label(), v(center), v(half))
        })
        .collect();
    out += &controls.join(",\n");
    out += "\n  ],\n";
    let mounts: Vec<String> = kind.turret_mounts().into_iter().map(v).collect();
    out += &format!("  \"turrets\": [{}],\n", mounts.join(", "));
    let (c, r) = kind.shield();
    out += &format!("  \"shield\": {{\"center\": {}, \"radii\": {}}}\n}}\n", v(c), v(r));
    print!("{out}");
}
