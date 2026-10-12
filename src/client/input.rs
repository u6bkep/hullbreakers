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
//! | LMB / RMB        | use the main / off hand's item (gun: fire; grapple: fire or let go) |
//! | Ctrl+LMB / Ctrl+RMB | that hand's alternate (grapple: reel in) |
//! | Alt+LMB / Alt+RMB   | that hand's second alternate (grapple: pay out) |
//! | Mouse4 / Mouse5 (hold) | reel in / pay out, whichever hand holds the grapple |
//! | R                | reload (main hand first)                   |
//! | 1-4              | stance (by default SMG+grapple, rifle+grapple, rifle braced, grapple only; see the menu's loadout) |
//! | I                | the menu, on your loadout                  |
//! | F1               | tuning panel (frees cursor)                |
//! | Escape           | the menu (frees cursor)                    |
//! | L                | debug: ship lurch                          |

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::LocalInput;
use super::bindings::{Action, Bindings, Held};
use super::menu::{Menu, StanceLayout, Tab};
use crate::sim::input::HandHeld;
use crate::sim::ship::ShipCommand;
use crate::sim::tuning::Tuning;

/// Whether the mouse is captured for look: false while the menu or the
/// tuning panel is open.
#[derive(Resource)]
pub struct MouseCaptured(pub bool);

/// Whether the tuning panel is open.
#[derive(Resource, Default)]
pub struct TuningOpen(pub bool);

/// Opens and closes the menu and the tuning panel, and frees the cursor
/// while either is open. Escape always opens and closes the menu, whatever
/// else is bound to it.
pub fn toggle_capture(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    bindings: Res<Bindings>,
    mut menu: ResMut<Menu>,
    mut tuning: ResMut<TuningOpen>,
    mut captured: ResMut<MouseCaptured>,
    mut cursor: Single<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let held = Held { keys: &keys, mouse: &mouse };
    // A key being bound in the menu is only that.
    if !menu.binding() {
        // Escape works whatever Menu is bound to, so the menu can't be lost.
        let escape = keys.just_pressed(KeyCode::Escape);
        if held.started(&bindings, Action::Menu) || escape {
            if menu.open {
                menu.close();
            } else if tuning.0 {
                tuning.0 = false;
            } else {
                menu.open = true;
            }
        }
        if held.started(&bindings, Action::Loadout) {
            if menu.open && menu.tab == Tab::Loadout {
                menu.close();
            } else {
                menu.open_on(Tab::Loadout);
            }
        }
        if held.started(&bindings, Action::TuningPanel) {
            tuning.0 = !tuning.0;
        }
    }
    captured.0 = !menu.open && !tuning.0;
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
    stances: Res<StanceLayout>,
    mut local: ResMut<LocalInput>,
    mut ship_orders: MessageWriter<ShipCommand>,
) {
    let cmd = &mut local.0;
    cmd.stances = Some(stances.0);
    if !captured.0 {
        cmd.movement = Vec3::ZERO;
        cmd.roll = 0.0;
        cmd.sprint = false;
        cmd.brake = false;
        cmd.reel_in = false;
        cmd.pay_out = false;
        cmd.main = HandHeld::default();
        cmd.off = HandHeld::default();
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
    cmd.main = HandHeld {
        held: on(Action::MainUse),
        alt: on(Action::MainAlt),
        alt2: on(Action::MainAlt2),
    };
    cmd.off = HandHeld {
        held: on(Action::OffUse),
        alt: on(Action::OffAlt),
        alt2: on(Action::OffAlt2),
    };
    for (i, action) in [Action::Stance1, Action::Stance2, Action::Stance3, Action::Stance4]
        .into_iter()
        .enumerate()
    {
        if held.started(&bindings, action) {
            cmd.stance = i as u8;
            cmd.presses.stance.press();
        }
    }
    cmd.add_look(motion.delta * tuning.mouse_sensitivity);
    let presses = &mut cmd.presses;
    for (action, counter) in [
        (Action::Jump, &mut presses.jump),
        (Action::ToggleBoots, &mut presses.toggle_boots),
        (Action::ToggleAssist, &mut presses.toggle_assist),
        (Action::SetReference, &mut presses.set_reference),
        (Action::Interact, &mut presses.interact),
        (Action::MainUse, &mut presses.main),
        (Action::OffUse, &mut presses.off),
        (Action::Reload, &mut presses.reload),
        (Action::ReloadMain, &mut presses.reload_main),
        (Action::ReloadOff, &mut presses.reload_off),
    ] {
        if held.started(&bindings, action) {
            counter.press();
        }
    }

    if held.started(&bindings, Action::DebugLurch) {
        ship_orders.write(ShipCommand::Lurch);
    }
}
