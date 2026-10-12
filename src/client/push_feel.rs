//! Feeling a push the client couldn't predict (see
//! docs/proposal-remote-forces.md, option A): a shot's shove, a blast, a
//! rope's yank. The client learns of it a round trip late, as a correction
//! of its own player's state; the camera blends over the correction (more
//! slowly the bigger it is: `camera::blend_out`). This starts the felt
//! reaction the moment the client knows: a head kick the way it was pushed,
//! shake, and a thump through the suit, so the push reads as a blow even
//! while the view glides to where it put you.
//!
//! Two sources, both already on the client:
//! - the correction itself (`ViewCorrection`, from the rollback): where
//!   the authority says you went that you didn't expect, and how far;
//! - your `HurtLog` (replicated): what hurt you and where from, for hits
//!   that shove too little to correct (a booted player's stagger).
//!
//! Presentation only: nothing here changes the simulation or the network.

use bevy::audio::{PlaybackMode, Volume};
use bevy::prelude::*;

use super::LocalPlayer;
use super::ViewCorrection;
use super::camera::{CameraFx, PlayerCamera};
use super::sfx::Sounds;
use crate::sim::vitals::HurtLog;

/// A correction smaller than this (m) is noise, not a push.
const FELT: f32 = 0.15;

/// How many of the local player's hurts have been felt.
#[derive(Default)]
pub struct Felt(Option<u32>);

pub fn feel_pushes(
    correction: Res<ViewCorrection>,
    hurts: Option<Single<&HurtLog, With<LocalPlayer>>>,
    camera: Option<Single<&Transform, With<PlayerCamera>>>,
    sounds: Option<Res<Sounds>>,
    mut fx: ResMut<CameraFx>,
    mut felt: Local<Felt>,
    mut commands: Commands,
) {
    let mut thump = 0.0f32;
    // A correction: the authority moved us. The way we went is new minus
    // old (the correction holds old minus new).
    let size = correction.position.length();
    if size > FELT {
        let dir = -correction.position / size;
        fx.push(dir, 0.02 + 0.025 * size);
        fx.shake((0.15 + 0.15 * size).min(0.8));
        thump = thump.max((0.3 + 0.25 * size).min(1.0));
    }
    // Hurts: away from where they came from.
    if let Some(log) = hurts {
        let seen = felt.0.unwrap_or(log.count);
        for hurt in log.since(seen) {
            let k = (hurt.amount / 60.0).clamp(0.1, 1.0);
            if let (Some(from), Some(eye)) = (hurt.from, camera.as_ref().map(|c| c.translation)) {
                fx.push((eye - from).normalize_or(Vec3::Y), 0.03 * k);
            }
            fx.shake(0.2 * k);
            thump = thump.max(0.4 * k);
        }
        felt.0 = Some(log.count);
    }
    if thump > 0.0
        && let Some(sounds) = sounds
    {
        commands.spawn((
            AudioPlayer(sounds.impact().clone()),
            PlaybackSettings {
                mode: PlaybackMode::Despawn,
                volume: Volume::Linear(thump),
                speed: 0.7,
                ..default()
            },
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn felt_after(correction: Vec3) -> (f32, f32) {
        let mut app = App::new();
        app.insert_resource(ViewCorrection {
            position: correction,
            rotation: Quat::IDENTITY,
        });
        app.init_resource::<CameraFx>();
        app.add_systems(Update, feel_pushes);
        app.update();
        app.world().resource::<CameraFx>().felt()
    }

    #[test]
    fn a_push_the_client_did_not_predict_is_felt_at_once() {
        let (shake, kick) = felt_after(Vec3::new(-2.5, 0.0, 0.0));
        assert!(shake > 0.3 && kick > 0.05, "a blast's correction: shake {shake}, kick {kick}");
        let (small_shake, small_kick) = felt_after(Vec3::new(-0.5, 0.0, 0.0));
        assert!(small_kick > 0.0 && small_kick < kick && small_shake < shake, "a shove's: shake {small_shake}, kick {small_kick}");
        assert_eq!(felt_after(Vec3::new(0.05, 0.0, 0.0)), (0.0, 0.0), "noise isn't a push");
    }
}

