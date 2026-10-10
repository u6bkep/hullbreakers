//! Keyboard and mouse to [`PlayerInput`].
//!
//! | Input            | Booted            | EVA                    |
//! |------------------|-------------------|------------------------|
//! | WASD             | walk              | thrust                 |
//! | Space / Ctrl     | jump / —          | thrust up / down       |
//! | Shift            | sprint            | boost                  |
//! | Q / E            | —                 | roll                   |
//! | F                | toggle mag boots  | toggle mag boots       |
//! | V                | toggle flight assist (on by default)       |
//! | T                | set reference body to the one under the crosshair |
//! | X (hold)         | —                 | brake vs. last body    |
//! | LMB              | fire / release grapple                     |
//! | RMB (hold)       | reel in                                    |
//! | R (hold)         | pay out rope                               |
//! | Tab              | tuning panel (frees cursor)                |
//! | L                | debug: ship lurch                          |

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::LocalInput;
use crate::sim::ship::ShipCommand;
use crate::sim::tuning::Tuning;

/// Whether the mouse is captured for look. False while the tuning panel is open.
#[derive(Resource)]
pub struct MouseCaptured(pub bool);

pub fn toggle_capture(
    keys: Res<ButtonInput<KeyCode>>,
    mut captured: ResMut<MouseCaptured>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    if keys.just_pressed(KeyCode::Tab) {
        captured.0 = !captured.0;
    }
    if keys.just_pressed(KeyCode::Escape) {
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
    let axis = |pos: KeyCode, neg: KeyCode| {
        keys.pressed(pos) as i32 as f32 - keys.pressed(neg) as i32 as f32
    };
    cmd.movement = Vec3::new(
        axis(KeyCode::KeyD, KeyCode::KeyA),
        axis(KeyCode::Space, KeyCode::ControlLeft),
        axis(KeyCode::KeyW, KeyCode::KeyS),
    );
    cmd.roll = axis(KeyCode::KeyE, KeyCode::KeyQ);
    cmd.sprint = keys.pressed(KeyCode::ShiftLeft);
    cmd.brake = keys.pressed(KeyCode::KeyX);
    cmd.reel_in = mouse.pressed(MouseButton::Right);
    cmd.pay_out = keys.pressed(KeyCode::KeyR);
    cmd.add_look(motion.delta * tuning.mouse_sensitivity);
    let presses = &mut cmd.presses;
    for (pressed, counter) in [
        (keys.just_pressed(KeyCode::Space), &mut presses.jump),
        (keys.just_pressed(KeyCode::KeyF), &mut presses.toggle_boots),
        (keys.just_pressed(KeyCode::KeyV), &mut presses.toggle_assist),
        (keys.just_pressed(KeyCode::KeyT), &mut presses.set_reference),
        (mouse.just_pressed(MouseButton::Left), &mut presses.grapple),
    ] {
        if pressed {
            counter.press();
        }
    }

    if keys.just_pressed(KeyCode::KeyL) {
        ship_orders.write(ShipCommand::Lurch);
    }
}
