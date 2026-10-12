# The Book

A ledger of friction. When a library or one of our own subsystems gets in
the way, write it down here, even if the workaround took five minutes. One
papercut never justifies replacing anything; the point of the book is to see
them pile up. When a section gets long, or the same kind of entry keeps
coming back, that's the case for building the thing in house (or rewriting
our own subsystem), and it gets raised with the user at a review.

## How to add an entry

- Find the section for the library or subsystem, or start one (`## name`).
- One bullet per incident. Give the date, what we were trying to do, what
  got in the way, what we did about it, and roughly what it cost (minutes,
  hours, a design compromise, a performance hit). Link the commit or the
  file where the workaround lives, so a reviewer can find it and so it can
  be removed if we replace the library.
- Write it when it happens, not at the end of the task. Agents: this is part
  of the job, the same as tests.
- Don't argue for replacing the library in the entry. Record the facts; the
  verdict belongs to a review.

## Reviews

When the book is reviewed, note the date and the verdict under the section
(keep, watch, or replace with what), so later entries can be read against
it.

---

## egui

- 2026-10-12. The menu window was given `fixed_size([600, 0])` to stop it
  jumping as tabs changed height. egui treats the 0 as a maximum height, so
  the Controls tab's scroll area collapsed to two rows, and the window
  became user-resizable as a side effect. Fix: pin the width with
  min/max width, size the scroll area from the screen height by hand, and
  turn off resizing explicitly (`src/client/menu.rs`, b00fc70). Cost: one
  round-trip with the user. Pattern to watch: egui's sizing is implicit and
  layout-pass dependent, so every non-trivial window needs a screenshot to
  check.
- 2026-10-12. The HUD wanted letter-spaced caps labels. `Painter::text`
  has no letter spacing; every spaced label goes through a `LayoutJob`
  with `extra_letter_spacing` (`style::galley` in `src/client/hud/style.rs`).
  Cost: minutes; one more helper every element has to remember to use.
- 2026-10-12. Scaling the HUD with the window (`Context::set_zoom_factor`)
  silently breaks anything that places egui shapes from a Bevy projection:
  `Camera::world_to_viewport` gives logical pixels and egui wants points,
  which differ by the zoom. Every world marker goes through
  `hud::Projector`, which divides it out. Cost: designed for up front; it
  would have put every marker in the wrong place at any height but 1080.
- 2026-10-12. Egui's tessellation is a real share of the HUD's cost:
  `process_output_system` (which tessellates) went from 0.047 to 0.150 ms
  a frame with the new HUD (traced release build, boarding at 1080p),
  against 0.098 ms for all the HUD's own systems. Segmented bars were
  turned into one mesh each (plain quads, no feathering) on the theory
  that dozens of anti-aliased rects were the cost; it didn't measurably
  move the number, so the cost is spread over text, plates and strokes.
  Not chased further: the whole HUD is about 0.2 ms over the old one.
- 2026-10-12. The inventory screen puts its item cards and the chosen
  item's details side by side, with `ui.separator()` between them in a
  horizontal layout. A separator there spans the *available* height, which
  was the rest of the screen, so the window ran off the bottom. Fix: the
  side-by-side part lives in an `allocate_ui_with_layout` of fixed height
  (`ITEMS_HEIGHT`, `src/client/menu.rs`). Cost: one harness round-trip.
  Same pattern as the entry above: implicit sizing, caught only on screen.

## HUD (our subsystem: what it can read)

- 2026-10-12. The damage-direction indicator was specified "from the hit
  log", but `HitLog` records the hits a player *dealt* (for hit markers);
  nothing records where a player's own damage came from. The HUD infers it
  client-side: robots' `Fired` events are in their replicated logs, beams
  are traced on every peer, and when your health drops the hostile shot
  whose line passed within 2.5 m of you (or the beam you were caught in)
  is taken as the source (`src/client/hud/vitals.rs`). Cost: an hour, and
  a heuristic: two robots firing at once can be confused. A sim-side
  record (the source of the last hurt on `Vitals`) would make it exact.
  (Done the same day: `sim::vitals::HurtLog`, written where damage is
  applied, with where each hurt came from, the killing blow included; the
  inference is gone.)
- 2026-10-12. The interact prompt tested the camera's rendered eye
  against the ship's physics pose. On a flying ship those are a tick
  apart, enough to name the neighbouring button (seen: aiming at
  Boarding action, prompted Model gallery). It now asks the sim's view and
  poses, as the press does (`hud/reticle.rs`). Cost: half an hour; any
  HUD element that mixes rendered and sim poses on a moving ship has the
  same trap.
- 2026-10-12. A pickup gets its `Transform` from the renderer a frame
  after it appears, so `Query<(Entity, &Transform), Added<Pickup>>` never
  matches it (the kit marker missed every death). Query `Added<Pickup>`
  alone and look the transform up later. Cost: one screenshot round.

## Our own: screenshot tour

- 2026-10-12. `HULLBREAKERS_SHOTS=<dir>` doesn't create its directory; Bevy's
  `save_to_disk` logs an IO error per shot and the tour finishes with
  nothing. Worked around with `mkdir -p` first. Cost: one run (~1 min).
  (`src/client/debug_shots.rs`; not fixed.)

## Bevy rendering

