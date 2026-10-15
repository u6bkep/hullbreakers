//! How the cost of asteroids scales with their count, by body kind.
//!
//! A benchmark, not a check: one `#[ignore]`d test, run with
//!
//! ```text
//! cargo test --release --test rock_scaling -- --ignored --nocapture
//! ```
//!
//! It is a single test on purpose (the harness runs tests on parallel
//! threads, which would skew each other's timings). Two parts:
//!
//! 1. **Spawn cost**: wall time to build N rocks' colliders with
//!    `asteroid_collider` (ico3 hulls, as the game does) and with a local copy
//!    of it that takes the icosphere subdivision (ico2: 162 points), the mass
//!    properties, and the client's `collider_mesh` over the colliders.
//! 2. **Tick cost**: a headless sim app (as `tests/grapple_run.rs`), N rocks
//!    spread through a 3 km x 600 m x 600 m volume, one local player floating
//!    in EVA among them, mean / p50 / p95 / max of `app.update()` (exactly one
//!    60 Hz fixed tick each) over a window after a warmup, plus where inside
//!    the tick the time goes (marker systems between Avian's phases).
//!
//! Knobs (environment): `ROCK_NS=300,1000` (counts), `ROCK_KINDS=Dynamic,Static`
//! (see [`Kind`]), `ROCK_DIST=today|grapple` (radius distribution), `ROCK_LAYOUT=course|field`
//! (`field`: today's 800x400x400 streaming box with overlaps allowed, for calibration), `ROCK_REPS=2`,
//! `ROCK_TICKS=300`, `ROCK_SKIP_SPAWN=1`, `ROCK_SKIP_TICKS=1`.
//!
//! The rocks are spawned without the `Asteroid` component (whose observer would
//! rebuild the collider in every cell): each gets its own pre-built collider
//! (distinct shapes, not shared), `dynamic_mass`, and the body components
//! `spawn_rock` gives. So the streaming-field systems (`recycle_field`,
//! `apply_field_speed`; ~0.1 ms in the traced field) are not in these numbers;
//! they are off here anyway (`asteroids: false`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use hullbreakers::client::render::collider_mesh;
use hullbreakers::sim::asteroids::{ROCK_DENSITY, asteroid_collider};
use hullbreakers::sim::bodies::{PreviousVelocity, dynamic_mass};
use hullbreakers::sim::bots::Robot;
use hullbreakers::sim::mission::Director;
use hullbreakers::sim::player::{Mode, Player};
use hullbreakers::sim::tuning::Tuning;
use hullbreakers::sim::{SimMaterial, SimPlugin, SimTick};
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::{Rng, SeedableRng};
use rand_chacha::ChaCha8Rng;

const COUNTS: [usize; 5] = [300, 1000, 2000, 4000, 8000];
const MAX_N: usize = 8000;
/// A rock reaches about twice its radius (lobes offset up to 0.8 r, each up to
/// 0.8 r, noise up to 1.35x): keep centres apart by (r1 + r2) * REACH.
const REACH: f32 = 2.2;

/// Where the rocks go.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Layout {
    /// The race course's volume: x, y within +-300 m, z within +-1500 m
    /// (3 km x 600 m x 600 m), rocks not overlapping.
    Course,
    /// Today's streaming field, for calibration against perf.md: x, y within
    /// +-200 m, z within +-400 m, rocks placed at random as `spawn_field`
    /// does (overlaps allowed, so some start in contact).
    Field,
}

impl Layout {
    fn half_xy(self) -> f32 {
        if self == Layout::Course { 300.0 } else { 200.0 }
    }
    fn half_z(self) -> f32 {
        if self == Layout::Course { 1500.0 } else { 400.0 }
    }
    /// Nothing within this of the origin (the home ship is there).
    fn origin_clear(self) -> f32 {
        if self == Layout::Course { 150.0 } else { 40.0 }
    }
    fn player(self) -> Vec3 {
        if self == Layout::Course { Vec3::new(0.0, 0.0, 400.0) } else { Vec3::new(0.0, 0.0, 100.0) }
    }
}
const WARMUP_TICKS: usize = 120;

// ---------------------------------------------------------------- rocks

#[derive(Clone, Copy, PartialEq, Debug)]
enum Dist {
    /// `asteroids::random_radius`: 0.35 * (30/0.35)^(u^3), 0.35 to 30 m.
    Today,
    /// The grapple run's `1.0 * 9^(u^2)`, 1 to 9 m.
    Grapple,
}

