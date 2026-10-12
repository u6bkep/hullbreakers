//! The in-game menu: your inventory (what you carry, and what your hands
//! hold in each stance), and the controls.
//!
//! The game goes on while it's open (it's co-op; there's no pausing), but
//! the cursor is free and your input is let go. Escape (or the Menu
//! binding) opens and closes it; the Loadout binding (I) opens it on the
//! inventory.
//!
//! What you carry is your player's (predicted) state: the inventory screen
//! asks the simulation for changes through your input (see [`ItemOps`]).
//! Stances are yours: what each stance puts in each hand is kept in
//! `stances.ron` and given to the simulation with your input, so it holds
//! through respawns and across games. Controls are kept in `keybinds.ron`
//! (see [`super::bindings`]). Both are saved as soon as they change.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use std::collections::VecDeque;

use super::LocalPlayer;
use super::bindings::{Action, Binding, Bindings, Button, KEYBINDS_PATH, Modifier, SLOTS};
use super::items_hud::{self, AMBER, DIM, INK};
use crate::sim::items::{AmmoKind, Inventory, Item, ItemOp, PACK_SLOTS, Place, WEAPON_SLOTS};
use crate::net::ProfileRequests;
use crate::sim::player::Player;
use crate::sim::progression::Profile;
use crate::sim::ship::HomeShip;
use crate::sim::weapons::{DEFAULT_STANCES, Hand, Loadout, STANCES, Slot, Stance};

pub const STANCES_PATH: &str = "stances.ron";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Inventory,
    /// The stash, at the weapons locker (aboard our ship).
    Armory,
    /// Tuning, refitting, repainting and scrapping, at the workbench.
    Workbench,
    Controls,
}

#[derive(Resource, Default)]
pub struct Menu {
    pub open: bool,
    pub tab: Tab,
    /// A binding waiting for its key.
    capture: Option<Capture>,
    /// The last thing saved, or what went wrong.
    status: String,
    /// A mouse button was just bound: its release isn't a click on the
    /// menu (it would start another binding, or clear the one just made).
    swallow_click: bool,
    /// The inventory item chosen, to move or look at.
    chosen: Option<Place>,
    /// What's chosen at the armory.
    pub armory: Option<crate::sim::progression::Holding>,
}

/// Inventory changes asked for in the menu, waiting to go to the
/// simulation one at a time.
#[derive(Resource, Default)]
pub struct ItemOps(pub VecDeque<ItemOp>);

/// Which binding is waiting for a key.
struct Capture {
    action: Action,
    slot: usize,
}

impl Menu {
    pub fn open_on(&mut self, tab: Tab) {
        self.open = true;
        self.tab = tab;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.capture = None;
    }

    /// Whether a binding is waiting for its key (so keys aren't commands).
    pub fn binding(&self) -> bool {
        self.capture.is_some()
    }
}

/// What the local player wants each stance to hold.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct StanceLayout(pub [Stance; STANCES]);

/// Loads `stances.ron` if there is one.
pub fn load_stances(mut commands: Commands) {
    let layout = match std::fs::read_to_string(STANCES_PATH) {
        Ok(text) => match ron::from_str(&text) {
            Ok(stances) => {
                info!("stances from {STANCES_PATH}");
                stances
            }
            Err(e) => {
                warn!("{STANCES_PATH}: {e}");
                DEFAULT_STANCES
            }
        },
        Err(_) => DEFAULT_STANCES,
    };
    commands.insert_resource(StanceLayout(layout));
}

fn save_stances(layout: &StanceLayout) -> String {
    let text = ron::ser::to_string_pretty(&layout.0, ron::ser::PrettyConfig::default()).expect("stances serialize");
    match std::fs::write(STANCES_PATH, text) {
        Ok(()) => format!("saved {STANCES_PATH}"),
        Err(e) => format!("couldn't save {STANCES_PATH}: {e}"),
    }
}

