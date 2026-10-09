//! Grapple rope constraint.
//!
//! The rope is an inextensible, one-sided distance limit between the player
//! and an anchor point on a body. It is solved at the velocity level: when taut
//! and separating, an impulse removes the separating velocity (perfectly
//! inelastic, so it can never add kinetic energy except for the small
//! position-error bias controlled by `stiffness`).

use bevy::prelude::*;

/// One end of the rope, as seen by the solver.
#[derive(Clone, Copy, Debug)]
pub struct RopeEnd {
    pub position: Vec3,
    pub velocity: Vec3,
    /// Inverse effective mass along the rope direction (0 = immovable).
    pub inverse_mass: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RopeImpulse {
    /// Unit vector from the player toward the anchor.
    pub direction: Vec3,
    /// Impulse magnitude. The player receives `+direction * magnitude`, the
    /// anchor `-direction * magnitude`.
    pub magnitude: f32,
    /// True if the impulse hit `max_impulse`.
    pub clamped: bool,
}

/// Slack within which a running winch still engages the rope.
const WINCH_ENGAGE: f32 = 0.02;

/// Computes the rope impulse for this tick, or `None` if the rope is slack or
/// already closing fast enough.
///
/// With `winch_speed`, the winch is a force-limited motor: it pulls until the
/// ends close at that speed. The caller takes in slack as the ends approach
/// (`length = min(length, distance)`), so the rope never runs ahead of the
/// player and goes slack/taut every tick, which a length-shrinking winch on a
/// rigid constraint does.
pub fn solve_rope(
    player: RopeEnd,
    anchor: RopeEnd,
    length: f32,
    dt: f32,
    stiffness: f32,
    max_impulse: f32,
    winch_speed: Option<f32>,
) -> Option<RopeImpulse> {
    let delta = anchor.position - player.position;
    let distance = delta.length();
    let engage = if winch_speed.is_some() {
        length - WINCH_ENGAGE
    } else {
        length
    };
    if distance <= engage || distance < 1e-4 {
        return None;
    }
    let direction = delta / distance;
    // Positive when the player is moving toward the anchor.
    let closing_speed = (player.velocity - anchor.velocity).dot(direction);
    let target = winch_speed.unwrap_or(0.0) + stiffness * (distance - length).max(0.0) / dt;
    if closing_speed >= target {
        return None;
    }
    let inverse_mass_sum = player.inverse_mass + anchor.inverse_mass;
    if inverse_mass_sum <= 0.0 {
        return None;
    }
    let wanted = (target - closing_speed) / inverse_mass_sum;
    let magnitude = wanted.min(max_impulse);
    Some(RopeImpulse {
        direction,
        magnitude,
        clamped: magnitude < wanted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 60.0;

    fn kinetic(m: f32, v: Vec3) -> f32 {
        0.5 * m * v.length_squared()
    }

    #[test]
    fn slack_rope_does_nothing() {
        let p = RopeEnd {
            position: Vec3::ZERO,
            velocity: Vec3::X * -5.0,
            inverse_mass: 1.0,
        };
        let a = RopeEnd {
            position: Vec3::X * 5.0,
            velocity: Vec3::ZERO,
            inverse_mass: 0.0,
        };
        assert!(solve_rope(p, a, 10.0, DT, 0.3, f32::MAX, None).is_none());
    }

    #[test]
    fn taut_rope_never_adds_energy_without_bias() {
        // Two free masses flying apart at the rope's full length.
        let (m1, m2) = (120.0, 400.0);
        let mut p = RopeEnd {
            position: Vec3::ZERO,
            velocity: Vec3::new(-3.0, 2.0, 0.5),
            inverse_mass: 1.0 / m1,
        };
        let mut a = RopeEnd {
            position: Vec3::X * 10.0,
            velocity: Vec3::new(4.0, -1.0, 0.0),
            inverse_mass: 1.0 / m2,
        };
        let before = kinetic(m1, p.velocity) + kinetic(m2, a.velocity);
        // Nudge just past full length so the constraint is active with no bias.
        let imp = solve_rope(p, a, 10.0 - 1e-6, DT, 0.0, f32::MAX, None).expect("taut");
        p.velocity += imp.direction * imp.magnitude * p.inverse_mass;
        a.velocity -= imp.direction * imp.magnitude * a.inverse_mass;
        let after = kinetic(m1, p.velocity) + kinetic(m2, a.velocity);
        assert!(after <= before + 1e-3, "energy grew: {before} -> {after}");
        // Momentum is conserved.
        let mom_before = Vec3::new(-3.0, 2.0, 0.5) * m1 + Vec3::new(4.0, -1.0, 0.0) * m2;
        let mom_after = p.velocity * m1 + a.velocity * m2;
        assert!(mom_before.abs_diff_eq(mom_after, 1e-2));
        // And they no longer separate.
        assert!((p.velocity - a.velocity).dot(imp.direction) >= -1e-4);
    }

    #[test]
    fn immovable_anchor_turns_radial_motion_into_swing() {
        // Player at the end of a rope from a fixed anchor, moving tangentially
        // and outward: outward part is removed, tangential part survives.
        let p = RopeEnd {
            position: Vec3::ZERO,
            velocity: Vec3::new(-2.0, 5.0, 0.0),
            inverse_mass: 1.0 / 120.0,
        };
        let a = RopeEnd {
            position: Vec3::X * 10.0,
            velocity: Vec3::ZERO,
            inverse_mass: 0.0,
        };
        let imp = solve_rope(p, a, 10.0 - 1e-6, DT, 0.0, f32::MAX, None).unwrap();
        let v = p.velocity + imp.direction * imp.magnitude * p.inverse_mass;
        assert!(v.abs_diff_eq(Vec3::new(0.0, 5.0, 0.0), 1e-3));
    }

    #[test]
    fn winch_force_is_limited() {
        let p = RopeEnd {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            inverse_mass: 1.0 / 120.0,
        };
        let a = RopeEnd {
            position: Vec3::X * 10.0,
            velocity: Vec3::ZERO,
            inverse_mass: 0.0,
        };
        let max = 1000.0 * DT;
        let imp = solve_rope(p, a, 9.0, DT, 0.3, max, None).unwrap();
        assert!(imp.clamped);
        assert_eq!(imp.magnitude, max);
    }

    #[test]
    fn winch_reels_in_smoothly() {
        // A player hanging at the end of a rope from a fixed anchor, reeled in.
        let (m, force, speed) = (120.0, 8000.0, 20.0);
        let anchor = RopeEnd {
            position: Vec3::ZERO,
            velocity: Vec3::ZERO,
            inverse_mass: 0.0,
        };
        let mut pos = Vec3::X * 40.0;
        let mut vel = Vec3::ZERO;
        let mut length = 40.0;
        let mut engaged = Vec::new();
        for _ in 0..90 {
            let p = RopeEnd {
                position: pos,
                velocity: vel,
                inverse_mass: 1.0 / m,
            };
            let imp = solve_rope(p, anchor, length, DT, 0.3, force * DT, Some(speed));
            if let Some(imp) = imp {
                vel += imp.direction * imp.magnitude / m;
            }
            engaged.push(imp.is_some());
            pos += vel * DT;
            length = length.min(pos.length());
        }
        // Accelerates at force/mass until reaching reel speed, then holds it:
        // never overshoots into slack and re-engages.
        let closing = -vel.x;
        assert!((closing - speed).abs() < 0.5, "closing at {closing}");
        let flips = engaged.windows(2).filter(|w| w[0] != w[1]).count();
        assert!(
            flips <= 1,
            "winch engaged/disengaged {flips} times: {engaged:?}"
        );
    }
}
