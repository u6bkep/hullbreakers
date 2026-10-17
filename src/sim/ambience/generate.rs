//! Ambiences grown from a seed: for places that only need to be different
//! (a jump run's nodes, the sky preview's seeds). An [`Archetype`] gives a
//! generated sky its character; the seed, everything else. The same
//! archetype and seed give the same ambience on every peer and every run.
//!
//! Palettes are picked by hand, clear of the weak spots' magenta, and every
//! sky made here passes [`super::problems`] (tested over many seeds).

use bevy::prelude::*;
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};

use super::{Ambience, Band, Body, BodyKind, Layers, Nebula, Rgb, Rings, SpaceLight, Stars, Sun, degrees_between};

/// What kind of sky a generated ambience is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Archetype {
    /// A plain sky: a white sun, the band, a faint nebula or two.
    Calm,
    /// A dim, warm sun near a big disc; little light, long shadows.
    RedDwarf,
    /// Deep in a nebula: clouds on every side, lit at their edges.
    Nebula,
    /// In a gas giant's sky: the giant (maybe ringed) and its moons.
    GasGiant,
}

impl Archetype {
    pub const ALL: [Archetype; 4] = [Archetype::Calm, Archetype::RedDwarf, Archetype::Nebula, Archetype::GasGiant];

    /// What `HULLBREAKERS_AMBIENCE` calls it (`<id>:<seed>`).
    pub fn id(self) -> &'static str {
        match self {
            Archetype::Calm => "calm",
            Archetype::RedDwarf => "red-dwarf",
            Archetype::Nebula => "nebula",
            Archetype::GasGiant => "gas-giant",
        }
    }

    pub fn from_id(id: &str) -> Option<Archetype> {
        Self::ALL.into_iter().find(|a| a.id() == id)
    }
}

