//! What's in your hands, bottom right: a card for each hand (off hand
//! left, main hand right, as you hold them) and the stances above them,
//! the current one lit. Cards draw from [`HandView`]s only.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use egui::{Align2, Color32, Rect, pos2, vec2};

use super::style::{self, EDGE, HOSTILE, INK, INK_DIM, MICRO, NUMERAL, RARITY, SMALL, SUIT, WARNING};
use super::view::{self, HandItem, HandTag, HandView};
use crate::client::LocalPlayer;
use crate::client::bindings::{Action, Bindings};
use crate::sim::player::Player;
use crate::sim::tuning::Tuning;
use crate::sim::weapons::{Hand, SWITCH_SECONDS};

const CARD: egui::Vec2 = vec2(264.0, 92.0);
const GAP: f32 = 8.0;

pub fn hands_hud(
    mut contexts: EguiContexts,
    time: Res<Time>,
    tuning: Res<Tuning>,
    bindings: Res<Bindings>,
    player: Option<Single<&Player, With<LocalPlayer>>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Some(player) = player else {
        return Ok(());
    };
    let now = time.elapsed_secs();
    let loadout = &player.loadout;
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("hands")));
    let right = screen.max.x - EDGE;
    let bottom = screen.max.y - EDGE;
    let switching = loadout.switching > 0.0;

    for (column, view) in view::hand_views(&player, &tuning).iter().enumerate() {
        let max = pos2(right - column as f32 * (CARD.x + GAP), bottom);
        let action = match view.hand {
            Hand::Main => Action::MainUse,
            Hand::Off => Action::OffUse,
        };
        let alpha = if switching { 0.5 } else { 1.0 };
        card(&painter, Rect::from_min_max(max - CARD, max), view, &bindings.label(action), alpha, now);
    }

    // Stances, a strip over the cards: "1  SMG · GRP", off hand first as
    // the cards are.
    let strip = 2.0 * CARD.x + GAP;
    let chip = vec2((strip - 3.0 * 6.0) / 4.0, 26.0);
    let top = bottom - CARD.y - 8.0 - chip.y;
    for (i, s) in view::stance_views(&player).iter().enumerate() {
        let rect = Rect::from_min_size(pos2(right - strip + i as f32 * (chip.x + 6.0), top), chip);
        let current = i == loadout.current as usize;
        style::plate(&painter, rect, if current { SUIT } else { Color32::TRANSPARENT }, if current { 1.0 } else { 0.6 });
        let (number, words) = if current { (SUIT, INK) } else { (INK_DIM, INK_DIM) };
        style::text(&painter, pos2(rect.min.x + 10.0, rect.center().y), Align2::LEFT_CENTER, (i + 1).to_string(), style::num(SMALL + 1.0), number);
        style::caps(&painter, pos2(rect.min.x + 26.0, rect.center().y), Align2::LEFT_CENTER, &format!("{} · {}", s.off, s.main), MICRO, words);
        if current {
            let done = if switching { 1.0 - loadout.switching / SWITCH_SECONDS } else { 1.0 };
            let line = Rect::from_min_size(pos2(rect.min.x, rect.max.y - 2.0), vec2(rect.width() * done, 2.0));
            painter.rect_filled(line, 0.0, SUIT);
        }
    }

    // No weapons at all: say where they are.
    if !view::armed(&player) {
        let g = style::galley(&painter, "UNARMED  ·  WEAPONS LOCKER, AFT HOLD", style::ui(MICRO + 1.0), WARNING, 1.2);
        style::shadowed(&painter, pos2(right, top - 8.0), Align2::RIGHT_BOTTOM, g, WARNING);
    }
    Ok(())
}

/// A tag's word and colour.
fn tag_look(tag: HandTag, now: f32) -> (&'static str, Color32) {
    match tag {
        HandTag::Reloading => ("Reloading", SUIT),
        HandTag::Queued => ("Reload queued", style::fade(SUIT, 0.7)),
        HandTag::Dry => ("Empty", style::fade(HOSTILE, style::pulse(now))),
        HandTag::Low => ("Low", WARNING),
        HandTag::Braced => ("Braced", INK_DIM),
    }
}

