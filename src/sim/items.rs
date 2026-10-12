//! Items: what players carry, pick up, drop and loot.
//!
//! An [`Item`] is a weapon, loose ammo, or cargo (mission items). A
//! [`Weapon`] is a frame (a [`GunKind`]: the archetype, with its base
//! stats), a seed, a rarity, and the rounds in its mag. Everything else
//! about it (its maker, its parts, its perks, its stats, its name, its look)
//! derives from those three numbers by integer hashing, so a weapon costs a
//! few bytes in the predicted player state and on the wire, and every peer
//! agrees on it bit for bit.
//!
//! Generation trades, it doesn't only inflate: every part variant gives
//! something and takes something (a long barrel shoots faster, tighter
//! bullets but fires slower; a drum holds more but reloads slower; a light
//! stock switches faster but sprays worse unbraced). Makers lean their guns
//! a way of their own and favour the parts that suit it. Rarity adds
//! quality (a little more damage, extra boosts) and, from rare up, perks.
//!
//! What a player carries is an [`Inventory`]: a few weapon slots (which the
//! stances point at by position), a pack for spares, ammo pools by kind (a
//! reload draws from the pool), and bulky cargo in both hands.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::weapons::{GunKind, GunStats};

// ---------------------------------------------------------------------------
// Hashing: every roll is an integer function of the seed.

fn mix(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

/// The `salt`th number drawn from `seed`. The seed is hashed before the
/// salt goes in, so two seeds don't differ the same way in every draw (as
/// `mix(seed ^ salt)` would have them: their names came out alike).
pub fn draw(seed: u32, salt: u32) -> u32 {
    mix(mix(seed).wrapping_add(mix(salt.wrapping_mul(0x9e37_79b9).wrapping_add(0x632b_e5ab))))
}

/// One of `options` by weight, chosen by `r`.
fn pick<T: Copy>(r: u32, options: &[(T, u32)]) -> T {
    let total: u32 = options.iter().map(|o| o.1).sum();
    let mut at = r % total.max(1);
    for &(value, weight) in options {
        if at < weight {
            return value;
        }
        at -= weight;
    }
    options[0].0
}

// ---------------------------------------------------------------------------
// Rarity, ammo, cargo.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Rarity {
    /// Issued from the weapons locker (and robots' own guns): base stats,
    /// no generation.
    Standard,
    Common,
    Uncommon,
    Rare,
    Exotic,
}

impl Rarity {
    pub fn name(self) -> &'static str {
        match self {
            Rarity::Standard => "Standard issue",
            Rarity::Common => "Common",
            Rarity::Uncommon => "Uncommon",
            Rarity::Rare => "Rare",
            Rarity::Exotic => "Exotic",
        }
    }

    /// Its colour (sRGB). Grey, white, green, blue, orange: no purples,
    /// which would sit too near the weak spots' magenta.
    pub fn color(self) -> [u8; 3] {
        match self {
            Rarity::Standard => [150, 160, 170],
            Rarity::Common => [232, 234, 236],
            Rarity::Uncommon => [100, 214, 100],
            Rarity::Rare => [80, 165, 255],
            Rarity::Exotic => [255, 165, 40],
        }
    }

    fn tier(self) -> u32 {
        self as u32
    }

    const ALL: [Rarity; 5] = [Rarity::Standard, Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::Exotic];

    /// The next tier up, if there is one (standard issue has none).
    pub fn next(self) -> Option<Rarity> {
        match self {
            Rarity::Standard | Rarity::Exotic => None,
            r => Some(Rarity::ALL[r as usize + 1]),
        }
    }

    /// Perks it carries.
    fn perks(self) -> usize {
        match self {
            Rarity::Rare => 1,
            Rarity::Exotic => 2,
            _ => 0,
        }
    }

    /// Extra damage (percent) and boosted stats.
    fn quality(self) -> (i32, usize) {
        match self {
            Rarity::Standard | Rarity::Common => (0, 0),
            Rarity::Uncommon => (4, 1),
            Rarity::Rare => (8, 1),
            Rarity::Exotic => (12, 2),
        }
    }

    /// A rarity, by `r`; `luck` (0–100) shifts the odds toward rarer.
    pub fn roll(r: u32, luck: u32) -> Rarity {
        let l = luck.min(100);
        pick(
            r,
            &[
                (Rarity::Common, 60 - 45 * l / 100),
                (Rarity::Uncommon, 28 + 7 * l / 100),
                (Rarity::Rare, 10 + 25 * l / 100),
                (Rarity::Exotic, 2 + 13 * l / 100),
            ],
        )
    }
}

/// Kinds of ammunition: each frame takes one, and the inventory keeps a
/// pool of each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect, Serialize, Deserialize)]
pub enum AmmoKind {
    /// SMG rounds.
    Light,
    /// Rifle and sniper rounds.
    Heavy,
    /// Shotgun shells.
    Shells,
    /// Energy cells: plasma and arc guns.
    Cells,
}

impl AmmoKind {
    pub const ALL: [AmmoKind; 4] = [AmmoKind::Light, AmmoKind::Heavy, AmmoKind::Shells, AmmoKind::Cells];
    pub const COUNT: usize = Self::ALL.len();

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            AmmoKind::Light => "Light rounds",
            AmmoKind::Heavy => "Heavy rounds",
            AmmoKind::Shells => "Shells",
            AmmoKind::Cells => "Energy cells",
        }
    }

    /// The most a pool holds.
    pub fn cap(self) -> u16 {
        match self {
            AmmoKind::Light => 400,
            AmmoKind::Heavy => 200,
            AmmoKind::Shells => 60,
            AmmoKind::Cells => 240,
        }
    }

    /// A box of it: what a robot drops, and what dropping ammo leaves.
    pub fn box_rounds(self) -> u16 {
        match self {
            AmmoKind::Light => 60,
            AmmoKind::Heavy => 30,
            AmmoKind::Shells => 12,
            AmmoKind::Cells => 40,
        }
    }
}

/// Of loot that's a gun, how much is one of the frames that came later.
const NEW_FRAME_CHANCE: u32 = 35;
/// Of SMGs and rifles found, how many are of a family other than ballistic.
const FAMILY_CHANCE: u32 = 35;

/// How a gun acts on the world: what it fires and what that does when it
/// gets there (see `docs/plan-weapons.md`). Rolled on the SMG and rifle
/// frames; the others are ballistic.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Reflect, Serialize, Deserialize)]
pub enum Family {
    /// Bullets at real speeds.
    #[default]
    Ballistic,
    /// Slow, heavy bolts that burst: splash and a shove.
    Plasma,
    /// Instant and short; jumps between nearby targets in sight of each other.
    Arc,
    /// A continuous beam; heat instead of ammo.
    Laser,
}

impl Family {
    pub const ROLLED: [Family; 3] = [Family::Plasma, Family::Arc, Family::Laser];

    pub fn name(self) -> &'static str {
        match self {
            Family::Ballistic => "Ballistic",
            Family::Plasma => "Plasma",
            Family::Arc => "Arc",
            Family::Laser => "Laser",
        }
    }

    /// What it does differently, for the screens.
    pub fn effect(self) -> &'static str {
        match self {
            Family::Ballistic => "Bullets",
            Family::Plasma => "Slow bolts that burst: splash and a shove; cells",
            Family::Arc => "Short and instant; jumps to two more targets in sight; cells",
            Family::Laser => "A beam while held; heat, not ammo",
        }
    }

    /// A frame's stats for this family (after its parts and rolls).
    fn apply(self, s: GunStats) -> GunStats {
        match self {
            Family::Ballistic => s,
            Family::Plasma => GunStats {
                muzzle_speed: s.muzzle_speed * 0.12,
                bullet_mass: s.bullet_mass * 6.0 / 0.12,
                damage: s.damage * 1.6,
                rate: s.rate * 0.5,
                splash: 2.5,
                ammo: AmmoKind::Cells,
                ..s
            },
            Family::Arc => GunStats {
                damage: s.damage * 0.9,
                rate: s.rate * 0.6,
                chain: 2,
                range: 35.0,
                bullet_mass: 0.0,
                spread: s.spread_braced,
                ammo: AmmoKind::Cells,
                ..s
            },
            Family::Laser => GunStats {
                // A beam: a hit every tick it's held.
                rate: crate::sim::TICK_HZ as f32,
                damage: s.damage * s.rate / crate::sim::TICK_HZ as f32 * 0.8,
                spread: 0.0,
                spread_braced: 0.0,
                range: 400.0,
                bullet_mass: 0.0,
                // Full in about four seconds of beam.
                heat: 1.0 / (4.0 * crate::sim::TICK_HZ as f32),
                mag: 0,
                ..s
            },
        }
    }
}

/// Mission items.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CargoKind {
    /// Bulky: carried in both hands (no shooting, no grapple).
    PowerCell,
    /// Small: goes in the pack.
    DataCore,
    /// Small: goes in the pack.
    BlackBox,
}

impl CargoKind {
    pub fn name(self) -> &'static str {
        match self {
            CargoKind::PowerCell => "Power cell",
            CargoKind::DataCore => "Data core",
            CargoKind::BlackBox => "Black box",
        }
    }

    /// Whether it takes both hands to carry.
    pub fn bulky(self) -> bool {
        matches!(self, CargoKind::PowerCell)
    }
}

// ---------------------------------------------------------------------------
// Makers and parts.

/// Who made a gun: a character, a lean in its stats, and parts it favours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Maker {
    /// The crew's own supplier: standard issue, no lean.
    Issue,
    Kessler,
    Brakk,
    Hoshi,
    Corvid,
}

impl Maker {
    const ROLLED: [Maker; 4] = [Maker::Kessler, Maker::Brakk, Maker::Hoshi, Maker::Corvid];

