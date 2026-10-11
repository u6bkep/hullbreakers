//! Player input as the simulation sees it.
//!
//! Input is *absolute*: look is a running total, and each button press bumps
//! a counter. The sim compares against what it last applied ([`InputCursor`],
//! part of player state) and acts on the difference. Reading the same input
//! twice is therefore a no-op, and a skipped input is caught up by the next
//! one. That is what a network needs: a lost input is replaced by repeating
//! the last one, and a rollback replays stored inputs tick by tick, neither of
//! which may double a jump or a mouse movement.

use std::f32::consts::PI;

use bevy::ecs::entity::MapEntities;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// A button press counter. Wraps; only differences matter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub struct Press(u8);

impl Press {
    pub fn press(&mut self) {
        self.0 = self.0.wrapping_add(1);
    }

    /// Whether it was pressed since `seen`.
    fn since(self, seen: Press) -> bool {
        self.0 != seen.0
    }
}

/// Button presses, as counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub struct Presses {
    pub jump: Press,
    pub toggle_boots: Press,
    pub toggle_assist: Press,
    /// The hands' buttons (main: primary, off: secondary).
    pub main: Press,
    pub off: Press,
    /// Reload everything; reload one hand.
    pub reload: Press,
    pub reload_main: Press,
    pub reload_off: Press,
    /// Press the control under the crosshair.
    pub interact: Press,
    /// Make the body under the crosshair the reference frame.
    pub set_reference: Press,
}

/// The player's input, written by whoever drives the player (the local input
/// layer, a network, a test) and only read by the simulation.
#[derive(Component, Clone, Copy, Debug, Default, PartialEq, Reflect, Serialize, Deserialize)]
pub struct PlayerInput {
    /// Total mouse look in radians, x = yaw right, y = pitch down, each
    /// wrapped to [-LOOK_WRAP/2, LOOK_WRAP/2). Only its change between ticks
    /// is applied.
    pub look: Vec2,
    /// -1..1, positive rolls clockwise.
    pub roll: f32,
    /// x = right, y = up, z = forward, each -1..1.
    pub movement: Vec3,
    pub sprint: bool,
    pub brake: bool,
    pub reel_in: bool,
    pub pay_out: bool,
    /// What's held on each hand's buttons.
    pub main: HandHeld,
    pub off: HandHeld,
    /// The stance asked for (0-based).
    pub stance: u8,
    pub presses: Presses,
}

/// A hand's buttons held: its own, and its two alternate chords.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
pub struct HandHeld {
    pub held: bool,
    pub alt: bool,
    pub alt2: bool,
}

/// A hand's input for one tick.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HandInput {
    pub held: bool,
    /// Pressed since the last tick.
    pub pressed: bool,
    pub alt: bool,
    pub alt2: bool,
}

/// Input names no entities; required to send it over the network.
impl MapEntities for PlayerInput {
    fn map_entities<M: EntityMapper>(&mut self, _mapper: &mut M) {}
}

impl PlayerInput {
    /// Adds mouse look (radians), keeping the total wrapped.
    pub fn add_look(&mut self, delta: Vec2) {
        self.look += delta;
        // Wrapping costs precision; only do it when needed.
        if self.look.abs().max_element() >= LOOK_WRAP / 2.0 {
            self.look = wrap(self.look);
        }
    }
}

/// How much of the input the simulation has applied. Part of player state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct InputCursor {
    look: Vec2,
    presses: Presses,
}

/// One tick's worth of input: what changed since the cursor, plus held state.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TickInput {
    /// Look change in radians since the last tick.
    pub look: Vec2,
    pub roll: f32,
    pub movement: Vec3,
    pub sprint: bool,
    pub brake: bool,
    pub reel_in: bool,
    pub pay_out: bool,
    pub jump: bool,
    pub toggle_boots: bool,
    pub toggle_assist: bool,
    pub main: HandInput,
    pub off: HandInput,
    pub reload: bool,
    pub reload_main: bool,
    pub reload_off: bool,
    pub stance: u8,
    pub interact: bool,
    pub set_reference: bool,
}

