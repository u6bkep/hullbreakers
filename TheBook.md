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

## Our tuning (sim/tuning.rs)

- 2026-10-12. The `tuning!` macro's field lists don't take doc comments
  (`///` is an attribute its patterns don't match); a field's note has to
  be a `//` comment. Cost: one build.

## Our fauna (sim/fauna.rs)

- 2026-10-12, merging wave 4's weapons. Shots settle "living" targets from
  players only, so a grazer (health, but a body) was pushed and never hurt;
  and plasma splash measures to a target's middle, right for a crew capsule
  but not a 3 m grazer (a burst 1.5 m off its bell was 3 m from its
  middle). Targets now include creature bodies (no side: everyone's shots
  hit them; pushed as bodies, not shoved as players), and splash reaches a
  body's nearest surface (`src/sim/shots.rs`). Cost: one failed test.
- 2026-10-12. Code-drawn colours escape the asset verifier's magenta rule:
  the limpet's tendril was drawn pink and a dead grazer's spores
  heart-magenta, both breaking "magenta means shoot here". Fixed by hand;
  nothing checks code colours. Cost: caught in review of screenshots.

- 2026-10-12. A grazer may change its motion only in its pulses (so the
  network sends nothing between them), and its jet pushes only bell
  first. Steered that way (turning toward where it wants to go, pushing
  when it faces it) it orbited its goal in 15 m loops: its spin, set at
  one pulse, overshot by the next, and it couldn't brake without turning
  round. Fix: a pulse can push up to 35° off its axis (it squeezes one side
  of its bell harder), it eases off in time to stop (speed limited by
  `sqrt(2·a·d)`), and each pulse sets the spin that turns it exactly to
  its heading by the next. Cost: one trace, an hour.

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

## Our netcode (net/mod.rs)

- 2026-10-12. A death where the respawn puts you back where you stood
  (killed at your deck spot, carrying nothing) never reached the client:
  the client only takes the authority's `Player` on a rollback, and
  `player_diverged` saw nothing to roll back for, so the authority-only
  `Died` stayed out of the client's log (no SUIT FAILURE, no fade). Found
  in the live co-op smoke test. Fix: a death in the confirmed log that the
  prediction lacks is a divergence (`died_unpredicted`; test
  `a_death_where_you_stand_reaches_the_client`). Cost: an hour. Pattern to
  watch: any other authority-only event that changes nothing predicted
  would be lost the same way.
  (Replaced the same day: authority-only events (`Died`, `Respawned`,
  `Detached(Blasted)`, a ship's break-up snapping a rope) moved out of the
  predicted `EventLog` into a replicated, unpredicted `AuthorityLog`, read
  by count, so nothing depends on a rollback to deliver an event;
  `died_unpredicted` is gone. Tests `a_death_where_you_stand_is_presented_*`
  and `a_blast_at_250_ms_is_presented`. Cost: two hours.)

## lightyear

- 2026-10-12. At 250 ms of round trip an authority's shove (a blast off
  the deck) never corrected the client: it kept standing while the server
  had it floating, for seconds or for good. lightyear's
  `RollbackPolicy::max_rollback_ticks` defaults to 20, and a confirmed
  state is the client's lead plus the trip back old when it arrives (~22
  ticks at 250 ms); older rollbacks are refused with a `warn!` that
  headless tests don't show, so it read as "no divergence" (only jitter
  occasionally let one through). Raised to 60 (`MAX_ROLLBACK_TICKS` in
  `src/net/mod.rs`). Cost: an hour, found by a test asserting the
  client's mode, not just its events. Watch: deeper rollbacks re-simulate
  more ticks each (CPU at high lag).

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
- 2026-10-12. A system that both reads and writes the same message type
  (`MessageReader<Damage>` and `MessageWriter<Damage>`) panics at startup
  (B0002), not at compile time. The grazers' alarm (read damage) and
  trample (deal damage) became two systems (`sim/fauna.rs`). Cost: one
  test run.

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