/// Puts `slot` in `hand` of `stance`. Whatever was there goes to the other
/// hand if that hand held `slot` (a swap); one thing can't be in both.
pub fn put(stance: &mut Stance, hand: Hand, slot: Slot) {
    let (this, other) = match hand {
        Hand::Main => (&mut stance.main, &mut stance.off),
        Hand::Off => (&mut stance.off, &mut stance.main),
    };
    if slot != Slot::Empty && *other == slot {
        *other = *this;
    }
    *this = slot;
}

/// Takes the key pressed for a binding being set. Runs before the game's
/// own reading of keys, which ignores them meanwhile.
pub fn capture_binding(
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut menu: ResMut<Menu>,
    mut bindings: ResMut<Bindings>,
) {
    let Some(capture) = menu.capture.as_ref() else {
        return;
    };
    if keys.just_pressed(KeyCode::Escape) {
        menu.capture = None;
        menu.status = "not changed".into();
        return;
    }
    let held: Vec<Modifier> = Modifier::ALL
        .into_iter()
        .filter(|m| m.keys().iter().any(|k| keys.pressed(*k)))
        .collect();
    let pressed = keys
        .get_just_pressed()
        .map(|k| Button::Key(*k))
        .chain(mouse.get_just_pressed().map(|m| Button::Mouse(*m)))
        .find(|b| Modifier::of(*b).is_none());
    let binding = if let Some(button) = pressed {
        Some(Binding { modifiers: held, button })
    } else {
        // A modifier pressed and let go alone is bound as a button.
        keys.get_just_released()
            .map(|k| Button::Key(*k))
            .find(|b| Modifier::of(*b).is_some())
            .map(|button| Binding {
                modifiers: held.into_iter().filter(|m| Some(*m) != Modifier::of(button)).collect(),
                button,
            })
    };
    let Some(binding) = binding else {
        return;
    };
    let (action, slot) = (capture.action, capture.slot);
    menu.capture = None;
    menu.swallow_click = matches!(binding.button, Button::Mouse(_));
    // Only what can be written down can be kept.
    if binding.to_string().parse::<Binding>().ok().as_ref() != Some(&binding) {
        menu.status = format!("{binding} can't be bound");
        return;
    }
    bindings.set(action, slot, Some(binding));
    menu.status = save_bindings(&bindings);
}

fn save_bindings(bindings: &Bindings) -> String {
    match bindings.save() {
        Ok(()) => format!("saved {KEYBINDS_PATH}"),
        Err(e) => format!("couldn't save {KEYBINDS_PATH}: {e}"),
    }
}

/// The menu's width (fixed, so it doesn't jump between tabs).
const MENU_WIDTH: f32 = 760.0;

