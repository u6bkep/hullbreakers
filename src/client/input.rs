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
//! | 1-4              | stance (by default weapon slot 1+grapple, slot 2+grapple, slot 2 braced, grapple only; see the inventory) |
//! | I                | the menu, on your inventory                |
//! | G                | put down what's in hand                    |
//! | F1               | tuning panel (frees cursor)                |
//! | Escape           | the menu (frees cursor)                    |
//! | L                | debug: ship lurch                          |

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use super::LocalInput;
use super::bindings::{Action, Bindings, Held};
use super::LocalPlayer;
use super::menu::{ItemOps, Menu, StanceLayout, Tab};
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
            if menu.open && menu.tab == Tab::Inventory {
                menu.close();
            } else {
                menu.open_on(Tab::Inventory);
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
    mut ops: ResMut<ItemOps>,
    player: Option<Single<(Entity, &crate::sim::player::Player), With<LocalPlayer>>>,
    mut local: ResMut<LocalInput>,
    mut ship_orders: MessageWriter<ShipCommand>,
    mut loot_orders: MessageWriter<crate::sim::pickups::SpawnLoot>,
) {
    let cmd = &mut local.0;
    cmd.stances = Some(stances.0);
    // The inventory screen's changes, one at a time: the next once the
    // simulation has done the last. (Without a player, they're dropped.)
    match player.as_deref() {
        Some((_, player)) if player.input.item_op_done(cmd) => {
            if let Some(op) = ops.0.pop_front() {
                cmd.ask(op);
            }
        }
        Some(_) => {}
        None => ops.0.clear(),
    }
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
    // Zoomed, the view is narrower and look slows to match.
    let zoom = player.as_deref().map_or(1.0, |(_, p)| p.loadout.zoom.max(1.0));
    cmd.add_look(motion.delta * tuning.mouse_sensitivity / zoom);
    let presses = &mut cmd.presses;
    for (action, counter) in [
        (Action::Jump, &mut presses.jump),
        (Action::ToggleBoots, &mut presses.toggle_boots),
        (Action::ToggleAssist, &mut presses.toggle_assist),
        (Action::SetReference, &mut presses.set_reference),
        (Action::Interact, &mut presses.interact),
        (Action::DropItem, &mut presses.drop),
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
    if held.started(&bindings, Action::DebugLoot)
        && let Some((near, _)) = player.as_deref()
    {
        loot_orders.write(crate::sim::pickups::SpawnLoot { near: *near });
    }
}
