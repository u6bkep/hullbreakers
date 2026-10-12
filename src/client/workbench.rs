//! The workbench: the menu's tab at the bench beside the weapons locker,
//! for spending salvage on guns (tune a tier up, refit a part, repaint)
//! and breaking them down for salvage and their parts. Shown while you're
//! aboard our ship; pressing the bench opens it.
//!
//! As at the armory, what it asks goes to the host (see
//! `sim::progression`), and what it shows is the host's copy.

use bevy_egui::egui;

use super::armory::{carried_grid, stash_grid, weapon_details};
use super::items_hud::{self, AMBER, DIM, INK};
use super::menu::{CELL, GAP, heading, title};
use crate::net::ProfileRequests;
use crate::sim::items::{PAINTS, Rarity, SLOTS, Weapon};
use crate::sim::player::Player;
use crate::sim::progression::{self, Holding, Profile, ProfileOp, REFIT_COST, REPAINT_COST, rank, scrap_value, tune_cost};

pub fn workbench_tab(
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
    let inv = &player.loadout.inventory;
    if chosen.is_some_and(|c| c.weapon(inv, profile).is_none()) {
        *chosen = None;
    }
    progress_strip(ui, profile);
    ui.separator();
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            // Three cards to a row, and room for a scroll bar.
            ui.set_width(3.0 * CELL.x + 2.0 * GAP + 14.0);
            carried_grid(ui, player, chosen);
            ui.add_space(6.0);
            stash_grid(ui, profile, chosen, 230.0);
        });
        ui.vertical(|ui| {
            ui.set_width(ui.available_width());
            let Some(h) = *chosen else {
                ui.label(egui::RichText::new("Nothing chosen").color(DIM));
                ui.label(
                    egui::RichText::new(
                        "Choose a gun. Tuning raises its rarity a tier (more quality, and perks from \
                         rare up) without changing its parts. Refitting swaps one part for one you've \
                         unlocked, by scrapping a gun that had it.",
                    )
                    .small()
                    .color(DIM),
                );
                return;
            };
            let Some(weapon) = h.weapon(inv, profile) else { return };
            egui::ScrollArea::vertical().id_salt("bench").max_height(440.0).show(ui, |ui| {
                bench(ui, player, profile, h, &weapon, &mut |op| requests.0.push(op), chosen);
            });
        });
    });
}

/// Rank, experience to the next, salvage, and what's unlocked.
pub fn progress_strip(ui: &mut egui::Ui, profile: &Profile) {
    use super::hud::style;
    let r = rank(profile.xp);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("RANK {r}")).font(style::bold(style::BODY)).color(style::SUIT));
        if r < progression::MAX_RANK {
            let (from, to) = (progression::xp_for(r), progression::xp_for(r + 1));
            let bar = egui::ProgressBar::new((profile.xp - from) as f32 / (to - from) as f32)
                .desired_width(160.0)
                .text(egui::RichText::new(format!("{} / {} xp", profile.xp - from, to - from)).small());
            ui.add(bar);
        }
        ui.label(egui::RichText::new(format!("{}", profile.salvage)).font(style::num(style::BODY)).color(style::INK));
        ui.label(egui::RichText::new("SALVAGE").font(style::ui(style::MICRO)).color(style::INK_DIM));
        let parts = profile.parts.count_ones();
        ui.label(egui::RichText::new(format!("{parts} parts unlocked")).small().color(DIM));
        if let Some((at, what)) = progression::LADDER.iter().find(|(at, _)| *at > r) {
            ui.label(egui::RichText::new(format!("Rank {at}: {what}")).small().color(DIM));
        }
    });
}

