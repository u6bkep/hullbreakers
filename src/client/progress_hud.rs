//! A notice when your profile grows: salvage and experience earned (a
//! mission's pay, a gun scrapped) and a new rank with what it brought.
//! The debrief's own breakdown (`Debrief::rewards`) is the HUD's to draw.

use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};

use super::LocalPlayer;
use super::items_hud::{AMBER, INK};
use crate::sim::progression::{LADDER, Profile, rank};

/// What the notice says, and how long it's been up (s).
#[derive(Default)]
pub struct Notice {
    lines: Vec<String>,
    age: f32,
}

/// Seconds a notice stays up.
const NOTICE_SECONDS: f32 = 7.0;

pub fn progress_notice(
    mut contexts: EguiContexts,
    time: Res<Time>,
    profile: Option<Single<&Profile, With<LocalPlayer>>>,
    mut last: Local<Option<(u32, u32)>>,
    mut notice: Local<Notice>,
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
        let mut lines = vec![match (now.0.saturating_sub(salvage), now.1.saturating_sub(xp)) {
            (s, 0) => format!("+{s} salvage"),
            (0, x) => format!("+{x} xp"),
            (s, x) => format!("+{s} salvage   +{x} xp"),
        }];
        let (was, is) = (rank(xp), rank(now.1));
        if is > was {
            lines.push(format!("RANK {is}"));
            lines.extend(LADDER.iter().filter(|(at, _)| *at > was && *at <= is).map(|(_, what)| what.to_string()));
        }
        *notice = Notice { lines, age: 0.0 };
    }
    *last = Some(now);
    if notice.lines.is_empty() {
        return Ok(());
    }
    notice.age += time.delta_secs();
    if notice.age > NOTICE_SECONDS {
        notice.lines.clear();
        return Ok(());
    }
    let fade = (NOTICE_SECONDS - notice.age).clamp(0.0, 1.0);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("progress notice")));
    let screen = ctx.content_rect();
    let mut at = egui::pos2(screen.center().x, screen.max.y * 0.68);
    for (i, line) in notice.lines.iter().enumerate() {
        let (size, color) = match i {
            0 => (18.0, AMBER),
            1 => (22.0, INK),
            _ => (14.0, INK),
        };
        let galley = painter.layout_no_wrap(line.clone(), egui::FontId::proportional(size), color.gamma_multiply(fade));
        let rect = egui::Rect::from_center_size(at, galley.size() + egui::vec2(16.0, 6.0));
        painter.rect_filled(rect, 4.0, egui::Color32::from_black_alpha((170.0 * fade) as u8));
        painter.galley(rect.min + egui::vec2(8.0, 3.0), galley, color);
        at.y += rect.height() + 4.0;
    }
    Ok(())
}
