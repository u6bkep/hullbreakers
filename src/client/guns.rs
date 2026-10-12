//! Crew guns, put together: a frame's receiver, with each of a weapon's
//! parts (see `sim::items::Parts`) on the receiver's socket for its slot,
//! and its muzzle device on the barrel's muzzle, in the weapon's paint.
//! The one way a crew gun is drawn: in first person (`viewmodel`), in
//! crewmates' hands (`crew`), and on pickups (`pickup_look`).
//!
//! The models are in `assets/weapons/<frame>_receiver` and
//! `assets/weapons/parts/<frame>_<slot>_<variant>` (built by
//! `assets/weapons/parts/gunparts.py`); which part is which file is here,
//! client side. A gun is spawned when what's shown changes, never per frame;
//! its parts are hung on its receiver's sockets once those have loaded. The
//! models stay loaded once used ([`GunModels`]), so a gun taken up again
//! doesn't wait for its files.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot};

use super::pickup_look::WeaponPaint;
use crate::sim::items::{Barrel, Magazine, Muzzle, Paint, PartVariant, Parts, Rarity, Sight, Stock, Weapon};
use crate::sim::weapons::GunKind;

/// What a crew gun looks like, as drawn: its frame, its parts, and its
/// paint (`None`: standard issue, in its models' own colours).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GunLook {
    pub frame: GunKind,
    pub parts: Parts,
    pub paint: Option<Paint>,
}

impl GunLook {
    pub fn of(weapon: &Weapon) -> GunLook {
        let look = weapon.look();
        GunLook {
            frame: look.frame,
            parts: look.parts,
            paint: (weapon.rarity != Rarity::Standard).then_some(look.paint),
        }
    }

    /// A frame with its standard parts, as modelled.
    pub fn issue(frame: GunKind) -> GunLook {
        GunLook {
            frame,
            parts: Parts::STANDARD,
            paint: None,
        }
    }
}

/// The frames that have models (crew guns; robots' guns are built into
/// them).
fn frame_tag(frame: GunKind) -> Option<&'static str> {
    match frame {
        GunKind::Smg => Some("smg"),
        GunKind::Rifle => Some("rifle"),
        GunKind::GunnerRifle | GunKind::SwarmerSprayer | GunKind::MarksmanRifle | GunKind::TurretCannon => None,
    }
}

/// A frame's receiver model, if it has one.
pub fn receiver_model(frame: GunKind) -> Option<String> {
    frame_tag(frame).map(|f| format!("weapons/{f}_receiver/{f}_receiver.glb"))
}

/// Which slot a part fits, and its variant, as the part models name them.
fn part_names(part: PartVariant) -> (&'static str, Option<&'static str>) {
    match part {
        PartVariant::Barrel(b) => (
            "barrel",
            Some(match b {
                Barrel::Standard => "standard",
                Barrel::Long => "long",
                Barrel::Short => "short",
                Barrel::Heavy => "heavy",
            }),
        ),
        PartVariant::Magazine(m) => (
            "mag",
            Some(match m {
                Magazine::Standard => "standard",
                Magazine::Extended => "extended",
                Magazine::Drum => "drum",
                Magazine::Quick => "quick",
            }),
        ),
        PartVariant::Stock(s) => (
            "stock",
            Some(match s {
                Stock::Standard => "standard",
                Stock::Light => "light",
                Stock::Heavy => "heavy",
            }),
        ),
        PartVariant::Sight(s) => (
            "sight",
            Some(match s {
                Sight::Iron => "iron",
                Sight::Reflex => "reflex",
                Sight::Scope => "scope",
            }),
        ),
        PartVariant::Muzzle(m) => (
            "muzzle",
            match m {
                // A bare muzzle is the barrel's own crown: nothing on it.
                Muzzle::Bare => None,
                Muzzle::Compensator => Some("compensator"),
                Muzzle::Booster => Some("booster"),
            },
        ),
    }
}

/// A part's model on a frame, if it has one (a bare muzzle has none).
pub fn part_model(frame: GunKind, part: PartVariant) -> Option<String> {
    let f = frame_tag(frame)?;
    let (slot, variant) = part_names(part);
    let name = format!("{f}_{slot}_{}", variant?);
    Some(format!("weapons/parts/{name}/{name}.glb"))
}