    pub fn name(self) -> &'static str {
        match self {
            Maker::Issue => "Issue",
            Maker::Kessler => "Kessler",
            Maker::Brakk => "Brakk",
            Maker::Hoshi => "Hoshi",
            Maker::Corvid => "Corvid",
        }
    }

    /// What it calls its models: the word between its name and the
    /// epithet, from a mark `n` (1–6).
    fn model(self, frame: GunKind, n: u32) -> String {
        // Each frame its own line (the SMG's is what any other frame had
        // before it had its own).
        let line = |smg, rifle, shotgun, sniper| match frame {
            GunKind::Rifle => rifle,
            GunKind::Shotgun => shotgun,
            GunKind::Sniper => sniper,
            _ => smg,
        };
        match self {
            Maker::Issue => String::new(),
            // Precise, numbered, German-sounding.
            Maker::Kessler => format!("{}{n}", line("MP-", "KG-", "FL-", "ZG-")),
            // Industrial: a series number.
            Maker::Brakk => format!("{}{}0", line("Grind ", "Ram ", "Maul ", "Lance "), n + 1),
            // Sleek: a type.
            Maker::Hoshi => format!("Type {}", ["I", "II", "III", "IV", "V", "VI"][(n as usize - 1).min(5)]),
            // Rebuilt: how many times.
            Maker::Corvid => format!("Scrap #{n}"),
        }
    }

    /// Words for its guns when no part or perk names them.
    fn epithets(self) -> &'static [&'static str] {
        match self {
            Maker::Issue => &[""],
            Maker::Kessler => &["Exacta", "Ordnung", "Gauge"],
            Maker::Brakk => &["Workhorse", "Lump", "Slab"],
            Maker::Hoshi => &["Breeze", "Petal", "Origami"],
            Maker::Corvid => &["Scavenger", "Leftovers", "Mongrel"],
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            Maker::Issue => "Standard issue from the ship's locker. Nothing fancy.",
            Maker::Kessler => "Kessler Arms: precision work. Fast, true rounds; a slower trigger.",
            Maker::Brakk => "Brakk Industrial: heavy and crude. Big rounds, big kick, slow hands.",
            Maker::Hoshi => "Hoshi Dynamics: light and quick. Fires fast, switches faster, hits lighter.",
            Maker::Corvid => "Corvid Salvage: scavenged and rebuilt. Deep magazines, loose tolerances.",
        }
    }

    fn lean(self) -> Mods {
        match self {
            Maker::Issue => Mods::default(),
            Maker::Kessler => Mods { speed: 10, cone: -15, rate: -8, ..default() },
            Maker::Brakk => Mods { damage: 14, mass: 30, rate: -10, reload: 15, cone: 10, ..default() },
            Maker::Hoshi => Mods { rate: 12, switch: -20, damage: -8, ..default() },
            Maker::Corvid => Mods { mag: 25, cone: 12, damage: -4, ..default() },
        }
    }

    /// The paint jobs it sells: (body, accent), sRGB.
    fn palette(self) -> &'static [([u8; 3], [u8; 3])] {
        match self {
            Maker::Issue => &[([196, 108, 44], [108, 237, 255])],
            Maker::Kessler => &[
                ([70, 86, 104], [220, 226, 232]),
                ([38, 44, 52], [200, 170, 90]),
                ([186, 190, 184], [40, 60, 90]),
            ],
            Maker::Brakk => &[
                ([214, 168, 40], [30, 30, 30]),
                ([200, 96, 30], [40, 40, 40]),
                ([98, 104, 60], [200, 190, 150]),
            ],
            Maker::Hoshi => &[
                ([232, 234, 236], [60, 200, 230]),
                ([190, 40, 40], [240, 240, 240]),
                ([40, 150, 140], [240, 240, 230]),
            ],
            Maker::Corvid => &[
                ([130, 70, 40], [160, 160, 150]),
                ([110, 120, 100], [200, 120, 40]),
                ([90, 90, 96], [210, 180, 60]),
            ],
        }
    }

    /// How much more often it fits a part (a multiplier on its weight).
    fn favours(self, part: PartVariant) -> u32 {
        use PartVariant as P;
        let liked: &[PartVariant] = match self {
            Maker::Issue => &[],
            Maker::Kessler => &[P::Barrel(Barrel::Long), P::Sight(Sight::Scope), P::Muzzle(Muzzle::Compensator)],
            Maker::Brakk => &[P::Barrel(Barrel::Heavy), P::Magazine(Magazine::Drum), P::Stock(Stock::Heavy)],
            Maker::Hoshi => &[
                P::Barrel(Barrel::Short),
                P::Stock(Stock::Light),
                P::Magazine(Magazine::Quick),
                P::Sight(Sight::Reflex),
            ],
            Maker::Corvid => &[P::Magazine(Magazine::Extended), P::Magazine(Magazine::Drum), P::Muzzle(Muzzle::Booster)],
        };
        if liked.contains(&part) { 3 } else { 1 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Barrel {
    Standard,
    Long,
    Short,
    Heavy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Magazine {
    Standard,
    Extended,
    Drum,
    Quick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Stock {
    Standard,
    Light,
    Heavy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Sight {
    Iron,
    Reflex,
    Scope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Muzzle {
    Bare,
    Compensator,
    Booster,
}

/// One variant per part slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Parts {
    pub barrel: Barrel,
    pub magazine: Magazine,
    pub stock: Stock,
    pub sight: Sight,
    pub muzzle: Muzzle,
}

impl Parts {
    pub const STANDARD: Parts = Parts {
        barrel: Barrel::Standard,
        magazine: Magazine::Standard,
        stock: Stock::Standard,
        sight: Sight::Iron,
        muzzle: Muzzle::Bare,
    };

    /// From variants in [`SLOTS`] order.
    fn from_slots(p: [PartVariant; 5]) -> Parts {
        match p {
            [PartVariant::Barrel(barrel), PartVariant::Magazine(magazine), PartVariant::Stock(stock), PartVariant::Sight(sight), PartVariant::Muzzle(muzzle)] => {
                Parts { barrel, magazine, stock, sight, muzzle }
            }
            _ => unreachable!("parts out of their slots"),
        }
    }

    /// Every slot's variant, in [`SLOTS`] order.
    pub fn slots(&self) -> [PartVariant; 5] {
        [
            PartVariant::Barrel(self.barrel),
            PartVariant::Magazine(self.magazine),
            PartVariant::Stock(self.stock),
            PartVariant::Sight(self.sight),
            PartVariant::Muzzle(self.muzzle),
        ]
    }

    /// Every slot's variant, barrel first (in the order names are drawn
    /// from: see [`Weapon::name`]).
    pub fn all(&self) -> [PartVariant; 5] {
        [
            PartVariant::Barrel(self.barrel),
            PartVariant::Magazine(self.magazine),
            PartVariant::Muzzle(self.muzzle),
            PartVariant::Stock(self.stock),
            PartVariant::Sight(self.sight),
        ]
    }
}

/// A part, whichever slot it fits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PartVariant {
    Barrel(Barrel),
    Magazine(Magazine),
    Stock(Stock),
    Sight(Sight),
    Muzzle(Muzzle),
}

impl PartVariant {
    pub fn slot(self) -> &'static str {
        match self {
            PartVariant::Barrel(_) => "Barrel",
            PartVariant::Magazine(_) => "Magazine",
            PartVariant::Stock(_) => "Stock",
            PartVariant::Sight(_) => "Sight",
            PartVariant::Muzzle(_) => "Muzzle",
        }
    }

    pub fn standard(self) -> bool {
        matches!(
            self,
            PartVariant::Barrel(Barrel::Standard)
                | PartVariant::Magazine(Magazine::Standard)
                | PartVariant::Stock(Stock::Standard)
                | PartVariant::Sight(Sight::Iron)
                | PartVariant::Muzzle(Muzzle::Bare)
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            PartVariant::Barrel(b) => match b {
                Barrel::Standard => "Standard barrel",
                Barrel::Long => "Long barrel",
                Barrel::Short => "Short barrel",
                Barrel::Heavy => "Heavy barrel",
            },
            PartVariant::Magazine(m) => match m {
                Magazine::Standard => "Standard magazine",
                Magazine::Extended => "Extended magazine",
                Magazine::Drum => "Drum magazine",
                Magazine::Quick => "Quick-change magazine",
            },
            PartVariant::Stock(s) => match s {
                Stock::Standard => "Standard stock",
                Stock::Light => "Light stock",
                Stock::Heavy => "Heavy stock",
            },
            PartVariant::Sight(s) => match s {
                Sight::Iron => "Iron sights",
                Sight::Reflex => "Reflex sight",
                Sight::Scope => "Scope",
            },
            PartVariant::Muzzle(m) => match m {
                Muzzle::Bare => "Bare muzzle",
                Muzzle::Compensator => "Compensator",
                Muzzle::Booster => "Booster",
            },
        }
    }

    /// What it does, in a few words.
    pub fn effect(self) -> &'static str {
        match self {
            PartVariant::Barrel(b) => match b {
                Barrel::Standard => "",
                Barrel::Long => "faster, tighter shots; slower rate",
                Barrel::Short => "faster rate and switch; slower, wider shots",
                Barrel::Heavy => "harder hits; slower rate, more kick",
            },
            PartVariant::Magazine(m) => match m {
                Magazine::Standard => "",
                Magazine::Extended => "bigger mag; slower reload",
                Magazine::Drum => "twice the mag; much slower reload and switch",
                Magazine::Quick => "fast reload; smaller mag",
            },
            PartVariant::Stock(s) => match s {
                Stock::Standard => "",
                Stock::Light => "fast switch; wilder unbraced",
                Stock::Heavy => "steadier; slow switch",
            },
            PartVariant::Sight(s) => match s {
                Sight::Iron => "",
                Sight::Reflex => "better unbraced; a touch worse braced",
                Sight::Scope => "much tighter braced; worse unbraced",
            },
            PartVariant::Muzzle(m) => match m {
                Muzzle::Bare => "",
                Muzzle::Compensator => "tighter cone; slightly slower shots",
                Muzzle::Booster => "faster, harder shots; wider cone",
            },
        }
    }

    /// Words a gun may be named for when it has this part, by frame (an
    /// SMG's long barrel is a lance, a rifle's a long shot). Empty for a
    /// stock part.
    fn epithets(self, frame: GunKind) -> &'static [&'static str] {
        let rifle = frame == GunKind::Rifle;
        match self {
            PartVariant::Barrel(Barrel::Long) if rifle => &["Longshot", "Reach", "Lancer"],
            PartVariant::Barrel(Barrel::Long) => &["Needle", "Pike", "Stretch"],
            PartVariant::Barrel(Barrel::Short) if rifle => &["Snub", "Bulldog", "Brawler"],
            PartVariant::Barrel(Barrel::Short) => &["Stub", "Hornet", "Pocket"],
            PartVariant::Barrel(Barrel::Heavy) if rifle => &["Hammer", "Maul", "Sledge"],
            PartVariant::Barrel(Barrel::Heavy) => &["Mallet", "Knuckle", "Brick"],
            PartVariant::Magazine(Magazine::Extended) => &["Feeder", "Glutton", "Hoarder"],
            PartVariant::Magazine(Magazine::Drum) if rifle => &["Drummer", "Cauldron", "Kettle"],
            PartVariant::Magazine(Magazine::Drum) => &["Drum", "Hailstorm", "Downpour"],
            PartVariant::Magazine(Magazine::Quick) => &["Snap", "Flicker", "Quickdraw"],
            PartVariant::Muzzle(Muzzle::Compensator) => &["Steady", "Level", "Plumbline"],
            PartVariant::Muzzle(Muzzle::Booster) => &["Howler", "Shrike", "Bellow"],
            PartVariant::Stock(Stock::Light) => &["Sprinter", "Feather", "Skip"],
            PartVariant::Stock(Stock::Heavy) => &["Anchor", "Bedrock", "Mule"],
            PartVariant::Sight(Sight::Reflex) => &["Glance", "Blink", "Snapshot"],
            PartVariant::Sight(Sight::Scope) if rifle => &["Watcher", "Overlook", "Hawkeye"],
            PartVariant::Sight(Sight::Scope) => &["Spyglass", "Peeper", "Squint"],
            _ => &[],
        }
    }

    fn mods(self) -> Mods {
        match self {
            PartVariant::Barrel(b) => match b {
                Barrel::Standard => Mods::default(),
                Barrel::Long => Mods { speed: 25, cone: -20, rate: -12, switch: 10, ..default() },
                Barrel::Short => Mods { rate: 12, switch: -15, speed: -15, cone: 20, ..default() },
                Barrel::Heavy => Mods { damage: 20, mass: 40, rate: -15, speed: -5, ..default() },
            },
            PartVariant::Magazine(m) => match m {
                Magazine::Standard => Mods::default(),
                Magazine::Extended => Mods { mag: 40, reload: 15, ..default() },
                Magazine::Drum => Mods { mag: 100, reload: 45, switch: 15, ..default() },
                Magazine::Quick => Mods { mag: -25, reload: -35, ..default() },
            },
            PartVariant::Stock(s) => match s {
                Stock::Standard => Mods::default(),
                Stock::Light => Mods { switch: -30, unbraced: 25, ..default() },
                Stock::Heavy => Mods { braced: -15, unbraced: -20, switch: 25, ..default() },
            },
            PartVariant::Sight(s) => match s {
                Sight::Iron => Mods::default(),
                Sight::Reflex => Mods { unbraced: -12, braced: 5, ..default() },
                Sight::Scope => Mods { braced: -25, unbraced: 15, switch: 10, ..default() },
            },
            PartVariant::Muzzle(m) => match m {
                Muzzle::Bare => Mods::default(),
                Muzzle::Compensator => Mods { cone: -10, speed: -5, ..default() },
                Muzzle::Booster => Mods { speed: 15, damage: 5, cone: 10, ..default() },
            },
        }
    }
}

/// Perks: what rare and exotic guns do besides their numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Perk {
    Overclocked,
    Hardened,
    Quickload,
    Steady,
    DeepMag,
    HotLoads,
    /// Every fourth shot doesn't use a round.
    Recycler,
    /// The last round in the mag hits three times as hard.
    LastWord,
}

