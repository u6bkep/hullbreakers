//! Ambience: how a place looks from inside it. Its sky (the sun, stars, a
//! galactic band, nebulae, the bodies hanging in it) and the light that sky
//! gives, as data.
//!
//! Cosmetic only: nothing in the simulation reads it. A scenario says which
//! ambience its world is in ([`Choice`]), and the authority spawns it with
//! the world as a [`Staged`](super::mission::Staged) entity, replicated
//! whole (it's a few hundred bytes), so a client draws exactly what the
//! authority chose without any files of its own (see `client::ambience`).
//!
//! An ambience is either **named**, written by hand in
//! `assets/ambience/<name>.ron` (embedded at build time, like the ship
//! designs; the sky preview reads the files from disk to reload them as
//! they're edited), or **generated** from an [`Archetype`] and a seed
//! (see [`generate`]), for places that only need to be different.
//!
//! Directions are in world axes, toward the thing (the sun's is toward the
//! sun), and needn't be unit length. Angular sizes are in degrees across.
//! Colours are sRGB hex. Brightness is in **sky units**: 1.0 is about the
//! brightest a backdrop could sensibly be (the client scales it to cd/m²).
//! [`problems`] judges an ambience against what play needs of a sky (see
//! docs/plan-ambience.md): the weak-spot magenta kept clear, the backdrop
//! dark enough that models still read as silhouettes against it.

pub mod generate;

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use bevy::color::{Oklcha, Srgba};
use bevy::prelude::*;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

pub use generate::{Archetype, generate};

include!(concat!(env!("OUT_DIR"), "/ambience_files.rs"));

/// A colour as written: sRGB, `"#rrggbb"`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub [u8; 3]);

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Rgb {
        Rgb([r, g, b])
    }

    /// From `"#rrggbb"` (the `#` optional).
    pub fn parse(text: &str) -> Result<Rgb, String> {
        let hex = text.strip_prefix('#').unwrap_or(text);
        let bad = || format!("{text:?} isn't a colour (#rrggbb)");
        if hex.len() != 6 {
            return Err(bad());
        }
        let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).map_err(|_| bad());
        Ok(Rgb([byte(0)?, byte(2)?, byte(4)?]))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0[0], self.0[1], self.0[2])
    }

    pub fn srgba(self) -> Srgba {
        Srgba::rgb_u8(self.0[0], self.0[1], self.0[2])
    }

    /// Linear RGB, 0..1.
    pub fn linear(self) -> Vec3 {
        let l = LinearRgba::from(self.srgba());
        Vec3::new(l.red, l.green, l.blue)
    }

    /// Its relative luminance (linear).
    pub fn luminance(self) -> f32 {
        self.linear().dot(Vec3::new(0.2126, 0.7152, 0.0722))
    }

    /// Oklch chroma and hue (degrees).
    pub fn chroma_hue(self) -> (f32, f32) {
        let c = Oklcha::from(self.srgba());
        (c.chroma, c.hue)
    }
}

impl Serialize for Rgb {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Rgb {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Rgb, D::Error> {
        let text = String::deserialize(d)?;
        Rgb::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// How a place looks: its sky and the light it gives. See the module docs.
#[derive(Component, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ambience {
    /// What it's called: a file's name, or the archetype and seed it was
    /// grown from.
    #[serde(default)]
    pub name: String,
    pub sun: Sun,
    pub light: SpaceLight,
    pub stars: Stars,
    #[serde(default)]
    pub band: Option<Band>,
    #[serde(default)]
    pub nebulae: Vec<Nebula>,
    #[serde(default)]
    pub bodies: Vec<Body>,
    /// Which layers are drawn (all, unless the sky preview's levers have
    /// taken some away).
    #[serde(default)]
    pub show: Layers,
}

/// The star the place is lit by. It casts the shadows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sun {
    /// Toward the sun.
    pub direction: Vec3,
    /// The colour of its light, and of its disc.
    pub color: Rgb,
    /// Its light (lux).
    pub illuminance: f32,
    /// Its disc, degrees across.
    pub size: f32,
    /// How far its corona reaches past the disc, in disc radii (0: none).
    #[serde(default)]
    pub corona: f32,
}

/// The light of open space besides the sun.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpaceLight {
    /// Bevy's global ambient light: its colour and brightness.
    pub ambient: Rgb,
    pub ambient_brightness: f32,
    /// The sky's fill: the strength of the light the sky itself gives (the
    /// cameras' environment light, cd/m² per sky unit).
    pub fill: f32,
}

