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
