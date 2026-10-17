//! Performance measurement, off unless asked for. `tools/stress` drives it.
//!
//! ```text
//! HULLBREAKERS_PROFILE=<dir>        measure, write <dir>/<label>.json, then exit
//! HULLBREAKERS_PROFILE_LABEL=<name> the run's name (default: the scenario's, or "run")
//! HULLBREAKERS_PROFILE_WARMUP=<s>   settle this long after the local player appears (default 8)
//! HULLBREAKERS_PROFILE_SECS=<s>     then measure this long (default 15)
//! HULLBREAKERS_PROFILE_KEEP=1       keep running after writing
//! HULLBREAKERS_PROFILE_GPU_SYSFS=<dir>  a GPU's sysfs device directory (e.g.
//!                                   /sys/class/drm/renderD128/device): sample its
//!                                   utilisation and power (amdgpu)
//! HULLBREAKERS_PROFILE_AB=a,b,...   switch rendering features off, to price them:
//!     static_envmap  stop re-filtering the generated environment maps every
//!                    frame (keeps the light they made in the first seconds)
//!     no_envmap      no environment-map light at all
//!     no_shadows     no shadow maps on any light
//!     no_bloom, no_msaa
//!     no_backdrop    no baked backdrop behind everything (the band, nebulae)
//!     no_stars, no_bodies  none of the sky's stars, or bodies and sun disc
//! ```
//!
//! Measured over the window: frame time (mean, percentiles, FPS), time in
//! the fixed-tick loop per frame (`RunFixedMainLoop`: lightyear, the sim and
//! Avian's step), GPU time per frame and per render pass (Bevy's
//! `RenderDiagnosticsPlugin`, timestamp queries), the process's CPU use
//! (from `/proc/self/stat`), and counts of what's in the world, sampled
//! twice a second. Per-system time comes from Bevy's tracing spans instead
//! (the `profile-trace` feature); the window's edges are marked in the trace
//! as `hb_profile_window` spans so a summary can cut to it.

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::Instant;

use avian3d::prelude::{Collider, RigidBody};
use bevy::app::RunFixedMainLoopSystems;
use bevy::camera::visibility::ViewVisibility;
use bevy::diagnostic::DiagnosticsStore;
use bevy::light::{DirectionalLight, GeneratedEnvironmentMapLight, LightProbe, NotShadowCaster, PointLight, SpotLight};
use bevy::prelude::*;
use bevy::render::renderer::RenderAdapterInfo;
use bevy::window::PrimaryWindow;

use super::LocalPlayer;

#[derive(Clone, Debug)]
pub struct ProfileConfig {
    pub dir: PathBuf,
    pub label: String,
    pub warmup: f32,
    pub seconds: f32,
    pub keep: bool,
    pub gpu_sysfs: Option<PathBuf>,
}

pub fn profile_from_env() -> Option<ProfileConfig> {
    let dir = std::env::var_os("HULLBREAKERS_PROFILE")?;
    let secs = |name: &str, default: f32| {
        std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
    };
    let label = std::env::var("HULLBREAKERS_PROFILE_LABEL")
        .or_else(|_| std::env::var("HULLBREAKERS_SCENARIO"))
        .unwrap_or_else(|_| "run".into());
    Some(ProfileConfig {
        dir: dir.into(),
        label,
        warmup: secs("HULLBREAKERS_PROFILE_WARMUP", 8.0),
        seconds: secs("HULLBREAKERS_PROFILE_SECS", 15.0),
        keep: std::env::var("HULLBREAKERS_PROFILE_KEEP").is_ok_and(|v| v == "1"),
        gpu_sysfs: std::env::var_os("HULLBREAKERS_PROFILE_GPU_SYSFS").map(PathBuf::from),
    })
}

pub struct ProfilePlugin(pub ProfileConfig);