impl Perk {
    /// Words a gun with this perk may be named for.
    fn epithets(self) -> &'static [&'static str] {
        match self {
            Perk::Overclocked => &["Overdrive", "Redline", "Frenzy"],
            Perk::Hardened => &["Tungsten", "Ironclad", "Spike"],
            Perk::Quickload => &["Juggler", "Slick", "Swift"],
            Perk::Steady => &["Calm", "Surgeon", "Stillwater"],
            Perk::DeepMag => &["Deepwell", "Bounty", "Abundance"],
            Perk::HotLoads => &["Scorcher", "Ember", "Hotfoot"],
            Perk::Recycler => &["Thrift", "Miser", "Scrounger"],
            Perk::LastWord => &["Finale", "Epitaph", "Farewell"],
        }
    }

    const ALL: [Perk; 8] = [
        Perk::Overclocked,
        Perk::Hardened,
        Perk::Quickload,
        Perk::Steady,
        Perk::DeepMag,
        Perk::HotLoads,
        Perk::Recycler,
        Perk::LastWord,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Perk::Overclocked => "Overclocked",
            Perk::Hardened => "Hardened rounds",
            Perk::Quickload => "Quickload",
            Perk::Steady => "Steady hands",
            Perk::DeepMag => "Deep mag",
            Perk::HotLoads => "Hot loads",
            Perk::Recycler => "Recycler",
            Perk::LastWord => "Last word",
        }
    }

    pub fn effect(self) -> &'static str {
        match self {
            Perk::Overclocked => "+15% rate of fire",
            Perk::Hardened => "+15% damage",
            Perk::Quickload => "30% faster reload",
            Perk::Steady => "unbraced cone 30% tighter",
            Perk::DeepMag => "+35% magazine",
            Perk::HotLoads => "+25% muzzle speed",
            Perk::Recycler => "every fourth shot is free",
            Perk::LastWord => "the last round in the mag hits x3",
        }
    }

    fn mods(self) -> Mods {
        match self {
            Perk::Overclocked => Mods { rate: 15, ..default() },
            Perk::Hardened => Mods { damage: 15, ..default() },
            Perk::Quickload => Mods { reload: -30, ..default() },
            Perk::Steady => Mods { unbraced: -30, ..default() },
            Perk::DeepMag => Mods { mag: 35, ..default() },
            Perk::HotLoads => Mods { speed: 25, mass: 10, ..default() },
            Perk::Recycler | Perk::LastWord => Mods::default(),
        }
    }
}

/// Stat changes, in percent of the frame's base. `cone` changes both
/// cones; `braced` and `unbraced` one each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Mods {
    rate: i32,
    speed: i32,
    damage: i32,
    cone: i32,
    braced: i32,
    unbraced: i32,
    mag: i32,
    reload: i32,
    switch: i32,
    mass: i32,
}

impl std::ops::AddAssign for Mods {
    fn add_assign(&mut self, o: Mods) {
        self.rate += o.rate;
        self.speed += o.speed;
        self.damage += o.damage;
        self.cone += o.cone;
        self.braced += o.braced;
        self.unbraced += o.unbraced;
        self.mag += o.mag;
        self.reload += o.reload;
        self.switch += o.switch;
        self.mass += o.mass;
    }
}

/// `base` changed by `percent` (never below a tenth of it). Plain IEEE
/// arithmetic, the same on every platform.
fn scale(base: f32, percent: i32) -> f32 {
    base * (100 + percent).max(10) as f32 / 100.0
}

/// Two numbers multiplied as percentages: e.g. +20% and +10% make +32%.
fn compound(a: i32, b: i32) -> i32 {
    ((100 + a) * (100 + b)) / 100 - 100
}

impl Mods {
    fn apply(&self, base: &GunStats) -> GunStats {
        GunStats {
            rate: scale(base.rate, self.rate),
            muzzle_speed: scale(base.muzzle_speed, self.speed),
            damage: scale(base.damage, self.damage),
            spread_braced: scale(base.spread_braced, compound(self.cone, self.braced)),
            spread: scale(base.spread, compound(self.cone, self.unbraced)),
            mag: ((base.mag as i32 * (100 + self.mag).max(10) + 50) / 100).max(4) as u16,
            reload: scale(base.reload, self.reload),
            bullet_mass: scale(base.bullet_mass, self.mass),
            switch: scale(base.switch, self.switch),
            ..*base
        }
    }
}

// ---------------------------------------------------------------------------
// Weapons.
//
// FROZEN: players' profiles keep their guns as (frame, seed, rarity) plus
// their workbench changes (`sim::progression`). Everything a gun is derives
// from those numbers here, so changing how it's derived (the hash, the
// salts, the tables of parts, makers, perks and names, their order, or the
// stat arithmetic) changes every gun everyone has saved. The pinned rolls
// in this module's tests say when that's happened; if it's meant, bump
// `ProfileFile::VERSION` and say so in the README.

/// A gun as an object. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Weapon {
    pub frame: GunKind,
    pub seed: u32,
    /// As it is now: as found, plus `tune` tiers from the workbench.
    pub rarity: Rarity,
    /// Rounds in the mag.
    pub mag: u16,
    /// Tiers the workbench has tuned it up by.
    #[serde(default)]
    pub tune: u8,
    /// Parts and paint the workbench has fitted.
    #[serde(default)]
    pub refit: Refit,
    /// How it acts on the world (ballistic, for every gun from before
    /// families).
    #[serde(default)]
    pub family: Family,
    /// A laser's heat, of [`HEAT_FULL`]: it's the gun's, wherever the gun
    /// is (a hot laser swapped to the other hand or the pack stays hot, and
    /// cools there).
    #[serde(default)]
    pub heat: u16,
    /// Overheated: it won't fire until it's cooled to [`COOLED`].
    #[serde(default)]
    pub overheated: bool,
}

/// A laser's full heat, in the units [`Weapon::heat`] counts (whole
/// numbers, so the same on every peer).
pub const HEAT_FULL: u16 = 10_000;
/// A laser cools this much a second when not firing (of its full heat).
pub const COOL_RATE: f32 = 0.5;
/// An overheated laser fires again once it's cooled to this (of full).
pub const COOLED: f32 = 0.33;