/// The stars: generated from this on every peer, the same everywhere.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stars {
    pub seed: u64,
    pub count: u32,
    /// The share of them gathered along the band (0..1), if there is one.
    #[serde(default)]
    pub banded: f32,
    /// How bright the brightest are (sky units; most are far fainter).
    pub brightness: f32,
    /// What colours they come in (picked from evenly).
    pub colors: Vec<Rgb>,
    /// How many of the brightest get a four-point glint.
    #[serde(default)]
    pub glints: u32,
}

/// A galactic band: the glow of a galaxy's disc seen edge-on, all the way
/// round the sky.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Band {
    /// The band's axis: it runs round the great circle square to this.
    pub pole: Vec3,
    /// Degrees across.
    pub width: f32,
    /// Where along it the galaxy's core is (brighter and wider there).
    pub bulge: Vec3,
    pub color: Rgb,
    pub bulge_color: Rgb,
    pub brightness: f32,
    /// How much dark dust crosses it (0..1).
    #[serde(default)]
    pub dust: f32,
    #[serde(default)]
    pub seed: u32,
    /// How many steps of brightness it's drawn in.
    #[serde(default = "three")]
    pub tones: u32,
}

/// A nebula: a cloud of glowing gas.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Nebula {
    /// Toward its middle.
    pub direction: Vec3,
    /// Degrees across.
    pub size: f32,
    /// Its colour in its densest parts, and at its edges.
    pub inner: Rgb,
    pub outer: Rgb,
    pub brightness: f32,
    #[serde(default)]
    pub seed: u32,
    /// How many steps of brightness it's drawn in.
    #[serde(default = "three")]
    pub tones: u32,
    /// How much dark dust there is in it (0..1).
    #[serde(default)]
    pub dust: f32,
    /// How brightly its edges facing the sun are lit (0: not at all).
    #[serde(default)]
    pub rim: f32,
    /// How much longer than wide (1: round), and which way it's drawn out
    /// (degrees round its direction).
    #[serde(default = "one")]
    pub stretch: f32,
    #[serde(default)]
    pub turn: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyKind {
    /// Faceted rock, like the asteroids.
    Rocky,
    /// Banded cloud tops.
    GasGiant,
    /// Faceted ice: paler, with a sheen.
    Ice,
    /// A companion star: a glowing disc that can light the place too.
    Star,
}

/// A planet, moon or companion star, at infinity: background only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Body {
    pub kind: BodyKind,
    /// Toward it.
    pub direction: Vec3,
    /// Degrees across.
    pub size: f32,
    /// Its palette: one to three colours (bands, facets).
    pub colors: Vec<Rgb>,
    #[serde(default)]
    pub seed: u32,
    /// Its pole: the way its bands and its rings lie square to.
    #[serde(default = "up")]
    pub axis: Vec3,
    #[serde(default)]
    pub rings: Option<Rings>,
    /// The colour of the glow round its edge, if it has an atmosphere.
    #[serde(default)]
    pub atmosphere: Option<Rgb>,
    /// A star's light (lux), unshadowed; 0 for anything else.
    #[serde(default)]
    pub light: f32,
}

/// A body's rings, in its equator's plane.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rings {
    /// From and to, in body radii.
    pub inner: f32,
    pub outer: f32,
    pub color: Rgb,
}

/// Which of the sky's layers are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Layers {
    pub stars: bool,
    pub band: bool,
    pub nebulae: bool,
    pub bodies: bool,
    /// The sun's disc (its light stays).
    pub sun: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Layers {
            stars: true,
            band: true,
            nebulae: true,
            bodies: true,
            sun: true,
        }
    }
}