impl Plugin for ProfilePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin)
            .insert_resource(Profile::new(self.0.clone()))
            .init_resource::<crate::net::SendMeter>()
            .add_systems(First, frame_begin)
            .add_systems(
                RunFixedMainLoop,
                (
                    fixed_begin.in_set(RunFixedMainLoopSystems::BeforeFixedMainLoop),
                    fixed_end.in_set(RunFixedMainLoopSystems::AfterFixedMainLoop),
                ),
            )
            .add_systems(FixedFirst, count_tick)
            .add_systems(Last, (collect_gpu, sample_counts));
        let ab: Vec<String> = std::env::var("HULLBREAKERS_PROFILE_AB")
            .unwrap_or_default()
            .split(',')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        if !ab.is_empty() {
            app.insert_resource(AbSwitches(ab)).add_systems(Update, apply_ab_switches);
        }
    }
}

/// Rendering features switched off for an A/B measurement.
#[derive(Resource)]
struct AbSwitches(Vec<String>);

#[allow(clippy::too_many_arguments)]
fn apply_ab_switches(
    switches: Res<AbSwitches>,
    profile: Res<Profile>,
    mut commands: Commands,
    generated: Query<Entity, With<GeneratedEnvironmentMapLight>>,
    env_maps: Query<Entity, With<bevy::light::EnvironmentMapLight>>,
    cameras: Query<Entity, With<Camera3d>>,
    mut points: Query<&mut PointLight>,
    mut spots: Query<&mut SpotLight>,
    mut dirs: Query<&mut DirectionalLight>,
    off: Option<ResMut<super::ambience::LayersOff>>,
) {
    let on = |name: &str| switches.0.iter().any(|s| s == name);
    if let Some(mut off) = off {
        let wanted = super::ambience::LayersOff {
            stars: on("no_stars"),
            bodies: on("no_bodies"),
        };
        if *off != wanted {
            *off = wanted;
        }
    }
    // Leave the generated maps a moment to be filtered once.
    let settled = match profile.phase {
        Phase::Starting => false,
        Phase::Warmup => profile.since.elapsed().as_secs_f32() > 2.0,
        Phase::Measuring | Phase::Done => true,
    };
    if on("static_envmap") && settled {
        for e in &generated {
            commands.entity(e).remove::<GeneratedEnvironmentMapLight>();
        }
    }
    if on("no_envmap") {
        for e in generated.iter().chain(env_maps.iter()) {
            commands
                .entity(e)
                .remove::<(GeneratedEnvironmentMapLight, bevy::light::EnvironmentMapLight)>();
        }
    }
    if on("no_shadows") {
        for mut l in &mut points {
            l.shadow_maps_enabled = false;
        }
        for mut l in &mut spots {
            l.shadow_maps_enabled = false;
        }
        for mut l in &mut dirs {
            l.shadow_maps_enabled = false;
        }
    }
    for camera in &cameras {
        if on("no_bloom") {
            commands.entity(camera).remove::<bevy::post_process::bloom::Bloom>();
        }
        if on("no_msaa") {
            commands.entity(camera).insert(Msaa::Off);
        }
        if on("no_backdrop") {
            commands.entity(camera).remove::<bevy::light::Skybox>();
        }
    }
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
enum Phase {
    /// Waiting for the local player.
    Starting,
    Warmup,
    Measuring,
    Done,
}

#[derive(Default)]
struct Count {
    sum: f64,
    max: f64,
    samples: u32,
}

#[derive(Resource)]
struct Profile {
    config: ProfileConfig,
    phase: Phase,
    /// When the current phase began.
    since: Instant,
    cpu_at_start: f64,
    frames: Vec<f32>,
    fixed: Vec<f32>,
    ticks: Vec<u32>,
    fixed_started: Option<Instant>,
    ticks_this_frame: u32,
    /// GPU time per frame (sum of top-level passes), and per pass: total
    /// over the window and how many frames it ran in.
    gpu_frames: Vec<f32>,
    gpu_passes: BTreeMap<String, (f64, u32)>,
    gpu_seen: HashMap<String, bevy::platform::time::Instant>,
    counts: BTreeMap<&'static str, Count>,
    next_count_sample: f32,
    /// What the authority had sent each remote client (packets, bytes) at
    /// the window's start and end (see `net::SendMeter`).
    sent: Option<(HashMap<Entity, (u64, u64)>, HashMap<Entity, (u64, u64)>)>,
}

impl Profile {
    fn new(config: ProfileConfig) -> Self {
        Profile {
            config,
            phase: Phase::Starting,
            since: Instant::now(),
            cpu_at_start: 0.0,
            frames: Vec::new(),
            fixed: Vec::new(),
            ticks: Vec::new(),
            fixed_started: None,
            ticks_this_frame: 0,
            gpu_frames: Vec::new(),
            gpu_passes: BTreeMap::new(),
            gpu_seen: HashMap::new(),
            counts: BTreeMap::new(),
            next_count_sample: 0.0,
            sent: None,
        }
    }

    fn measuring(&self) -> bool {
        self.phase == Phase::Measuring
    }
}

/// CPU seconds this process has used, all threads (Linux; 0 elsewhere).
fn process_cpu_seconds() -> f64 {
    let Ok(stat) = std::fs::read_to_string("/proc/self/stat") else {
        return 0.0;
    };
    // Fields after the parenthesised command name; utime and stime are the
    // 14th and 15th fields overall, in USER_HZ (100 on Linux).
    let Some(rest) = stat.rsplit_once(')').map(|(_, r)| r) else {
        return 0.0;
    };
    let fields: Vec<&str> = rest.split_whitespace().collect();
    let tick = |i: usize| fields.get(i).and_then(|f| f.parse::<f64>().ok()).unwrap_or(0.0);
    (tick(11) + tick(12)) / 100.0
}

fn window_marker(edge: &'static str) {
    let _span = info_span!("hb_profile_window", edge).entered();
}

#[allow(clippy::too_many_arguments)]
fn frame_begin(
    time: Res<Time<Real>>,
    mut profile: ResMut<Profile>,
    local: Query<(), With<LocalPlayer>>,
    window: Query<&Window, With<PrimaryWindow>>,
    adapter: Option<Res<RenderAdapterInfo>>,
    meter: Option<Res<crate::net::SendMeter>>,
    mut exit: MessageWriter<AppExit>,
) {
    let elapsed = profile.since.elapsed().as_secs_f32();
    let sent = || meter.as_ref().map(|m| m.links.clone());
    match profile.phase {
        Phase::Starting => {
            if !local.is_empty() {
                info!("profile: warming up {:.0} s", profile.config.warmup);
                profile.phase = Phase::Warmup;
                profile.since = Instant::now();
            }
        }
        Phase::Warmup => {
            if elapsed >= profile.config.warmup {
                info!("profile: measuring {:.0} s", profile.config.seconds);
                window_marker("start");
                profile.phase = Phase::Measuring;
                profile.since = Instant::now();
                profile.cpu_at_start = process_cpu_seconds();
                profile.next_count_sample = 0.0;
                profile.sent = sent().map(|s| (s, HashMap::new()));
            }
        }
        Phase::Measuring => {
            profile.frames.push(time.delta_secs() * 1000.0);
            if elapsed >= profile.config.seconds {
                window_marker("end");
                let cpu = (process_cpu_seconds() - profile.cpu_at_start) / elapsed as f64;
                if let (Some((_, end)), Some(now)) = (profile.sent.as_mut(), sent()) {
                    *end = now;
                }
                let report = report(&profile, elapsed, cpu, window.single().ok(), adapter.as_deref());
                let dir = &profile.config.dir;
                let path = dir.join(format!("{}.json", profile.config.label));
                match std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&path, report)) {
                    Ok(()) => info!("profile: wrote {}", path.display()),
                    Err(err) => error!("profile: can't write {}: {err}", path.display()),
                }
                profile.phase = Phase::Done;
                if !profile.config.keep {
                    exit.write(AppExit::Success);
                }
            }
        }
        Phase::Done => {}
    }
}