pub fn menu(
    mut contexts: EguiContexts,
    mut menu: ResMut<Menu>,
    mut bindings: ResMut<Bindings>,
    mut layout: ResMut<StanceLayout>,
    player: Option<Single<(&Player, Option<&Profile>), With<LocalPlayer>>>,
    homes: Query<(), With<HomeShip>>,
    mut ops: ResMut<ItemOps>,
    mut requests: Option<ResMut<ProfileRequests>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut exit: MessageWriter<AppExit>,
) -> Result {
    if !menu.open {
        menu.close();
        return Ok(());
    }
    // Clicks count again once the bound button is up (and its release seen).
    if mouse.get_pressed().next().is_none() && mouse.get_just_released().next().is_none() {
        menu.swallow_click = false;
    }
    let ctx = contexts.ctx_mut()?;
    // The world, dimmed behind it.
    ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("menu shade")))
        .rect_filled(ctx.content_rect(), 0.0, egui::Color32::from_black_alpha(140));
    // The controls list takes the screen's height, less the window around it.
    let list_height = (ctx.content_rect().height() - 48.0 - 200.0).max(160.0);
    let menu = &mut *menu;
    let (player, profile) = match player.as_deref() {
        Some((player, profile)) => (Some(*player), *profile),
        None => (None, None),
    };
    // The armory is aboard our ship.
    let aboard = player.is_some_and(|p| p.mode.body().is_some_and(|b| homes.contains(b)));
    if matches!(menu.tab, Tab::Armory | Tab::Workbench) && !aboard {
        menu.tab = Tab::Inventory;
    }
    egui::Window::new("Hullbreakers")
        // Pinned at the top, so nothing moves when what's in it grows.
        .anchor(egui::Align2::CENTER_TOP, [0.0, 48.0])
        .collapsible(false)
        .resizable([false, false])
        .min_width(MENU_WIDTH)
        .max_width(MENU_WIDTH)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut menu.tab, Tab::Inventory, egui::RichText::new("Inventory").size(16.0));
                if aboard {
                    ui.selectable_value(&mut menu.tab, Tab::Armory, egui::RichText::new("Armory").size(16.0));
                    ui.selectable_value(&mut menu.tab, Tab::Workbench, egui::RichText::new("Workbench").size(16.0));
                }
                ui.selectable_value(&mut menu.tab, Tab::Controls, egui::RichText::new("Controls").size(16.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Quit game").clicked() {
                        exit.write(AppExit::Success);
                    }
                    if ui.button("Resume").clicked() {
                        menu.close();
                    }
                });
            });
            ui.separator();
            match menu.tab {
                Tab::Inventory => inventory_tab(
                    ui,
                    &mut layout,
                    player,
                    &bindings,
                    &mut menu.status,
                    &mut menu.chosen,
                    &mut ops,
                ),
                Tab::Armory => super::armory::armory_tab(ui, player, profile, &mut menu.armory, requests.as_deref_mut()),
                Tab::Workbench => super::workbench::workbench_tab(ui, player, profile, &mut menu.armory, requests.as_deref_mut()),
                Tab::Controls => controls_tab(ui, menu, &mut bindings, list_height),
            }
            if !menu.status.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new(&menu.status).small().color(DIM));
            }
        });
    Ok(())
}

/// What a stance slot holds, given what's carried: "1: Issue SMG".
pub fn slot_name(slot: Slot, inventory: &Inventory) -> String {
    match slot {
        Slot::Empty => "Empty".into(),
        Slot::Grapple => "Grapple".into(),
        Slot::Gun(i) => match inventory.weapons.get(i as usize).copied().flatten() {
            Some(w) => format!("{}: {}", i + 1, w.name()),
            None => format!("{}: (empty slot)", i + 1),
        },
    }
}

/// Asks the simulation to change the inventory (sent one at a time with
/// the input; see `input::read_input`).
fn ask(ops: &mut ItemOps, op: ItemOp) {
    ops.0.push_back(op);
}

/// An inventory place's card, three to a row.
pub(super) const CELL: egui::Vec2 = egui::vec2(150.0, 46.0);
pub(super) const GAP: f32 = 8.0;
/// The height of the inventory above the stances (fixed, so the stances
/// don't move as the chosen item's details change).
const ITEMS_HEIGHT: f32 = 365.0;

