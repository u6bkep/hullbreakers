//! Marks on things in the world: every ship, the reference body (with the
//! prograde and retrograde of your motion against it), the current
//! objectives, and the case your kit was left in. Off screen, each becomes
//! an arrow at the edge of the view.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use avian3d::prelude::ColliderAabb;
use bevy_egui::{EguiContexts, egui};
use egui::{Align2, Color32, Pos2, Rect, Stroke, pos2, vec2};

use super::flight::relative_motion;
use super::style::{self, BOLD, EDGE, FRIENDLY, HOSTILE, INK, LINE, MICRO, OBJECTIVE, SMALL, SUIT};
use super::{CameraView, HudMemory, Projector, view};
use crate::client::LocalPlayer;
use crate::sim::bodies::Bodies;
use crate::sim::combat::Combatant;
use crate::sim::mission::{Mission, Phase, ScenarioKind};
use crate::sim::pickups::Pickup;
use crate::sim::player::Player;
use crate::sim::ship::{Control, ControlAction, HomeShip, ShipHull};
use crate::sim::tuning::Tuning;

/// "123 m", or "1.4 km" further out.
pub fn distance_label(metres: f32) -> String {
    if metres < 1000.0 { format!("{metres:.0} m") } else { format!("{:.1} km", metres / 1000.0) }
}

/// How visible a mark is at `p`: it fades as it nears the crosshair, so it
/// never sits on it.
fn clear_of_centre(screen: Rect, p: Pos2) -> f32 {
    let r = p.distance(screen.center()) / screen.height();
    0.3 + 0.7 * ((r - 0.04) / 0.08).clamp(0.0, 1.0)
}

/// Where this frame's labels went, so later (less important) ones keep
/// clear of them. Marks draw most important first: objectives, the
/// reference body, then ships.
#[derive(Default)]
pub struct Labels(Vec<Rect>);

impl Labels {
    pub fn clear(&mut self) {
        self.0.clear();
    }

    fn free(&self, rect: Rect) -> bool {
        !self.0.iter().any(|r| r.intersects(rect))
    }
}

/// A label on a small dark backing, so it reads over a lit hull or a
/// shield.
fn backed(painter: &egui::Painter, rect: Rect, g: std::sync::Arc<egui::Galley>, color: Color32) {
    let alpha = color.a() as f32 / 255.0;
    painter.rect_filled(rect.expand2(vec2(4.0, 1.0)), 0.0, style::fade(style::SCRIM, alpha * 0.8));
    painter.galley(rect.min, g, color);
}

/// A mark's name over it and its range under it, unless that would sit on
/// a more important label (then the mark goes unlabelled).
fn tag(painter: &egui::Painter, labels: &mut Labels, above: Pos2, below: Pos2, name: &str, range: &str, color: Color32) {
    let name_g = style::galley(painter, name.to_uppercase(), style::ui(MICRO + 1.0), color, 1.0);
    let name_r = Align2::CENTER_BOTTOM.anchor_size(above, name_g.size());
    let ink = style::fade(INK, color.a() as f32 / 255.0);
    let range_g = style::galley(painter, range, style::num(SMALL - 1.0), ink, 0.0);
    let range_r = Align2::CENTER_TOP.anchor_size(below, range_g.size());
    let (name_r, range_r) = (name_r.expand2(vec2(4.0, 1.0)), range_r.expand2(vec2(4.0, 1.0)));
    if !labels.free(name_r) || !labels.free(range_r) {
        return;
    }
    labels.0.extend([name_r, range_r]);
    backed(painter, name_r.shrink2(vec2(4.0, 1.0)), name_g, color);
    backed(painter, range_r.shrink2(vec2(4.0, 1.0)), range_g, ink);
}

/// An arrow at the screen's edge toward something off screen, with its
/// name and range inward of it, stepped along the edge clear of the
/// labels already there.
fn edge_mark(painter: &egui::Painter, labels: &mut Labels, screen: Rect, dir: egui::Vec2, name: &str, range: &str, color: Color32) {
    let tip = style::edge_point(screen, dir, EDGE + 4.0);
    style::edge_arrow(painter, tip, dir, 13.0, color);
    let align = style::inward(dir);
    let g = style::galley(painter, format!("{}  {range}", name.to_uppercase()), style::ui(MICRO + 1.0), color, 1.0);
    // On a side edge, step up or down; on the top or bottom, across; either
    // way toward the middle of the edge, so labels stay on screen.
    let inner = screen.shrink(EDGE + 6.0);
    let side = tip.x <= inner.min.x || tip.x >= inner.max.x;
    let toward = screen.center() - tip;
    let step = if side {
        vec2(0.0, (g.size().y + 3.0).copysign(toward.y))
    } else {
        vec2((g.size().x + 12.0).copysign(toward.x), 0.0)
    };
    let mut rect = align.anchor_size(tip - dir * 20.0, g.size());
    for _ in 0..6 {
        if labels.free(rect.expand(1.0)) {
            break;
        }
        rect = rect.translate(step);
    }
    labels.0.push(rect.expand(1.0));
    backed(painter, rect, g, color);
}