fn fixed_begin(mut profile: ResMut<Profile>) {
    profile.fixed_started = Some(Instant::now());
    profile.ticks_this_frame = 0;
}

fn count_tick(mut profile: ResMut<Profile>) {
    profile.ticks_this_frame += 1;
}

fn fixed_end(mut profile: ResMut<Profile>) {
    let Some(started) = profile.fixed_started.take() else { return };
    if profile.measuring() {
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        let ticks = profile.ticks_this_frame;
        profile.fixed.push(ms);
        profile.ticks.push(ticks);
    }
}

/// GPU pass times from the render diagnostics: every `render/…/elapsed_gpu`
/// measured since the last look (a pass that runs more than once a frame,
/// such as one per light probe, is summed).
fn collect_gpu(store: Res<DiagnosticsStore>, mut profile: ResMut<Profile>) {
    let measuring = profile.measuring();
    let mut frame_total = 0.0f64;
    let mut any = false;
    for diagnostic in store.iter() {
        let path = diagnostic.path().as_str();
        if !(path.starts_with("render/") && path.ends_with("/elapsed_gpu")) {
            continue;
        }
        let Some(latest) = diagnostic.measurement().map(|m| m.time) else { continue };
        let seen = profile.gpu_seen.get(path).copied();
        if seen.is_some_and(|s| s >= latest) {
            continue;
        }
        let value: f64 = diagnostic
            .measurements()
            .filter(|m| seen.is_none_or(|s| m.time > s) && m.time == latest)
            .map(|m| m.value)
            .sum();
        profile.gpu_seen.insert(path.to_string(), latest);
        if !measuring {
            continue;
        }
        let name = path.trim_start_matches("render/").trim_end_matches("/elapsed_gpu");
        let entry = profile.gpu_passes.entry(name.to_string()).or_default();
        entry.0 += value;
        entry.1 += 1;
        // Top-level passes make up the frame; nested ones are inside them.
        if !name.contains('/') {
            frame_total += value;
            any = true;
        }
    }
    if measuring && any {
        profile.gpu_frames.push(frame_total as f32);
    }
}

