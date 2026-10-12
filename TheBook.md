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
- 2026-10-12. The inventory screen puts its item cards and the chosen
  item's details side by side, with `ui.separator()` between them in a
  horizontal layout. A separator there spans the *available* height, which
  was the rest of the screen, so the window ran off the bottom. Fix: the
  side-by-side part lives in an `allocate_ui_with_layout` of fixed height
  (`ITEMS_HEIGHT`, `src/client/menu.rs`). Cost: one harness round-trip.
  Same pattern as the entry above: implicit sizing, caught only on screen.

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
  for sites.
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