/// One place in the inventory, drawn as a card: what's there, in its
/// colour. `chosen` is lit; `target` (where the chosen item could go) is
/// outlined.
pub(super) fn cell(ui: &mut egui::Ui, size: egui::Vec2, tag: &str, item: Option<Item>, chosen: bool, target: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let painter = ui.painter_at(rect);
    let color = item.as_ref().map_or(DIM, items_hud::item_color);
    let hovered = response.hovered();
    painter.rect_filled(rect, 4.0, egui::Color32::from_black_alpha(if chosen { 230 } else if hovered { 200 } else { 160 }));
    let stroke = if chosen {
        egui::Stroke::new(2.0, color)
    } else if target {
        egui::Stroke::new(1.0, INK.gamma_multiply(0.45))
    } else if item.is_some() {
        egui::Stroke::new(1.0, color.gamma_multiply(0.6))
    } else {
        egui::Stroke::new(1.0, egui::Color32::from_white_alpha(25))
    };
    painter.rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Inside);
    // A rarity edge down the left.
    if item.is_some() {
        painter.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())), 2.0, color);
    }
    let inner = rect.shrink2(egui::vec2(9.0, 5.0));
    painter.text(inner.left_top(), egui::Align2::LEFT_TOP, tag, egui::FontId::monospace(9.0), DIM);
    let Some(item) = item else {
        painter.text(inner.left_center() + egui::vec2(0.0, 5.0), egui::Align2::LEFT_CENTER, "empty", egui::FontId::proportional(12.0), DIM);
        return response;
    };
    let name = painter.layout_no_wrap(item.name(), egui::FontId::proportional(13.0), color);
    let name = if name.size().x <= inner.width() {
        name
    } else {
        painter.layout_no_wrap(item.name(), egui::FontId::proportional(10.5), color)
    };
    painter.galley(inner.left_top() + egui::vec2(0.0, 11.0), name, color);
    let detail = match item {
        Item::Weapon(w) => format!("{} · {}/{}", w.frame.name(), w.mag, w.stats().mag),
        Item::Ammo { .. } => String::new(),
        Item::Cargo(c) if c.bulky() => "bulky: both hands".into(),
        Item::Cargo(_) => "cargo".into(),
    };
    painter.text(inner.left_bottom(), egui::Align2::LEFT_BOTTOM, detail, egui::FontId::monospace(9.0), DIM);
    response
}

/// What a click on an inventory place does: with nothing chosen, choose
/// what's there; on the chosen one, let go of it; elsewhere, move the
/// chosen item there if it can go (else choose what's there). A right
/// click drops what's there.
fn click(inv: &Inventory, place: Place, response: &egui::Response, ops: &mut ItemOps, chosen: &mut Option<Place>) {
    if response.secondary_clicked() && inv.get(place).is_some() {
        ask(ops, ItemOp::Drop(place));
        *chosen = None;
    } else if response.clicked() {
        match *chosen {
            Some(from) if from == place => *chosen = None,
            Some(from) if can_move(inv, from, place) => {
                ask(ops, ItemOp::Move { from, to: place });
                *chosen = Some(place);
            }
            _ => *chosen = inv.get(place).is_some().then_some(place),
        }
    }
}

/// Whether what's at `from` can go to `to` (swapping with what's there).
fn can_move(inv: &Inventory, from: Place, to: Place) -> bool {
    let mut trial = *inv;
    trial.move_item(from, to)
}

