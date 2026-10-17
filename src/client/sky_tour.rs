//! The sky tour: screenshots of every sky, for reviewing ambiences without
//! a human at the controls.
//!
//! `HULLBREAKERS_SCENARIO=sky HULLBREAKERS_SHOTS=<dir> HULLBREAKERS_SHOTS_SHIP=sky`
//! goes through every named ambience, then `HULLBREAKERS_SKY_SEEDS` seeds
//! (default 2) of each archetype, and in each takes the same views from the
//! sky preview's visitors' place: the test stand, toward the sun, away from
//! it, along the band (or at the biggest nebula), at the biggest body, and a
//! narrow field of view for the stars' sharpness. `HULLBREAKERS_SKY_ONLY=a,b`
//! takes only the skies whose names start with those. Each sky is set straight
//! into the authority's state (the tour runs on a host), where the preview
//! leaves it until its console is worked. Shots are `<sky>_<view>.png`;
//! `tools/sky-sheet <dir>` tiles them into a contact sheet per sky.

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

use crate::sim::ambience::{self, Ambience, Archetype};
use crate::sim::player::{Mode, Player};
use crate::sim::scenarios::sky::STAND_AT;
use crate::sim::tuning::Tuning;

/// Where the visitors float, looking at the stand along −Z.
const EYE: Vec3 = Vec3::new(0.0, STAND_AT.y, STAND_AT.z + 9.0);
/// How long a new sky gets to bake and light before its first shot, and a
/// view to settle before its shot (s).
const NEW_SKY_SECONDS: f32 = 2.0;
const VIEW_SECONDS: f32 = 0.6;
/// The narrow view's field of view (degrees).
const NARROW_FOV: f32 = 24.0;

#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Stand,
    Sun,
    Away,
    Band,
    Body,
    Narrow,
}

impl View {
    const ALL: [View; 6] = [View::Stand, View::Sun, View::Away, View::Band, View::Body, View::Narrow];

    fn name(self) -> &'static str {
        match self {
            View::Stand => "stand",
            View::Sun => "sun",
            View::Away => "away",
            View::Band => "band",
            View::Body => "body",
            View::Narrow => "narrow",
        }
    }

    /// Where it looks in `sky`, if it has anything to look at.
    fn looking(self, sky: &Ambience) -> Option<Vec3> {
        let sun = sky.sun_toward();
        match self {
            View::Stand => Some(Vec3::NEG_Z),
            View::Sun => Some(sun),
            View::Away => Some(-sun),
            View::Band => sky
                .band
                .as_ref()
                .map(|b| b.bulge)
                .or_else(|| sky.nebulae.iter().max_by(|a, b| a.size.total_cmp(&b.size)).map(|n| n.direction)),
            View::Body => sky.bodies.iter().max_by(|a, b| a.size.total_cmp(&b.size)).map(|b| b.direction),
            // Off the sun and the bodies, among the stars.
            View::Narrow => Some((-sun + Vec3::new(0.3, 0.4, 0.2)).normalize()),
        }
        .map(|v| v.normalize_or(Vec3::NEG_Z))
    }
}

#[derive(Resource)]
pub struct SkyTour {
    dir: std::path::PathBuf,
    skies: Vec<Ambience>,
    sky: usize,
    view: usize,
    timer: f32,
    /// The shot of the current view is taken; move on next frame.
    captured: bool,
    fov: Option<f32>,
}

pub fn sky_tour_from_env() -> Option<SkyTour> {
    if std::env::var("HULLBREAKERS_SHOTS_SHIP").as_deref() != Ok("sky") {
        return None;
    }
    let dir = std::env::var_os("HULLBREAKERS_SHOTS")?;
    let seeds: u64 = std::env::var("HULLBREAKERS_SKY_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(2);
    let mut skies: Vec<Ambience> = ambience::names().into_iter().filter_map(|n| Ambience::named(n).ok()).collect();
    for archetype in Archetype::ALL {
        skies.extend((1..=seeds).map(|seed| ambience::generate(archetype, seed)));
    }
    if let Ok(only) = std::env::var("HULLBREAKERS_SKY_ONLY") {
        let only: Vec<&str> = only.split(',').map(str::trim).filter(|s| !s.is_empty()).collect();
        if !only.is_empty() {
            skies.retain(|s| only.iter().any(|o| s.name.starts_with(o)));
        }
    }
    Some(SkyTour {
        dir: dir.into(),
        skies,
        sky: 0,
        view: 0,
        timer: -NEW_SKY_SECONDS,
        captured: false,
        fov: None,
    })
}

pub fn run_sky_tour(
    time: Res<Time>,
    mut tour: ResMut<SkyTour>,
    mut player: Single<&mut Player, With<super::LocalPlayer>>,
    sky: Option<Single<&mut Ambience>>,
    mut tuning: ResMut<Tuning>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let Some(mut sky) = sky else { return };
    let tour = &mut *tour;
    let Some(wanted) = tour.skies.get(tour.sky) else {
        if let Some(fov) = tour.fov.take() {
            tuning.fov_deg = fov;
        }
        exit.write(AppExit::Success);
        return;
    };
    if **sky != *wanted {
        **sky = wanted.clone();
    }
    let view = View::ALL[tour.view];
    // Skip views with nothing to look at.
    let Some(looking) = view.looking(wanted) else {
        tour.view += 1;
        if tour.view == View::ALL.len() {
            tour.view = 0;
            tour.sky += 1;
            tour.timer = -NEW_SKY_SECONDS;
        }
        return;
    };
    // Hold the view (the sim moves players; this puts them back).
    let up = if looking.y.abs() > 0.95 { Vec3::Z } else { Vec3::Y };
    player.mode = Mode::Eva {
        position: EYE,
        velocity: Vec3::ZERO,
        orientation: Transform::default().looking_to(looking, up).rotation,
        pitch: 0.0,
    };
    let narrow = view == View::Narrow;
    match (narrow, tour.fov) {
        (true, None) => {
            tour.fov = Some(tuning.fov_deg);
            tuning.fov_deg = NARROW_FOV;
        }
        (false, Some(fov)) => {
            tuning.fov_deg = fov;
            tour.fov = None;
        }
        _ => {}
    }
    tour.timer += time.delta_secs();
    if tour.timer < VIEW_SECONDS {
        return;
    }
    if !tour.captured {
        let name = wanted.name.replace(' ', "_");
        let path = tour.dir.join(format!("{name}_{}.png", view.name()));
        commands.spawn(Screenshot::primary_window()).observe(save_to_disk(path));
        tour.captured = true;
        return;
    }
    tour.captured = false;
    tour.timer = 0.0;
    tour.view += 1;
    if tour.view == View::ALL.len() {
        tour.view = 0;
        tour.sky += 1;
        tour.timer = -NEW_SKY_SECONDS;
    }
}
