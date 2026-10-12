//! Items on screen: rarity colours, weapon stat tables (shared with the
//! inventory screen), and the card under the crosshair that says what's in
//! the pickup you're looking at and how it compares with what you hold.
//!
//! The colours are the HUD's tokens (`hud::style`, `docs/hud-style.md`),
//! named here for what they mean on an item.

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::LocalPlayer;
use super::bindings::{Action, Bindings};
use super::camera::PlayerCamera;
use crate::sim::bodies::Bodies;
use crate::sim::items::{Item, Rarity, Taken, Weapon};
use crate::sim::pickups::Pickup;
use crate::sim::player::{CONTROL_REACH, Player, ray_box};
use crate::sim::ship::Control;
use crate::sim::weapons::{GunStats, Hand};
use crate::sim::Layer;

use super::hud::style;

pub const INK: egui::Color32 = style::INK;
pub const DIM: egui::Color32 = style::INK_DIM;
pub const AMBER: egui::Color32 = style::WARNING;
/// Stat names in the tables.
pub const LABEL: egui::Color32 = style::INK_DIM;
/// The stat tables' label and value columns (px).
const STAT_LABEL_WIDTH: f32 = 96.0;
const STAT_VALUE_WIDTH: f32 = 84.0;
pub const BETTER: egui::Color32 = style::FRIENDLY;
/// Worse is a caution, not a threat: amber, not red.
pub const WORSE: egui::Color32 = style::WARNING;
/// Supplies (ammo) and cargo, which have no rarity: ammo is plain, cargo
/// is what a mission is about.
pub const SUPPLY: egui::Color32 = style::INK;
pub const CARGO: egui::Color32 = style::OBJECTIVE;