/// What's in the world, twice a second.
fn sample_counts(world: &mut World) {
    let dt = world.resource::<Time<Real>>().delta_secs();
    {
        let mut profile = world.resource_mut::<Profile>();
        if !profile.measuring() {
            return;
        }
        profile.next_count_sample -= dt;
        if profile.next_count_sample > 0.0 {
            return;
        }
        profile.next_count_sample = 0.5;
    }
    fn count<F: bevy::ecs::query::QueryFilter>(world: &mut World) -> f64 {
        world.query_filtered::<(), F>().iter(world).count() as f64
    }
    let mut samples: Vec<(&'static str, f64)> = vec![
        ("entities", world.entities().count_spawned() as f64),
        ("named", count::<With<Name>>(world)),
        ("meshes", count::<With<Mesh3d>>(world)),
        ("shadow_casters", count::<(With<Mesh3d>, Without<NotShadowCaster>)>(world)),
        ("point_lights", count::<With<PointLight>>(world)),
        ("spot_lights", count::<With<SpotLight>>(world)),
        ("directional_lights", count::<With<DirectionalLight>>(world)),
        ("light_probes", count::<With<LightProbe>>(world)),
        ("generated_env_maps", count::<With<GeneratedEnvironmentMapLight>>(world)),
        ("rigid_bodies", count::<With<RigidBody>>(world)),
        ("colliders", count::<With<Collider>>(world)),
        ("players", count::<With<crate::sim::player::Player>>(world)),
        ("robots", count::<With<crate::sim::bots::Robot>>(world)),
        ("avatars", count::<With<super::render::Avatar>>(world)),
        ("exhibits", count::<With<super::gallery::Exhibit>>(world)),
        ("tracers", count::<With<super::weapons_fx::Tracer>>(world)),
        ("bullet_sparks", count::<With<super::weapons_fx::Spark>>(world)),
        ("hull_sparks", count::<With<super::combat_fx::Spark>>(world)),
        ("scorches", count::<With<super::combat_fx::Scorch>>(world)),
        ("asteroids", count::<With<crate::sim::asteroids::Asteroid>>(world)),
        (
            "shots_in_flight",
            world.get_resource::<crate::sim::shots::Shots>().map_or(0, |s| s.0.len()) as f64,
        ),
    ];
    // Lights that render shadow maps (each is a pass per frame; a point
    // light's is six).
    let shadowed_points = world
        .query::<&PointLight>()
        .iter(world)
        .filter(|l| l.shadow_maps_enabled)
        .count();
    let shadowed_spots = world
        .query::<&SpotLight>()
        .iter(world)
        .filter(|l| l.shadow_maps_enabled)
        .count();
    let shadowed_dirs = world
        .query::<&DirectionalLight>()
        .iter(world)
        .filter(|l| l.shadow_maps_enabled)
        .count();
    samples.push(("shadowed_point_lights", shadowed_points as f64));
    samples.push(("shadowed_spot_lights", shadowed_spots as f64));
    samples.push(("shadowed_directional_lights", shadowed_dirs as f64));
    // Meshes in view of some camera, and their triangles.
    let mut visible = 0usize;
    let mut triangles = 0usize;
    {
        let mut query = world.query::<(&ViewVisibility, &Mesh3d)>();
        let meshes = world.resource::<Assets<Mesh>>();
        for (seen, mesh) in query.iter(world) {
            if !seen.get() {
                continue;
            }
            visible += 1;
            // (A mesh kept only in the render world, the sky's stars, can't
            // be read here: left out.)
            if let Some(mesh) = meshes.get(&mesh.0) {
                triangles += match mesh.try_indices_option() {
                    Ok(Some(indices)) => indices.len() / 3,
                    Ok(None) => mesh.count_vertices() / 3,
                    Err(_) => 0,
                };
            }
        }
    }
    // The whole card's utilisation and power draw (amdgpu's sysfs), which
    // see the passes Bevy doesn't time (shadows).
    let sysfs = world.resource::<Profile>().config.gpu_sysfs.clone();
    if let Some(dir) = sysfs {
        let read = |path: PathBuf| std::fs::read_to_string(path).ok().and_then(|s| s.trim().parse::<f64>().ok());
        if let Some(busy) = read(dir.join("gpu_busy_percent")) {
            samples.push(("gpu_busy_percent", busy));
        }
        let power = std::fs::read_dir(dir.join("hwmon"))
            .ok()
            .and_then(|mut d| d.next())
            .and_then(|e| e.ok())
            .and_then(|e| read(e.path().join("power1_average")));
        if let Some(microwatts) = power {
            samples.push(("gpu_power_watts", microwatts / 1e6));
        }
    }
    samples.push(("visible_meshes", visible as f64));
    samples.push(("visible_triangles", triangles as f64));
    let mut profile = world.resource_mut::<Profile>();
    for (name, value) in samples {
        let c = profile.counts.entry(name).or_default();
        c.sum += value;
        c.max = c.max.max(value);
        c.samples += 1;
    }
}