/// The receiver socket a part hangs on (a muzzle device hangs on the
/// barrel's `SOCKET_muzzle` instead).
fn socket_of(part: PartVariant) -> &'static str {
    match part {
        PartVariant::Barrel(_) => "SOCKET_barrel",
        PartVariant::Magazine(_) => "SOCKET_mag",
        PartVariant::Stock(_) => "SOCKET_stock",
        PartVariant::Sight(_) => "SOCKET_sight",
        PartVariant::Muzzle(_) => "SOCKET_muzzle",
    }
}

/// Every part variant, by slot (for the gallery's racks and the tests).
/// [`variant_index`] keeps it complete: a new variant doesn't compile
/// until it has a place here.
pub const VARIANTS: [PartVariant; 16] = [
    PartVariant::Barrel(Barrel::Standard),
    PartVariant::Barrel(Barrel::Long),
    PartVariant::Barrel(Barrel::Short),
    PartVariant::Barrel(Barrel::Heavy),
    PartVariant::Magazine(Magazine::Standard),
    PartVariant::Magazine(Magazine::Extended),
    PartVariant::Magazine(Magazine::Drum),
    PartVariant::Magazine(Magazine::Quick),
    PartVariant::Stock(Stock::Standard),
    PartVariant::Stock(Stock::Light),
    PartVariant::Stock(Stock::Heavy),
    PartVariant::Sight(Sight::Iron),
    PartVariant::Sight(Sight::Reflex),
    PartVariant::Sight(Sight::Scope),
    PartVariant::Muzzle(Muzzle::Compensator),
    PartVariant::Muzzle(Muzzle::Booster),
];

/// A variant's place in [`VARIANTS`] (`None`: a bare muzzle, which isn't a
/// part to show).
pub fn variant_index(part: PartVariant) -> Option<usize> {
    Some(match part {
        PartVariant::Barrel(Barrel::Standard) => 0,
        PartVariant::Barrel(Barrel::Long) => 1,
        PartVariant::Barrel(Barrel::Short) => 2,
        PartVariant::Barrel(Barrel::Heavy) => 3,
        PartVariant::Magazine(Magazine::Standard) => 4,
        PartVariant::Magazine(Magazine::Extended) => 5,
        PartVariant::Magazine(Magazine::Drum) => 6,
        PartVariant::Magazine(Magazine::Quick) => 7,
        PartVariant::Stock(Stock::Standard) => 8,
        PartVariant::Stock(Stock::Light) => 9,
        PartVariant::Stock(Stock::Heavy) => 10,
        PartVariant::Sight(Sight::Iron) => 11,
        PartVariant::Sight(Sight::Reflex) => 12,
        PartVariant::Sight(Sight::Scope) => 13,
        PartVariant::Muzzle(Muzzle::Compensator) => 14,
        PartVariant::Muzzle(Muzzle::Booster) => 15,
        PartVariant::Muzzle(Muzzle::Bare) => return None,
    })
}

/// The gun models loaded so far, kept so they stay loaded.
#[derive(Resource, Default)]
pub struct GunModels(HashMap<String, Handle<WorldAsset>>);

impl GunModels {
    pub fn get(&mut self, assets: &AssetServer, path: &str) -> Handle<WorldAsset> {
        self.0
            .entry(path.to_string())
            .or_insert_with(|| assets.load(GltfAssetLabel::Scene(0).from_asset(path.to_string())))
            .clone()
    }
}

/// A gun being put together: on its receiver's root, until every part is
/// on.
#[derive(Component)]
pub struct GunAssembly {
    look: GunLook,
    /// The barrel's model, once hung (its muzzle device goes on it).
    barrel: Option<Entity>,
    /// The receiver's parts are on; the muzzle device is on.
    parts_on: bool,
    done: bool,
}

/// A part of an assembled gun (under its receiver).
#[derive(Component)]
pub struct GunPart;

/// Spawns a gun that looks like `look`, with `extra` (where it is, what
/// it's part of), and returns it, or `None` for a frame with no model. Its
/// origin is its grip. Its parts follow once its receiver has loaded (see
/// [`assemble_guns`]).
pub fn spawn_gun(commands: &mut Commands, models: &mut GunModels, assets: &AssetServer, look: GunLook, extra: impl Bundle) -> Option<Entity> {
    let receiver = receiver_model(look.frame)?;
    let mut gun = commands.spawn((
        Name::new("Gun"),
        GunAssembly {
            look,
            barrel: None,
            parts_on: false,
            done: false,
        },
        WorldAssetRoot(models.get(assets, &receiver)),
        extra,
    ));
    if let Some(paint) = look.paint {
        gun.insert(WeaponPaint(paint));
    }
    Some(gun.id())
}

