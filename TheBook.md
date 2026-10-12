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