struct Stats {
    n: usize,
    mean: f64,
    p50: f64,
    p95: f64,
    p99: f64,
    max: f64,
}

fn stats(values: &[f32]) -> Stats {
    if values.is_empty() {
        return Stats { n: 0, mean: 0.0, p50: 0.0, p95: 0.0, p99: 0.0, max: 0.0 };
    }
    let mut v: Vec<f64> = values.iter().map(|&x| x as f64).collect();
    v.sort_by(f64::total_cmp);
    let at = |q: f64| v[((v.len() - 1) as f64 * q).round() as usize];
    Stats {
        n: v.len(),
        mean: v.iter().sum::<f64>() / v.len() as f64,
        p50: at(0.5),
        p95: at(0.95),
        p99: at(0.99),
        max: v[v.len() - 1],
    }
}

fn stats_json(s: &Stats) -> String {
    format!(
        "{{\"n\": {}, \"mean\": {:.3}, \"p50\": {:.3}, \"p95\": {:.3}, \"p99\": {:.3}, \"max\": {:.3}}}",
        s.n, s.mean, s.p50, s.p95, s.p99, s.max
    )
}

fn json_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn report(profile: &Profile, seconds: f32, cpu_cores: f64, window: Option<&Window>, adapter: Option<&RenderAdapterInfo>) -> String {
    let frames = stats(&profile.frames);
    let fixed = stats(&profile.fixed);
    let gpu = stats(&profile.gpu_frames);
    let ticks: u32 = profile.ticks.iter().sum();
    let mut out = String::from("{\n");
    let _ = writeln!(out, "  \"label\": {},", json_string(&profile.config.label));
    let _ = writeln!(
        out,
        "  \"build\": {},",
        json_string(if cfg!(debug_assertions) { "debug" } else { "release" })
    );
    let _ = writeln!(out, "  \"trace\": {},", cfg!(feature = "profile-trace"));
    for var in ["HULLBREAKERS_SCENARIO", "HULLBREAKERS_STRESS", "HULLBREAKERS_PRESENT", "HULLBREAKERS_PROFILE_AB"] {
        let value = std::env::var(var).unwrap_or_default();
        let _ = writeln!(out, "  {}: {},", json_string(&var.to_lowercase()), json_string(&value));
    }
    if let Some(w) = window {
        let _ = writeln!(out, "  \"present_mode\": {},", json_string(&format!("{:?}", w.present_mode)));
        let _ = writeln!(
            out,
            "  \"resolution\": [{}, {}],",
            w.resolution.physical_width(),
            w.resolution.physical_height()
        );
    }
    if let Some(a) = adapter {
        let _ = writeln!(out, "  \"adapter\": {},", json_string(&format!("{} ({:?})", a.name, a.backend)));
    }
    let _ = writeln!(out, "  \"seconds\": {seconds:.3},");
    let _ = writeln!(out, "  \"fps\": {:.2},", frames.n as f64 / seconds as f64);
    let _ = writeln!(out, "  \"frame_ms\": {},", stats_json(&frames));
    let _ = writeln!(out, "  \"fixed_ms_per_frame\": {},", stats_json(&fixed));
    let _ = writeln!(out, "  \"fixed_ticks\": {ticks},");
    let _ = writeln!(
        out,
        "  \"fixed_ms_per_tick\": {:.4},",
        profile.fixed.iter().map(|&x| x as f64).sum::<f64>() / ticks.max(1) as f64
    );
    let _ = writeln!(out, "  \"gpu_ms\": {},", stats_json(&gpu));
    let mut passes: Vec<_> = profile.gpu_passes.iter().collect();
    passes.sort_by(|a, b| b.1.0.total_cmp(&a.1.0));
    let _ = writeln!(out, "  \"gpu_passes_ms_per_frame\": {{");
    let gpu_frames = profile.gpu_frames.len().max(1) as f64;
    let passes: Vec<String> = passes
        .iter()
        .map(|(name, (total, _))| format!("    {}: {:.4}", json_string(name), total / gpu_frames))
        .collect();
    let _ = writeln!(out, "{}\n  }},", passes.join(",\n"));
    let _ = writeln!(out, "  \"cpu_cores\": {cpu_cores:.3},");
    // Sent to each remote client over the window (an authority's).
    if let Some((start, end)) = &profile.sent {
        let per_link: Vec<String> = end
            .iter()
            .map(|(link, (packets, bytes))| {
                let (p0, b0) = start.get(link).copied().unwrap_or_default();
                format!(
                    "{{\"packets_per_s\": {:.1}, \"kbytes_per_s\": {:.2}}}",
                    (packets - p0) as f64 / seconds as f64,
                    (bytes - b0) as f64 / 1000.0 / seconds as f64
                )
            })
            .collect();
        let _ = writeln!(out, "  \"sent_per_client\": [{}],", per_link.join(", "));
    }
    for (key, name) in [("gpu_busy_percent", "gpu_busy_percent"), ("gpu_power_watts", "gpu_power_watts")] {
        if let Some(c) = profile.counts.get(key) {
            let _ = writeln!(out, "  \"{name}\": {:.2},", c.sum / c.samples.max(1) as f64);
        }
    }
    let _ = writeln!(out, "  \"counts\": {{");
    let counts: Vec<String> = profile
        .counts
        .iter()
        .map(|(name, c)| {
            format!(
                "    {}: {{\"mean\": {:.1}, \"max\": {:.0}}}",
                json_string(name),
                c.sum / c.samples.max(1) as f64,
                c.max
            )
        })
        .collect();
    let _ = writeln!(out, "{}\n  }}", counts.join(",\n"));
    out.push_str("}\n");
    out
}