impl Dist {
    fn radius(self, rng: &mut impl Rng) -> f32 {
        match self {
            Dist::Today => {
                let (min, max) = (0.35f32, 30.0f32);
                min * (max / min).powf(rng.random::<f32>().powi(3))
            }
            Dist::Grapple => 1.0 * 9.0f32.powf(rng.random::<f32>().powi(2)),
        }
    }
}

#[derive(Clone, Copy)]
struct Place {
    at: Vec3,
    radius: f32,
    seed: u64,
}

fn random_unit(rng: &mut impl Rng) -> Vec3 {
    loop {
        let v = Vec3::new(
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
            rng.random_range(-1.0..1.0),
        );
        let len2 = v.length_squared();
        if len2 > 1e-3 && len2 <= 1.0 {
            return v / len2.sqrt();
        }
    }
}

/// Up to `MAX_N` non-overlapping rocks in the volume, in placement order, so
/// the first N of them are an N-rock field. `player` is kept clear.
fn place_rocks(dist: Dist, layout: Layout) -> Vec<Place> {
    let player = layout.player();
    let mut rng = ChaCha8Rng::seed_from_u64(0x70c4_5ca1e);
    let cell = 140.0f32; // >= (30 + 30) * REACH + 1
    let key = |p: Vec3| ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32, (p.z / cell).floor() as i32);
    let mut grid: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();
    let mut placed: Vec<Place> = Vec::new();
    for _ in 0..MAX_N * 60 {
        if placed.len() == MAX_N {
            break;
        }
        let radius = dist.radius(&mut rng);
        let at = Vec3::new(
            rng.random_range(-layout.half_xy()..layout.half_xy()),
            rng.random_range(-layout.half_xy()..layout.half_xy()),
            rng.random_range(-layout.half_z()..layout.half_z()),
        );
        if at.length() < radius * REACH + layout.origin_clear() || at.distance(player) < radius * REACH + 25.0 {
            continue;
        }
        let (kx, ky, kz) = key(at);
        if layout == Layout::Field {
            // Like `spawn_field`: no overlap test.
            placed.push(Place { at, radius, seed: rng.random() });
            continue;
        }
        let mut clear = true;
        'scan: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(ids) = grid.get(&(kx + dx, ky + dy, kz + dz)) {
                        for &i in ids {
                            let o = placed[i];
                            if o.at.distance(at) < (o.radius + radius) * REACH + 1.0 {
                                clear = false;
                                break 'scan;
                            }
                        }
                    }
                }
            }
        }
        if !clear {
            continue;
        }
        grid.entry((kx, ky, kz)).or_default().push(placed.len());
        placed.push(Place { at, radius, seed: rng.random() });
    }
    placed
}

/// `asteroids::asteroid_collider` with the icosphere subdivision a parameter
/// (3 reproduces the game's; checked in the test).
fn collider_ico(seed: u64, radius: f32, ico: u32) -> Collider {
    let rng = &mut ChaCha8Rng::seed_from_u64(seed);
    let lobe_count = if radius < 1.0 { 1 } else { rng.random_range(1..=3) };
    let mut lobes = Vec::new();
    for lobe in 0..lobe_count {
        let lobe_radius = if lobe == 0 { radius } else { radius * rng.random_range(0.5..0.8) };
        let offset = if lobe == 0 {
            Vec3::ZERO
        } else {
            random_unit(rng) * radius * rng.random_range(0.5..0.8)
        };
        let squash = Vec3::new(1.0, rng.random_range(0.65..1.0), rng.random_range(0.6..1.0));
        let fbm = Fbm::<Perlin>::new(rng.random()).set_octaves(4).set_frequency(1.3);
        let sphere = Sphere::new(1.0).mesh().ico(ico).expect("ico subdivisions in range");
        let Some(bevy::mesh::VertexAttributeValues::Float32x3(positions)) =
            sphere.attribute(Mesh::ATTRIBUTE_POSITION)
        else {
            unreachable!("ico sphere has float3 positions");
        };
        let points: Vec<Vec3> = positions
            .iter()
            .map(|p| {
                let dir = Vec3::from(*p);
                let n = fbm.get([dir.x as f64, dir.y as f64, dir.z as f64]) as f32;
                dir * squash * lobe_radius * (1.0 + 0.35 * n)
            })
            .collect();
        let hull = Collider::convex_hull(points).expect("noisy sphere has a hull");
        lobes.push((offset, Quat::IDENTITY, hull));
    }
    if lobes.len() == 1 {
        lobes.pop().unwrap().2
    } else {
        Collider::compound(lobes)
    }
}

