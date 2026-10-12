//! Writes a ship design's layout as JSON, the ground truth a model of the
//! ship is built on (its collision stays the design's boxes):
//!
//!   cargo run --bin layout_dump -- carrier > assets/ships/carrier/layout.json
//!
//! Ship coordinates, metres: +Y up, the bow toward −Z.

use bevy::prelude::*;
use hullbreakers::sim::SimMaterial;
use hullbreakers::sim::design::{design, designs};

fn v(v: Vec3) -> String {
    format!("[{:.4}, {:.4}, {:.4}]", v.x, v.y, v.z)
}

fn main() {
    let ids: Vec<&str> = designs().map(|d| d.id.as_str()).collect();
    let Some(id) = std::env::args().nth(1).filter(|a| ids.contains(&a.as_str())) else {
        eprintln!("usage: layout_dump <design> (one of {})", ids.join(", "));
        std::process::exit(2);
    };
    let ship = design(&id);
    let mut out = String::from("{\n");
    out += "  \"doc\": \"Ship coordinates, metres, +Y up, bow toward -Z. boxes: the collision (center, full size, rotation as an [x, y, z, w] quaternion, material: hull = structure, trim = fittings and furniture). Walls are boxes; doorways are the gaps between them. controls: buttons (center, half size). turrets: mount points. shield: ellipsoid center and radii. lights: the ceiling lights.\",\n";
    out += "  \"boxes\": [\n";
    let boxes: Vec<String> = ship
        .boxes
        .iter()
        .map(|b| {
            let (center, size, rotation, material) = (b.center, b.size, b.rotation, b.material);
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
    let controls: Vec<String> = ship
        .controls
        .iter()
        .map(|&(action, center, half)| {
            format!("    {{\"label\": \"{}\", \"center\": {}, \"half_size\": {}}}", action.label(), v(center), v(half))
        })
        .collect();
    out += &controls.join(",\n");
    out += "\n  ],\n";
    let mounts: Vec<String> = ship.turrets.iter().copied().map(v).collect();
    out += &format!("  \"turrets\": [{}],\n", mounts.join(", "));
    let lights: Vec<String> = ship.lights.iter().copied().map(v).collect();
    out += &format!("  \"lights\": [{}],\n", lights.join(", "));
    let (c, r) = (ship.shield.center, ship.shield.radii);
    out += &format!("  \"shield\": {{\"center\": {}, \"radii\": {}}}\n}}\n", v(c), v(r));
    print!("{out}");
}