/// What the workbench has fitted to a gun, by slot (in [`SLOTS`] order):
/// 0 for what it was made with, else a variant (its index in the slot's
/// list, plus one). `paint` is 0 for its maker's paint, else one of
/// [`PAINTS`] (index plus one).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Refit {
    pub parts: [u8; 5],
    pub paint: u8,
}

/// The part slots, in the order a gun's parts are rolled and refitted,
/// with every variant each takes (stock first).
pub const SLOTS: [&[PartVariant]; 5] = [
    &[
        PartVariant::Barrel(Barrel::Standard),
        PartVariant::Barrel(Barrel::Long),
        PartVariant::Barrel(Barrel::Short),
        PartVariant::Barrel(Barrel::Heavy),
    ],
    &[
        PartVariant::Magazine(Magazine::Standard),
        PartVariant::Magazine(Magazine::Extended),
        PartVariant::Magazine(Magazine::Drum),
        PartVariant::Magazine(Magazine::Quick),
    ],
    &[PartVariant::Stock(Stock::Standard), PartVariant::Stock(Stock::Light), PartVariant::Stock(Stock::Heavy)],
    &[PartVariant::Sight(Sight::Iron), PartVariant::Sight(Sight::Reflex), PartVariant::Sight(Sight::Scope)],
    &[PartVariant::Muzzle(Muzzle::Bare), PartVariant::Muzzle(Muzzle::Compensator), PartVariant::Muzzle(Muzzle::Booster)],
];

/// The crew's own paint jobs, for the workbench: (name, body, accent), and
/// the rank each comes with (see `sim::progression`).
pub const PAINTS: [(&str, [u8; 3], [u8; 3], u8); 8] = [
    ("Hull grey", [120, 128, 136], [230, 230, 225], 2),
    ("Signal orange", [230, 110, 30], [40, 40, 44], 2),
    ("Deep-space blue", [36, 60, 110], [200, 210, 230], 2),
    ("Bone", [222, 214, 190], [70, 64, 58], 5),
    ("Racing green", [32, 90, 56], [220, 190, 90], 5),
    ("Gunmetal", [58, 62, 68], [180, 120, 60], 7),
    ("Ice", [210, 236, 244], [40, 110, 150], 7),
    ("Gold leaf", [212, 170, 60], [30, 28, 26], 9),
];

impl PartVariant {
    /// Its slot (in [`SLOTS`] order) and index in the slot's list.
    pub fn place(self) -> (usize, usize) {
        SLOTS
            .iter()
            .enumerate()
            .find_map(|(i, options)| Some((i, options.iter().position(|p| *p == self)?)))
            .expect("every variant is in a slot")
    }
}

/// What a weapon's seed made of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Build {
    pub maker: Maker,
    pub parts: Parts,
    pub perks: [Option<Perk>; 2],
    /// Stats boosted by its rarity (each +8%).
    boosts: [Option<Boost>; 2],
    /// Its own small variation (percent), damage and rate.
    jitter: (i32, i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Boost {
    Damage,
    Rate,
    Reload,
    Cone,
    Mag,
    Speed,
}

impl Boost {
    const ALL: [Boost; 6] = [Boost::Damage, Boost::Rate, Boost::Reload, Boost::Cone, Boost::Mag, Boost::Speed];

    fn mods(self) -> Mods {
        match self {
            Boost::Damage => Mods { damage: 8, ..default() },
            Boost::Rate => Mods { rate: 8, ..default() },
            Boost::Reload => Mods { reload: -10, ..default() },
            Boost::Cone => Mods { cone: -10, ..default() },
            Boost::Mag => Mods { mag: 15, ..default() },
            Boost::Speed => Mods { speed: 10, ..default() },
        }
    }
}

/// How a weapon looks: what the client builds the visible gun from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeaponLook {
    pub frame: GunKind,
    pub parts: Parts,
    pub paint: Paint,
}

/// A paint job, sRGB: the body (a model's `paint_primary`) and accent
/// (`paint_secondary`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paint {
    pub body: [u8; 3],
    pub accent: [u8; 3],
}

/// Salts for each roll from a seed, so adding a roll doesn't shift others.
mod salt {
    pub const MAKER: u32 = 1;
    pub const PARTS: u32 = 10;
    pub const PERKS: u32 = 20;
    pub const BOOSTS: u32 = 30;
    pub const JITTER: u32 = 40;
    pub const PAINT: u32 = 50;
    pub const MARK: u32 = 60;
    pub const NAME: u32 = 70;
}

impl Weapon {
    /// A standard-issue gun of `frame`, its mag full.
    pub fn issue(frame: GunKind) -> Weapon {
        Weapon::new(frame, 0, Rarity::Standard)
    }

    /// The gun `seed` makes of `frame` at `rarity`, its mag full.
    pub fn new(frame: GunKind, seed: u32, rarity: Rarity) -> Weapon {
        let mut w = Weapon {
            frame,
            seed,
            rarity,
            mag: 0,
            tune: 0,
            refit: Refit::default(),
            family: Family::Ballistic,
            heat: 0,
            overheated: false,
        };
        w.mag = w.stats().mag;
        w
    }

    /// Its heat, of 1 (only a laser heats).
    pub fn heat_fraction(&self) -> f32 {
        self.heat as f32 / HEAT_FULL as f32
    }

    /// Cools it for `dt` seconds; an overheated one that's cooled enough
    /// fires again.
    pub fn cool(&mut self, dt: f32) {
        let step = (COOL_RATE * dt * HEAT_FULL as f32).round() as u16;
        self.heat = self.heat.saturating_sub(step);
        if self.overheated && self.heat_fraction() <= COOLED {
            self.overheated = false;
        }
    }

    /// Heats it by a shot's `heat` (of 1); true if that overheated it.
    pub fn heat_by(&mut self, heat: f32) -> bool {
        let step = (heat * HEAT_FULL as f32).round() as u16;
        self.heat = self.heat.saturating_add(step).min(HEAT_FULL);
        if self.heat >= HEAT_FULL {
            self.overheated = true;
        }
        self.overheated
    }

    /// A gun rolled from `seed`: a crew frame, a rarity by `luck` (0–100).
    ///
    /// The frame and rarity are drawn as they were before the shotgun and
    /// sniper came; new salts decide whether it's one of those instead, and
    /// (an SMG or rifle) its family.
    pub fn roll(seed: u32, luck: u32) -> Weapon {
        let mut frame = GunKind::FIRST[(draw(seed, 101) % GunKind::FIRST.len() as u32) as usize];
        if draw(seed, 104) % 100 < NEW_FRAME_CHANCE {
            frame = [GunKind::Shotgun, GunKind::Sniper][(draw(seed, 105) % 2) as usize];
        }
        let mut w = Weapon::new(frame, draw(seed, 102), Rarity::roll(draw(seed, 103), luck));
        if GunKind::FAMILIES.contains(&frame) && draw(seed, 106) % 100 < FAMILY_CHANCE {
            w = w.with_family(Family::ROLLED[(draw(seed, 107) % Family::ROLLED.len() as u32) as usize]);
        }
        w
    }

    /// The same gun of another family, its mag full.
    pub fn with_family(self, family: Family) -> Weapon {
        let mut w = Weapon { family, ..self };
        w.mag = w.stats().mag;
        w
    }

    /// What kind of gun it is: "Plasma rifle", "Sniper".
    pub fn kind_name(&self) -> String {
        match self.family {
            Family::Ballistic => self.frame.name().to_string(),
            f => format!("{} {}", f.name(), self.frame.name().to_lowercase()),
        }
    }

    /// The rarity it was found at, before any tuning.
    pub fn rolled(&self) -> Rarity {
        Rarity::ALL[(self.rarity as usize).saturating_sub(self.tune as usize).max(1).min(self.rarity as usize)]
    }

    /// Whether two are the same gun (whatever is in their mags).
    pub fn same_gun(&self, other: &Weapon) -> bool {
        (self.frame, self.seed, self.rarity, self.tune, self.refit, self.family)
            == (other.frame, other.seed, other.rarity, other.tune, other.refit, other.family)
    }

    /// Its maker, parts and perks.
    pub fn build(&self) -> Build {
        if self.rarity == Rarity::Standard {
            return Build {
                maker: Maker::Issue,
                parts: Parts::STANDARD,
                perks: [None; 2],
                boosts: [None; 2],
                jitter: (0, 0),
            };
        }
        let s = self.seed;
        let maker = Maker::ROLLED[(draw(s, salt::MAKER) % Maker::ROLLED.len() as u32) as usize];
        // Rarer guns come stock less often (by the rarity it was found at:
        // tuning it up later changes none of its parts).
        let plain = 40 - 8 * self.rolled().tier();
        let mut parts = [PartVariant::Barrel(Barrel::Standard); 5];
        for (i, (part, options)) in parts.iter_mut().zip(SLOTS).enumerate() {
            // On the stack: stats are derived every tick, rollbacks too.
            let mut weighted = [(options[0], 0); 4];
            for (w, &p) in weighted.iter_mut().zip(options) {
                *w = (p, if p.standard() { plain } else { 20 * maker.favours(p) });
            }
            *part = pick(draw(s, salt::PARTS + i as u32), &weighted[..options.len()]);
            // A refit's part instead, if it has one here.
            if let Some(&fitted) = self.refit.parts[i].checked_sub(1).and_then(|v| options.get(v as usize)) {
                *part = fitted;
            }
        }
        let parts = Parts::from_slots(parts);
        // Perks, all different.
        let mut perks = [None; 2];
        for i in 0..self.rarity.perks() {
            let first = draw(s, salt::PERKS + i as u32) as usize % Perk::ALL.len();
            let perk = (0..Perk::ALL.len())
                .map(|k| Perk::ALL[(first + k) % Perk::ALL.len()])
                .find(|p| !perks.contains(&Some(*p)))
                .expect("more perks than slots");
            perks[i] = Some(perk);
        }
        let mut boosts = [None; 2];
        for (i, b) in boosts.iter_mut().enumerate().take(self.rarity.quality().1) {
            *b = Some(Boost::ALL[draw(s, salt::BOOSTS + i as u32) as usize % Boost::ALL.len()]);
        }
        let jitter = |i| (draw(s, salt::JITTER + i) % 7) as i32 - 3;
        Build {
            maker,
            parts,
            perks,
            boosts,
            jitter: (jitter(0), jitter(1)),
        }
    }