const fn hex(v: u32) -> Rgb {
    Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Stars: bluish, white, warm.
const STAR_COLOURS: [Rgb; 4] = [hex(0xcfdcff), hex(0xffffff), hex(0xffe6c4), hex(0xffc896)];
/// Cool gas: teals, blues, a sea green.
const COOL_GAS: [(Rgb, Rgb); 4] = [
    (hex(0x5fc4c9), hex(0x1d4a63)),
    (hex(0x6fa8ff), hex(0x1c2f66)),
    (hex(0x7fd6a8), hex(0x1d4d45)),
    (hex(0x9ad8ff), hex(0x24476e)),
];
/// Warm gas: golds and ambers.
const WARM_GAS: [(Rgb, Rgb); 3] = [
    (hex(0xffc36b), hex(0x6b3a1c)),
    (hex(0xffd98f), hex(0x5c4a22)),
    (hex(0xf2a65a), hex(0x5a2e16)),
];
/// Gas giants' bands.
const GIANT_BANDS: [[Rgb; 3]; 4] = [
    [hex(0xd9b48a), hex(0xa8794f), hex(0xf0dcc0)],
    [hex(0x8fb8c8), hex(0x5d8aa0), hex(0xd4e6ea)],
    [hex(0xc9c28a), hex(0x8f8a52), hex(0xe8e2b8)],
    [hex(0xd99a6c), hex(0x9c5a3c), hex(0xf2c9a2)],
];
/// Rock and ice.
const ROCK: [[Rgb; 2]; 3] = [[hex(0x8a8580), hex(0x5c5853)], [hex(0x9c8a72), hex(0x625545)], [hex(0x7d8590), hex(0x4e5560)]];
const ICE: [[Rgb; 2]; 2] = [[hex(0xdfeef5), hex(0x9ab8c8)], [hex(0xe8f0e6), hex(0xa6bfae)]];

/// A generated ambience. Every one passes [`super::problems`].
pub fn generate(archetype: Archetype, seed: u64) -> Ambience {
    let mut rng = ChaCha8Rng::seed_from_u64(seed ^ (archetype as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    let mut sky = match archetype {
        Archetype::Calm => calm(&mut rng),
        Archetype::RedDwarf => red_dwarf(&mut rng),
        Archetype::Nebula => nebula(&mut rng),
        Archetype::GasGiant => gas_giant(&mut rng),
    };
    sky.name = format!("{} {seed}", archetype.id());
    sky
}

fn unit(rng: &mut ChaCha8Rng) -> Vec3 {
    let z: f32 = rng.random_range(-1.0..1.0);
    let a: f32 = rng.random_range(0.0..std::f32::consts::TAU);
    let r = (1.0 - z * z).sqrt();
    Vec3::new(r * a.cos(), z, r * a.sin())
}

/// Toward the sun: up a little or a lot, from any side, never straight
/// overhead or underfoot (where a ship's deck would hide it, or its light
/// would come down a cabin's roof).
fn sun_direction(rng: &mut ChaCha8Rng) -> Vec3 {
    let a: f32 = rng.random_range(0.0..std::f32::consts::TAU);
    let up: f32 = rng.random_range(-0.25..0.75);
    Vec3::new(a.cos(), up, a.sin()).normalize()
}

/// A direction at least `clear` degrees from every one of `away` (as
/// (direction, degrees) pairs), or the best of a few tries.
fn clear_of(rng: &mut ChaCha8Rng, away: &[(Vec3, f32)]) -> Vec3 {
    let mut best = (Vec3::X, f32::MIN);
    for _ in 0..64 {
        let d = unit(rng);
        let margin = away.iter().map(|&(a, clear)| degrees_between(d, a) - clear).fold(f32::MAX, f32::min);
        if margin > 0.0 {
            return d;
        }
        if margin > best.1 {
            best = (d, margin);
        }
    }
    best.0
}

fn pick<T: Copy>(rng: &mut ChaCha8Rng, from: &[T]) -> T {
    from[rng.random_range(0..from.len())]
}

/// A direction square to `v`.
fn square_to(rng: &mut ChaCha8Rng, v: Vec3) -> Vec3 {
    let v = v.normalize();
    let other = if v.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
    let a = v.cross(other).normalize();
    let b = v.cross(a);
    let t: f32 = rng.random_range(0.0..std::f32::consts::TAU);
    a * t.cos() + b * t.sin()
}

/// A band whose brightest part (its bulge) reaches `peak` (luminance, sky
/// units).
fn band(rng: &mut ChaCha8Rng, peak: f32, dust: f32) -> Band {
    let pole = unit(rng);
    let (color, bulge_color) = pick(rng, &[(hex(0xa9bedb), hex(0xe8d2a8)), (hex(0xb8c4d6), hex(0xf0dcb4)), (hex(0x9fb6c9), hex(0xd8c49a))]);
    Band {
        pole,
        width: rng.random_range(12.0..24.0),
        bulge: square_to(rng, pole),
        color,
        bulge_color,
        brightness: peak / (color.luminance() + super::BULGE_GAIN * bulge_color.luminance()),
        dust,
        seed: rng.random(),
        tones: rng.random_range(3..=5),
    }
}

/// A cloud whose brightest part (a lit rim) reaches `peak` (luminance, sky
/// units).
fn gas_cloud(rng: &mut ChaCha8Rng, colours: (Rgb, Rgb), direction: Vec3, size: f32, peak: f32) -> Nebula {
    let mut cloud = Nebula {
        direction,
        size,
        inner: colours.0,
        outer: colours.1,
        brightness: 0.0,
        seed: rng.random(),
        tones: rng.random_range(2..=4),
        dust: rng.random_range(0.0..0.6),
        rim: rng.random_range(0.2..0.8),
        stretch: rng.random_range(1.0..2.5),
        turn: rng.random_range(0.0..180.0),
    };
    cloud.set_peak(peak);
    cloud
}

impl Nebula {
    /// Sets its brightness so its brightest part reaches `peak` (as
    /// [`super::problems`] judges it).
    fn set_peak(&mut self, peak: f32) {
        self.brightness = peak / (self.inner.luminance().max(self.outer.luminance()) * (1.0 + self.rim));
    }
}

fn stars(rng: &mut ChaCha8Rng, count: u32, banded: f32) -> Stars {
    Stars {
        seed: rng.random(),
        count,
        banded,
        brightness: rng.random_range(5.0..9.0),
        colors: STAR_COLOURS.to_vec(),
        glints: rng.random_range(20..48),
    }
}

/// A small rocky (or icy) moon, clear of the sun and of `away`.
fn moon(rng: &mut ChaCha8Rng, away: &[(Vec3, f32)]) -> Body {
    let icy = rng.random_bool(0.3);
    let colors = if icy { pick(rng, &ICE) } else { pick(rng, &ROCK) };
    Body {
        kind: if icy { BodyKind::Ice } else { BodyKind::Rocky },
        direction: clear_of(rng, away),
        size: rng.random_range(1.5..5.0),
        colors: colors.to_vec(),
        seed: rng.random(),
        axis: unit(rng),
        rings: None,
        atmosphere: None,
        light: 0.0,
    }
}

fn calm(rng: &mut ChaCha8Rng) -> Ambience {
    let sun = Sun {
        direction: sun_direction(rng),
        color: pick(rng, &[hex(0xfff4e0), hex(0xffffff), hex(0xfff0d0)]),
        illuminance: rng.random_range(11_000.0..14_000.0),
        size: rng.random_range(1.2..2.0),
        corona: rng.random_range(1.0..2.5),
    };
    let (brightness, dust) = (rng.random_range(0.08..0.14), rng.random_range(0.3..0.7));
    let band = band(rng, brightness, dust);
    let mut nebulae = Vec::new();
    for _ in 0..rng.random_range(0..=2) {
        let (colours, direction) = (pick(rng, &COOL_GAS), unit(rng));
        let (size, brightness) = (rng.random_range(20.0..50.0), rng.random_range(0.08..0.16));
        nebulae.push(gas_cloud(rng, colours, direction, size, brightness));
    }
    let mut bodies = Vec::new();
    if rng.random_bool(0.5) {
        bodies.push(moon(rng, &[(sun.direction, 16.0)]));
    }
    Ambience {
        name: String::new(),
        light: SpaceLight {
            ambient: hex(0x99b3ff),
            ambient_brightness: 60.0,
            fill: 3500.0,
        },
        stars: stars(rng, 16_000, 0.25),
        sun,
        band: Some(band),
        nebulae,
        bodies,
        show: Layers::default(),
    }
}

fn red_dwarf(rng: &mut ChaCha8Rng) -> Ambience {
    let sun = Sun {
        direction: sun_direction(rng),
        color: pick(rng, &[hex(0xffa86b), hex(0xff9a5c), hex(0xffb27a)]),
        illuminance: rng.random_range(5_000.0..8_000.0),
        size: rng.random_range(3.0..6.0),
        corona: rng.random_range(0.6..1.4),
    };
    let (brightness, dust) = (rng.random_range(0.05..0.08), rng.random_range(0.5..0.9));
    let band = band(rng, brightness, dust);
    let mut nebulae = Vec::new();
    for _ in 0..rng.random_range(1..=2) {
        let (colours, direction) = (pick(rng, &WARM_GAS), unit(rng));
        let (size, brightness) = (rng.random_range(25.0..60.0), rng.random_range(0.05..0.1));
        nebulae.push(gas_cloud(rng, colours, direction, size, brightness));
    }
    let sun_clear = sun.size / 2.0 * (1.0 + sun.corona) + 12.0;
    let mut bodies = vec![moon(rng, &[(sun.direction, sun_clear)])];
    if rng.random_bool(0.5) {
        let first = (bodies[0].direction, bodies[0].reach() + 8.0);
        bodies.push(moon(rng, &[(sun.direction, sun_clear), first]));
    }
    Ambience {
        name: String::new(),
        light: SpaceLight {
            ambient: hex(0xc9a48a),
            ambient_brightness: 35.0,
            fill: 2200.0,
        },
        stars: stars(rng, 17_000, 0.25),
        sun,
        band: Some(band),
        nebulae,
        bodies,
        show: Layers::default(),
    }
}

fn nebula(rng: &mut ChaCha8Rng) -> Ambience {
    let sun = Sun {
        direction: sun_direction(rng),
        color: pick(rng, &[hex(0xe6f0ff), hex(0xffffff)]),
        illuminance: rng.random_range(10_000.0..14_000.0),
        size: rng.random_range(1.0..1.8),
        corona: rng.random_range(1.5..3.0),
    };
    let warm = rng.random_bool(0.35);
    let mut nebulae = Vec::new();
    for i in 0..rng.random_range(3..=5) {
        let colours = if warm && i % 2 == 1 { pick(rng, &WARM_GAS) } else { pick(rng, &COOL_GAS) };
        let direction = unit(rng);
        let (size, brightness) = (rng.random_range(50.0..120.0), rng.random_range(0.1..0.2));
        let mut cloud = gas_cloud(rng, colours, direction, size, brightness);
        cloud.dust = rng.random_range(0.3..0.9);
        cloud.rim = rng.random_range(0.6..1.2);
        cloud.set_peak(brightness);
        nebulae.push(cloud);
    }
    Ambience {
        name: String::new(),
        light: SpaceLight {
            ambient: hex(0x9fc4d6),
            ambient_brightness: 60.0,
            fill: 2600.0,
        },
        // Fewer stars show through the gas.
        stars: stars(rng, 9_000, 0.0),
        sun,
        band: None,
        nebulae,
        bodies: Vec::new(),
        show: Layers::default(),
    }
}

fn gas_giant(rng: &mut ChaCha8Rng) -> Ambience {
    let sun = Sun {
        direction: sun_direction(rng),
        color: pick(rng, &[hex(0xfff4e0), hex(0xffffff)]),
        illuminance: rng.random_range(10_000.0..13_000.0),
        size: rng.random_range(1.0..1.6),
        corona: rng.random_range(1.0..2.0),
    };
    let size = rng.random_range(18.0..36.0);
    let rings = rng.random_bool(0.6).then(|| {
        let inner = rng.random_range(1.3..1.7);
        Rings {
            inner,
            outer: inner + rng.random_range(0.4..1.0),
            color: pick(rng, &[hex(0xcbbfa8), hex(0xb8b0a0), hex(0xd8d0c0)]),
        }
    });
    let mut giant = Body {
        kind: BodyKind::GasGiant,
        // Far enough from the sun that it's lit from the side.
        direction: Vec3::X,
        size,
        colors: pick(rng, &GIANT_BANDS).to_vec(),
        seed: rng.random(),
        axis: Vec3::Y,
        rings,
        atmosphere: Some(pick(rng, &[hex(0xbfd8ff), hex(0xd8e8f0)])),
        light: 0.0,
    };
    let sun_clear = sun.size / 2.0 * (1.0 + sun.corona) + 3.0;
    giant.direction = clear_of(rng, &[(sun.direction, giant.reach() + sun_clear + 30.0)]);
    // Its pole tipped a little from square to the sun, so the rings show.
    let tip: f32 = rng.random_range(-0.4..0.4);
    giant.axis = (square_to(rng, giant.direction) + giant.direction * tip).normalize();
    let (brightness, dust) = (rng.random_range(0.06..0.1), rng.random_range(0.3..0.7));
    let band = band(rng, brightness, dust);
    let mut bodies = vec![giant];
    for _ in 0..rng.random_range(1..=2) {
        let mut away = vec![(sun.direction, sun_clear + 6.0)];
        away.extend(bodies.iter().map(|b| (b.direction, b.reach() + 6.0)));
        bodies.push(moon(rng, &away));
    }
    Ambience {
        name: String::new(),
        light: SpaceLight {
            ambient: hex(0xa6b8e6),
            ambient_brightness: 55.0,
            fill: 3200.0,
        },
        stars: stars(rng, 15_000, 0.25),
        sun,
        band: Some(band),
        nebulae: Vec::new(),
        bodies,
        show: Layers::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_grows_the_same_sky() {
        for archetype in Archetype::ALL {
            assert_eq!(generate(archetype, 7), generate(archetype, 7));
            assert_ne!(generate(archetype, 7), generate(archetype, 8));
        }
    }

    #[test]
    fn every_generated_sky_passes() {
        for archetype in Archetype::ALL {
            for seed in 0..500 {
                let sky = generate(archetype, seed);
                let problems = super::super::problems(&sky);
                assert!(problems.is_empty(), "{}: {problems:#?}", sky.name);
            }
        }
    }
}