- 2026-10-12, weapon parts. Thirty-two part variants share their geometry, but a
  `build.py` can't import a sibling module: Blender ignores `PYTHONPATH` and
  `run_build.py` only puts `tools/assets` on the path. Each part's `build.py` adds its
  parent directory to `sys.path` itself (`assets/weapons/parts/*/build.py`). Their
  `asset.json`s are written by a script from one table (`make_parts.py`), since 32
  hand-kept sidecars would drift. Cost: half an hour. The "bevel eats narrow steps"
  warning came back on every barrel (collar and crown steps of 1.5–3 mm under a 2 mm
  bevel); widened to 4 mm.
- 2026-10-12, wave 4 guns (shotgun, sniper, six family emitters; 40 assets).
  There's no way to see an assembled gun outside the game: the previews show
  each part alone, so overall length and fit took a throwaway Blender script
  that hangs part .glbs on a receiver's sockets and renders orthographic views
  (kept at `/tmp/hbcompose/compose.py` during the work; worth bringing in-tree
  as `tools/assets/compose`). The "flat planes have no triangles left"
  warning still doesn't name the plane (every case was a 2–3 mm crown step
  under a 1.5 mm bevel). Lathe end caps sit flush where a lens goes, so glass
  must stand proud of the cap to show. `paint_secondary` tape on a near-black
  secondary paint vanishes: the quick mags use `paint_primary`. Cost: a few
  hours of a modelling agent's time.

## Our gallery (client/gallery.rs)

- 2026-10-12. Laying the gallery out as areas around the start (models
  ahead, robots left, parts right, rolled guns behind), its world labels
  were drawn with `world_to_viewport` positions straight into egui, so with
  the HUD's zoom factor they all landed off their models (the entry under
  egui). Fix: they go through `hud::Projector` like the HUD's markers.
  Then each area's labels showed at the edges of its neighbours' views;
  every label now shows only while you face its area's middle (`FACING`).
  The turret's searchlight shaft is left out in the gallery, where it
  crossed the other areas. Cost: four harness round-trips.

- 2026-10-12. The gallery drew a socket gizmo for every `SOCKET_` node in the
  scenario, so once it showed live robots and weapon racks, those were a thicket of
  axes. Now only the model row's exhibits get them. Cost: one screenshot.

## Headless harness (system audio)

- 2026-10-12. The harness's ALSA null config didn't stop the game playing
  sound on the user's real devices: the system `alsa.conf` hooks load
  `/etc/alsa/conf.d/99-pipewire-default.conf` after anything on
  `ALSA_CONFIG_PATH`. Fix: point `ALSA_CONFIG_PATH` at our config alone
  (`tools/hl`, d46d850). Cost: a session's debugging.
- 2026-10-12. Driving a client in a two-window harness: keys go to the
  focused window (`tools/hl focus`), but clicks go to the window under the
  pointer, which starts on the host's half; the client's shots were the
  host's grapple. Fix: `swaymsg 'seat * cursor set X Y'` onto the client
  before clicking (`tools/coop-smoke`). Focus also wanders, so it's set
  again before each step. Cost: two smoke runs.
- 2026-10-12. Clicking egui menus in the harness: `hl-input`'s pointer is
  relative only, but `swaymsg 'seat * cursor set X Y'` places it
  absolutely, which is enough to click buttons at positions read off a
  screenshot (`tools/coop-smoke`, the armory and workbench). Scrolling
  needed a `wheel` command in `hl-input` (`tools/hl wheel`). Cost: a few
  calibration runs per menu; the positions break when the menus' layouts
  change.

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
  `GlobalAmbientLight` is global, and each camera's sky fill
  (`GeneratedEnvironmentMapLight`, 3500) lit its rooms through the walls
  too: dimming its cabin's light probe alone changed little (the walls
  stayed blue-grey). Fixed by lighting the eye rather than the world: inside
  the cabin of a ship lit by its devices, the ambient light and every
  camera's environment light are scaled to the fraction of that ship's room
  lights that are on, eased across the threshold
  (`client::devices::interior_ambient`). Open space and our own ship are
  untouched. Cost: an hour, most of it finding that the camera's fill, not
  the ambient, was the light. Limit: it's lit as the eye is, so from outside
  you see a dark ship's insides at space's light (fine through one airlock).

