//! The armory: the menu's tab at the weapons locker, for moving guns
//! between your stash and what you carry. Opened by pressing the locker
//! (which also fills your ammo; see `Loadout::restock`), and shown in the
//! menu while you're aboard our ship.
//!
//! What it asks goes to the host as profile requests (see
//! `sim::progression`); what it shows is the host's copy of your profile.

use bevy::prelude::*;
use bevy_egui::egui;

use super::events::Presented;
use super::items_hud::{self, DIM, INK};
use super::menu::{CELL, GAP, Menu, Tab, cell};
use crate::net::ProfileRequests;
use crate::sim::events::PlayerEvent;
use crate::sim::items::{Item, Place, Weapon};
use crate::sim::player::Player;
use crate::sim::progression::{Holding, Profile, ProfileOp, STASH_CAP};
use crate::sim::ship::ControlAction;
use crate::sim::weapons::GunKind;

/// Pressing the weapons locker opens the armory (and the workbench its tab).
pub fn open_on_press(mut presented: MessageReader<Presented>, mut menu: ResMut<Menu>) {
    for Presented(event) in presented.read() {
        if let PlayerEvent::Pressed { action, .. } = event {
            match action {
                ControlAction::Armory => menu.open_on(Tab::Armory),
                ControlAction::Workbench => menu.open_on(Tab::Workbench),
                _ => {}
            }
        }
    }
}

/// The stash, three cards to a row, with what's chosen lit.
pub fn stash_grid(ui: &mut egui::Ui, profile: &Profile, chosen: &mut Option<Holding>, height: f32) {
    ui.label(egui::RichText::new(format!("Stash  {} / {STASH_CAP}", profile.stash.len())).strong());
    egui::ScrollArea::vertical()
        .id_salt("stash")
        .max_height(height)
        .min_scrolled_height(height)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(GAP, 6.0);
                if profile.stash.is_empty() {
                    ui.label(egui::RichText::new("Empty. What you find is yours: store it here.").small().color(DIM));
                }
                for (i, w) in profile.stash.iter().enumerate() {
                    let here = Holding::Stash(i as u16);
                    let r = cell(ui, CELL, "STASH", Some(Item::Weapon(*w)), *chosen == Some(here), false);
                    if r.clicked() {
                        *chosen = if *chosen == Some(here) { None } else { Some(here) };
                    }
                }
            });
        });
}

/// What you carry, as cards: weapon slots, then the pack.
pub fn carried_grid(ui: &mut egui::Ui, player: &Player, chosen: &mut Option<Holding>) {
    let inv = &player.loadout.inventory;
    ui.label(egui::RichText::new("Carried").strong());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(GAP, 6.0);
        let places = (0..inv.weapon_slots()).map(|i| (Place::Weapon(i as u8), format!("SLOT {}", i + 1)));
        let pack = (0..inv.pack_slots()).map(|i| (Place::Pack(i as u8), "PACK".to_string()));
        for (place, tag) in places.chain(pack) {
            let here = Holding::Carried(place);
            let r = cell(ui, CELL, &tag, inv.get(place), *chosen == Some(here), false);
            if r.clicked() && inv.get(place).is_some() {
                *chosen = if *chosen == Some(here) { None } else { Some(here) };
            }
        }
    });
}

/// A chosen gun's details, against what it's compared with.
pub fn weapon_details(ui: &mut egui::Ui, player: &Player, weapon: &Weapon) {
    ui.label(egui::RichText::new(weapon.name()).size(17.0).strong().color(items_hud::rarity_color(weapon.rarity)));
    for (line, color) in items_hud::weapon_lines(weapon) {
        ui.label(egui::RichText::new(line).small().color(color));
    }
    let theirs = items_hud::compared_with(player, weapon);
    ui.label(
        egui::RichText::new(match theirs {
            Some(t) => format!("Against {}", t.name()),
            None => "Stats".into(),
        })
        .small()
        .color(DIM),
    );
    items_hud::stats_table(ui, "armory stats", &weapon.stats(), theirs.map(|w| w.stats()).as_ref());
}

pub fn armory_tab(
    ui: &mut egui::Ui,
    player: Option<&Player>,
    profile: Option<&Profile>,
    chosen: &mut Option<Holding>,
    requests: Option<&mut ProfileRequests>,
) {
    let (Some(player), Some(profile), Some(requests)) = (player, profile, requests) else {
        ui.label(egui::RichText::new("Your profile hasn't reached the host yet.").color(DIM));
        return;
    };
    if chosen.is_some_and(|c| c.weapon(&player.loadout.inventory, profile).is_none()) {
        *chosen = None;
    }
    let mut ask = |op| requests.0.push(op);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Standard issue, free:").color(DIM));
        for frame in GunKind::CREW {
            if ui.button(format!("Take an issue {}", frame.name())).clicked() {
                ask(ProfileOp::TakeIssue(frame));
            }
        }
    });
    ui.separator();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(3.0 * CELL.x + 2.0 * GAP);
            carried_grid(ui, player, chosen);
            ui.add_space(6.0);
            stash_grid(ui, profile, chosen, 230.0);
        });
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            let Some(c) = *chosen else {
                ui.label(egui::RichText::new("Nothing chosen").color(DIM));
                ui.label(
                    egui::RichText::new(
                        "Every gun you pick up is yours. Choose one you carry to store it, or one in \
                         your stash to take it. What you leave where you die is back here when the \
                         mission's over.",
                    )
                    .small()
                    .color(DIM),
                );
                return;
            };
            let Some(weapon) = c.weapon(&player.loadout.inventory, profile) else { return };
            weapon_details(ui, player, &weapon);
            ui.add_space(4.0);
            match c {
                Holding::Carried(place) => {
                    let issue = weapon.rarity == crate::sim::items::Rarity::Standard;
                    let full = profile.stash.len() >= STASH_CAP && !issue;
                    let label = if issue { "Put back on the rack" } else { "Store in the stash" };
                    if ui.add_enabled(!full, egui::Button::new(label)).clicked() {
                        ask(ProfileOp::Store(place));
                        *chosen = None;
                    }
                    if full {
                        ui.label(egui::RichText::new("The stash is full: scrap something at the workbench.").small().color(DIM));
                    }
                }
                Holding::Stash(i) => {
                    if ui.button("Take").clicked() {
                        ask(ProfileOp::Take(i));
                        *chosen = None;
                    }
                }
            }
            ui.label(egui::RichText::new(format!("{} salvage", profile.salvage)).color(INK));
        });
    });
}