impl TickInput {
    pub fn hand(&self, hand: super::weapons::Hand) -> HandInput {
        match hand {
            super::weapons::Hand::Main => self.main,
            super::weapons::Hand::Off => self.off,
        }
    }
}

impl InputCursor {
    /// Look not yet applied by the simulation, for zero-latency aiming.
    pub fn pending_look(&self, input: &PlayerInput) -> Vec2 {
        wrap(input.look - self.look)
    }

    /// Advances the cursor to `input`, returning what this tick should do.
    pub fn advance(&mut self, input: &PlayerInput) -> TickInput {
        let p = input.presses;
        let seen = self.presses;
        let tick = TickInput {
            look: self.pending_look(input),
            roll: input.roll,
            movement: input.movement,
            sprint: input.sprint,
            brake: input.brake,
            reel_in: input.reel_in,
            pay_out: input.pay_out,
            jump: p.jump.since(seen.jump),
            toggle_boots: p.toggle_boots.since(seen.toggle_boots),
            toggle_assist: p.toggle_assist.since(seen.toggle_assist),
            main: HandInput {
                held: input.main.held,
                pressed: p.main.since(seen.main),
                alt: input.main.alt,
                alt2: input.main.alt2,
            },
            off: HandInput {
                held: input.off.held,
                pressed: p.off.since(seen.off),
                alt: input.off.alt,
                alt2: input.off.alt2,
            },
            reload: p.reload.since(seen.reload),
            reload_main: p.reload_main.since(seen.reload_main),
            reload_off: p.reload_off.since(seen.reload_off),
            stance: input.stance,
            interact: p.interact.since(seen.interact),
            set_reference: p.set_reference.since(seen.set_reference),
        };
        self.look = input.look;
        self.presses = p;
        tick
    }
}

/// Look totals wrap at this many radians. It only bounds the float (small,
/// to keep precision); any look change between two ticks smaller than half
/// of it is read correctly. A 2π wrap would misread a fast flick in a hitched
/// frame as a turn the other way.
const LOOK_WRAP: f32 = 8.0 * PI;

fn wrap(v: Vec2) -> Vec2 {
    let w = |x: f32| (x + LOOK_WRAP / 2.0).rem_euclid(LOOK_WRAP) - LOOK_WRAP / 2.0;
    Vec2::new(w(v.x), w(v.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_input_is_a_no_op() {
        let mut cursor = InputCursor::default();
        let mut input = PlayerInput::default();
        input.presses.jump.press();
        input.add_look(Vec2::new(0.1, 0.0));
        let first = cursor.advance(&input);
        assert!(first.jump);
        assert!((first.look.x - 0.1).abs() < 1e-6);
        let again = cursor.advance(&input);
        assert!(!again.jump);
        assert_eq!(again.look, Vec2::ZERO);
    }

    #[test]
    fn skipped_input_is_caught_up() {
        let mut cursor = InputCursor::default();
        let mut input = PlayerInput::default();
        input.add_look(Vec2::new(0.1, 0.0));
        input.add_look(Vec2::new(0.1, 0.0));
        input.presses.off.press();
        let tick = cursor.advance(&input);
        assert!((tick.look.x - 0.2).abs() < 1e-6);
        assert!(tick.off.pressed);
    }

    #[test]
    fn look_survives_wrapping() {
        let mut cursor = InputCursor::default();
        let mut input = PlayerInput::default();
        for _ in 0..400 {
            input.add_look(Vec2::new(0.7, 0.0));
            let tick = cursor.advance(&input);
            assert!((tick.look.x - 0.7).abs() < 1e-3, "{}", tick.look.x);
        }
    }

    #[test]
    fn a_flick_bigger_than_half_a_turn_keeps_its_direction() {
        let mut cursor = InputCursor::default();
        let mut input = PlayerInput::default();
        input.add_look(Vec2::new(4.0, 0.0));
        assert!((cursor.advance(&input).look.x - 4.0).abs() < 1e-4);
    }
}