/// The inventory: weapon slots, pack, ammo and cargo, with the chosen
/// item's details beside them; then the stances.
fn inventory_tab(
    ui: &mut egui::Ui,
    layout: &mut StanceLayout,
    player: Option<&Player>,
    bindings: &Bindings,
    status: &mut String,
    chosen: &mut Option<Place>,
    ops: &mut ItemOps,
) {
    let fallback = Loadout::base();
    let loadout = player.map_or(&fallback, |p| &p.loadout);
    let inv = &loadout.inventory;
    if chosen.is_some_and(|p| inv.get(p).is_none()) {
        *chosen = None;
    }
    let target = |chosen: &Option<Place>, to: Place| chosen.is_some_and(|from| from != to && can_move(inv, from, to));

    let size = egui::vec2(ui.available_width(), ITEMS_HEIGHT);
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Min), |ui| {
        ui.set_min_height(ITEMS_HEIGHT);
        // Left: what's carried.
        ui.vertical(|ui| {
            ui.set_width(3.0 * CELL.x + 2.0 * GAP);
            ui.label(egui::RichText::new("Weapon slots").strong());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(GAP, 6.0);
                for i in 0..WEAPON_SLOTS as u8 {
                    let place = Place::Weapon(i);
                    let in_hand = Hand::BOTH.iter().any(|h| loadout.weapon_slot(*h) == Some(i as usize));
                    let tag = if in_hand { format!("SLOT {} · IN HAND", i + 1) } else { format!("SLOT {}", i + 1) };
                    let r = cell(ui, CELL, &tag, inv.get(place), *chosen == Some(place), target(chosen, place));
                    click(inv, place, &r, ops, chosen);
                }
            });
            ui.add_space(4.0);
            ui.label(egui::RichText::new("Pack").strong());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(GAP, 6.0);
                for i in 0..PACK_SLOTS as u8 {
                    let place = Place::Pack(i);
                    let r = cell(ui, CELL, "PACK", inv.get(place), *chosen == Some(place), target(chosen, place));
                    click(inv, place, &r, ops, chosen);
                }
            });
            ui.add_space(4.0);
            egui::Grid::new("ammo").num_columns(3).spacing([12.0, 4.0]).show(ui, |ui| {
                for kind in AmmoKind::ALL {
                    let rounds = inv.ammo(kind);
                    ui.label(egui::RichText::new(kind.name()).color(items_hud::SUPPLY));
                    let bar = egui::ProgressBar::new(rounds as f32 / kind.cap() as f32)
                        .desired_width(170.0)
                        .text(egui::RichText::new(format!("{rounds} / {}", kind.cap())).monospace().small());
                    ui.add(bar);
                    let label = if rounds > 0 { format!("Drop {}", rounds.min(kind.box_rounds())) } else { "Drop".into() };
                    let drop = egui::Button::new(egui::RichText::new(label).small());
                    if ui
                        .add_enabled(rounds > 0, drop)
                        .on_hover_text("Leave a box of it at your feet, for a crewmate")
                        .clicked()
                    {
                        ask(ops, ItemOp::DropAmmo(kind));
                    }
                    ui.end_row();
                }
                ui.label(egui::RichText::new("Hands").color(items_hud::CARGO));
                match inv.carrying {
                    Some(cargo) => {
                        ui.label(egui::RichText::new(format!("{} (no guns, no grapple)", cargo.name())).color(INK));
                        if ui.button(egui::RichText::new(format!("Put down [{}]", bindings.label(Action::DropItem))).small()).clicked() {
                            ask(ops, ItemOp::Drop(Place::Hands));
                        }
                    }
                    None => {
                        ui.label(egui::RichText::new("free").color(DIM));
                        ui.label("");
                    }
                }
                ui.end_row();
            });
        });
        ui.separator();
        // Right: the chosen item.
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            let item = chosen.and_then(|p| inv.get(p));
            details(ui, player, chosen, item, ops);
        });
    });
    ui.label(
        egui::RichText::new(format!(
            "Click an item, then a slot to move it there (or swap). Right-click drops it; {} drops what's in hand.",
            bindings.label(Action::DropItem)
        ))
        .small()
        .color(DIM),
    );
    ui.separator();

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Stances").strong());
        ui.label(
            egui::RichText::new("A gun with the other hand empty is braced: it shoots tighter. Without the grapple in hand, a rope that's out stays out.")
                .small()
                .color(DIM),
        );
    });
    let mut options = vec![Slot::Empty, Slot::Grapple];
    options.extend((0..WEAPON_SLOTS as u8).map(Slot::Gun));
    let before = layout.0;
    egui::Grid::new("stances").num_columns(3).spacing([16.0, 3.0]).show(ui, |ui| {
        ui.label("");
        ui.label(egui::RichText::new(format!("Main hand  [{}]", bindings.label(Action::MainUse))).color(DIM));
        ui.label(egui::RichText::new(format!("Off hand  [{}]", bindings.label(Action::OffUse))).color(DIM));
        ui.end_row();
        let keys = [Action::Stance1, Action::Stance2, Action::Stance3, Action::Stance4];
        for (i, stance) in layout.0.iter_mut().enumerate() {
            let current = i == loadout.current as usize;
            let key = bindings.label(keys[i]);
            let label = if key == (i + 1).to_string() {
                key
            } else {
                format!("{}  [{key}]", i + 1)
            };
            ui.label(if current {
                egui::RichText::new(label).strong().color(INK)
            } else {
                egui::RichText::new(label).color(DIM)
            });
            for hand in Hand::BOTH {
                let held = stance.slot(hand);
                egui::ComboBox::from_id_salt(("stance", i, hand as u8))
                    .width(260.0)
                    .selected_text(slot_name(held, inv))
                    .show_ui(ui, |ui| {
                        for option in &options {
                            let carried = match option {
                                Slot::Gun(g) => inv.weapons[*g as usize].is_some(),
                                _ => true,
                            };
                            let text = egui::RichText::new(slot_name(*option, inv));
                            let text = if carried { text } else { text.color(DIM) };
                            if ui.selectable_label(held == *option, text).clicked() {
                                put(stance, hand, *option);
                            }
                        }
                    });
            }
            ui.end_row();
        }
    });
    if ui.button("Default stances").clicked() {
        layout.0 = DEFAULT_STANCES;
    }
    if layout.0 != before {
        *status = save_stances(layout);
    }
}