/// (hulls, hull vertices) of a rock collider.
fn hull_stats(c: &Collider) -> (usize, usize) {
    let shape = c.shape();
    if let Some(h) = shape.as_convex_polyhedron() {
        (1, h.points().len())
    } else if let Some(comp) = shape.as_compound() {
        let mut v = 0;
        for (_, part) in comp.shapes() {
            v += part.as_convex_polyhedron().map_or(0, |h| h.points().len());
        }
        (comp.shapes().len(), v)
    } else {
        (0, 0)
    }
}

// ---------------------------------------------------------------- environment

fn loadavg() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .map(|s| s.split_whitespace().take(3).collect::<Vec<_>>().join("/"))
        .unwrap_or_else(|_| "?".into())
}

fn env_list(name: &str) -> Option<Vec<String>> {
    std::env::var(name).ok().map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

// ---------------------------------------------------------------- spawn cost

fn spawn_cost(places: &[Place]) {
    println!("\n== SPAWN COST (radii: today's random_radius; {} rocks placed) ==", places.len());
    // The replica must reproduce the game's colliders exactly.
    for p in places.iter().take(40) {
        let a = asteroid_collider(p.seed, p.radius);
        let b = collider_ico(p.seed, p.radius, 3);
        assert_eq!(hull_stats(&a), hull_stats(&b), "replica differs from asteroid_collider");
        let (aa, bb) = (a.shape().compute_local_aabb(), b.shape().compute_local_aabb());
        assert!((aa.mins - bb.mins).length() < 1e-4 && (aa.maxs - bb.maxs).length() < 1e-4);
    }
    println!("(local ico3 replica matches asteroid_collider on the first 40 rocks)");
    println!("loadavg before: {}", loadavg());
    println!(
        "{:>6} | {:>14} {:>9} {:>9} | {:>14} {:>9} {:>9} | {:>11} {:>11}",
        "N", "ico3 build ms", "per rock", "hulls/pts", "ico2 build ms", "per rock", "hulls/pts", "mass3 ms", "mesh3 ms"
    );
    // Build cumulatively and read the clock at each checkpoint: building N
    // rocks serially is the prefix of building 8000.
    let mut ico3: Vec<Collider> = Vec::new();
    let mut ico2: Vec<Collider> = Vec::new();
    let (mut t3, mut t2, mut tm, mut tmesh) = (Duration::ZERO, Duration::ZERO, Duration::ZERO, Duration::ZERO);
    let mut next = 0;
    for (i, p) in places.iter().enumerate() {
        let s = Instant::now();
        ico3.push(asteroid_collider(p.seed, p.radius));
        t3 += s.elapsed();
        let s = Instant::now();
        ico2.push(collider_ico(p.seed, p.radius, 2));
        t2 += s.elapsed();
        let s = Instant::now();
        let _ = dynamic_mass(&ico3[i], ROCK_DENSITY);
        tm += s.elapsed();
        let s = Instant::now();
        let mesh = collider_mesh(&ico3[i]);
        tmesh += s.elapsed();
        std::hint::black_box(mesh);
        let n = i + 1;
        if next < COUNTS.len() && n == COUNTS[next].min(places.len()) {
            let (h3, v3) = ico3.iter().map(hull_stats).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
            let (h2, v2) = ico2.iter().map(hull_stats).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
            println!(
                "{:>6} | {:>14.1} {:>7.2}ms {:>4.2}/{:<4.0} | {:>14.1} {:>7.2}ms {:>4.2}/{:<4.0} | {:>11.1} {:>11.1}",
                n,
                ms(t3),
                ms(t3) / n as f64,
                h3 as f64 / n as f64,
                v3 as f64 / n as f64,
                ms(t2),
                ms(t2) / n as f64,
                h2 as f64 / n as f64,
                v2 as f64 / n as f64,
                ms(tm),
                ms(tmesh),
            );
            next += 1;
        }
    }
    // The mesh from ico2 hulls, for the whole set.
    let s = Instant::now();
    for c in &ico2 {
        std::hint::black_box(collider_mesh(c));
    }
    println!(
        "mesh2 (collider_mesh over all {} ico2 colliders): {:.1} ms; triangles ico3/ico2 over the set: {}/{}",
        ico2.len(),
        ms(s.elapsed()),
        tri_count(&ico3),
        tri_count(&ico2)
    );
    println!("loadavg after: {}", loadavg());
}

fn tri_count(cs: &[Collider]) -> usize {
    cs.iter().map(|c| collider_mesh(c).map_or(0, |m| m.count_vertices() / 3)).sum()
}

// ---------------------------------------------------------------- tick cost

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// As today: dynamic, drifting and tumbling, sleeping disabled.
    Dynamic,
    /// As Dynamic, with ico2 hulls.
    DynamicIco2,
    /// Dynamic, zero velocity, sleeping disabled (isolates "moving").
    DynamicStill,
    /// Dynamic, zero velocity, Avian's sleeping allowed.
    DynamicAsleep,
    /// Static bodies: no sleeping component, no velocity.
    Static,
    /// Kinematic bodies with a slow spin (0 to 0.08 rad/s), no translation,
    /// sleeping disabled (so they really turn).
    Kinematic,
    /// The same, with sleeping allowed: Avian puts a kinematic body under its
    /// sleep threshold to sleep (see the `asleep` and `turned` columns).
    KinematicSleepy,
}