/// One line of text no wider than `width`: a step smaller if it must be,
/// then cut with an ellipsis.
fn fitted(painter: &egui::Painter, text: &str, size: f32, width: f32, color: Color32) -> std::sync::Arc<egui::Galley> {
    let g = style::galley(painter, text, style::ui(size), color, 0.0);
    if g.size().x <= width {
        return g;
    }
    let mut job = egui::text::LayoutJob::single_section(
        text.to_string(),
        egui::TextFormat {
            font_id: style::ui(size * 0.84),
            color,
            ..default()
        },
    );
    job.wrap = egui::text::TextWrapping {
        max_width: width,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    painter.layout_job(job)
}

fn card(painter: &egui::Painter, rect: Rect, view: &HandView, key: &str, alpha: f32, now: f32) {
    let accent = match (&view.item, view.rarity) {
        (HandItem::Weapon, Some(r)) => RARITY[r as usize],
        (HandItem::Grapple { .. }, _) => SUIT,
        _ => INK_DIM,
    };
    let empty = view.item == HandItem::Empty;
    style::plate(painter, rect, accent, alpha * if empty { 0.6 } else { 1.0 });
    let a = |c: Color32| style::fade(c, alpha);
    // The rarity strip down the left edge.
    if !empty {
        painter.rect_filled(
            Rect::from_min_max(pos2(rect.min.x + 1.0, rect.min.y + style::CUT + 4.0), pos2(rect.min.x + 4.0, rect.max.y - 1.0)),
            0.0,
            a(accent),
        );
    }
    let inner = rect.shrink2(vec2(14.0, 8.0));
    let side = match view.hand {
        Hand::Main => "Main",
        Hand::Off => "Off",
    };
    // Which hand and its button; tags on the right.
    let r = style::caps(painter, inner.left_top(), Align2::LEFT_TOP, side, MICRO, a(INK_DIM));
    style::key_cap(painter, pos2(r.max.x + 6.0, r.center().y), key, a(INK_DIM));
    let mut x = inner.max.x;
    for tag in view.tags.iter().rev() {
        let (word, color) = tag_look(*tag, now);
        let r = style::caps(painter, pos2(x, inner.min.y), Align2::RIGHT_TOP, word, MICRO, a(color));
        x = r.min.x - 10.0;
    }

    let numbers_w = 76.0;
    let name_y = inner.min.y + 20.0;
    match &view.item {
        HandItem::Empty => {
            style::caps(painter, pos2(inner.min.x, name_y + 6.0), Align2::LEFT_TOP, "Empty hand", SMALL, a(INK_DIM));
        }
        HandItem::Missing => {
            let g = fitted(painter, &view.name, 21.0, inner.width() - numbers_w, a(INK_DIM));
            painter.galley(pos2(inner.min.x, name_y), g, a(INK_DIM));
            style::caps(painter, pos2(inner.min.x, name_y + 28.0), Align2::LEFT_TOP, "Not carried", MICRO, a(WARNING));
        }
        HandItem::Grapple { line, range } => {
            let g = style::galley(painter, "Grapple", style::ui(21.0), a(INK), 0.0);
            painter.galley(pos2(inner.min.x, name_y), g, a(INK));
            style::caps(painter, pos2(inner.min.x, name_y + 28.0), Align2::LEFT_TOP, "Suit system", MICRO, a(INK_DIM));
            let bar = Rect::from_min_size(pos2(inner.min.x, inner.max.y - 5.0), vec2(inner.width(), 4.0));
            match line {
                Some(line) => {
                    let r = style::unit(painter, pos2(inner.max.x, name_y + 13.0), Align2::RIGHT_CENTER, "m", a(INK_DIM));
                    style::text(painter, pos2(r.min.x - 3.0, name_y + 10.0), Align2::RIGHT_CENTER, format!("{line:.0}"), style::num(26.0), a(SUIT));
                    style::seg_bar(painter, bar, 1, line / range.max(1.0), 0.0, a(SUIT));
                }
                None => {
                    style::caps(painter, pos2(inner.max.x, name_y + 10.0), Align2::RIGHT_CENTER, "Ready", SMALL, a(SUIT));
                    style::seg_bar(painter, bar, 1, 0.0, 0.0, a(SUIT));
                }
            }
        }
        HandItem::Weapon => {
            let g = fitted(painter, &view.name, 21.0, inner.width() - numbers_w, a(INK));
            painter.galley(pos2(inner.min.x, name_y), g, a(INK));
            if let Some(r) = view.rarity.filter(|r| *r != view::Rarity::Common) {
                style::caps(painter, pos2(inner.min.x, name_y + 28.0), Align2::LEFT_TOP, r.label(), MICRO, a(RARITY[r as usize]));
            }
            let (mag, capacity) = view.mag.unwrap_or((0, 1));
            let dry = view.tags.contains(&HandTag::Dry) || mag == 0;
            let color = if dry {
                style::fade(HOSTILE, style::pulse(now))
            } else if view.tags.contains(&HandTag::Low) {
                WARNING
            } else {
                INK
            };
            let n = style::text(painter, pos2(inner.max.x, name_y - 4.0), Align2::RIGHT_TOP, mag.to_string(), style::num(NUMERAL), a(color));
            if let Some(reserve) = view.reserve {
                let reserve_color = if reserve == 0 { HOSTILE } else { INK_DIM };
                style::text(painter, pos2(inner.max.x, n.max.y - 2.0), Align2::RIGHT_TOP, format!("/ {reserve}"), style::num(SMALL), a(reserve_color));
            }
            // The mag: a cell per round when they fit; the reload sweeps it.
            let bar = Rect::from_min_size(pos2(inner.min.x, inner.max.y - 5.0), vec2(inner.width(), 5.0));
            let cells = if capacity <= 50 { capacity as usize } else { 25 };
            match view.reload {
                Some(done) => style::seg_bar(painter, bar, cells, done, 0.0, a(SUIT)),
                None => style::seg_bar(painter, bar, cells, mag as f32 / capacity.max(1) as f32, 0.0, a(color)),
            }
        }
    }
}