/// The chosen item: what it is, its stats against what you'd compare it
/// with, and what can be done with it.
fn details(ui: &mut egui::Ui, player: Option<&Player>, chosen: &mut Option<Place>, item: Option<Item>, ops: &mut ItemOps) {
    let (Some(player), Some(place), Some(item)) = (player, *chosen, item) else {
        ui.label(egui::RichText::new("Nothing chosen").color(DIM));
        ui.label(
            egui::RichText::new(
                "Choose an item to see what it is and how it compares with your guns. Loot comes from \
                 robots and from crates aboard enemy ships: better the deeper in they are.",
            )
            .small()
            .color(DIM),
        );
        return;
    };
    let inv = &player.loadout.inventory;
    ui.label(egui::RichText::new(item.name()).size(17.0).strong().color(items_hud::item_color(&item)));
    match item {
        Item::Weapon(weapon) => {
            for (line, color) in items_hud::weapon_lines(&weapon) {
                ui.label(egui::RichText::new(line).small().color(color));
            }
            ui.label(egui::RichText::new(weapon.build().maker.blurb()).small().italics().color(DIM));
            let stats = weapon.stats();
            ui.label(
                egui::RichText::new(format!(
                    "{} / {} in the mag · {} ({} carried)",
                    weapon.mag,
                    stats.mag,
                    stats.ammo.name(),
                    inv.ammo(stats.ammo)
                ))
                .small()
                .color(INK),
            );
            let theirs = items_hud::compared_with(player, &weapon);
            ui.label(
                egui::RichText::new(match theirs {
                    Some(t) => format!("Against {}", t.name()),
                    None => "Stats".into(),
                })
                .small()
                .color(DIM),
            );
            items_hud::stats_table(ui, "chosen stats", &stats, theirs.map(|w| w.stats()).as_ref());
        }
        Item::Cargo(cargo) => {
            ui.label(
                egui::RichText::new(if cargo.bulky() {
                    "Bulky: carried in both hands. Whoever carries it can't shoot or grapple."
                } else {
                    "Small: rides in the pack."
                })
                .small()
                .color(DIM),
            );
        }
        Item::Ammo { .. } => {}
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if let Item::Weapon(_) = item {
            for i in 0..WEAPON_SLOTS as u8 {
                let to = Place::Weapon(i);
                if to != place && ui.small_button(format!("To slot {}", i + 1)).clicked() {
                    ask(ops, ItemOp::Move { from: place, to });
                    *chosen = Some(to);
                }
            }
            let pack = (0..PACK_SLOTS as u8).map(Place::Pack).find(|p| inv.get(*p).is_none());
            if let (Place::Weapon(_), Some(to)) = (place, pack)
                && ui.small_button("To pack").clicked()
            {
                ask(ops, ItemOp::Move { from: place, to });
                *chosen = Some(to);
            }
        }
        if ui.small_button("Drop").clicked() {
            ask(ops, ItemOp::Drop(place));
            *chosen = None;
        }
    });
}

