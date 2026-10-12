//! Items on screen: rarity colours, weapon stat tables (shared with the
//! inventory screen), and the card under the crosshair that says what's in
//! the pickup you're looking at and how it compares with what you hold.
//!
//! The look is kept to a few named colours and sizes here, so a HUD style
//! can take it over in one place.

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::LocalPlayer;
use super::bindings::{Action, Bindings};
use super::camera::PlayerCamera;
use crate::sim::bodies::Bodies;
use crate::sim::items::{Item, Rarity, Weapon};
use crate::sim::pickups::Pickup;
use crate::sim::player::{CONTROL_REACH, Player, ray_box};
use crate::sim::ship::Control;
use crate::sim::weapons::{GunStats, Hand};
use crate::sim::Layer;

pub const INK: egui::Color32 = egui::Color32::from_rgb(200, 225, 235);
pub const DIM: egui::Color32 = egui::Color32::from_rgb(120, 135, 145);
pub const AMBER: egui::Color32 = egui::Color32::from_rgb(255, 190, 70);
pub const BETTER: egui::Color32 = egui::Color32::from_rgb(110, 220, 120);
pub const WORSE: egui::Color32 = egui::Color32::from_rgb(255, 110, 90);
/// Supplies (ammo) and cargo, which have no rarity.
pub const SUPPLY: egui::Color32 = egui::Color32::from_rgb(90, 220, 150);
pub const CARGO: egui::Color32 = egui::Color32::from_rgb(110, 200, 255);

pub fn rarity_color(rarity: Rarity) -> egui::Color32 {
    let [r, g, b] = rarity.color();
    egui::Color32::from_rgb(r, g, b)
}

pub fn item_color(item: &Item) -> egui::Color32 {
    match item {
        Item::Weapon(w) => rarity_color(w.rarity),
        Item::Ammo { .. } => SUPPLY,
        Item::Cargo(_) => CARGO,
    }
}

/// One line of a stat table.
pub struct StatLine {
    pub label: &'static str,
    pub value: f32,
    /// Whether more is better.
    pub more_is_better: bool,
    pub decimals: usize,
    pub unit: &'static str,
}

/// A gun's stats as the tables show them.
pub fn stat_lines(s: &GunStats) -> Vec<StatLine> {
    let line = |label, value, more_is_better, decimals, unit| StatLine {
        label,
        value,
        more_is_better,
        decimals,
        unit,
    };
    vec![
        line("Damage", s.damage, true, 1, ""),
        line("Rate", s.rate, true, 1, "/s"),
        line("Damage/s", s.damage * s.rate, true, 0, ""),
        line("Muzzle speed", s.muzzle_speed, true, 0, " m/s"),
        line("Cone, braced", s.spread_braced, false, 2, "°"),
        line("Cone, unbraced", s.spread, false, 2, "°"),
        line("Magazine", s.mag as f32, true, 0, ""),
        line("Reload", s.reload, false, 2, " s"),
        line("Switch", s.switch, false, 2, " s"),
    ]
}

/// A stats table: each stat, and how it differs from `against`'s.
pub fn stats_table(ui: &mut egui::Ui, id: &str, stats: &GunStats, against: Option<&GunStats>) {
    let theirs = against.map(stat_lines);
    egui::Grid::new(id).num_columns(3).spacing([12.0, 1.0]).show(ui, |ui| {
        for (i, line) in stat_lines(stats).iter().enumerate() {
            ui.label(egui::RichText::new(line.label).color(DIM).small());
            ui.label(egui::RichText::new(format!("{:.*}{}", line.decimals, line.value, line.unit)).monospace().color(INK));
            match theirs.as_ref().map(|t| line.value - t[i].value) {
                Some(d) if d.abs() > 0.5 * 10f32.powi(-(line.decimals as i32)) => {
                    let good = (d > 0.0) == line.more_is_better;
                    ui.label(
                        egui::RichText::new(format!("{:+.*}", line.decimals, d))
                            .monospace()
                            .small()
                            .color(if good { BETTER } else { WORSE }),
                    );
                }
                _ => {
                    ui.label("");
                }
            }
            ui.end_row();
        }
    });
}

/// What a weapon is: its frame, maker, rarity, parts that aren't stock,
/// perks.
pub fn weapon_lines(weapon: &Weapon) -> Vec<(String, egui::Color32)> {
    let build = weapon.build();
    let mut lines = vec![(
        format!("{} {} · {}", weapon.rarity.name(), weapon.frame.name(), build.maker.name()),
        rarity_color(weapon.rarity),
    )];
    for part in build.parts.all().into_iter().filter(|p| !p.standard()) {
        lines.push((format!("{}: {}", part.name(), part.effect()), INK));
    }
    for perk in build.perks.iter().flatten() {
        lines.push((format!("★ {}: {}", perk.name(), perk.effect()), rarity_color(weapon.rarity)));
    }
    lines
}