fn bench(
    ui: &mut egui::Ui,
    player: &Player,
    profile: &Profile,
    h: Holding,
    weapon: &Weapon,
    ask: &mut dyn FnMut(ProfileOp),
    chosen: &mut Option<Holding>,
) {
    if weapon.rarity == Rarity::Standard {
        weapon_details(ui, player, weapon);
        ui.label(egui::RichText::new("Standard issue: nothing to do to it here.").small().color(DIM));
        return;
    }
    ui.label(title(weapon.name(), items_hud::rarity_color(weapon.rarity)));
    for (line, color) in items_hud::weapon_lines(weapon) {
        ui.label(egui::RichText::new(line).small().color(color));
    }
    ui.add_space(4.0);

    // Tune.
    match weapon.rarity.next() {
        Some(next) => {
            let cost = tune_cost(next);
            let label = format!("Tune to {} ({cost} salvage)", next.name());
            if ui.add_enabled(profile.salvage >= cost, egui::Button::new(label)).clicked() {
                ask(ProfileOp::Tune(h));
            }
            let tuned = Weapon { rarity: next, tune: weapon.tune + 1, ..*weapon };
            ui.label(egui::RichText::new(format!("As {}: against now", next.name())).small().color(DIM));
            items_hud::stats_table(ui, "tuned", &tuned.stats(), Some(&weapon.stats()));
        }
        None => {
            ui.label(egui::RichText::new("Exotic: as good as tuning gets.").small().color(DIM));
            items_hud::stats_table(ui, "bench stats", &weapon.stats(), None);
        }
    }
    ui.add_space(4.0);

    // Refit.
    ui.label(heading(&format!("Refit a part: {REFIT_COST} salvage")));
    let fitted = weapon.build().parts.slots();
    egui::Grid::new("refits").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| for (slot, options) in SLOTS.iter().enumerate() {
        {
            ui.label(egui::RichText::new(fitted[slot].slot()).small().color(DIM));
            egui::ComboBox::from_id_salt(("refit", slot))
                .width(150.0)
                .selected_text(fitted[slot].name())
                .show_ui(ui, |ui| {
                    if weapon.refit.parts[slot] != 0 && ui.selectable_label(false, "As it was made").clicked() {
                        ask(ProfileOp::Refit(h, slot as u8, None));
                    }
                    for (v, part) in options.iter().enumerate() {
                        let have = profile.has_part(slot, v);
                        let text = if have {
                            egui::RichText::new(part.name())
                        } else {
                            egui::RichText::new(format!("{} (scrap a gun with one)", part.name())).color(DIM)
                        };
                        let r = ui.add_enabled(have && profile.salvage >= REFIT_COST, egui::Button::selectable(fitted[slot] == *part, text));
                        if r.clicked() && fitted[slot] != *part {
                            ask(ProfileOp::Refit(h, slot as u8, Some(v as u8)));
                        }
                    }
                });
        }
        ui.end_row();
    });
    ui.add_space(4.0);

    // Repaint.
    let r = rank(profile.xp);
    ui.horizontal(|ui| {
        ui.label(heading(&format!("Paint: {REPAINT_COST} salvage")));
        let current = weapon.refit.paint.checked_sub(1).and_then(|i| PAINTS.get(i as usize)).map_or("Maker's own", |p| p.0);
        egui::ComboBox::from_id_salt("paint").width(140.0).selected_text(current).show_ui(ui, |ui| {
            if weapon.refit.paint != 0 && ui.selectable_label(false, "Maker's own").clicked() {
                ask(ProfileOp::Repaint(h, None));
            }
            for (i, (name, body, _, needs)) in PAINTS.iter().enumerate() {
                let open = r >= *needs && profile.salvage >= REPAINT_COST;
                let [cr, cg, cb] = *body;
                let text = if r >= *needs {
                    egui::RichText::new(format!("■ {name}")).color(egui::Color32::from_rgb(cr, cg, cb))
                } else {
                    egui::RichText::new(format!("{name} (rank {needs})")).color(DIM)
                };
                if ui.add_enabled(open, egui::Button::selectable(weapon.refit.paint == i as u8 + 1, text)).clicked() {
                    ask(ProfileOp::Repaint(h, Some(i as u8)));
                }
            }
        });
    });
    ui.add_space(4.0);

    // Scrap: twice, to be sure.
    let value = scrap_value(weapon);
    let unlocks: Vec<&str> = fitted
        .iter()
        .enumerate()
        .filter(|(slot, part)| {
            let (_, v) = part.place();
            v != 0 && !profile.has_part(*slot, v)
        })
        .map(|(_, part)| part.name())
        .collect();
    let sure = ui.id().with("scrap");
    let armed = ui.data(|d| d.get_temp::<Holding>(sure)) == Some(h);
    let label = if armed {
        format!("Really scrap it? (+{value} salvage)")
    } else {
        format!("Scrap for {value} salvage")
    };
    if ui.button(egui::RichText::new(label).color(if armed { AMBER } else { INK })).clicked() {
        if armed {
            ask(ProfileOp::Scrap(h));
            *chosen = None;
            ui.data_mut(|d| d.remove::<Holding>(sure));
        } else {
            ui.data_mut(|d| d.insert_temp(sure, h));
        }
    }
    if !unlocks.is_empty() {
        ui.label(egui::RichText::new(format!("Unlocks: {}", unlocks.join(", "))).small().color(DIM));
    }
}