fn three() -> u32 {
    3
}

fn one() -> f32 {
    1.0
}

fn up() -> Vec3 {
    Vec3::Y
}

/// How many nebulae and bodies a sky can have (what the shaders hold).
pub const MAX_NEBULAE: usize = 8;
pub const MAX_BODIES: usize = 4;
pub const MAX_STARS: u32 = 60_000;

/// The brightest a backdrop feature may be at its peak (luminance, sky
/// units): past this, a dark model in front of it stops reading as a dark
/// outline, and the HUD's hairlines stop glowing over it. Brightness
/// belongs in small hot points: stars and the sun.
pub const BACKDROP_CAP: f32 = 0.3;
/// How much brighter than the band's own colour its bulge gets (the
/// shader's gain, which the cap judges by).
pub const BULGE_GAIN: f32 = 1.5;

/// The weak spots' magenta (art-style.md), and how far (degrees of Oklch
/// hue) every coloured thing in the sky keeps from it, unless it's nearly
/// grey. The same rule as `tools/assets/verify_glb.py`.
pub const WEAK_SPOT: Rgb = Rgb::new(0xff, 0x40, 0xc8);
pub const WEAK_SPOT_HUE_CLEARANCE: f32 = 45.0;
pub const GREY_CHROMA: f32 = 0.04;

impl Sun {
    /// Unit length, toward the sun.
    pub fn toward(&self) -> Vec3 {
        self.direction.normalize_or(Vec3::Y)
    }
}

impl Body {
    /// How far it reaches from its middle (degrees), rings and air included.
    pub fn reach(&self) -> f32 {
        let mut reach = self.size / 2.0;
        if let Some(rings) = &self.rings {
            reach *= rings.outer.max(1.0);
        }
        if self.atmosphere.is_some() {
            reach *= 1.1;
        }
        reach
    }
}

impl Ambience {
    /// The named ambience `name` (embedded at build time).
    pub fn named(name: &str) -> Result<Ambience, String> {
        let text = AMBIENCE_FILES
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, text)| *text)
            .ok_or_else(|| format!("no ambience called {name:?} ({})", names().join(", ")))?;
        parse(name, text)
    }

    /// The reference sky: what models are judged under (`asset_view`), and
    /// what a client shows until the authority's ambience arrives.
    pub fn reference() -> Ambience {
        Ambience::named(REFERENCE).expect("the reference ambience is shipped and parses")
    }

    /// Unit length, toward the sun.
    pub fn sun_toward(&self) -> Vec3 {
        self.sun.toward()
    }
}

/// The reference ambience's name.
pub const REFERENCE: &str = "reference";

/// Every named ambience, sorted.
pub fn names() -> Vec<&'static str> {
    AMBIENCE_FILES.iter().map(|(n, _)| *n).collect()
}

/// An ambience from its file's text, named `name`.
pub fn parse(name: &str, text: &str) -> Result<Ambience, String> {
    let options = ron::Options::default().with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME);
    let mut ambience: Ambience = options.from_str(text).map_err(|e| format!("{name}.ron: {e}"))?;
    ambience.name = name.to_string();
    Ok(ambience)
}

/// Where the named ambiences' files are on disk, to read them as they're
/// edited: the repo's when run from a build of it, otherwise beside
/// wherever the game is started.
pub fn source_dir() -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/ambience");
    if repo.is_dir() { repo } else { PathBuf::from("assets/ambience") }
}

/// The named ambience `name` as its file on disk says now (falling back to
/// the embedded one if there's no file).
pub fn read_named(name: &str) -> Result<Ambience, String> {
    match std::fs::read_to_string(source_dir().join(format!("{name}.ron"))) {
        Ok(text) => parse(name, &text),
        Err(_) => Ambience::named(name),
    }
}

/// Which ambience a place is in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choice {
    /// A file in `assets/ambience`.
    Named(Cow<'static, str>),
    /// Grown from an archetype and a seed.
    Generated(Archetype, u64),
}

impl Choice {
    pub const fn named(name: &'static str) -> Choice {
        Choice::Named(Cow::Borrowed(name))
    }