fn controls_tab(ui: &mut egui::Ui, menu: &mut Menu, bindings: &mut Bindings, height: f32) {
    ui.label(
        egui::RichText::new(
            "Click a binding, then press the key, button or chord (e.g. Ctrl+LMB). \
             Escape cancels; right-click clears. Modifiers: Shift, Ctrl, Alt, Tab, CapsLock.",
        )
        .small()
        .color(DIM),
    );
    ui.add_space(4.0);
    let mut changed = false;
    egui::ScrollArea::vertical()
        .min_scrolled_height(height)
        .max_height(height)
        .auto_shrink([false, true])
        .show(ui, |ui| {
        let mut group = "";
        for action in Action::ALL {
            if action.group() != group {
                if !group.is_empty() {
                    ui.add_space(6.0);
                }
                group = action.group();
                ui.label(egui::RichText::new(group).strong());
            }
            ui.horizontal(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(220.0, 20.0),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_width(220.0);
                        ui.label(action.name());
                    },
                );
                for slot in 0..SLOTS {
                    let binding = bindings.get(action).get(slot).cloned();
                    let waiting = menu.capture.as_ref().is_some_and(|c| c.action == action && c.slot == slot);
                    let sharing = binding.as_ref().map(|b| bindings.sharing(action, b)).unwrap_or_default();
                    let text = match (&binding, waiting) {
                        (_, true) => egui::RichText::new("press…").color(AMBER),
                        (Some(b), _) if !sharing.is_empty() => egui::RichText::new(b.to_string()).color(AMBER),
                        (Some(b), _) => egui::RichText::new(b.to_string()).color(INK),
                        (None, _) => egui::RichText::new("—").color(DIM),
                    };
                    let mut button = ui.add_sized([120.0, 20.0], egui::Button::new(text.monospace()));
                    if !sharing.is_empty() {
                        let names: Vec<&str> = sharing.iter().map(|a| a.name()).collect();
                        button = button.on_hover_text(format!("Also: {}", names.join(", ")));
                    }
                    if menu.swallow_click {
                        continue;
                    }
                    if button.clicked() {
                        menu.capture = Some(Capture { action, slot });
                        menu.status = format!("{}: press a key", action.name());
                    }
                    if button.secondary_clicked() && binding.is_some() {
                        bindings.set(action, slot, None);
                        changed = true;
                    }
                }
                let defaults = action.default_bindings();
                if bindings.get(action) != defaults.as_slice()
                    && ui.small_button("default").on_hover_text(defaults.iter().map(Binding::to_string).collect::<Vec<_>>().join(", ")).clicked()
                {
                    for slot in (0..SLOTS).rev() {
                        bindings.set(action, slot, None);
                    }
                    for (slot, b) in defaults.into_iter().enumerate() {
                        bindings.set(action, slot, Some(b));
                    }
                    changed = true;
                }
            });
        }
    });
    ui.add_space(6.0);
    if ui.button("All default controls").clicked() {
        *bindings = Bindings::default();
        changed = true;
    }
    if changed {
        menu.status = save_bindings(bindings);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn putting_what_the_other_hand_holds_swaps_hands() {
        let mut stance = Stance { main: Slot::Gun(0), off: Slot::Grapple };
        put(&mut stance, Hand::Main, Slot::Grapple);
        assert_eq!(stance, Stance { main: Slot::Grapple, off: Slot::Gun(0) });
        put(&mut stance, Hand::Off, Slot::Gun(1));
        assert_eq!(stance, Stance { main: Slot::Grapple, off: Slot::Gun(1) });
        put(&mut stance, Hand::Off, Slot::Empty);
        put(&mut stance, Hand::Main, Slot::Empty);
        assert_eq!(stance, Stance { main: Slot::Empty, off: Slot::Empty });
    }

    #[test]
    fn saved_stances_read_back() {
        let text = ron::ser::to_string_pretty(&DEFAULT_STANCES, ron::ser::PrettyConfig::default()).unwrap();
        let again: [Stance; STANCES] = ron::from_str(&text).unwrap();
        assert_eq!(again, DEFAULT_STANCES);
    }
}
