//! The in-game menu: what your hands hold in each stance, and the controls.
//!
//! The game goes on while it's open (it's co-op; there's no pausing), but
//! the cursor is free and your input is let go. Escape (or the Menu
//! binding) opens and closes it; the Loadout binding opens it on the
//! loadout.
//!
//! The loadout is yours: what each stance puts in each hand is kept in
//! `stances.ron` and given to the simulation with your input, so it holds
//! through respawns and across games. Controls are kept in `keybinds.ron`
//! (see [`super::bindings`]). Both are saved as soon as they change.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::LocalPlayer;
use super::bindings::{Action, Binding, Bindings, Button, KEYBINDS_PATH, Modifier, SLOTS};
use crate::sim::player::Player;
use crate::sim::weapons::{DEFAULT_STANCES, Gun, Hand, Loadout, STANCES, Slot, Stance};

pub const STANCES_PATH: &str = "stances.ron";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Loadout,
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
}

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

const INK: egui::Color32 = egui::Color32::from_rgb(200, 225, 235);
const DIM: egui::Color32 = egui::Color32::from_rgb(120, 135, 145);
const AMBER: egui::Color32 = egui::Color32::from_rgb(255, 190, 70);

pub fn menu(
    mut contexts: EguiContexts,
    mut menu: ResMut<Menu>,
    mut bindings: ResMut<Bindings>,
    mut layout: ResMut<StanceLayout>,
    player: Option<Single<&Player, With<LocalPlayer>>>,
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
    let menu = &mut *menu;
    egui::Window::new("Hullbreakers")
        // Pinned at the top, so nothing moves when what's in it grows.
        .anchor(egui::Align2::CENTER_TOP, [0.0, 48.0])
        .collapsible(false)
        .resizable(false)
        .fixed_size([600.0, 0.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut menu.tab, Tab::Loadout, egui::RichText::new("Loadout").size(16.0));
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
                Tab::Loadout => loadout_tab(ui, &mut layout, player.as_deref().map(|p| &p.loadout), &bindings, &mut menu.status),
                Tab::Controls => controls_tab(ui, menu, &mut bindings),
            }
            if !menu.status.is_empty() {
                ui.separator();
                ui.label(egui::RichText::new(&menu.status).small().color(DIM));
            }
        });
    Ok(())
}

/// A slot's name, given what's carried.
pub fn slot_name(slot: Slot, guns: &[Gun]) -> &'static str {
    match slot {
        Slot::Empty => "Empty",
        Slot::Grapple => "Grapple",
        Slot::Gun(i) => guns.get(i as usize).map_or("Empty", |g| g.kind.stats().name),
    }
}

fn loadout_tab(
    ui: &mut egui::Ui,
    layout: &mut StanceLayout,
    loadout: Option<&Loadout>,
    bindings: &Bindings,
    status: &mut String,
) {
    // The gun slots every player has (whether or not they carry the gun).
    let fallback = Loadout::default();
    let loadout = loadout.unwrap_or(&fallback);
    let guns = &loadout.guns;

    ui.label(egui::RichText::new("Carried").strong());
    egui::Grid::new("carried").num_columns(3).spacing([24.0, 4.0]).show(ui, |ui| {
        ui.label(egui::RichText::new("Grapple").color(INK));
        ui.label(egui::RichText::new("suit system").color(DIM));
        ui.label("");
        ui.end_row();
        for gun in guns {
            let stats = gun.kind.stats();
            if gun.carried {
                ui.label(egui::RichText::new(stats.name).color(INK));
                ui.label(egui::RichText::new(format!("{} / {} in mag, {} spare", gun.mag, stats.mag, gun.reserve)).monospace());
                ui.label(egui::RichText::new(format!("{} dmg · {}/s", stats.damage, stats.rate)).color(DIM));
            } else {
                ui.label(egui::RichText::new(stats.name).color(DIM));
                ui.label(egui::RichText::new("not carried — take one from the weapons locker").color(AMBER));
                ui.label("");
            }
            ui.end_row();
        }
    });
    ui.add_space(10.0);

    ui.label(egui::RichText::new("Stances").strong());
    ui.label(
        egui::RichText::new(
            "What each hand holds in each stance. A gun with the other hand empty is braced: \
             it shoots tighter. Without the grapple in hand, a rope that's out stays out.",
        )
        .small()
        .color(DIM),
    );
    ui.add_space(4.0);
    let mut options = vec![Slot::Empty, Slot::Grapple];
    options.extend((0..guns.len() as u8).map(Slot::Gun));
    let before = layout.0;
    egui::Grid::new("stances").num_columns(3).spacing([16.0, 6.0]).show(ui, |ui| {
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
                    .width(150.0)
                    .selected_text(slot_name(held, guns))
                    .show_ui(ui, |ui| {
                        for option in &options {
                            let carried = match option {
                                Slot::Gun(g) => guns[*g as usize].carried,
                                _ => true,
                            };
                            let text = egui::RichText::new(slot_name(*option, guns));
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
    ui.add_space(6.0);
    if ui.button("Default stances").clicked() {
        layout.0 = DEFAULT_STANCES;
    }
    if layout.0 != before {
        *status = save_stances(layout);
    }
}

fn controls_tab(ui: &mut egui::Ui, menu: &mut Menu, bindings: &mut Bindings) {
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
    egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, false]).show(ui, |ui| {
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