pub fn rarity_color(rarity: Rarity) -> egui::Color32 {
    super::hud::style::rarity(rarity)
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
    // Columns of a fixed width: a grid sizes its columns from the frame
    // before, so a new table's first frames would draw labels under values.
    let cell = |ui: &mut egui::Ui, width: f32, text: egui::RichText| {
        ui.allocate_ui_with_layout(egui::vec2(width, 16.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.set_min_width(width);
            ui.add(egui::Label::new(text).wrap_mode(egui::TextWrapMode::Extend));
        });
    };
    egui::Grid::new(id).num_columns(3).spacing([8.0, 1.0]).show(ui, |ui| {
        for (i, line) in stat_lines(stats).iter().enumerate() {
            cell(ui, STAT_LABEL_WIDTH, egui::RichText::new(line.label).color(LABEL).small());
            cell(
                ui,
                STAT_VALUE_WIDTH,
                egui::RichText::new(format!("{:.*}{}", line.decimals, line.value, line.unit)).monospace().color(INK),
            );
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
/// there's room for it. (The prompt to take it is the HUD's.) Drawn to the
/// HUD's style (`docs/hud-style.md`): a scrim plate under the prompt.
#[allow(clippy::too_many_arguments)]
pub fn pickup_card(
    mut contexts: EguiContexts,
    bindings: Res<Bindings>,
    player: Option<Single<(Entity, &Player), With<LocalPlayer>>>,
    camera: Single<&Transform, With<PlayerCamera>>,
    spatial: SpatialQuery,
    bodies: Bodies,
    controls: Query<(&Control, &Transform, &ChildOf)>,
    pickups: Query<(&Pickup, &Transform, &Visibility)>,
    menu: Res<super::menu::Menu>,
) -> Result {
    use super::hud::style::{self, FRIENDLY, INK, INK_DIM, MICRO, SMALL, WARNING};
    use egui::{Align2, pos2, vec2};

    let ctx = contexts.ctx_mut()?;
    let Some(player) = player.filter(|_| !menu.open) else { return Ok(()) };
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
        .filter(|(.., visibility)| **visibility != Visibility::Hidden)
        .filter_map(|(pickup, transform, _)| {
            let turn = transform.rotation.inverse();
            let t = ray_box(turn * (eye - transform.translation), turn * *dir, pickup.half())?;
            (t <= CONTROL_REACH && t <= blocked + 0.25 && t < button).then_some((t, pickup))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
    else {
        return Ok(());
    };

    let mut room = player.loadout.inventory;
    let fits: Vec<bool> = pickup.items.iter().map(|item| room.take_what_fits(*item) != Taken::Nothing).collect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("pickup card")));

    // Lay it out first (rows of galleys), then the plate under it.
    enum Row {
        Text(std::sync::Arc<egui::Galley>, egui::Color32, f32),
        Stat(std::sync::Arc<egui::Galley>, std::sync::Arc<egui::Galley>, Option<(std::sync::Arc<egui::Galley>, egui::Color32)>),
        Rule,
    }
    let mut rows = Vec::new();
    for (item, fits) in pickup.items.iter().zip(&fits) {
        let color = if *fits { item_color(item) } else { INK_DIM };
        rows.push(Row::Text(style::galley(&painter, item.name(), style::ui(style::BODY), color, 0.0), color, 22.0));
    }
    if let [Item::Weapon(weapon)] = pickup.items.as_slice() {
        for (line, color) in weapon_lines(weapon) {
            rows.push(Row::Text(style::galley(&painter, line, style::ui(SMALL), color, 0.0), color, 18.0));
        }
        let theirs = compared_with(player, weapon);
        rows.push(Row::Rule);
        if let Some(other) = theirs {
            let line = format!("VS {}", other.name().to_uppercase());
            rows.push(Row::Text(style::galley(&painter, line, style::ui(MICRO), INK_DIM, 1.0), INK_DIM, 18.0));
        }
        let against = theirs.map(|w| stat_lines(&w.stats()));
        for (i, line) in stat_lines(&weapon.stats()).iter().enumerate() {
            let label = style::galley(&painter, line.label.to_uppercase(), style::ui(MICRO), INK_DIM, 1.0);
            let value = style::galley(&painter, format!("{:.*}{}", line.decimals, line.value, line.unit), style::num(SMALL), INK, 0.0);
            let delta = against.as_ref().map(|t| line.value - t[i].value).filter(|d| d.abs() > 0.5 * 10f32.powi(-(line.decimals as i32))).map(|d| {
                let color = if (d > 0.0) == line.more_is_better { FRIENDLY } else { WARNING };
                (style::galley(&painter, format!("{:+.*}", line.decimals, d), style::num(SMALL), color, 0.0), color)
            });
            rows.push(Row::Stat(label, value, delta));
        }
    }
    // A crewmate's death case: theirs alone to take.
    let theirs = pickup.owner_only && pickup.owner != Some(own);
    if theirs {
        rows.push(Row::Rule);
        let line = "A crewmate's kit: only they can take it".to_string();
        rows.push(Row::Text(style::galley(&painter, line, style::ui(SMALL), WARNING, 0.0), WARNING, 18.0));
    }
    if !theirs && fits.iter().any(|f| !f) {
        rows.push(Row::Rule);
        let line = format!(
            "No room for some of it: drop something ({}) or sort your pack ({})",
            bindings.label(Action::DropItem),
            bindings.label(Action::Loadout)
        );
        rows.push(Row::Text(style::galley(&painter, line, style::ui(SMALL), WARNING, 0.0), WARNING, 18.0));
    }
    const LABEL_W: f32 = 110.0;
    const VALUE_W: f32 = 80.0;
    let width = rows
        .iter()
        .map(|r| match r {
            Row::Text(g, ..) => g.size().x,
            Row::Stat(..) => LABEL_W + VALUE_W + 50.0,
            Row::Rule => 0.0,
        })
        .fold(220.0, f32::max)
        + 24.0;
    let height = rows
        .iter()
        .map(|r| match r {
            Row::Text(.., h) => *h,
            Row::Stat(..) => 17.0,
            Row::Rule => 9.0,
        })
        .sum::<f32>()
        + 16.0;
    // Under the interact prompt, clear of the crosshair.
    let center = ctx.content_rect().center();
    let plate = egui::Rect::from_min_size(center + vec2(26.0, 24.0), vec2(width, height));
    let accent = pickup.items.first().map_or(INK_DIM, item_color);
    style::plate(&painter, plate, accent, 1.0);
    let x = plate.min.x + 12.0;
    let mut y = plate.min.y + 8.0;
    for row in rows {
        match row {
            Row::Text(g, color, h) => {
                painter.galley(pos2(x, y + (h - g.size().y) / 2.0), g, color);
                y += h;
            }
            Row::Stat(label, value, delta) => {
                painter.galley(pos2(x, y + (17.0 - label.size().y) / 2.0), label, INK_DIM);
                painter.galley(Align2::RIGHT_TOP.anchor_size(pos2(x + LABEL_W + VALUE_W, y), value.size()).min, value, INK);
                if let Some((g, color)) = delta {
                    painter.galley(pos2(x + LABEL_W + VALUE_W + 10.0, y), g, color);
                }
                y += 17.0;
            }
            Row::Rule => {
                painter.line_segment([pos2(x, y + 4.0), pos2(plate.max.x - 12.0, y + 4.0)], egui::Stroke::new(style::HAIR, style::INK_FAINT));
                y += 9.0;
            }
        }
    }
    Ok(())
}
