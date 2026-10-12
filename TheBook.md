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

## Headless harness (system audio)

- 2026-10-12. The harness's ALSA null config didn't stop the game playing
  sound on the user's real devices: the system `alsa.conf` hooks load
  `/etc/alsa/conf.d/99-pipewire-default.conf` after anything on
  `ALSA_CONFIG_PATH`. Fix: point `ALSA_CONFIG_PATH` at our config alone
  (`tools/hl`, d46d850). Cost: a session's debugging.