    /// Its stats: the frame's base, changed by its maker, parts, rarity and
    /// perks. A standard-issue gun has the frame's base stats exactly.
    pub fn stats(&self) -> GunStats {
        let mut stats = self.rolled_stats();
        // A scope zooms a rifle, and zooms a sniper further.
        if self.rarity != Rarity::Standard && self.build().parts.sight == Sight::Scope {
            stats.zoom = match self.frame {
                GunKind::Rifle => 2.5,
                GunKind::Sniper => 6.0,
                _ => stats.zoom,
            };
        }
        GunStats { family: self.family, ..self.family.apply(stats) }
    }

    /// Its stats as a ballistic gun: frame, maker, parts, rarity, perks.
    fn rolled_stats(&self) -> GunStats {
        let base = self.frame.base();
        if self.rarity == Rarity::Standard {
            return base;
        }
        let build = self.build();
        let mut mods = build.maker.lean();
        for part in build.parts.all() {
            mods += part.mods();
        }
        for perk in build.perks.iter().flatten() {
            mods += perk.mods();
        }
        for boost in build.boosts.iter().flatten() {
            mods += boost.mods();
        }
        mods.damage += self.rarity.quality().0 + build.jitter.0;
        mods.rate += build.jitter.1;
        let mut stats = mods.apply(&base);
        stats.perks = build.perks;
        stats
    }

    /// Its name: its maker, the maker's model (in the maker's style, its
    /// mark higher the rarer it is), and an epithet for one of what makes
    /// it itself: a part that isn't stock, or a perk (perks weigh double).
    /// "Kessler KG-3 Longshot", "Hoshi Type II Hornet", "Corvid Scrap #4
    /// Miser". Standard issue is "Issue SMG".
    pub fn name(&self) -> String {
        if self.rarity == Rarity::Standard {
            return format!("Issue {}", self.frame.name());
        }
        let build = self.build();
        let mark = 1 + draw(self.seed, salt::MARK) % 3 + self.rarity.tier().saturating_sub(1);
        let mut sources: [(&[&str], u32); 7] = [(&[], 0); 7];
        for (source, part) in sources.iter_mut().zip(build.parts.all()) {
            *source = (part.epithets(self.frame), 1);
        }
        for (source, perk) in sources[5..].iter_mut().zip(build.perks) {
            *source = (perk.map_or(&[][..], Perk::epithets), 2);
        }
        let mut sources: Vec<(&[&str], u32)> = sources.into_iter().filter(|(words, _)| !words.is_empty()).collect();
        if sources.is_empty() {
            sources.push((build.maker.epithets(), 1));
        }
        let words = pick(draw(self.seed, salt::NAME), &sources);
        let epithet = words[draw(self.seed, salt::NAME + 1) as usize % words.len()];
        format!("{} {} {epithet}", build.maker.name(), build.maker.model(self.frame, mark))
    }

    /// How it looks.
    pub fn look(&self) -> WeaponLook {
        let build = self.build();
        let palette = build.maker.palette();
        let (body, accent) = palette[draw(self.seed, salt::PAINT) as usize % palette.len()];
        // Standard issue wears the models' own paint.
        let (body, accent) = match (build.maker, self.frame) {
            (Maker::Issue, GunKind::Rifle) => ([214, 207, 186], [59, 70, 88]),
            _ => (body, accent),
        };
        // Or the paint it was given at the workbench.
        let (body, accent) = match self.refit.paint.checked_sub(1).and_then(|i| PAINTS.get(i as usize)) {
            Some(&(_, body, accent, _)) => (body, accent),
            None => (body, accent),
        };
        WeaponLook {
            frame: self.frame,
            parts: build.parts,
            paint: Paint { body, accent },
        }
    }
}

// ---------------------------------------------------------------------------
// Items and the inventory.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Item {
    Weapon(Weapon),
    Ammo { kind: AmmoKind, rounds: u16 },
    Cargo(CargoKind),
}

impl Item {
    pub fn name(&self) -> String {
        match self {
            Item::Weapon(w) => w.name(),
            Item::Ammo { kind, rounds } => format!("{rounds} {}", kind.name().to_lowercase()),
            Item::Cargo(c) => c.name().to_string(),
        }
    }

    pub fn rarity(&self) -> Option<Rarity> {
        match self {
            Item::Weapon(w) => Some(w.rarity),
            _ => None,
        }
    }

    /// Bulky cargo, carried in both hands.
    pub fn bulky(&self) -> bool {
        matches!(self, Item::Cargo(c) if c.bulky())
    }
}

/// How much of an item was taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    All,
    /// Some of a box of ammo: `left` rounds stay behind.
    Part { left: u16 },
    Nothing,
}

/// Weapon slots, to start with: what stances point at. Rank adds more,
/// up to [`MAX_WEAPON_SLOTS`] (see `sim::progression`).
pub const WEAPON_SLOTS: usize = 3;
/// Pack slots, for spares and small cargo, to start with; up to
/// [`MAX_PACK_SLOTS`].
pub const PACK_SLOTS: usize = 6;
pub const MAX_WEAPON_SLOTS: usize = 4;
pub const MAX_PACK_SLOTS: usize = 10;

/// Where in an inventory an item is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Reflect, Serialize, Deserialize)]
pub enum Place {
    Weapon(u8),
    Pack(u8),
    /// Bulky cargo, in both hands.
    Hands,
}

/// Something done to the inventory, from the inventory screen (sent with
/// the input; see [`super::input::PlayerInput::item_op`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub enum ItemOp {
    #[default]
    None,
    /// Move what's at `from` to `to`, swapping with what's there if that
    /// can go back to `from`.
    Move { from: Place, to: Place },
    /// Drop what's there at your feet.
    Drop(Place),
    /// Drop a box of ammo of a kind.
    DropAmmo(AmmoKind),
}

/// What a player carries. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    pub weapons: [Option<Weapon>; MAX_WEAPON_SLOTS],
    pub pack: [Option<Item>; MAX_PACK_SLOTS],
    /// Rounds in each pool, by [`AmmoKind::index`]. (A save from before a
    /// kind of ammo existed reads with none of it.)
    #[serde(with = "pools")]
    pub ammo: [u16; AmmoKind::COUNT],
    /// Bulky cargo in both hands, if any.
    pub carrying: Option<CargoKind>,
    /// How many of the weapon slots and pack slots it has (rank gives
    /// more).
    pub slots: (u8, u8),
}

impl Default for Inventory {
    fn default() -> Inventory {
        Inventory {
            weapons: [None; MAX_WEAPON_SLOTS],
            pack: [None; MAX_PACK_SLOTS],
            ammo: [0; AmmoKind::COUNT],
            carrying: None,
            slots: (WEAPON_SLOTS as u8, PACK_SLOTS as u8),
        }
    }
}

impl Inventory {
    /// How many weapon slots it has.
    pub fn weapon_slots(&self) -> usize {
        (self.slots.0 as usize).min(MAX_WEAPON_SLOTS)
    }

    /// How many pack slots it has.
    pub fn pack_slots(&self) -> usize {
        (self.slots.1 as usize).min(MAX_PACK_SLOTS)
    }

    /// Gives it `weapons` weapon slots and `pack` pack slots (no fewer
    /// than it has). What's past them, if it had more, is returned.
    pub fn grow(&mut self, weapons: usize, pack: usize) {
        self.slots.0 = self.slots.0.max(weapons.min(MAX_WEAPON_SLOTS) as u8);
        self.slots.1 = self.slots.1.max(pack.min(MAX_PACK_SLOTS) as u8);
    }

    /// Everything outside its slots, taken out (a file from a higher rank,
    /// say).
    pub fn overflow(&mut self) -> Vec<Item> {
        let (w, p) = (self.weapon_slots(), self.pack_slots());
        let mut out: Vec<Item> = self.weapons[w..].iter_mut().filter_map(Option::take).map(Item::Weapon).collect();
        out.extend(self.pack[p..].iter_mut().filter_map(Option::take));
        out
    }

    pub fn ammo(&self, kind: AmmoKind) -> u16 {
        self.ammo[kind.index()]
    }

    pub fn ammo_mut(&mut self, kind: AmmoKind) -> &mut u16 {
        &mut self.ammo[kind.index()]
    }

    pub fn get(&self, place: Place) -> Option<Item> {
        match place {
            Place::Weapon(i) => self.weapons.get(i as usize).copied().flatten().map(Item::Weapon),
            Place::Pack(i) => self.pack.get(i as usize).copied().flatten(),
            Place::Hands => self.carrying.map(Item::Cargo),
        }
    }

    /// Whether `item` can go at `place` (whatever is there now).
    pub fn fits(&self, place: Place, item: &Item) -> bool {
        match place {
            Place::Weapon(i) => (i as usize) < self.weapon_slots() && matches!(item, Item::Weapon(_)),
            Place::Pack(i) => (i as usize) < self.pack_slots() && !item.bulky() && !matches!(item, Item::Ammo { .. }),
            Place::Hands => item.bulky(),
        }
    }

    /// Puts `item` at `place`, which must fit it.
    fn put(&mut self, place: Place, item: Option<Item>) {
        match place {
            Place::Weapon(i) => {
                self.weapons[i as usize] = item.map(|item| match item {
                    Item::Weapon(w) => w,
                    _ => unreachable!("not a weapon"),
                })
            }
            Place::Pack(i) => self.pack[i as usize] = item,
            Place::Hands => {
                self.carrying = item.map(|item| match item {
                    Item::Cargo(c) => c,
                    _ => unreachable!("not cargo"),
                })
            }
        }
    }

    /// Takes out all cargo (mission items belong to the mission: they
    /// don't leave its world or go in a saved profile). Whether there was
    /// any.
    pub fn strip_cargo(&mut self) -> bool {
        let mut any = self.carrying.take().is_some();
        for slot in &mut self.pack {
            if matches!(slot, Some(Item::Cargo(_))) {
                *slot = None;
                any = true;
            }
        }
        any
    }