/// Hangs each gun's parts on its receiver's sockets once the receiver has
/// loaded, then its muzzle device on its barrel's muzzle once the barrel
/// has.
pub fn assemble_guns(
    assets: Res<AssetServer>,
    mut models: ResMut<GunModels>,
    mut guns: Query<(Entity, &mut GunAssembly)>,
    children: Query<&Children>,
    names: Query<&Name>,
    mut commands: Commands,
) {
    let hang = |commands: &mut Commands, models: &mut GunModels, path: String, socket: Entity| {
        commands
            .spawn((GunPart, WorldAssetRoot(models.get(&assets, &path)), Transform::default(), ChildOf(socket)))
            .id()
    };
    let find = |root: Entity, name: &str| children.iter_descendants(root).find(|e| names.get(*e).is_ok_and(|n| n.as_str() == name));
    for (gun, mut assembly) in &mut guns {
        if assembly.done {
            continue;
        }
        let look = assembly.look;
        let parts = look.parts.all();
        if !assembly.parts_on {
            // The receiver's sockets, once it's in.
            let sockets: Vec<(PartVariant, Option<Entity>)> = parts
                .iter()
                .filter(|p| !matches!(p, PartVariant::Muzzle(_)))
                .map(|p| (*p, find(gun, socket_of(*p))))
                .collect();
            if sockets.iter().any(|(_, s)| s.is_none()) {
                continue;
            }
            for (part, socket) in sockets {
                let (Some(path), Some(socket)) = (part_model(look.frame, part), socket) else { continue };
                let model = hang(&mut commands, &mut models, path, socket);
                if matches!(part, PartVariant::Barrel(_)) {
                    assembly.barrel = Some(model);
                }
            }
            assembly.parts_on = true;
            continue;
        }
        // The muzzle device, on the barrel once that's in.
        let muzzle = parts.into_iter().find(|p| matches!(p, PartVariant::Muzzle(_)));
        match muzzle.and_then(|m| part_model(look.frame, m)) {
            None => assembly.done = true,
            Some(path) => {
                let Some(socket) = assembly.barrel.and_then(|b| find(b, "SOCKET_muzzle")) else { continue };
                hang(&mut commands, &mut models, path, socket);
                assembly.done = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::gallery::asset_root;

    /// The JSON of a `.glb` (its first chunk).
    fn gltf_json(path: &std::path::Path) -> String {
        let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(&bytes[0..4], b"glTF", "{} isn't a glb", path.display());
        let len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        assert_eq!(&bytes[16..20], b"JSON");
        String::from_utf8(bytes[20..20 + len].to_vec()).unwrap()
    }

    fn has_node(json: &str, name: &str) -> bool {
        json.contains(&format!("\"name\":\"{name}\""))
    }

    #[test]
    fn every_variant_is_listed_once() {
        for (i, part) in VARIANTS.iter().enumerate() {
            assert_eq!(variant_index(*part), Some(i), "{part:?}");
        }
    }

    #[test]
    fn every_part_of_every_crew_frame_has_a_model_with_its_sockets() {
        let root = asset_root();
        for frame in GunKind::CREW {
            let receiver = gltf_json(&root.join(receiver_model(frame).unwrap()));
            for socket in ["SOCKET_grip", "SOCKET_barrel", "SOCKET_mag", "SOCKET_stock", "SOCKET_sight"] {
                assert!(has_node(&receiver, socket), "{frame:?} receiver has no {socket}");
            }
            if frame == GunKind::Rifle {
                // Where the off hand braces it (see `viewmodel`).
                assert!(has_node(&receiver, "SOCKET_foregrip"));
            }
            for part in VARIANTS {
                let path = part_model(frame, part).unwrap_or_else(|| panic!("{frame:?} {part:?}: no model"));
                let json = gltf_json(&root.join(&path));
                if matches!(part, PartVariant::Barrel(_)) {
                    assert!(has_node(&json, "SOCKET_muzzle"), "{path} has no SOCKET_muzzle");
                }
            }
            assert!(part_model(frame, PartVariant::Muzzle(Muzzle::Bare)).is_none());
        }
    }
}