/// Every ship, always marked: a diamond with its name and range, or an arrow
/// at the screen edge when it's off screen, so nobody loses the fight (or
/// the way home) among the rocks. The ship you're standing on or inside
/// isn't marked. A ship the mission names as the enemy is marked HOSTILE;
/// ours FRIENDLY.
pub fn ship_markers(
    mut contexts: EguiContexts,
    player: Single<&Player, With<LocalPlayer>>,
    ships: Query<(Entity, &ShipHull, &Transform, &Visibility, Has<HomeShip>)>,
    camera: CameraView,
    mut memory: ResMut<HudMemory>,
) -> Result {
    let labels = &mut memory.labels;
    let ctx = contexts.ctx_mut()?;
    let (camera, cam) = *camera;
    let view = Projector::new(camera, cam, ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("ship markers")));
    let screen = ctx.content_rect();
    for (entity, hull, transform, visibility, home) in &ships {
        let ship = hull.design();
        let inside = ((transform.rotation.inverse() * (cam.translation - transform.translation) - ship.shield.center)
            / ship.shield.radii)
            .length()
            < 1.0;
        // The reference body is marked by its brackets instead.
        if player.mode.body() == Some(entity) || player.reference_body == Some(entity) || inside || *visibility == Visibility::Hidden {
            continue;
        }
        let color = if ship.hostile {
            HOSTILE
        } else if home {
            FRIENDLY
        } else {
            INK
        };
        // The middle of the hull.
        let center = transform.translation + transform.rotation * Vec3::new(0.0, 1.5, 0.0);
        let range = distance_label(center.distance(cam.translation));
        match view.to_screen(center).filter(|p| screen.shrink(EDGE).contains(*p)) {
            Some(p) => {
                let color = style::fade(color, clear_of_centre(screen, p));
                let r = 7.0;
                painter.add(egui::Shape::closed_line(style::diamond(p, r + 1.5), Stroke::new(LINE + 2.0, style::SHADOW)));
                style::glow_line(&painter, style::diamond(p, r), color, LINE, true);
                painter.circle_filled(p, 1.5, color);
                tag(&painter, labels, p - vec2(0.0, r + 4.0), p + vec2(0.0, r + 3.0), &ship.name, &range, color);
            }
            None => edge_mark(&painter, labels, screen, view.edge_dir(center), &ship.name, &range, color),
        }
    }
    Ok(())
}