    /// From `<name>` or `<archetype>:<seed>` (`HULLBREAKERS_AMBIENCE`).
    pub fn parse(text: &str) -> Result<Choice, String> {
        if let Some((archetype, seed)) = text.split_once(':') {
            let archetype = Archetype::from_id(archetype).ok_or_else(|| {
                let known: Vec<&str> = Archetype::ALL.iter().map(|a| a.id()).collect();
                format!("no archetype {archetype:?} ({})", known.join(", "))
            })?;
            let seed = seed.parse().map_err(|_| format!("{seed:?} isn't a seed"))?;
            return Ok(Choice::Generated(archetype, seed));
        }
        read_named(text)?;
        Ok(Choice::Named(Cow::Owned(text.to_string())))
    }

    /// The ambience chosen. A named one is read from its file if it's
    /// there (the sky preview's edits), else as embedded.
    pub fn make(&self) -> Result<Ambience, String> {
        match self {
            Choice::Named(name) => read_named(name),
            Choice::Generated(archetype, seed) => Ok(generate(*archetype, *seed)),
        }
    }
}

/// Degrees between two directions.
pub fn degrees_between(a: Vec3, b: Vec3) -> f32 {
    a.normalize_or_zero().dot(b.normalize_or_zero()).clamp(-1.0, 1.0).acos().to_degrees()
}