    /// Puts `item` at an empty `place` it fits. Whether it did.
    pub fn put_back(&mut self, place: Place, item: Item) -> bool {
        let ok = self.get(place).is_none() && self.fits(place, &item);
        if ok {
            self.put(place, Some(item));
        }
        ok
    }

    /// Takes out what's at `place`.
    pub fn remove(&mut self, place: Place) -> Option<Item> {
        let item = self.get(place)?;
        self.put(place, None);
        Some(item)
    }

    /// Moves what's at `from` to `to`, swapping with what's there if it can
    /// go back to `from`. Whether anything moved.
    pub fn move_item(&mut self, from: Place, to: Place) -> bool {
        let Some(moving) = self.get(from) else { return false };
        if from == to || !self.fits(to, &moving) {
            return false;
        }
        let there = self.get(to);
        if there.is_some_and(|t| !self.fits(from, &t)) {
            return false;
        }
        self.put(from, there);
        self.put(to, Some(moving));
        true
    }

    /// The first free place `item` fits: a weapon slot, else the pack (or
    /// the hands, for bulky cargo).
    pub fn free_place(&self, item: &Item) -> Option<Place> {
        let weapons = (0..self.weapon_slots() as u8).map(Place::Weapon);
        let pack = (0..self.pack_slots() as u8).map(Place::Pack);
        weapons
            .chain(pack)
            .chain([Place::Hands])
            .find(|p| self.fits(*p, item) && self.get(*p).is_none())
    }

    /// Puts up to `rounds` of `kind` in its pool, as many as fit under its
    /// cap. How many went in.
    pub fn add_ammo(&mut self, kind: AmmoKind, rounds: u16) -> u16 {
        let pool = self.ammo_mut(kind);
        let fits = kind.cap().saturating_sub(*pool).min(rounds);
        *pool += fits;
        fits
    }

    /// Takes `item` in whole: a gun into the first free weapon slot, else
    /// the pack; ammo into its pool, if it all fits; cargo into the hands or
    /// the pack. Whether it was taken. (Picking up takes part of a box of
    /// ammo, too: see `pickups::take_from`.)
    pub fn take(&mut self, item: Item) -> bool {
        if let Item::Ammo { kind, rounds } = item {
            if kind.cap().saturating_sub(self.ammo(kind)) < rounds {
                return false;
            }
            self.add_ammo(kind, rounds);
            return true;
        }
        match self.free_place(&item) {
            Some(place) => {
                self.put(place, Some(item));
                true
            }
            None => false,
        }
    }

    /// Takes what fits of `item`: all of it (as [`Self::take`]), or, of a
    /// box of ammo, as many rounds as the pool has room for.
    pub fn take_what_fits(&mut self, item: Item) -> Taken {
        match item {
            Item::Ammo { kind, rounds } => match self.add_ammo(kind, rounds) {
                0 => Taken::Nothing,
                n if n == rounds => Taken::All,
                n => Taken::Part { left: rounds - n },
            },
            item if self.take(item) => Taken::All,
            _ => Taken::Nothing,
        }
    }

    /// Whether a gun of `frame` is carried anywhere.
    pub fn has_frame(&self, frame: GunKind) -> bool {
        self.weapons.iter().flatten().any(|w| w.frame == frame)
            || self.pack.iter().flatten().any(|i| matches!(i, Item::Weapon(w) if w.frame == frame))
    }

    /// Every weapon carried, in slots and pack.
    pub fn all_weapons_mut(&mut self) -> impl Iterator<Item = &mut Weapon> {
        let pack = self.pack.iter_mut().flatten().filter_map(|i| match i {
            Item::Weapon(w) => Some(w),
            _ => None,
        });
        self.weapons.iter_mut().flatten().chain(pack)
    }

    /// Everything carried, emptied out (for leaving behind on death): the
    /// guns with what's in their mags, the pack, the ammo pools, the cargo.
    pub fn empty_out(&mut self) -> Vec<Item> {
        let mut items: Vec<Item> = self.weapons.iter().flatten().map(|w| Item::Weapon(*w)).collect();
        items.extend(self.carrying.map(Item::Cargo));
        items.extend(self.pack.iter().flatten().copied());
        for kind in AmmoKind::ALL {
            let rounds = self.ammo(kind);
            if rounds > 0 {
                items.push(Item::Ammo { kind, rounds });
            }
        }
        // The slots it had, it keeps.
        *self = Inventory {
            slots: self.slots,
            ..Inventory::default()
        };
        items
    }
}

/// Ammo pools as saved and sent: a tuple of every kind, read back padded
/// with zeros if it's from when there were fewer kinds.
mod pools {
    use super::AmmoKind;
    use serde::de::{SeqAccess, Visitor};
    use serde::ser::SerializeTuple;
    use serde::{Deserializer, Serializer};

    pub fn serialize<S: Serializer>(ammo: &[u16; AmmoKind::COUNT], s: S) -> Result<S::Ok, S::Error> {
        let mut t = s.serialize_tuple(AmmoKind::COUNT)?;
        for a in ammo {
            t.serialize_element(a)?;
        }
        t.end()
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<[u16; AmmoKind::COUNT], D::Error> {
        struct Pools;
        impl<'de> Visitor<'de> for Pools {
            type Value = [u16; AmmoKind::COUNT];
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "up to {} ammo pools", AmmoKind::COUNT)
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
                let mut out = [0; AmmoKind::COUNT];
                for slot in &mut out {
                    match seq.next_element()? {
                        Some(v) => *slot = v,
                        None => break,
                    }
                }
                Ok(out)
            }
        }
        d.deserialize_tuple(AmmoKind::COUNT, Pools)
    }
}

// ---------------------------------------------------------------------------
// Loot.

/// What a robot leaves: mostly ammo, sometimes a gun. `toughness` (its full
/// health) makes guns likelier and better.
pub fn robot_loot(seed: u32, toughness: f32) -> Vec<Item> {
    let tough = (toughness.clamp(0.0, 200.0) as u32).min(200);
    let mut items = Vec::new();
    // Rounds and heavy rounds two times in six each; shells and cells one.
    let kind = AmmoKind::ALL[[0, 0, 1, 1, 2, 3][(draw(seed, 1) % 6) as usize]];
    let rounds = kind.box_rounds() / 2 + (draw(seed, 2) % (kind.box_rounds() as u32 / 2 + 1)) as u16;
    items.push(Item::Ammo { kind, rounds });
    // A Swarmer (30) drops a gun one time in ten; a Gunner (120) one in four.
    let chance = 7 + tough / 7;
    if draw(seed, 3) % 100 < chance {
        items.push(Item::Weapon(Weapon::roll(draw(seed, 4), tough / 4)));
    }
    items
}