/// Brackets the reference body (or points at it from the screen edge) and
/// marks the prograde/retrograde directions of motion relative to it. The
/// reference body is a suit system, so all of it is SUIT.
pub fn reference_overlay(
    mut contexts: EguiContexts,
    tuning: Res<Tuning>,
    player: Single<&Player, With<LocalPlayer>>,
    bodies: Bodies,
    names: Query<&Name>,
    aabbs: Query<&ColliderAabb>,
    camera: CameraView,
    mut memory: ResMut<HudMemory>,
) -> Result {
    let labels = &mut memory.labels;
    let ctx = contexts.ctx_mut()?;
    let Some(reference) = player.reference_body else {
        return Ok(());
    };
    let Some((position, relative_velocity)) = relative_motion(&player, &bodies, &tuning) else {
        return Ok(());
    };
    let (camera, cam) = *camera;
    let view = Projector::new(camera, cam, ctx);
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("reference")));

    // Prograde (circle with three ticks) and retrograde (circle with a cross).
    if relative_velocity.length() > 0.2 {
        let dir = relative_velocity.normalize();
        if let Some(p) = view.to_screen(cam.translation + dir * 100.0) {
            let color = style::fade(SUIT, clear_of_centre(screen, p));
            painter.circle_stroke(p, 8.0, Stroke::new(LINE + 2.0, style::SHADOW));
            painter.circle_stroke(p, 7.0, Stroke::new(LINE, color));
            for tick in [vec2(0.0, -1.0), vec2(-1.0, 0.0), vec2(1.0, 0.0)] {
                painter.line_segment([p + tick * 7.0, p + tick * 13.0], Stroke::new(LINE, color));
            }
            painter.circle_filled(p, 1.5, color);
        }
        if let Some(p) = view.to_screen(cam.translation - dir * 100.0) {
            let color = style::fade(SUIT, clear_of_centre(screen, p) * 0.8);
            painter.circle_stroke(p, 8.0, Stroke::new(LINE + 2.0, style::SHADOW));
            painter.circle_stroke(p, 7.0, Stroke::new(LINE, color));
            let d = 7.0 * std::f32::consts::FRAC_1_SQRT_2;
            painter.line_segment([p + vec2(-d, -d), p + vec2(d, d)], Stroke::new(LINE, color));
            painter.line_segment([p + vec2(-d, d), p + vec2(d, -d)], Stroke::new(LINE, color));
        }
    }

    // Standing on the reference body: no need to point it out.
    if player.mode.body() == Some(reference) {
        return Ok(());
    }
    let Ok(aabb) = aabbs.get(reference) else {
        return Ok(());
    };
    let (min, max) = (aabb.min, aabb.max);
    let center = (min + max) / 2.0;
    let offset = center - position;
    let distance = offset.length();
    // Positive when approaching.
    let closing = relative_velocity.dot(offset / distance.max(1e-3));
    let name = names.get(reference).map(|n| n.as_str().to_owned()).unwrap_or_default();
    let range = format!("{}  {closing:+.1} m/s", distance_label(distance));

    let corners: Vec<Option<Pos2>> = (0..8)
        .map(|i| {
            let pick = |bit: usize, lo: f32, hi: f32| if i & bit == 0 { lo } else { hi };
            view.to_screen(Vec3::new(pick(1, min.x, max.x), pick(2, min.y, max.y), pick(4, min.z, max.z)))
        })
        .collect();
    if corners.iter().all(Option::is_some) {
        let mut rect = Rect::NOTHING;
        for p in corners.into_iter().flatten() {
            rect.extend_with(p);
        }
        let rect = Rect::from_center_size(rect.center(), rect.size().max(vec2(26.0, 26.0)) + vec2(10.0, 10.0));
        // A body that fills the view is bracketed at the view's edge, and
        // labelled clear of the bottom of the HUD.
        let bounds = screen.shrink2(vec2(EDGE * 3.0, EDGE * 2.5));
        let rect = rect.intersect(bounds);
        if rect.is_positive() {
            let arm = (rect.width().min(rect.height()) * 0.22).clamp(6.0, 18.0);
            style::brackets(&painter, rect, arm, SUIT);
            let label_at = pos2(rect.center().x, (rect.max.y + 6.0).min(screen.max.y - 200.0));
            tag(&painter, labels, label_at + vec2(0.0, 16.0), label_at + vec2(0.0, 18.0), &name, &range, SUIT);
        }
    } else {
        // Off screen (or partly behind us): an arrow at the screen edge.
        edge_mark(&painter, labels, screen, view.edge_dir(center), &name, &range, SUIT);
    }
    Ok(())
}

/// What a marker marks; sets how it's drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkerKind {
    /// What to do: the mission's goal.
    Objective,
    /// On the way to it.
    Waypoint,
    /// Somewhere to get kit.
    Supply,
    /// Your own kit, where you fell.
    Kit,
}

/// A mark on something in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct Objective {
    pub label: String,
    pub position: Vec3,
    pub kind: MarkerKind,
}

