//! What your profile just gained (see `sim::progression`): a short line of
//! salvage and experience earned (a mission's pay, a gun scrapped), and a
//! card for a new rank with what it brought. Bottom centre, between the
//! suit block and the hands; appears fast, leaves slow, and nothing moves.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use egui::{Align2, Rect, pos2, vec2};

use super::HudMemory;
use super::style::{self, EDGE, FRIENDLY, INK, INK_DIM, MICRO, SMALL, SUIT, TITLE};
use crate::client::LocalPlayer;
use crate::sim::progression::{LADDER, Profile, rank};

/// A gain on show: what it was, and when it came.
#[derive(Default)]
pub struct Gain {
    salvage: u32,
    xp: u32,
    /// The rank reached, if it was new, and what it brought.
    rank: Option<(u8, Vec<&'static str>)>,
    at: f32,
}

/// Seconds a gain stays up; a new rank stays longer.
const GAIN_SECONDS: f32 = 5.0;
const RANK_SECONDS: f32 = 9.0;

pub fn progress_hud(
    mut contexts: EguiContexts,
    memory: Res<HudMemory>,
    profile: Option<Single<&Profile, With<LocalPlayer>>>,
    mut last: Local<Option<(u32, u32)>>,
    mut gain: Local<Option<Gain>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let Some(profile) = profile else {
        *last = None;
        return Ok(());
    };
    let now = (profile.salvage, profile.xp);
    if let Some((salvage, xp)) = *last
        && (now.0 > salvage || now.1 > xp)
    {
        let (was, is) = (rank(xp), rank(now.1));
        *gain = Some(Gain {
            salvage: now.0.saturating_sub(salvage),
            xp: now.1.saturating_sub(xp),
            rank: (is > was).then(|| (is, LADDER.iter().filter(|(at, _)| *at > was && *at <= is).map(|(_, w)| *w).collect())),
            at: memory.now,
        });
    }
    *last = Some(now);
    let Some(g) = gain.as_ref() else { return Ok(()) };
    let age = memory.now - g.at;
    let life = if g.rank.is_some() { RANK_SECONDS } else { GAIN_SECONDS };
    if age > life {
        *gain = None;
        return Ok(());
    }
    let alpha = style::appear(age, life);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("progress")));
    let screen = ctx.content_rect();
    let ink = style::fade(INK, alpha);
    let dim = style::fade(INK_DIM, alpha);

    // The gain: "+120 SALVAGE   +300 XP" in a slim plate.
    let parts: Vec<(String, &str)> = [(g.salvage, "salvage"), (g.xp, "xp")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, what)| (format!("+{n}"), what))
        .collect();
    let numbers: Vec<_> = parts.iter().map(|(n, _)| style::galley(&painter, n.clone(), style::num(SMALL + 3.0), ink, 0.0)).collect();
    let labels: Vec<_> = parts
        .iter()
        .map(|(_, w)| style::galley(&painter, w.to_uppercase(), style::ui(MICRO), dim, MICRO * 0.08))
        .collect();
    let row_w: f32 = numbers.iter().zip(&labels).map(|(n, l)| n.size().x + 6.0 + l.size().x).sum::<f32>() + 18.0 * (parts.len().max(1) - 1) as f32;
    let (card_w, rank_lines) = match &g.rank {
        Some((_, brought)) => {
            let w = brought.iter().map(|b| style::galley(&painter, b.to_string(), style::ui(SMALL), ink, 0.0).size().x).fold(row_w, f32::max);
            (w.max(220.0) + 40.0, brought.len())
        }
        None => (row_w + 32.0, 0),
    };
    let height = if g.rank.is_some() { 74.0 + rank_lines as f32 * 20.0 } else { 32.0 };
    let bottom = screen.max.y - EDGE - 120.0;
    let plate = Rect::from_min_size(pos2(screen.center().x - card_w / 2.0, bottom - height), vec2(card_w, height));
    let accent = if g.rank.is_some() { SUIT } else { FRIENDLY };
    style::plate(&painter, plate, accent, alpha);
    let mut y = plate.min.y + 16.0;
    if let Some((r, brought)) = &g.rank {
        style::caps(&painter, pos2(plate.center().x, plate.min.y + 10.0), Align2::CENTER_TOP, "New rank", MICRO, style::fade(SUIT, alpha));
        let title = style::galley(&painter, format!("RANK {r}"), style::bold(TITLE + 4.0), style::fade(SUIT, alpha), 2.0);
        painter.galley(pos2(plate.center().x - title.size().x / 2.0, plate.min.y + 26.0), title, style::fade(SUIT, alpha));
        for (i, b) in brought.iter().enumerate() {
            let at = pos2(plate.center().x, plate.min.y + 62.0 + i as f32 * 20.0);
            style::text(&painter, at, Align2::CENTER_TOP, b.to_string(), style::ui(SMALL), ink);
        }
        // The gain sits under the rank card, on a plate of its own.
        y = plate.max.y + 22.0;
        let gp = Rect::from_min_size(pos2(screen.center().x - (row_w + 32.0) / 2.0, plate.max.y + 6.0), vec2(row_w + 32.0, 32.0));
        style::plate(&painter, gp, FRIENDLY, alpha);
    }
    let mut x = screen.center().x - row_w / 2.0;
    for (n, l) in numbers.into_iter().zip(labels) {
        let (nw, lw) = (n.size().x, l.size().x);
        painter.galley(pos2(x, y - n.size().y / 2.0), n, ink);
        painter.galley(pos2(x + nw + 6.0, y - l.size().y / 2.0 + 1.0), l, dim);
        x += nw + 6.0 + lw + 18.0;
    }
    Ok(())
}