## Our player controller (flight assist)

- 2026-10-12. Towing a survivor on a rope, letting go of the keys made
  flight assist brake against the reference body, and the load ran into
  the player at the tow's speed (measured 9.7 m/s). Assist already stood
  aside while a rope was taut ("swinging"), but a coasting tow's rope has no
  tension, so that lasted a tick. Added a tow mode: with a rope on a body
  under 1 t and no input, assist keeps station with the load
  (`sim/player.rs`, `TOW_MAX_MASS`; test `towing_a_survivor_is_steady`).
  Cost: an hour with the measurement. Open: stopping a tow still means
  braking with the load behind you (a rope can't push), which a player
  learns to do slowly.

- 2026-10-12. Even with tow mode, a towing player at full thrust reached
  22 m/s with the lifeboat and couldn't stop. The cause: hooking any
  anchored body made it `reference_body`, and the assist's speed limit and
  brakes are relative to the reference, so the limit rode along with the
  load. Hooking a towable body (under `TOW_MAX_MASS`, not `untowable`) now
  leaves the reference alone (`fire_grapple`; test
  `a_tow_keeps_the_assist_flying_by_our_ship`). Found by playing recovery
  by real inputs; the unit tests set `reference_body` by hand and never
  saw it. Cost: a lost playtest run.

- 2026-10-12. Ropes snapped on tows, silently. The rope solver is nearly
  rigid, so a slack rope going taut takes up the whole speed difference in
  one tick. If that impulse is over `rope_break_force * dt`, the rope
  snaps. Setting off from a lifeboat at full thrust, the rope goes taut at
  about 17 m/s: m_eff ≈ 100 kg × 17 m/s ≈ 1700 N·s against the limit of
  833 N·s. It snaps every time, and so does a survivor's (67 kg × 17). Ropes
  on light loads (under `TOW_MAX_MASS`, not `untowable`) now pull at their
  limit instead of snapping (`light_load` in the EVA rope step). Found with
  a scripted sim pilot (tests/recovery_solo.rs); the tow tests start with
  the rope taut, so they never saw it. A tow also coasts with its load when
  you let go (tow mode), so stopping one takes the brake (X), early. Cost:
  three playtest runs.

- 2026-10-12. Playing scenarios by real inputs in the harness: set
  `HULLBREAKERS_TELEMETRY=<file>` and the client writes the player's view
  about 30 times a second (eye, forward, mode, feet in ship-local
  coordinates, grapple, kit, mission lines, markers with world positions,
  every ship's pose, robots). It writes beside the file and renames over
  it, because readers kept catching it empty. `tools/hl-aim <file> <x,y,z |
  marker:<label> | ship:<design>:<x,y,z>> [--go]` turns the mouse onto a
  point (0.0022 rad per pixel; positive dx turns right, positive dy looks
  down). Traps:
  - Boots on climb crates and walls, onto the ceiling, so walk in short
    straight legs, around crates.
  - In EVA with boots on, you land on the nearest surface, which can be a
    wall or the hull's underside.
  - Releasing W in EVA coasts several metres, so approach slowly and stop
    early.
  - The game's profile is `$HL_STATE/profile-N.ron`, not `./profile.ron`.
  Cost: most of a day.

- 2026-10-12. A shotgun's shove (or a plasma burst's) is an impulse on a
  floating player's velocity, and flight assist (on by default) brakes any
  velocity the keys didn't ask for: a shoved crewmate with assist on stops
  within a second, one with it off tumbles on. That's the intended feel, but
  a test that measures a shove must turn the target's assist off (or it
  reads zero a few ticks later). Robots float with their own brains' assist,
  so a Swarmer's shove fades the same way. Cost: half an hour
  (`tests/families.rs`, `target`).