/// What a crate holds: one or two guns and some ammo. `luck` (0–100) is how
/// deep in the ship it is.
pub fn crate_loot(seed: u32, luck: u32) -> Vec<Item> {
    let mut items = vec![Item::Weapon(Weapon::roll(draw(seed, 1), luck))];
    if draw(seed, 2) % 100 < 35 + luck / 3 {
        items.push(Item::Weapon(Weapon::roll(draw(seed, 3), luck)));
    }
    for kind in AmmoKind::ALL {
        if draw(seed, 10 + kind.index() as u32) % 100 < 60 {
            items.push(Item::Ammo { kind, rounds: kind.box_rounds() });
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_issue_has_the_frames_base_stats() {
        for frame in [GunKind::Smg, GunKind::Rifle, GunKind::GunnerRifle, GunKind::SwarmerSprayer] {
            assert_eq!(Weapon::issue(frame).stats(), frame.base());
        }
        assert_eq!(Weapon::issue(GunKind::Smg).name(), "Issue SMG");
    }

    #[test]
    fn rarity_rolls_shift_with_luck() {
        let count = |luck| (0..2000).filter(|i| Rarity::roll(draw(*i, 0), luck) >= Rarity::Rare).count();
        assert!(count(100) > 2 * count(0), "{} vs {}", count(100), count(0));
    }

    #[test]
    fn a_pool_takes_ammo_until_its_cap() {
        let mut inv = Inventory::default();
        let heavy = |rounds| Item::Ammo { kind: AmmoKind::Heavy, rounds };
        assert!(inv.take(heavy(150)));
        assert!(!inv.take(heavy(150)), "took more than fits, whole");
        assert_eq!(inv.take_what_fits(heavy(150)), Taken::Part { left: 100 });
        assert_eq!(inv.ammo(AmmoKind::Heavy), AmmoKind::Heavy.cap());
        assert_eq!(inv.take_what_fits(heavy(1)), Taken::Nothing, "full pool");
    }

    const ROLLED: [Rarity; 4] = [Rarity::Common, Rarity::Uncommon, Rarity::Rare, Rarity::Exotic];

    #[test]
    fn a_weapon_is_the_same_from_the_same_numbers() {
        for seed in 0..300 {
            for rarity in ROLLED {
                let a = Weapon::new(GunKind::Rifle, seed, rarity);
                let b = Weapon::new(GunKind::Rifle, seed, rarity);
                assert_eq!(a, b);
                assert_eq!(a.stats(), b.stats());
                assert_eq!(a.name(), b.name());
                assert_eq!(a.look(), b.look());
            }
        }
        // Pinned: the hash and the tables are the save format, in effect.
        // If this changes, every gun anyone carries changes with it.
        let w = Weapon::new(GunKind::Rifle, 12345, Rarity::Rare);
        let pinned = (w.name(), w.build().maker, w.build().parts, w.build().perks);
        eprintln!("{pinned:?}");
        assert_eq!(pinned, PINNED.clone());
    }

    static PINNED: std::sync::LazyLock<(String, Maker, Parts, [Option<Perk>; 2])> = std::sync::LazyLock::new(|| {
        (
            "Corvid Scrap #5 Redline".to_string(),
            Maker::Corvid,
            Parts {
                barrel: Barrel::Short,
                magazine: Magazine::Standard,
                stock: Stock::Standard,
                sight: Sight::Reflex,
                muzzle: Muzzle::Booster,
            },
            [Some(Perk::Overclocked), None],
        )
    });

    #[test]
    fn parts_roll_by_their_weights() {
        let mut counts = std::collections::HashMap::new();
        for s in 0..4000 {
            *counts.entry(Weapon::roll(draw(s, 7), 50).build().parts.barrel).or_insert(0) += 1;
        }
        // Standard, long, short, heavy: none of them most of the time.
        assert!(counts.values().all(|n| *n < 1800), "{counts:?}");
        // Nor in crates rolled from consecutive ticks.
        let mut counts = std::collections::HashMap::new();
        for tick in 1000..1400 {
            let seed = draw(tick, 0);
            for item in crate_loot(seed, draw(seed, 99) % 101) {
                if let Item::Weapon(w) = item {
                    *counts.entry((w.build().parts.barrel, w.build().maker)).or_insert(0) += 1;
                }
            }
        }
        assert!(counts.values().all(|n| *n < 100), "{counts:?}");
    }

    #[test]
    fn seeds_make_different_guns() {
        let guns: Vec<Weapon> = (0..400).map(|s| Weapon::roll(s, 50)).collect();
        let names: std::collections::HashSet<String> = guns.iter().map(Weapon::name).collect();
        assert!(names.len() > 60, "only {} names", names.len());
        let builds: Vec<Build> = guns.iter().map(Weapon::build).collect();
        for maker in Maker::ROLLED {
            assert!(builds.iter().any(|b| b.maker == maker), "no {maker:?}");
        }
        for frame in GunKind::CREW {
            assert!(guns.iter().any(|g| g.frame == frame), "no {frame:?}");
        }
        assert!(guns.iter().all(|g| GunKind::CREW.contains(&g.frame)), "rolled a robot's gun");
        let variants: std::collections::HashSet<PartVariant> = builds.iter().flat_map(|b| b.parts.all()).collect();
        assert_eq!(variants.len(), 4 + 4 + 3 + 3 + 3, "some part never rolls");
        let looks: std::collections::HashSet<[u8; 3]> = guns.iter().map(|g| g.look().paint.body).collect();
        assert!(looks.len() >= 8, "only {} paints", looks.len());
    }

    /// Each stat, signed so that positive is better for the shooter.
    fn benefits(m: &Mods) -> [i32; 9] {
        [m.rate, m.speed, m.damage, -m.cone, -m.braced, -m.unbraced, m.mag, -m.reload, -m.switch]
    }

    #[test]
    fn every_part_trades_something_for_something() {
        use PartVariant as P;
        let all = [
            P::Barrel(Barrel::Long),
            P::Barrel(Barrel::Short),
            P::Barrel(Barrel::Heavy),
            P::Magazine(Magazine::Extended),
            P::Magazine(Magazine::Drum),
            P::Magazine(Magazine::Quick),
            P::Stock(Stock::Light),
            P::Stock(Stock::Heavy),
            P::Sight(Sight::Reflex),
            P::Sight(Sight::Scope),
            P::Muzzle(Muzzle::Compensator),
            P::Muzzle(Muzzle::Booster),
        ];
        for part in all {
            let b = benefits(&part.mods());
            assert!(b.iter().any(|x| *x > 0) && b.iter().any(|x| *x < 0), "{part:?} doesn't trade: {b:?}");
            assert!(!part.epithets(GunKind::Smg).is_empty() && !part.epithets(GunKind::Rifle).is_empty(), "{part:?} has no name");
        }
        for maker in Maker::ROLLED {
            let b = benefits(&maker.lean());
            assert!(b.iter().any(|x| *x > 0) && b.iter().any(|x| *x < 0), "{maker:?} leans only one way");
        }
    }

    #[test]
    fn a_long_barrel_shoots_faster_and_tighter_but_slower() {
        let base = GunKind::Rifle.base();
        let long = PartVariant::Barrel(Barrel::Long).mods().apply(&base);
        assert!(long.muzzle_speed > base.muzzle_speed);
        assert!(long.spread < base.spread && long.spread_braced < base.spread_braced);
        assert!(long.rate < base.rate);
        let drum = PartVariant::Magazine(Magazine::Drum).mods().apply(&base);
        assert!(drum.mag == 2 * base.mag && drum.reload > base.reload);
        let light = PartVariant::Stock(Stock::Light).mods().apply(&base);
        assert!(light.switch < base.switch && light.spread > base.spread);
        assert_eq!(light.spread_braced, base.spread_braced);
    }

    #[test]
    fn rarer_guns_carry_perks_and_hit_harder() {
        let mean_damage = |rarity| {
            (0..300).map(|s| Weapon::new(GunKind::Smg, s, rarity).stats().damage).sum::<f32>() / 300.0
        };
        assert!(mean_damage(Rarity::Exotic) > mean_damage(Rarity::Common));
        for s in 0..100 {
            let perks = |r| Weapon::new(GunKind::Smg, s, r).build().perks;
            assert_eq!(perks(Rarity::Common), [None, None]);
            assert!(perks(Rarity::Rare)[0].is_some() && perks(Rarity::Rare)[1].is_none());
            let [a, b] = perks(Rarity::Exotic);
            assert!(a.is_some() && b.is_some() && a != b);
        }
    }

    #[test]
    fn names_read_maker_model_epithet() {
        for s in 0..200 {
            let w = Weapon::roll(s, 50);
            let name = w.name();
            assert!(name.starts_with(w.build().maker.name()), "{name}");
            // Short enough for a hand card.
            assert!(name.chars().count() <= 26, "{name}");
        }
    }

    /// A few rolls of each frame, every rarity, as they were when profiles
    /// started keeping guns. If this fails, generation changed: every saved
    /// gun changed with it (see the note at "Weapons").
    #[test]
    fn saved_guns_stay_the_guns_they_were() {
        let signature = |w: Weapon| {
            let s = w.stats();
            format!(
                "{}|{:.3}|{:.3}|{}|{:.3}|{:.3}|{:.3}|{:.1}|{:.3}|{:?}",
                w.name(),
                s.damage,
                s.rate,
                s.mag,
                s.reload,
                s.spread_braced,
                s.spread,
                s.muzzle_speed,
                s.switch,
                s.perks
            )
        };
        let rolls: Vec<String> = GunKind::FIRST
            .into_iter()
            .flat_map(|frame| ROLLED.into_iter().enumerate().map(move |(i, r)| Weapon::new(frame, 1000 + i as u32 * 7919, r)))
            .map(signature)
            .collect();
        let pinned: Vec<String> = PINNED_ROLLS.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
        if rolls != pinned {
            panic!("generation changed; now:\n{}", rolls.join("\n"));
        }
    }

    const PINNED_ROLLS: &str = "
        Kessler MP-1 Pike|9.000|12.150|40|1.600|0.780|1.300|513.0|0.440|[None, None]
        Kessler MP-2 Spyglass|11.520|11.850|40|1.600|0.756|1.940|456.0|0.440|[None, None]
        Hoshi Type III Howler|10.530|16.350|30|1.040|1.116|1.760|475.0|0.420|[Some(Hardened), None]
        Hoshi Type IV Frenzy|10.080|20.550|86|2.320|1.296|1.620|475.0|0.420|[Some(HotLoads), Some(Overclocked)]
        Kessler KG-1 Reach|24.000|6.075|30|2.000|0.228|1.625|1080.0|0.440|[None, None]
        Kessler KG-2 Watcher|30.720|5.925|30|2.000|0.220|2.425|960.0|0.440|[None, None]
        Hoshi Type III Howler|28.080|8.175|23|1.300|0.325|2.200|1000.0|0.420|[Some(Hardened), None]
        Hoshi Type IV Frenzy|26.880|10.275|65|2.900|0.378|2.025|1000.0|0.420|[Some(HotLoads), Some(Overclocked)]
    ";

    /// The same for wave 4's frames and families, as they were when they
    /// came: the shotgun and sniper by rarity, and each family on an SMG
    /// and a rifle.
    #[test]
    fn wave_4_guns_stay_the_guns_they_were() {
        let signature = |w: Weapon| {
            let s = w.stats();
            format!(
                "{}|{:?}|{:.3}|{:.3}|{}|{:.3}|{:.3}|{:.1}|{}|{}|{:.1}|{:.4}|{:?}",
                w.name(),
                w.family,
                s.damage,
                s.rate,
                s.mag,
                s.spread_braced,
                s.spread,
                s.muzzle_speed,
                s.pellets,
                s.pierce,
                s.zoom,
                s.heat,
                s.perks
            )
        };
        let frames = [GunKind::Shotgun, GunKind::Sniper]
            .into_iter()
            .flat_map(|frame| ROLLED.into_iter().enumerate().map(move |(i, r)| Weapon::new(frame, 2000 + i as u32 * 7919, r)));
        let families = Family::ROLLED
            .into_iter()
            .flat_map(|f| GunKind::FAMILIES.into_iter().map(move |frame| Weapon::new(frame, 3000 + f as u32, Rarity::Rare).with_family(f)));
        let rolls: Vec<String> = frames.chain(families).map(signature).collect();
        let pinned: Vec<String> = PINNED_WAVE_4.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
        if rolls != pinned {
            panic!("generation changed; now:\n{}", rolls.join("\n"));
        }
    }

    const PINNED_WAVE_4: &str = "
        Hoshi Type I Steady|Ballistic|8.190|1.968|12|3.280|8.470|336.0|8|0|0.0|0.0000|[None, None]
        Kessler FL-2 Glance|Ballistic|9.360|1.824|6|3.960|4.565|378.0|8|0|0.0|0.0000|[None, None]
        Hoshi Type V Glance|Ballistic|11.340|1.728|5|4.600|6.820|567.0|8|0|0.0|0.0000|[Some(HotLoads), None]
        Kessler FL-6 Spike|Ballistic|14.670|1.472|8|2.240|5.775|420.0|8|0|0.0|0.0000|[Some(Overclocked), Some(Hardened)]
        Hoshi Type I Steady|Ballistic|81.900|0.984|10|0.033|9.240|1120.0|1|1|6.0|0.0000|[None, None]
        Kessler ZG-2 Glance|Ballistic|93.600|0.912|5|0.040|4.980|1260.0|1|1|5.0|0.0000|[None, None]
        Hoshi Type V Glance|Ballistic|113.400|0.864|4|0.046|7.440|1890.0|1|1|5.0|0.0000|[Some(HotLoads), None]
        Kessler ZG-6 Spike|Ballistic|146.700|0.736|7|0.022|6.300|1400.0|1|1|6.0|0.0000|[Some(Overclocked), Some(Hardened)]
        Hoshi Type V Hotfoot|Plasma|14.400|9.450|30|0.984|3.080|50.2|1|0|0.0|0.0000|[Some(HotLoads), None]
        Hoshi Type V Hotfoot|Plasma|38.400|4.725|23|0.287|3.850|105.6|1|0|2.5|0.0000|[Some(HotLoads), None]
        Brakk Grind 50 Knuckle|Arc|11.745|6.570|80|1.200|1.200|361.0|1|0|0.0|0.0000|[Some(Quickload), None]
        Brakk Ram 50 Maul|Arc|31.320|3.285|60|0.350|0.350|760.0|1|0|0.0|0.0000|[Some(Quickload), None]
        Corvid Scrap #3 Scorcher|Laser|1.958|60.000|0|0.000|0.000|551.0|1|0|0.0|0.0042|[Some(HotLoads), None]
        Corvid Scrap #3 Scorcher|Laser|2.611|60.000|0|0.000|0.000|1160.0|1|0|0.0|0.0042|[Some(HotLoads), None]
    ";

    #[test]
    fn names_vary() {
        for frame in GunKind::CREW {
            let rolls = 400;
            let mut epithets = std::collections::HashMap::new();
            let mut names = std::collections::HashSet::new();
            for s in 0..rolls {
                let w = Weapon::new(frame, draw(s, 5), Rarity::roll(draw(s, 6), 50));
                let name = w.name();
                let model = w.build().maker.model(frame, 1);
                let epithet = name.rsplit(' ').next().unwrap().to_string();
                assert!(!model.is_empty());
                *epithets.entry(epithet).or_insert(0u32) += 1;
                names.insert(name);
            }
            let (word, most) = epithets.iter().max_by_key(|(_, n)| **n).unwrap();
            assert!(*most * 100 <= 15 * rolls, "{frame:?}: {word:?} in {most} of {rolls}");
            assert!(epithets.len() >= 30, "{frame:?}: only {} epithets", epithets.len());
            assert!(names.len() * 100 >= 70 * rolls as usize, "{frame:?}: only {} names", names.len());
        }
    }

    #[test]
    fn moving_items_swaps_only_where_both_fit() {
        let smg = Item::Weapon(Weapon::issue(GunKind::Smg));
        let rifle = Item::Weapon(Weapon::new(GunKind::Rifle, 3, Rarity::Rare));
        let core = Item::Cargo(CargoKind::DataCore);
        let mut inv = Inventory::default();
        assert!(inv.take(smg) && inv.take(rifle) && inv.take(core));
        assert_eq!(inv.get(Place::Weapon(0)), Some(smg));
        assert_eq!(inv.get(Place::Weapon(1)), Some(rifle));
        assert_eq!(inv.get(Place::Pack(0)), Some(core), "cargo goes in the pack");
        // Swap the two guns.
        assert!(inv.move_item(Place::Weapon(0), Place::Weapon(1)));
        assert_eq!(inv.get(Place::Weapon(0)), Some(rifle));
        // A gun to the pack, swapping with the data core: the core can't go
        // in a weapon slot.
        assert!(!inv.move_item(Place::Weapon(0), Place::Pack(0)));
        assert!(inv.move_item(Place::Weapon(0), Place::Pack(1)));
        assert_eq!(inv.get(Place::Weapon(0)), None);
        assert!(!inv.move_item(Place::Pack(0), Place::Weapon(0)), "cargo in a weapon slot");
        // Bulky cargo only goes in the hands, and only one at a time.
        let cell = Item::Cargo(CargoKind::PowerCell);
        assert!(inv.take(cell));
        assert_eq!(inv.carrying, Some(CargoKind::PowerCell));
        assert!(!inv.take(cell));
        assert!(!inv.move_item(Place::Hands, Place::Pack(2)));
    }

    #[test]
    fn a_full_inventory_takes_no_more_guns() {
        let mut inv = Inventory::default();
        for s in 0..(WEAPON_SLOTS + PACK_SLOTS) as u32 {
            assert!(inv.take(Item::Weapon(Weapon::new(GunKind::Smg, s, Rarity::Common))));
        }
        assert!(inv.weapons[..WEAPON_SLOTS].iter().all(Option::is_some) && inv.pack[..PACK_SLOTS].iter().all(Option::is_some));
        assert!(!inv.take(Item::Weapon(Weapon::issue(GunKind::Rifle))));
        // More slots, more room.
        inv.grow(MAX_WEAPON_SLOTS, PACK_SLOTS);
        assert!(inv.take(Item::Weapon(Weapon::issue(GunKind::Rifle))));
        assert!(inv.weapons[MAX_WEAPON_SLOTS - 1].is_some());
        inv.weapons[MAX_WEAPON_SLOTS - 1] = None;
        inv.slots.0 = WEAPON_SLOTS as u8;
        // Emptied out, it's all there.
        let all = inv.empty_out();
        assert_eq!(all.len(), WEAPON_SLOTS + PACK_SLOTS);
        assert_eq!(inv, Inventory::default());
    }

    #[test]
    fn robots_mostly_drop_ammo_and_tough_ones_more_guns() {
        let guns = |toughness| {
            (0..1000)
                .filter(|s| robot_loot(*s, toughness).iter().any(|i| matches!(i, Item::Weapon(_))))
                .count()
        };
        let (swarmer, gunner) = (guns(30.0), guns(120.0));
        assert!(swarmer < gunner && gunner < 400, "{swarmer} {gunner}");
        assert!((0..100).all(|s| robot_loot(s, 30.0).iter().any(|i| matches!(i, Item::Ammo { .. }))));
    }

    #[test]
    fn a_roll_is_the_same_from_the_same_seed_and_only_crew_frames_roll() {
        let mut frames = std::collections::HashSet::new();
        let mut families = std::collections::HashSet::new();
        for seed in 0..3000 {
            let w = Weapon::roll(seed, 50);
            assert_eq!(w, Weapon::roll(seed, 50));
            assert!(GunKind::CREW.contains(&w.frame), "{w:?}");
            // Families only on the frames that take them.
            assert!(w.family == Family::Ballistic || GunKind::FAMILIES.contains(&w.frame), "{w:?}");
            assert_eq!(w.mag, w.stats().mag);
            frames.insert(w.frame);
            families.insert(w.family);
        }
        assert_eq!(frames.len(), GunKind::CREW.len());
        assert_eq!(families.len(), 1 + Family::ROLLED.len());
    }

    #[test]
    fn a_seeds_old_draws_still_make_its_frame_and_gun() {
        // A roll that stays ballistic and among the first frames is the gun
        // it always was: the same draws, in the same order.
        for seed in 0..500 {
            let w = Weapon::roll(seed, 30);
            if w.family == Family::Ballistic && GunKind::FIRST.contains(&w.frame) {
                let old = GunKind::FIRST[(draw(seed, 101) % 2) as usize];
                assert_eq!(w, Weapon::new(old, draw(seed, 102), Rarity::roll(draw(seed, 103), 30)));
            }
        }
    }

    #[test]
    fn a_family_changes_how_a_gun_shoots_not_what_it_is() {
        let rifle = Weapon::new(GunKind::Rifle, 77, Rarity::Rare);
        for family in Family::ROLLED {
            let w = rifle.with_family(family);
            assert_eq!((w.name(), w.build().parts, w.build().perks), (rifle.name(), rifle.build().parts, rifle.build().perks));
            assert_eq!(w.stats().family, family);
            assert!(!w.same_gun(&rifle));
            assert!(w.kind_name().starts_with(family.name()));
        }
        let (plasma, arc, laser) = (rifle.with_family(Family::Plasma).stats(), rifle.with_family(Family::Arc).stats(), rifle.with_family(Family::Laser).stats());
        let base = rifle.stats();
        assert!(plasma.muzzle_speed < base.muzzle_speed * 0.2 && plasma.splash > 0.0 && plasma.ammo == AmmoKind::Cells);
        assert!(arc.chain > 0 && arc.range > 0.0 && arc.ammo == AmmoKind::Cells);
        assert!(laser.heat > 0.0 && laser.mag == 0);
    }

    #[test]
    fn a_save_from_before_shells_and_cells_reads_its_two_pools() {
        let mut inv = Inventory::default();
        *inv.ammo_mut(AmmoKind::Light) = 120;
        *inv.ammo_mut(AmmoKind::Heavy) = 45;
        let now = ron::to_string(&inv).unwrap();
        assert!(now.contains("ammo:(120,45,0,0)"), "{now}");
        let old = now.replace("ammo:(120,45,0,0)", "ammo:(120,45)");
        let read: Inventory = ron::from_str(&old).unwrap();
        assert_eq!(read, inv);
        // And a gun saved before families is ballistic.
        let gun = Weapon::new(GunKind::Smg, 5, Rarity::Common);
        let saved = ron::to_string(&gun).unwrap().replace(",family:Ballistic", "");
        assert!(!saved.contains("family"), "{saved}");
        assert_eq!(ron::from_str::<Weapon>(&saved).unwrap(), gun);
    }

    #[test]
    fn robots_drop_every_kind_of_ammo() {
        let kinds: std::collections::HashSet<AmmoKind> = (0..200)
            .flat_map(|s| robot_loot(s, 30.0))
            .filter_map(|i| match i {
                Item::Ammo { kind, .. } => Some(kind),
                _ => None,
            })
            .collect();
        assert_eq!(kinds.len(), AmmoKind::COUNT);
    }
}