/// What's wrong with an ambience, if anything: a line per problem.
pub fn problems(a: &Ambience) -> Vec<String> {
    let mut problems = Vec::new();
    let mut check = |ok: bool, what: String| {
        if !ok {
            problems.push(what);
        }
    };
    let direction = |v: Vec3| v.is_finite() && v.length() > 1e-3;
    let in_range = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);

    // Colours: clear of the weak spots' magenta.
    let (_, magenta) = WEAK_SPOT.chroma_hue();
    let mut colours: Vec<(String, Rgb)> = vec![
        ("sun".into(), a.sun.color),
        ("ambient light".into(), a.light.ambient),
    ];
    colours.extend(a.stars.colors.iter().enumerate().map(|(i, c)| (format!("star colour {i}"), *c)));
    if let Some(band) = &a.band {
        colours.push(("band".into(), band.color));
        colours.push(("band's bulge".into(), band.bulge_color));
    }
    for (i, n) in a.nebulae.iter().enumerate() {
        colours.push((format!("nebula {i}'s inner"), n.inner));
        colours.push((format!("nebula {i}'s outer"), n.outer));
    }
    for (i, b) in a.bodies.iter().enumerate() {
        colours.extend(b.colors.iter().map(|c| (format!("body {i}"), *c)));
        if let Some(rings) = &b.rings {
            colours.push((format!("body {i}'s rings"), rings.color));
        }
        if let Some(air) = b.atmosphere {
            colours.push((format!("body {i}'s atmosphere"), air));
        }
    }
    for (what, colour) in colours {
        let (chroma, hue) = colour.chroma_hue();
        let off = ((hue - magenta + 180.0).rem_euclid(360.0) - 180.0).abs();
        check(
            chroma < GREY_CHROMA || off >= WEAK_SPOT_HUE_CLEARANCE,
            format!("the {what} ({}) is {off:.0}° of hue off the weak spots' magenta (keep {WEAK_SPOT_HUE_CLEARANCE:.0}°)", colour.hex()),
        );
    }

    // The sun and the light.
    let sun = &a.sun;
    check(direction(sun.direction), "the sun has no direction".into());
    check(in_range(sun.illuminance, 0.0, 40_000.0), format!("the sun's light ({} lx) is out of range (0 to 40000)", sun.illuminance));
    check(in_range(sun.size, 0.2, 12.0), format!("the sun's disc ({}°) is out of range (0.2 to 12)", sun.size));
    check(in_range(sun.corona, 0.0, 8.0), format!("the sun's corona ({}) is out of range (0 to 8)", sun.corona));
    check(in_range(a.light.ambient_brightness, 0.0, 200.0), format!("the ambient light ({}) is out of range (0 to 200)", a.light.ambient_brightness));
    check(in_range(a.light.fill, 0.0, 10_000.0), format!("the sky's fill ({}) is out of range (0 to 10000)", a.light.fill));

    // Stars.
    let stars = &a.stars;
    check(stars.count <= MAX_STARS, format!("{} stars is more than {MAX_STARS}", stars.count));
    check(stars.glints <= stars.count.min(200), format!("{} glints is more than the stars or 200", stars.glints));
    check(in_range(stars.banded, 0.0, 1.0), format!("the stars' banded share ({}) isn't 0 to 1", stars.banded));
    check(in_range(stars.brightness, 0.0, 40.0), format!("the stars' brightness ({}) is out of range (0 to 40)", stars.brightness));
    check(!stars.colors.is_empty(), "the stars have no colours".into());

    // The backdrop: dark enough behind play.
    if let Some(band) = &a.band {
        check(direction(band.pole), "the band has no pole".into());
        check(direction(band.bulge), "the band's bulge has no direction".into());
        check(in_range(band.width, 2.0, 90.0), format!("the band's width ({}°) is out of range (2 to 90)", band.width));
        check(in_range(band.dust, 0.0, 1.0), format!("the band's dust ({}) isn't 0 to 1", band.dust));
        check((1..=8).contains(&band.tones), format!("the band's {} tones aren't 1 to 8", band.tones));
        let peak = band.brightness * (band.color.luminance() + BULGE_GAIN * band.bulge_color.luminance());
        check(
            band.brightness >= 0.0 && peak <= BACKDROP_CAP,
            format!("the band peaks at {peak:.2}, brighter than a backdrop may be ({BACKDROP_CAP})"),
        );
    }
    check(a.nebulae.len() <= MAX_NEBULAE, format!("{} nebulae is more than {MAX_NEBULAE}", a.nebulae.len()));
    for (i, n) in a.nebulae.iter().enumerate() {
        check(direction(n.direction), format!("nebula {i} has no direction"));
        check(in_range(n.size, 2.0, 180.0), format!("nebula {i}'s size ({}°) is out of range (2 to 180)", n.size));
        check(in_range(n.dust, 0.0, 1.0), format!("nebula {i}'s dust ({}) isn't 0 to 1", n.dust));
        check(in_range(n.rim, 0.0, 2.0), format!("nebula {i}'s rim ({}) isn't 0 to 2", n.rim));
        check(in_range(n.stretch, 1.0, 6.0), format!("nebula {i}'s stretch ({}) isn't 1 to 6", n.stretch));
        check((1..=8).contains(&n.tones), format!("nebula {i}'s {} tones aren't 1 to 8", n.tones));
        let peak = n.brightness * n.inner.luminance().max(n.outer.luminance()) * (1.0 + n.rim);
        check(
            n.brightness >= 0.0 && peak <= BACKDROP_CAP,
            format!("nebula {i} peaks at {peak:.2}, brighter than a backdrop may be ({BACKDROP_CAP})"),
        );
    }

    // Bodies: in the sky without hiding the sun or each other.
    check(a.bodies.len() <= MAX_BODIES, format!("{} bodies is more than {MAX_BODIES}", a.bodies.len()));
    for (i, b) in a.bodies.iter().enumerate() {
        check(direction(b.direction), format!("body {i} has no direction"));
        check(direction(b.axis), format!("body {i} has no axis"));
        check(in_range(b.size, 0.2, 60.0), format!("body {i}'s size ({}°) is out of range (0.2 to 60)", b.size));
        check((1..=3).contains(&b.colors.len()), format!("body {i} has {} colours (1 to 3)", b.colors.len()));
        if let Some(rings) = &b.rings {
            check(
                1.0 < rings.inner && rings.inner < rings.outer && rings.outer <= 4.0,
                format!("body {i}'s rings ({} to {}) aren't 1 < inner < outer <= 4 radii", rings.inner, rings.outer),
            );
        }
        match b.kind {
            BodyKind::Star => check(in_range(b.light, 0.0, 20_000.0), format!("body {i}'s light ({} lx) is out of range (0 to 20000)", b.light)),
            _ => check(b.light == 0.0, format!("body {i} gives light ({} lx) but isn't a star", b.light)),
        }
        let off = degrees_between(b.direction, sun.direction);
        check(
            off > b.reach() + sun.size / 2.0 * (1.0 + sun.corona) + 2.0,
            format!("body {i} is {off:.0}° from the sun: it covers the sun or its corona"),
        );
        for (j, c) in a.bodies.iter().enumerate().skip(i + 1) {
            let off = degrees_between(b.direction, c.direction);
            check(off > b.reach() + c.reach() + 1.0, format!("bodies {i} and {j} overlap ({off:.0}° apart)"));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_read_and_write_as_hex() {
        assert_eq!(Rgb::parse("#ff40c8"), Ok(Rgb::new(255, 64, 200)));
        assert_eq!(Rgb::parse("a0b1c2").map(Rgb::hex), Ok("#a0b1c2".to_string()));
        assert!(Rgb::parse("#ff40c").is_err());
        assert!(Rgb::parse("#gg0000").is_err());
    }

    #[test]
    fn every_shipped_ambience_parses_and_passes() {
        assert!(names().contains(&REFERENCE));
        for name in names() {
            let ambience = Ambience::named(name).unwrap_or_else(|e| panic!("{e}"));
            let problems = problems(&ambience);
            assert!(problems.is_empty(), "{name}: {problems:#?}");
        }
    }

    #[test]
    fn the_validator_keeps_magenta_out_of_the_sky() {
        let mut a = Ambience::reference();
        a.stars.colors.push(Rgb::new(0xb0, 0x7a, 0xff));
        assert!(problems(&a).iter().any(|p| p.contains("magenta")), "{:?}", problems(&a));
        // Today's old violet nebula colour, 26 degrees off.
        let mut a = Ambience::reference();
        a.nebulae.push(Nebula {
            direction: Vec3::X,
            size: 20.0,
            inner: Rgb::parse("#9567a7").unwrap(),
            outer: Rgb::new(10, 10, 10),
            brightness: 0.1,
            seed: 0,
            tones: 3,
            dust: 0.0,
            rim: 0.0,
            stretch: 1.0,
            turn: 0.0,
        });
        assert!(problems(&a).iter().any(|p| p.contains("magenta")), "{:?}", problems(&a));
    }

    #[test]
    fn the_validator_keeps_the_backdrop_dark_and_the_sun_clear() {
        let mut a = Ambience::reference();
        if let Some(band) = &mut a.band {
            band.brightness = 10.0;
        }
        assert!(problems(&a).iter().any(|p| p.contains("brighter than a backdrop")));
        let mut a = Ambience::reference();
        a.bodies = vec![Body {
            kind: BodyKind::Rocky,
            direction: a.sun.direction,
            size: 5.0,
            colors: vec![Rgb::new(80, 80, 80)],
            seed: 0,
            axis: Vec3::Y,
            rings: None,
            atmosphere: None,
            light: 0.0,
        }];
        assert!(problems(&a).iter().any(|p| p.contains("covers the sun")));
    }

    #[test]
    fn choices_parse() {
        assert_eq!(Choice::parse(REFERENCE), Ok(Choice::named(REFERENCE)));
        assert_eq!(Choice::parse("calm:42"), Ok(Choice::Generated(Archetype::Calm, 42)));
        assert!(Choice::parse("nowhere").is_err());
        assert!(Choice::parse("calm:x").is_err());
        assert!(Choice::parse("nowhere:1").is_err());
    }

    /// The whole description goes on the wire: it should stay small.
    #[test]
    fn an_ambience_is_small_on_the_wire() {
        for name in names() {
            let a = Ambience::named(name).unwrap();
            let bytes = postcard::to_allocvec(&a).unwrap();
            assert!(bytes.len() < 1500, "{name}: {} bytes", bytes.len());
            let back: Ambience = postcard::from_bytes(&bytes).unwrap();
            assert_eq!(back, a);
        }
    }
}