/// Where the current objectives are. Scenarios add theirs in
/// [`Objectives::list`]: a query here for what they need, an arm there.
#[derive(SystemParam)]
pub struct Objectives<'w, 's> {
    missions: Query<'w, 's, &'static Mission>,
    ships: Query<'w, 's, (Entity, &'static ShipHull, &'static Transform, Option<&'static Combatant>, Has<HomeShip>)>,
    controls: Query<'w, 's, (&'static Control, &'static Transform, &'static ChildOf)>,
}

impl Objectives<'_, '_> {
    /// Where the button for `action` on `ship` is, in the world.
    fn control(&self, ship: Entity, action: ControlAction) -> Option<Vec3> {
        let (_, _, transform, ..) = self.ships.get(ship).ok()?;
        self.controls
            .iter()
            .find(|(c, _, parent)| parent.parent() == ship && c.action == action)
            .map(|(_, local, _)| transform.transform_point(local.translation))
    }

    /// The marks for `player` just now.
    pub fn list(&self, player: &Player) -> Vec<Objective> {
        let mut list = Vec::new();
        let mut mark = |label: &str, position: Option<Vec3>, kind: MarkerKind| {
            if let Some(position) = position {
                list.push(Objective { label: label.into(), position, kind });
            }
        };
        let home = self.ships.iter().find(|s| s.4).map(|s| s.0);
        let enemy = self.ships.iter().find(|(_, hull, ..)| hull.design().hostile);
        if let Ok(mission) = self.missions.single()
            && mission.kind == ScenarioKind::Boarding
        {
            match (mission.phase, enemy) {
                (Phase::Underway, Some((ship, hull, transform, combatant, _))) if combatant.is_some_and(|c| c.shield) => {
                    // The way in, until you're aboard; then the console.
                    if player.mode.body() != Some(ship)
                        && let Some(&entry) = hull.design().markers.get("entry")
                    {
                        mark("Hangar", Some(transform.transform_point(entry)), MarkerKind::Waypoint);
                    }
                    mark("Disable shields", self.control(ship, ControlAction::DisableShields), MarkerKind::Objective);
                }
                (Phase::Won, _) => {
                    let wall = home.and_then(|h| self.control(h, ControlAction::Scenario(ScenarioKind::Field)));
                    mark("Mission wall", wall, MarkerKind::Objective);
                }
                _ => {}
            }
        }
        // Unarmed aboard our ship: where the guns are.
        if !view::armed(player) && player.mode.body().is_some() && player.mode.body() == home {
            mark("Weapons locker", home.and_then(|h| self.control(h, ControlAction::Armory)), MarkerKind::Supply);
        }
        list
    }
}

/// The current objectives, and your kit if it's lying about.
pub fn objective_markers(
    mut contexts: EguiContexts,
    player: Single<&Player, With<LocalPlayer>>,
    objectives: Objectives,
    mut memory: ResMut<HudMemory>,
    pickups: Query<&Transform, With<Pickup>>,
    camera: CameraView,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let (camera, cam) = *camera;
    let view = Projector::new(camera, cam, ctx);
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Background, egui::Id::new("objectives")));
    let mut marks = objectives.list(&player);
    match memory.kit.map(|kit| pickups.get(kit)) {
        Some(Ok(case)) => marks.push(Objective {
            label: "Your kit".into(),
            position: case.translation,
            kind: MarkerKind::Kit,
        }),
        // Picked up (or gone with its ship).
        Some(Err(_)) => memory.kit = None,
        None => {}
    }
    for mark in &marks {
        let color = match mark.kind {
            MarkerKind::Objective | MarkerKind::Waypoint => OBJECTIVE,
            MarkerKind::Supply => INK,
            MarkerKind::Kit => SUIT,
        };
        let range = distance_label(mark.position.distance(cam.translation));
        let Some(p) = view.to_screen(mark.position).filter(|p| screen.shrink(EDGE).contains(*p)) else {
            edge_mark(&painter, &mut memory.labels, screen, view.edge_dir(mark.position), &mark.label, &range, color);
            continue;
        };
        let color = style::fade(color, clear_of_centre(screen, p));
        let r = match mark.kind {
            MarkerKind::Objective => {
                // A diamond in chevrons: the loudest mark in the HUD.
                let r = 9.0;
                painter.add(egui::Shape::closed_line(style::diamond(p, r + 1.5), Stroke::new(BOLD + 2.0, style::SHADOW)));
                style::glow_line(&painter, style::diamond(p, r), color, BOLD, true);
                painter.add(egui::Shape::convex_polygon(style::diamond(p, 3.5), color, Stroke::NONE));
                for s in [-1.0, 1.0] {
                    let x = p.x + s * (r + 6.0);
                    style::glow_line(&painter, vec![pos2(x - s * 4.0, p.y - 5.0), pos2(x, p.y), pos2(x - s * 4.0, p.y + 5.0)], color, LINE, false);
                }
                r
            }
            MarkerKind::Waypoint | MarkerKind::Supply => {
                let r = 6.0;
                painter.add(egui::Shape::closed_line(style::diamond(p, r + 1.5), Stroke::new(LINE + 2.0, style::SHADOW)));
                style::glow_line(&painter, style::diamond(p, r), color, LINE, true);
                r
            }
            MarkerKind::Kit => {
                // A case.
                let case = Rect::from_center_size(p, vec2(14.0, 9.0));
                painter.rect_stroke(case.expand(1.0), 0.0, Stroke::new(LINE + 2.0, style::SHADOW), egui::StrokeKind::Middle);
                painter.rect_stroke(case, 0.0, Stroke::new(LINE, color), egui::StrokeKind::Middle);
                painter.line_segment([pos2(case.min.x, p.y), pos2(case.max.x, p.y)], Stroke::new(LINE, color));
                6.0
            }
        };
        tag(&painter, &mut memory.labels, p - vec2(0.0, r + 5.0), p + vec2(0.0, r + 4.0), &mark.label, &range, color);
    }
    Ok(())
}
