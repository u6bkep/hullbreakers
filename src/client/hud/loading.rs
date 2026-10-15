//! The veil: the view goes black while this peer can't show the world yet,
//! with what it's waiting on. Joining a host, that's the host's world and
//! the clock (nothing can be posed before the timeline's in step); starting
//! or joining a scenario with a big field, it's the rocks' shapes, built a
//! batch a tick (`asteroids::shape_asteroids`), so the bar moves and the
//! frame stays alive.

use avian3d::prelude::Collider;
use bevy::prelude::*;
use bevy_egui::{EguiContexts, egui};
use egui::{Align2, Rect, pos2, vec2};

use super::style::{self, INK, INK_DIM, SUIT};
use crate::net::Joining;
use crate::sim::asteroids::Asteroid;

/// A field this big (rocks still to shape) is worth a veil; a few rocks
/// arriving (the streaming field's) aren't.
const FIELD_WORTH_A_VEIL: usize = 100;
/// How long the veil takes to clear (s).
const CLEAR_SECONDS: f32 = 0.6;

#[derive(Default)]
pub struct Veil {
    /// What it says: the banner, the line under it, and progress (0..1), if
    /// any; kept while it clears.
    title: String,
    line: String,
    progress: Option<f32>,
    up: bool,
    cleared_at: Option<f32>,
}

#[allow(clippy::too_many_arguments)]
pub fn loading_overlay(
    mut contexts: EguiContexts,
    memory: Res<super::HudMemory>,
    ready: Res<style::HudReady>,
    mut veil: Local<Veil>,
    joining: Option<Res<Joining>>,
    shaped: Query<(), (With<Asteroid>, With<Collider>)>,
    pending: Query<(), (With<Asteroid>, Without<Collider>)>,
) -> Result {
    // The one HUD element that can be up on the first frame, before the
    // HUD's fonts are bound (a client is joining from its first frame).
    if !ready.0 {
        return Ok(());
    }
    let pending = pending.iter().count();
    let wanted = if let Some(joining) = joining {
        Some(("JOINING".to_string(), format!("the host at {}", joining.server), None))
    } else if pending >= FIELD_WORTH_A_VEIL || (veil.up && pending > 0) {
        let shaped = shaped.iter().count();
        let total = shaped + pending;
        Some(("CHARTING THE FIELD".to_string(), format!("{shaped} / {total} rocks"), Some(shaped as f32 / total.max(1) as f32)))
    } else {
        None
    };
    let alpha = match wanted {
        Some((title, line, progress)) => {
            veil.title = title;
            veil.line = line;
            veil.progress = progress;
            veil.up = true;
            veil.cleared_at = None;
            1.0
        }
        None => {
            if veil.up {
                veil.up = false;
                veil.cleared_at = Some(memory.now);
                // The bar fills before it goes.
                if veil.progress.is_some() {
                    veil.progress = Some(1.0);
                }
            }
            match veil.cleared_at {
                Some(at) => 1.0 - ((memory.now - at) / CLEAR_SECONDS).clamp(0.0, 1.0),
                None => return Ok(()),
            }
        }
    };
    if alpha <= 0.0 {
        return Ok(());
    }
    let ctx = contexts.ctx_mut()?;
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("loading")));
    painter.rect_filled(screen, 0.0, egui::Color32::from_black_alpha((alpha * 255.0) as u8));
    let c = pos2(screen.center().x, screen.min.y + screen.height() * 0.42);
    super::mission::banner(&painter, c, &veil.title, SUIT, alpha);
    let g = style::galley(&painter, veil.line.clone(), style::ui(style::BODY + 2.0), style::fade(INK, alpha), 0.0);
    style::shadowed(&painter, c + vec2(0.0, 44.0), Align2::CENTER_CENTER, g, style::fade(INK, alpha));
    if let Some(progress) = veil.progress {
        let bar = Rect::from_center_size(c + vec2(0.0, 84.0), vec2(320.0, 6.0));
        style::seg_bar(&painter, bar, 40, progress, 0.0, style::fade(SUIT, alpha));
        style::brackets(&painter, bar.expand2(vec2(8.0, 6.0)), 6.0, style::fade(INK_DIM, alpha));
    }
    Ok(())
}