impl Kind {
    const ALL: [Kind; 7] = [
        Kind::Dynamic,
        Kind::DynamicIco2,
        Kind::DynamicStill,
        Kind::DynamicAsleep,
        Kind::Static,
        Kind::Kinematic,
        Kind::KinematicSleepy,
    ];
    fn name(self) -> &'static str {
        match self {
            Kind::Dynamic => "Dynamic",
            Kind::DynamicIco2 => "DynamicIco2",
            Kind::DynamicStill => "DynamicStill",
            Kind::DynamicAsleep => "DynamicAsleep",
            Kind::Static => "Static",
            Kind::Kinematic => "Kinematic",
            Kind::KinematicSleepy => "KinematicSleepy",
        }
    }
}

/// When each boundary between Avian's phases was last crossed.
#[derive(Resource, Default)]
struct Marks([Option<Instant>; 5]);

fn mark<const I: usize>(mut m: ResMut<Marks>) {
    m.0[I] = Some(Instant::now());
}

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, TransformPlugin, AssetPlugin::default(), bevy::mesh::MeshPlugin));
    app.add_plugins(PhysicsPlugins::default());
    app.insert_resource(Tuning::default());
    app.add_plugins(SimPlugin { asteroids: false, local_player: true });
    app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(1.0 / 60.0)));
    app.world_mut().resource_mut::<Director>().robots = false;
    // Markers between Avian's phases, to say where a tick goes.
    app.init_resource::<Marks>().add_systems(
        PhysicsSchedule,
        (
            mark::<0>.in_set(PhysicsStepSystems::First),
            mark::<1>.after(PhysicsStepSystems::BroadPhase).before(PhysicsStepSystems::NarrowPhase),
            mark::<2>.after(PhysicsStepSystems::NarrowPhase).before(PhysicsStepSystems::Solver),
            mark::<3>.after(PhysicsStepSystems::Solver).before(PhysicsStepSystems::Sleeping),
            mark::<4>.in_set(PhysicsStepSystems::Last),
        ),
    );
    app.finish();
    app.cleanup();
    for _ in 0..4 {
        app.update();
    }
    app
}

fn spawn_rock(world: &mut World, kind: Kind, p: &Place, collider: Collider, rng: &mut ChaCha8Rng) {
    let rotation = Quat::from_scaled_axis(random_unit(rng) * rng.random_range(0.0..3.14));
    let mass = dynamic_mass(&collider, ROCK_DENSITY);
    let mut rock = world.spawn((
        mass,
        collider,
        SimMaterial::Rock,
        PreviousVelocity::default(),
        Position(p.at),
        Rotation(rotation),
    ));
    match kind {
        Kind::Dynamic | Kind::DynamicIco2 => {
            // `asteroids::drift`: the field's speed along Z plus a little wander.
            let linear = Vec3::Z * Tuning::default().field_speed + random_unit(rng) * rng.random_range(0.0..0.6);
            let angular = random_unit(rng) * rng.random_range(0.0..0.3);
            rock.insert((RigidBody::Dynamic, SleepingDisabled, LinearVelocity(linear), AngularVelocity(angular)));
        }
        Kind::DynamicStill => {
            rock.insert((RigidBody::Dynamic, SleepingDisabled));
        }
        Kind::DynamicAsleep => {
            rock.insert(RigidBody::Dynamic);
        }
        Kind::Static => {
            rock.insert(RigidBody::Static);
        }
        Kind::Kinematic | Kind::KinematicSleepy => {
            let angular = random_unit(rng) * rng.random_range(0.0..0.08);
            rock.insert((RigidBody::Kinematic, AngularVelocity(angular)));
            if kind == Kind::Kinematic {
                rock.insert(SleepingDisabled);
            }
        }
    }
}

