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

/// The `salt`th number drawn from `seed`.
pub fn draw(seed: u32, salt: u32) -> u32 {
    mix(seed ^ mix(salt.wrapping_mul(0x9e37_79b9).wrapping_add(0x632b_e5ab)))
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
    /// Rifle rounds.
    Heavy,
}

impl AmmoKind {
    pub const ALL: [AmmoKind; 2] = [AmmoKind::Light, AmmoKind::Heavy];
    pub const COUNT: usize = Self::ALL.len();

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            AmmoKind::Light => "Light rounds",
            AmmoKind::Heavy => "Heavy rounds",
        }
    }

    /// The most a pool holds.
    pub fn cap(self) -> u16 {
        match self {
            AmmoKind::Light => 400,
            AmmoKind::Heavy => 200,
        }
    }

    /// A box of it: what a robot drops, and what dropping ammo leaves.
    pub fn box_rounds(self) -> u16 {
        match self {
            AmmoKind::Light => 60,
            AmmoKind::Heavy => 30,
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

    /// Every slot's variant, barrel first.
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

    /// The word a gun is named for when this is its most telling part.
    fn epithet(self) -> Option<&'static str> {
        Some(match self {
            PartVariant::Barrel(Barrel::Long) => "Longshot",
            PartVariant::Barrel(Barrel::Short) => "Snub",
            PartVariant::Barrel(Barrel::Heavy) => "Hammer",
            PartVariant::Magazine(Magazine::Extended) => "Feeder",
            PartVariant::Magazine(Magazine::Drum) => "Drummer",
            PartVariant::Magazine(Magazine::Quick) => "Snap",
            PartVariant::Muzzle(Muzzle::Compensator) => "Steady",
            PartVariant::Muzzle(Muzzle::Booster) => "Howler",
            PartVariant::Stock(Stock::Light) => "Sprinter",
            PartVariant::Stock(Stock::Heavy) => "Anchor",
            PartVariant::Sight(Sight::Reflex) => "Glance",
            PartVariant::Sight(Sight::Scope) => "Watcher",
            _ => return None,
        })
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

/// A gun as an object. See the module docs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Weapon {
    pub frame: GunKind,
    pub seed: u32,
    pub rarity: Rarity,
    /// Rounds in the mag.
    pub mag: u16,
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
}

impl Weapon {
    /// A standard-issue gun of `frame`, its mag full.
    pub fn issue(frame: GunKind) -> Weapon {
        Weapon::new(frame, 0, Rarity::Standard)
    }

    /// The gun `seed` makes of `frame` at `rarity`, its mag full.
    pub fn new(frame: GunKind, seed: u32, rarity: Rarity) -> Weapon {
        let mut w = Weapon { frame, seed, rarity, mag: 0 };
        w.mag = w.stats().mag;
        w
    }

    /// A gun rolled from `seed`: a crew frame, a rarity by `luck` (0–100).
    pub fn roll(seed: u32, luck: u32) -> Weapon {
        let frame = GunKind::CREW[(draw(seed, 101) % GunKind::CREW.len() as u32) as usize];
        Weapon::new(frame, draw(seed, 102), Rarity::roll(draw(seed, 103), luck))
    }

    /// Whether two are the same gun (whatever is in their mags).
    pub fn same_gun(&self, other: &Weapon) -> bool {
        (self.frame, self.seed, self.rarity) == (other.frame, other.seed, other.rarity)
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
        // Rarer guns come stock less often.
        let plain = 40 - 8 * self.rarity.tier();
        let part = |i: u32, options: &[PartVariant]| -> PartVariant {
            let weighted: Vec<(PartVariant, u32)> = options
                .iter()
                .map(|&p| (p, if p.standard() { plain } else { 20 * maker.favours(p) }))
                .collect();
            pick(draw(s, salt::PARTS + i), &weighted)
        };
        use PartVariant as P;
        let parts = Parts {
            barrel: match part(0, &[P::Barrel(Barrel::Standard), P::Barrel(Barrel::Long), P::Barrel(Barrel::Short), P::Barrel(Barrel::Heavy)]) {
                P::Barrel(b) => b,
                _ => unreachable!(),
            },
            magazine: match part(
                1,
                &[
                    P::Magazine(Magazine::Standard),
                    P::Magazine(Magazine::Extended),
                    P::Magazine(Magazine::Drum),
                    P::Magazine(Magazine::Quick),
                ],
            ) {
                P::Magazine(m) => m,
                _ => unreachable!(),
            },
            stock: match part(2, &[P::Stock(Stock::Standard), P::Stock(Stock::Light), P::Stock(Stock::Heavy)]) {
                P::Stock(x) => x,
                _ => unreachable!(),
            },
            sight: match part(3, &[P::Sight(Sight::Iron), P::Sight(Sight::Reflex), P::Sight(Sight::Scope)]) {
                P::Sight(x) => x,
                _ => unreachable!(),
            },
            muzzle: match part(4, &[P::Muzzle(Muzzle::Bare), P::Muzzle(Muzzle::Compensator), P::Muzzle(Muzzle::Booster)]) {
                P::Muzzle(x) => x,
                _ => unreachable!(),
            },
        };
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

    /// Its name: "Kessler Mk3 Longshot". Standard issue is "Issue SMG".
    pub fn name(&self) -> String {
        if self.rarity == Rarity::Standard {
            return format!("Issue {}", self.frame.name());
        }
        let build = self.build();
        let mark = 1 + draw(self.seed, salt::MARK) % 3 + self.rarity.tier().saturating_sub(1);
        let epithet = build
            .parts
            .all()
            .into_iter()
            .find_map(PartVariant::epithet)
            .unwrap_or(match self.frame {
                GunKind::Rifle => "Ranger",
                _ => "Buzzer",
            });
        format!("{} Mk{mark} {epithet}", build.maker.name())
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

/// Weapon slots: what stances point at.
pub const WEAPON_SLOTS: usize = 3;
/// Pack slots, for spares and small cargo.
pub const PACK_SLOTS: usize = 6;

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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    pub weapons: [Option<Weapon>; WEAPON_SLOTS],
    pub pack: [Option<Item>; PACK_SLOTS],
    /// Rounds in each pool, by [`AmmoKind::index`].
    pub ammo: [u16; AmmoKind::COUNT],
    /// Bulky cargo in both hands, if any.
    pub carrying: Option<CargoKind>,
}

impl Inventory {
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
    pub fn fits(place: Place, item: &Item) -> bool {
        match place {
            Place::Weapon(i) => (i as usize) < WEAPON_SLOTS && matches!(item, Item::Weapon(_)),
            Place::Pack(i) => (i as usize) < PACK_SLOTS && !item.bulky() && !matches!(item, Item::Ammo { .. }),
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
        if from == to || !Self::fits(to, &moving) {
            return false;
        }
        let there = self.get(to);
        if there.is_some_and(|t| !Self::fits(from, &t)) {
            return false;
        }
        self.put(from, there);
        self.put(to, Some(moving));
        true
    }

    /// The first free place `item` fits: a weapon slot, else the pack (or
    /// the hands, for bulky cargo).
    pub fn free_place(&self, item: &Item) -> Option<Place> {
        let weapons = (0..WEAPON_SLOTS as u8).map(Place::Weapon);
        let pack = (0..PACK_SLOTS as u8).map(Place::Pack);
        weapons
            .chain(pack)
            .chain([Place::Hands])
            .find(|p| Self::fits(*p, item) && self.get(*p).is_none())
    }

    /// Takes `item` in: a gun into the first free weapon slot, else the
    /// pack; ammo into its pool (unless the pool is full; what's over the
    /// pool's cap is lost); cargo into the hands or the pack. Whether it
    /// was taken.
    pub fn take(&mut self, item: Item) -> bool {
        if let Item::Ammo { kind, rounds } = item {
            let pool = self.ammo_mut(kind);
            if *pool >= kind.cap() {
                return false;
            }
            *pool = pool.saturating_add(rounds).min(kind.cap());
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
        *self = Inventory::default();
        items
    }
}

// ---------------------------------------------------------------------------
// Loot.

/// What a robot leaves: mostly ammo, sometimes a gun. `toughness` (its full
/// health) makes guns likelier and better.
pub fn robot_loot(seed: u32, toughness: f32) -> Vec<Item> {
    let tough = (toughness.clamp(0.0, 200.0) as u32).min(200);
    let mut items = Vec::new();
    let kind = AmmoKind::ALL[(draw(seed, 1) % 2) as usize];
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
        assert!(inv.take(Item::Ammo { kind: AmmoKind::Heavy, rounds: 150 }));
        assert!(inv.take(Item::Ammo { kind: AmmoKind::Heavy, rounds: 150 }));
        assert_eq!(inv.ammo(AmmoKind::Heavy), AmmoKind::Heavy.cap());
        assert!(!inv.take(Item::Ammo { kind: AmmoKind::Heavy, rounds: 1 }), "full pool");
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
            "Kessler Mk4 Feeder".to_string(),
            Maker::Kessler,
            Parts {
                barrel: Barrel::Standard,
                magazine: Magazine::Extended,
                stock: Stock::Heavy,
                sight: Sight::Scope,
                muzzle: Muzzle::Compensator,
            },
            [Some(Perk::Steady), None],
        )
    });

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
            assert!(part.epithet().is_some(), "{part:?} has no name");
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
    fn names_read_maker_mark_epithet() {
        for s in 0..50 {
            let w = Weapon::new(GunKind::Rifle, s, Rarity::Uncommon);
            let name = w.name();
            let words: Vec<&str> = name.split(' ').collect();
            assert_eq!(words.len(), 3, "{name}");
            assert_eq!(words[0], w.build().maker.name());
            assert!(words[1].starts_with("Mk"), "{name}");
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
        assert!(inv.weapons.iter().all(Option::is_some) && inv.pack.iter().all(Option::is_some));
        assert!(!inv.take(Item::Weapon(Weapon::issue(GunKind::Rifle))));
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
}