## Our robots' brains (sim/bots.rs)

- 2026-10-12. The limpet (fauna on the robots' machinery) couldn't get
  back onto a rock: the EVA landing rule takes hold only by the feet
  (the bottom of the capsule), and the winch, reeling it in, stops 1.2 m
  short of its anchor by design (`WINCH_ARRIVE`, for players' assist). So
  it hung off the rock head first, never touching feet first. Fix: a body
  tuning flag, `land_any_side` (a limpet is all foot underneath; it rights
  itself as it boots), and its brain closes the last 2 m itself. Cost:
  two test round-trips. Yaw and pitch alone couldn't turn its feet to the
  rock (no roll for a robot's look); that was the dead end.
- 2026-10-12. Tests that looked up "the" player with `.single()` broke as
  soon as the field had limpets (they're players). Fixed in the movement
  harness (it turns fauna off, as tests of other things turn robots off)
  and the netplay one (it picks the crew). Pattern: robots-as-players
  leaks into anything that counts players.

- 2026-10-12. The hull crawler (walks walls and ceilings) never fired from a
  ceiling at someone below it: `on_target` compared the aim's yaw and pitch
  with the view's, and near the up axis yaw is degenerate, so tiny aim
  changes read as huge errors. Fix: `on_target` is now the angle between the
  view's forward and the aim, for every robot (`think`, src/sim/bots.rs).
  Cost: several test round-trips; walkers' and turrets' cone checks changed
  with it (their tests still pass).
- 2026-10-12. The crawler's climb up a wall oscillated: steering by the
  tangent toward its goal re-picked the wall each tick as its up turned, so
  it rocked at the wall's foot. Fix: when its goal is a ceiling perch it
  commits to a climb (`Crawling::climb`: the wall's direction in its body,
  held for a few seconds) instead of re-deciding. Cost: several test runs.

## Lightyear and Replicon (net/)

- 2026-10-12. Lightyear 0.30 replicates through Replicon, and has no
  per-component, per-client target of its own (`Replicate`,
  `PredictionTarget` are per entity). Keeping a profile to its owner took
  Replicon's own visibility filter (`net::profiles::ProfileOwner`, scoped to
  `(Profile, SavedProfile)`), so `bevy_replicon` is a direct dependency now
  (the same version lightyear uses). The filter can only be registered
  where Replicon's server is in: a pure client app has no `FilterRegistry`
  and panics on `add_visibility_filter`, and in `build` the server plugin
  may not be in yet; it's registered in `ProtocolPlugin::finish`, if the
  registry exists. The filter's client entity is lightyear's link entity
  (what `ControlledBy::owner` holds). An entity without the filter
  component has the scoped components hidden from every client, which is
  what the host's own player wants. Cost: an hour.

## Build cache (cargo, rustc)

- 2026-10-12. Several agents building in their own worktrees filled the
  disk twice in one day. Every dependency built with full debug info made
  each test binary 2.4 GB, and stale test binaries pile up in
  `target/debug/deps` (cargo never removes an old hash). Measures taken:
  line tables only for dependencies (8a88503); `split-debuginfo =
  "unpacked"` for our own code, so debug info stays in the object files
  instead of being copied into every linked binary (1.16 GB → 0.56 GB per
  binary); and cargo's `no-embed-metadata`, which is nightly-only, so the
  repo now pins a nightly in `rust-toolchain.toml`. A full `target/debug`
  is about 20 GB, down from 38 GB. Cost: a nightly toolchain, two full
  rebuilds, and a prune script run by hand (old binaries per test target,
  keep the newest). Stale binaries still accumulate; a `cargo sweep` habit
  or a tool of our own would close that.
