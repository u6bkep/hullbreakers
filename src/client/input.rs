//! Keyboard and mouse to [`PlayerInput`], through the [`Bindings`].
//!
//! | Default          | Booted            | EVA                    |
//! |------------------|-------------------|------------------------|
//! | WASD             | walk              | thrust                 |
//! | Space / C        | jump / —          | thrust up / down       |
//! | Shift            | sprint            | boost                  |
//! | Q / E            | —                 | roll                   |
//! | B                | toggle mag boots  | toggle mag boots       |
//! | V                | toggle flight assist (on by default)       |
//! | T                | set reference body to the one under the crosshair |
//! | X (hold)         | —                 | brake vs. last body    |
//! | F                | press the button under the crosshair       |
//! | LMB              | fire / release grapple                     |
//! | Mouse4, Ctrl+LMB (hold) | reel in                             |
//! | Mouse5, Alt+LMB (hold)  | pay out rope                        |
//! | F1               | tuning panel (frees cursor)                |
//! | Escape           | free the cursor                            |
//! | L                | debug: ship lurch                          |

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::LocalInput;
use super::bindings::{Action, Bindings, Held};
use crate::sim::ship::ShipCommand;
use crate::sim::tuning::Tuning;

/// Whether the mouse is captured for look. False while the tuning panel is open.
#[derive(Resource)]
pub struct MouseCaptured(pub bool);

pub fn toggle_capture(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<Bindings>,
    mut captured: ResMut<MouseCaptured>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let held = Held { keys: &keys, mouse: &mouse };
    if held.started(&bindings, Action::TuningPanel) {
        captured.0 = !captured.0;
    }
    if held.started(&bindings, Action::ReleaseMouse) {
        captured.0 = false;
    }
    let (grab_mode, visible) = if captured.0 {
        (CursorGrabMode::Locked, false)
    } else {
        (CursorGrabMode::None, true)
    };
    if cursor.grab_mode != grab_mode {
        cursor.grab_mode = grab_mode;
        cursor.visible = visible;
    }
}

pub fn read_input(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    bindings: Res<Bindings>,
    captured: Res<MouseCaptured>,
    tuning: Res<Tuning>,
    mut local: ResMut<LocalInput>,
    mut ship_orders: MessageWriter<ShipCommand>,
) {
    let cmd = &mut local.0;
    if !captured.0 {
        cmd.movement = Vec3::ZERO;
        cmd.roll = 0.0;
        cmd.sprint = false;
        cmd.brake = false;
        cmd.reel_in = false;
        cmd.pay_out = false;
        return;
    }
    let held = Held { keys: &keys, mouse: &mouse };
    let on = |action| held.active(&bindings, action);
    let axis = |pos, neg| on(pos) as i32 as f32 - on(neg) as i32 as f32;
    cmd.movement = Vec3::new(
        axis(Action::Right, Action::Left),
        axis(Action::Up, Action::Down),
        axis(Action::Forward, Action::Back),
    );
    cmd.roll = axis(Action::RollRight, Action::RollLeft);
    cmd.sprint = on(Action::Sprint);
    cmd.brake = on(Action::Brake);
    cmd.reel_in = on(Action::ReelIn);
    cmd.pay_out = on(Action::PayOut);
    cmd.add_look(motion.delta * tuning.mouse_sensitivity);
    let presses = &mut cmd.presses;
    for (action, counter) in [
        (Action::Jump, &mut presses.jump),
        (Action::ToggleBoots, &mut presses.toggle_boots),
        (Action::ToggleAssist, &mut presses.toggle_assist),
        (Action::SetReference, &mut presses.set_reference),
        (Action::Interact, &mut presses.interact),
        (Action::Grapple, &mut presses.grapple),
    ] {
        if held.started(&bindings, action) {
            counter.press();
        }
    }

    if held.started(&bindings, Action::DebugLurch) {
        ship_orders.write(ShipCommand::Lurch);
    }
}
