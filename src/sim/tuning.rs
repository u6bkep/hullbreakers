//! Live-tunable movement parameters.
//!
//! Every feel-relevant number lives here so it can be adjusted from the tuning
//! panel while playing and saved to `tuning.ron`. Fields are declared once in
//! the `tuning!` macro, which generates the struct, its defaults, and the slider
//! metadata the panel iterates over.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const TUNING_PATH: &str = "tuning.ron";

/// Metadata for one numeric field, used to build the tuning UI.
pub struct FloatField<'a> {
    pub section: &'static str,
    pub name: &'static str,
    pub value: &'a mut f32,
    pub min: f32,
    pub max: f32,
}

pub struct BoolField<'a> {
    pub section: &'static str,
    pub name: &'static str,
    pub value: &'a mut bool,
}

macro_rules! tuning {
    (
        floats { $( $section:literal { $( $f:ident : $fdefault:expr, $fmin:expr, $fmax:expr; )* } )* }
        bools { $( $bsection:literal { $( $b:ident : $bdefault:expr; )* } )* }
    ) => {
        #[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Tuning {
            $( $( pub $f: f32, )* )*
            $( $( pub $b: bool, )* )*
        }

        impl Default for Tuning {
            fn default() -> Self {
                Self {
                    $( $( $f: $fdefault, )* )*
                    $( $( $b: $bdefault, )* )*
                }
            }
        }

        impl Tuning {
            pub fn float_fields(&mut self) -> Vec<FloatField<'_>> {
                vec![ $( $( FloatField {
                    section: $section, name: stringify!($f), value: &mut self.$f, min: $fmin, max: $fmax,
                }, )* )* ]
            }

            pub fn bool_fields(&mut self) -> Vec<BoolField<'_>> {
                vec![ $( $( BoolField { section: $bsection, name: stringify!($b), value: &mut self.$b }, )* )* ]
            }
        }
    };
}

tuning! {
    floats {
        "Body" {
            player_mass: 120.0, 40.0, 400.0;
            capsule_radius: 0.35, 0.2, 0.6;
            capsule_height: 1.8, 1.0, 2.5;
            eye_height: 1.62, 0.8, 2.3;
        }
        "Boots" {
            walk_speed: 3.0, 0.5, 10.0;
            sprint_speed: 5.0, 0.5, 15.0;
            walk_accel: 25.0, 1.0, 100.0;
            jump_speed: 3.5, 0.5, 15.0;
            adhesion_accel: 8.0, 0.5, 50.0;
            shear_accel: 12.0, 0.5, 50.0;
            ground_probe: 0.3, 0.05, 1.0;
            max_ground_angle_deg: 50.0, 10.0, 89.0;
            surface_follow_rate: 10.0, 1.0, 60.0;
            attach_max_speed: 4.0, 0.5, 20.0;
            attach_max_angle_deg: 70.0, 10.0, 180.0;
            detach_cooldown: 0.25, 0.0, 2.0;
        }
        "Jetpack" {
            thrust_accel: 10.0, 0.5, 30.0;
            boost_multiplier: 2.0, 1.0, 5.0;
            brake_accel: 10.0, 0.5, 30.0;
            assist_max_speed: 10.0, 1.0, 50.0;
            match_surface_near: 2.0, 0.0, 20.0;
            match_surface_far: 10.0, 1.0, 100.0;
            roll_rate_deg: 90.0, 10.0, 360.0;
            bounce_restitution: 0.2, 0.0, 1.0;
            bounce_friction: 0.4, 0.0, 2.0;
        }
        "Grapple" {
            grapple_range: 200.0, 5.0, 500.0;
            reel_speed: 20.0, 0.5, 60.0;
            reel_force: 8000.0, 100.0, 40000.0;
            rope_break_force: 50000.0, 1000.0, 500000.0;
            rope_stiffness: 0.3, 0.0, 1.0;
        }
        "Ship" {
            ship_roll_rate_deg: 2.0, -30.0, 30.0;
            ship_yaw_wobble_deg: 1.0, 0.0, 20.0;
            ship_max_accel: 12.0, 0.5, 60.0;
            lurch_delta_v: 6.0, 0.0, 40.0;
            field_speed: 15.0, 0.0, 60.0;
        }
        "Mission" {
            home_survival_seconds: 210.0, 20.0, 1200.0;
            raider_shielded_seconds: 900.0, 20.0, 3600.0;
            raider_exposed_seconds: 12.0, 2.0, 120.0;
            wreck_return_seconds: 6.0, 1.0, 60.0;
            beam_push_speed: 8.0, 0.0, 40.0;
            duel_near: 100.0, 15.0, 300.0;
            duel_far: 400.0, 50.0, 800.0;
            duel_pass_seconds: 40.0, 5.0, 120.0;
        }
        "Camera" {
            mouse_sensitivity: 0.0022, 0.0002, 0.01;
            fov_deg: 85.0, 50.0, 120.0;
            smooth_time: 0.12, 0.0, 1.0;
            head_bob: 0.03, 0.0, 0.2;
        }
    }
    bools {
        "Boots" {
            wall_walk: true;
        }
        "Debug" {
            show_gizmos: false;
        }
    }
}

/// Sections that are each player's own preference rather than part of the
/// shared simulation.
const LOCAL_SECTIONS: [&str; 2] = ["Camera", "Debug"];

impl Tuning {
    /// Takes every simulation setting from the host's tuning, keeping this
    /// player's own preferences (camera, debug).
    pub fn adopt_shared(&mut self, host: &Tuning) {
        let mut host = host.clone();
        for (mine, theirs) in self.float_fields().into_iter().zip(host.float_fields()) {
            if !LOCAL_SECTIONS.contains(&mine.section) {
                *mine.value = *theirs.value;
            }
        }
        for (mine, theirs) in self.bool_fields().into_iter().zip(host.bool_fields()) {
            if !LOCAL_SECTIONS.contains(&mine.section) {
                *mine.value = *theirs.value;
            }
        }
    }

    pub fn load_or_default(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => ron::from_str(&text).unwrap_or_else(|err| {
                warn!("failed to parse {}: {err}; using defaults", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(std::io::Error::other)?;
        std::fs::write(path, text)
    }
}