/// The gun a new one is compared with: the one of its frame in the weapon
/// slots, or else the one in the main hand.
pub fn compared_with<'a>(player: &'a Player, weapon: &Weapon) -> Option<&'a Weapon> {
    let inv = &player.loadout.inventory;
    inv.weapons
        .iter()
        .flatten()
        .find(|w| w.frame == weapon.frame && !w.same_gun(weapon))
        .or_else(|| player.loadout.weapon(Hand::Main).filter(|w| !w.same_gun(weapon)))
}

/// The card by the crosshair for the pickup it's on: what's inside, in
/// rarity colours; for a gun, how it compares with yours; and whether
/// there's room for it. (The prompt to take it is the HUD's.)
#[allow(clippy::too_many_arguments)]
pub fn pickup_card(
    mut contexts: EguiContexts,
    bindings: Res<Bindings>,
    player: Option<Single<(Entity, &Player), With<LocalPlayer>>>,
    camera: Single<&Transform, With<PlayerCamera>>,
    spatial: SpatialQuery,
    bodies: Bodies,
    controls: Query<(&Control, &Transform, &ChildOf)>,
    pickups: Query<(&Pickup, &Transform)>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Some(player) = player else { return Ok(()) };
    let (own, player) = *player;
    let Ok(dir) = Dir3::new(camera.rotation * Vec3::NEG_Z) else { return Ok(()) };
    let eye = camera.translation;
    // The sim's rule (see `player::pickup_under_crosshair`): within reach,
    // nothing in front, and no button nearer.
    let blocked = spatial
        .cast_ray(eye, dir, CONTROL_REACH, true, &SpatialQueryFilter::from_mask(Layer::World).with_excluded_entities([own]))
        .map_or(f32::MAX, |hit| hit.distance);
    let button = controls
        .iter()
        .filter_map(|(control, transform, ship)| {
            let frame = bodies.frame(ship.parent())?;
            ray_box(frame.point_to_local(eye) - transform.translation, frame.dir_to_local(*dir), control.half_size)
        })
        .fold(f32::MAX, f32::min);
    let Some((_, pickup)) = pickups
        .iter()
        .filter_map(|(pickup, transform)| {
            let turn = transform.rotation.inverse();
            let t = ray_box(turn * (eye - transform.translation), turn * *dir, pickup.half())?;
            (t <= CONTROL_REACH && t <= blocked + 0.25 && t < button).then_some((t, pickup))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
    else {
        return Ok(());
    };

    let center = ctx.content_rect().center();
    let mut room = player.loadout.inventory;
    let fits: Vec<bool> = pickup.items.iter().map(|item| room.take(*item)).collect();
    egui::Area::new(egui::Id::new("pickup card"))
        .fixed_pos(center + egui::vec2(24.0, 22.0))
        .order(egui::Order::Foreground)
        .interactable(false)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(190))
                .stroke(egui::Stroke::new(1.0, egui::Color32::from_white_alpha(40)))
                .corner_radius(4.0)
                .inner_margin(egui::Margin::symmetric(10, 7))
                .show(ui, |ui| {
                    ui.set_max_width(300.0);
                    for (item, fits) in pickup.items.iter().zip(&fits) {
                        let text = egui::RichText::new(item.name()).color(item_color(item)).strong();
                        ui.label(if *fits { text } else { text.strikethrough() });
                    }
                    // One gun: what it is, and against yours.
                    if let [Item::Weapon(weapon)] = pickup.items.as_slice() {
                        for (line, color) in weapon_lines(weapon) {
                            ui.label(egui::RichText::new(line).small().color(color));
                        }
                        let theirs = compared_with(player, weapon);
                        if let Some(other) = theirs {
                            ui.label(egui::RichText::new(format!("vs {}", other.name())).small().color(DIM));
                        }
                        stats_table(ui, "pickup stats", &weapon.stats(), theirs.map(|w| w.stats()).as_ref());
                    }
                    if fits.iter().any(|f| !f) {
                        ui.label(
                            egui::RichText::new(format!(
                                "No room for some of it: drop something ({}) or sort your pack ({})",
                                bindings.label(Action::DropItem),
                                bindings.label(Action::Loadout)
                            ))
                            .small()
                            .color(AMBER),
                        );
                    }
                });
        });
    Ok(())
}