- 2026-10-12. We wanted the headlamp not to light the player's own arms.
  RenderLayers on lights look like the tool for this, but GPU light
  clustering ignores RenderLayers. Workaround: turn off GPU clustering
  (`GlobalClusterSettings.gpu_clustering = None`) and draw the hands with a
  second camera that has its own sun (d46d850). Cost: about 0.6 ms a frame
  and a two-camera setup (tonemapping, bloom and egui moved to the view
  model camera).

## Our player controller (sim/player.rs)

- 2026-10-12. Robots grappling the crew (the Grappler). `player_rope`, the
  solver for a rope between two players, moved both ends by the rope
  owner's tuning, so a 300 kg robot's rope treated the crew member as 300 kg
  with a robot's grip. And when both ends stood on boots on the same
  immovable body (one ship), each end's inverse mass was the ship's, zero,
  so the solver returned nothing and a running winch never tested anyone's
  grip: two crew on one deck couldn't reel each other either. Fix: each end
  by its own tuning, and with both held fast a winch pulls at its full force
  and whichever boots that beats let go. Cost: about 40 minutes, found by
  the grappler's test. A client still doesn't predict being pulled by
  someone else's rope (robot or crew); it's corrected.

## Bevy ECS

- 2026-10-12. A client system had a `Single<&Transform, With<PlayerCamera>>`
  next to a `Query<&mut Transform, Without<RobotLook>>`: a B0001 conflict
  that only shows as a panic when the game starts, since the tests don't run
  client systems (`src/client/robot_fx.rs`). Cost: one harness round trip.
  Pattern: client systems get no compile- or test-time check of their
  queries; every new one needs one launch.

## Blender pipeline (hb, the verifier, asset_view)

- 2026-10-12, from building the Grappler. Bevelled clevis plates (25 mm
  thick, 14 mm bevel) made 4 near-degenerate triangles, and the verifier
  failed "triangle winding agrees with stored normals" without naming the
  part; finding it (`thigh_l/r`) took a throwaway script. Fixed with
  `bevel=0` on the plates. A 17 mm radial step under a 10 mm bevel failed flat
  shading by 50° (the documented "bevel eats narrow steps"). A warning
  "1 modelled-flat plane has no triangles left" on `claw_l` never said which
  plane. Cost: maybe an hour of the agent's time.
- 2026-10-12. Affix parts live in the robots' own models and are hidden by
  the game, but the previews and `asset_view` always show every part, so
  there's no way to look at a model as it's seen without them (the turret's
  builder cut the section out of `build.py` and rebuilt to see it). Also,
  `asset_view` given several models draws them on the same spot, overlapping.
  Cost: a few iterations each.

## Headless harness (system audio)

- 2026-10-12. The harness's ALSA null config didn't stop the game playing
  sound on the user's real devices: the system `alsa.conf` hooks load
  `/etc/alsa/conf.d/99-pipewire-default.conf` after anything on
  `ALSA_CONFIG_PATH`. Fix: point `ALSA_CONFIG_PATH` at our config alone
  (`tools/hl`, d46d850). Cost: a session's debugging.

## Our ship designs (sim/design.rs, ship.rs)

- 2026-10-12. Sites that aren't ships (a derelict, a course of gates)
  are designs, which is right, but several things assumed every design is
  either the crew's ship or an enemy: the check demanded mission-wall
  buttons from every non-hostile design (now only `home`), and the check
  built a walking grid over the whole design, which for a 150 m course of
  gates is pointless (now skipped when there's nothing to walk to). Cost:
  half an hour. (`src/sim/design.rs`, `check`.)
- 2026-10-12. Every `ShipHull` gets a HUD ship marker (`hud::ship_markers`),
  so the grapple run's course is marked "COURSE" at its origin (put at the
  first gate, where that's useful) and the derelict as a ship. Not worked
  around; the HUD's objective markers should probably replace ship markers
  for sites. (Fixed in the HUD the same day: ship markers only for our
  ship and hostile ones; sites are marked by their scenario's markers.)
- 2026-10-12. `ControlAction::label` returns `&'static str`, so a device's
  label (from the design's data) couldn't come from the action. Fixed by
  putting the label on the `Control` (borrowed from the `'static` design),
  which the HUD's prompt now reads. Cost: minutes.
- 2026-10-12. A ship's collider is one compound (on purpose: child colliders
  are queried a tick stale). Doors that open and shut therefore rebuild the
  whole compound (`devices::fit_doors`, `ship::ship_collider`) when one
  changes. Cheap at a few hundred boxes and a change every few seconds; a
  ship with doors slamming every tick would want another way.
- 2026-10-12. Walking into a shut door climbs it, like any wall (the boots
  wrap onto whatever's in front): a crew can walk up a door, across the
  ceiling, and back. Harmless, and the door still blocks the way, but the
  door test had to stop asserting where the walk ends. (`tests/mission.rs`,
  `a_door_blocks_the_way_until_its_power_opens_it`.)

## Bevy rendering (lighting)

- 2026-10-12. The derelict is meant to be dark until its power is back, but
  `GlobalAmbientLight` is global, so its unlit rooms are only as dark as
  space's ambient (dim blue-grey walls, still readable without the lamp).
  Its cabin's light probe is dimmed to almost nothing while its lights are
  out (`client::devices::dim_dark_cabins`), which helps; truly dark interiors
  would need per-region ambient (more probes, or dropping the global
  ambient for probes everywhere). Not done; a design compromise.