#[derive(Default, Clone)]
struct Cell {
    n: usize,
    ingest_ms: f64,
    first_update_ms: f64,
    mean: f64,
    p50: f64,
    p95: f64,
    max: f64,
    /// Mean ms per tick: before broad phase (+ phase 0 to 1: broad), narrow,
    /// solver, sleeping+finalize, and everything outside the physics schedule.
    broad: f64,
    narrow: f64,
    solver: f64,
    after: f64,
    outside: f64,
    asleep: usize,
    /// Mean angle (degrees) the rocks turned over the measured window.
    turned: f64,
    load_before: String,
    load_after: String,
}

fn run_cell(kind: Kind, n: usize, places: &[Place], pool: &[Collider], player: Vec3, ticks: usize) -> Cell {
    let mut cell = Cell { n, load_before: loadavg(), ..default() };
    let mut app = app();
    // The player floats among the rocks, as the grapple run's runner does.
    {
        let world = app.world_mut();
        let e = world.query_filtered::<Entity, (With<Player>, Without<Robot>)>().single(world).unwrap();
        world.get_mut::<Player>(e).unwrap().mode =
            Mode::Eva { position: player, velocity: Vec3::ZERO, orientation: Quat::IDENTITY, pitch: 0.0 };
    }
    let rng = &mut ChaCha8Rng::seed_from_u64(0x5ca1ed);
    let s = Instant::now();
    for i in 0..n.min(places.len()) {
        spawn_rock(app.world_mut(), kind, &places[i], pool[i].clone(), rng);
    }
    cell.ingest_ms = ms(s.elapsed());
    cell.n = n.min(places.len());
    // The first update after the rocks arrive: Avian initialises every
    // collider, builds the broad phase.
    let s = Instant::now();
    app.update();
    cell.first_update_ms = ms(s.elapsed());
    for _ in 0..WARMUP_TICKS {
        app.update();
    }
    let rotations = |app: &mut App| -> Vec<Quat> {
        let world = app.world_mut();
        world.query_filtered::<&Rotation, With<Collider>>().iter(world).map(|r| r.0).collect()
    };
    let before = rotations(&mut app);
    let tick0 = app.world().resource::<SimTick>().0;
    let mut times = Vec::with_capacity(ticks);
    let (mut broad, mut narrow, mut solver, mut after, mut outside) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for _ in 0..ticks {
        let s = Instant::now();
        app.update();
        let total = s.elapsed();
        times.push(ms(total));
        let m = app.world().resource::<Marks>().0.map(|m| m.expect("marker ran"));
        let d = |a: usize, b: usize| ms(m[b].duration_since(m[a]));
        broad += d(0, 1);
        narrow += d(1, 2);
        solver += d(2, 3);
        after += d(3, 4);
        outside += ms(total) - d(0, 4);
    }
    assert_eq!(app.world().resource::<SimTick>().0 - tick0, ticks as u32, "each update must run exactly one tick");
    let mut sleeping = 0;
    {
        let world = app.world_mut();
        sleeping += world.query_filtered::<Entity, With<Sleeping>>().iter(world).count();
    }
    cell.asleep = sleeping;
    let after_rot = rotations(&mut app);
    // Rocks only (the home ship and others carry colliders too, but there
    // are few of them and they sit still).
    cell.turned = before.iter().zip(&after_rot).map(|(a, b)| a.angle_between(*b).to_degrees() as f64).sum::<f64>()
        / before.len().max(1) as f64;
    let k = ticks as f64;
    cell.mean = times.iter().sum::<f64>() / k;
    times.sort_by(|a, b| a.total_cmp(b));
    cell.p50 = times[times.len() / 2];
    cell.p95 = times[(times.len() * 95 / 100).min(times.len() - 1)];
    cell.max = *times.last().unwrap();
    (cell.broad, cell.narrow, cell.solver, cell.after, cell.outside) =
        (broad / k, narrow / k, solver / k, after / k, outside / k);
    cell.load_after = loadavg();
    cell
}

fn tick_cost(places_today: &[Place], places_grapple: &[Place], dist: Dist, layout: Layout) {
    let ticks: usize = std::env::var("ROCK_TICKS").ok().and_then(|v| v.parse().ok()).unwrap_or(300);
    let reps: usize = std::env::var("ROCK_REPS").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    let ns: Vec<usize> = env_list("ROCK_NS")
        .map(|l| l.iter().filter_map(|s| s.parse().ok()).collect())
        .unwrap_or_else(|| std::iter::once(0).chain(COUNTS).collect());
    let kinds: Vec<Kind> = env_list("ROCK_KINDS")
        .map(|l| Kind::ALL.iter().copied().filter(|k| l.iter().any(|s| s == k.name())).collect())
        .unwrap_or_else(|| Kind::ALL.to_vec());
    let places = if dist == Dist::Today { places_today } else { places_grapple };
    let player = layout.player();
    println!(
        "\n== TICK COST (radii: {:?}; layout {:?}; {} placed; {} ticks after {} warmup; {} reps; substeps {}) ==",
        dist,
        layout,
        places.len(),
        ticks,
        WARMUP_TICKS,
        reps,
        std::env::var("HULLBREAKERS_SUBSTEPS").unwrap_or_else(|_| "default (2)".into()),
    );
    let max_n = ns.iter().copied().max().unwrap_or(0).min(places.len());
    let t = Instant::now();
    let pool3: Vec<Collider> = places[..max_n].iter().map(|p| asteroid_collider(p.seed, p.radius)).collect();
    let pool2: Vec<Collider> = if kinds.contains(&Kind::DynamicIco2) {
        places[..max_n].iter().map(|p| collider_ico(p.seed, p.radius, 2)).collect()
    } else {
        Vec::new()
    };
    println!("(collider pools built in {:.1} s)", t.elapsed().as_secs_f64());
    println!(
        "{:<15} {:>5} {:>3} | {:>7} {:>7} {:>7} {:>7} | {:>6} {:>6} {:>6} {:>6} {:>7} | {:>8} {:>8} {:>6} {:>6} | load before -> after",
        "kind", "N", "rep", "mean", "p50", "p95", "max", "broad", "narrow", "solver", "post", "outside", "ingest", "1st upd", "asleep", "turned"
    );
    for &n in &ns {
        let kinds_here: Vec<Kind> = if n == 0 { vec![Kind::Dynamic] } else { kinds.clone() };
        for rep in 0..reps {
            for &kind in &kinds_here {
                let pool = if kind == Kind::DynamicIco2 { &pool2 } else { &pool3 };
                let c = run_cell(kind, n, places, pool, player, ticks);
                println!(
                    "{:<15} {:>5} {:>3} | {:>7.3} {:>7.3} {:>7.3} {:>7.3} | {:>6.3} {:>6.3} {:>6.3} {:>6.3} {:>7.3} | {:>8.1} {:>8.1} {:>6} {:>6.2} | {} -> {}",
                    if n == 0 { "(none)" } else { kind.name() },
                    c.n,
                    rep + 1,
                    c.mean,
                    c.p50,
                    c.p95,
                    c.max,
                    c.broad,
                    c.narrow,
                    c.solver,
                    c.after,
                    c.outside,
                    c.ingest_ms,
                    c.first_update_ms,
                    c.asleep,
                    c.turned,
                    c.load_before,
                    c.load_after,
                );
            }
        }
    }
    println!("(times in ms per tick; broad/narrow/solver/post are Avian's PhysicsSchedule phases, 'outside' is the rest of the update)");
}

#[test]
#[ignore = "benchmark: cargo test --release --test rock_scaling -- --ignored --nocapture"]
fn rock_scaling() {
    let dist = match std::env::var("ROCK_DIST").as_deref() {
        Ok("grapple") => Dist::Grapple,
        _ => Dist::Today,
    };
    let layout = match std::env::var("ROCK_LAYOUT").as_deref() {
        Ok("field") => Layout::Field,
        _ => Layout::Course,
    };
    let t = Instant::now();
    let today = place_rocks(Dist::Today, layout);
    let grapple = if dist == Dist::Grapple { place_rocks(Dist::Grapple, layout) } else { Vec::new() };
    println!("placed {} (today's radii) / {} (grapple radii) rocks in {:.2} s", today.len(), grapple.len(), t.elapsed().as_secs_f64());
    if std::env::var("ROCK_SKIP_SPAWN").is_err() {
        spawn_cost(&today);
    }
    if std::env::var("ROCK_SKIP_TICKS").is_err() {
        tick_cost(&today, &grapple, dist, layout);
    }
}
