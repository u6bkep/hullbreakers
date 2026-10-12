# Performance

Where the time and the bytes go, measured, so later work can compare. Add a
dated row when something moves a number; keep the old rows.

## How to measure

- **Frame time, GPU, per-system CPU:** `tools/stress` (see its header):
  release builds in the headless harness at 1920x1080, a warmup then a
  measured window, one JSON per run and a summary table; `--trace` adds
  per-system time from a `profile-trace` build. Situations: `gallery`,
  `field`, `boarding` (on our ship), `carrier` (aboard the carrier, firing,
  its crew coming), `derelict` (lit), `firefight`, `coop`, `coop-boarding`.
- **Bandwidth:** an authority's report has what it sent each remote client
  (`sent_per_client`: payload bytes before the transport's headers; the
  `coop` situations have one). In tests, `Session::received_rate` in
  `tests/netplay.rs`, and the ignored `replicated_changes` there counts how
  often each replicated component is re-marked changed (each mark sends it
  again): `cargo test --test netplay replicated_changes -- --ignored --nocapture`.

**Noise.** The machine these were taken on is shared (other agents build and
test on it: load average 12 to 45 on 32 cores during this audit), and a
loaded run is far worse than an unloaded one (the same boarding run: 196 fps
one time, 31 the next). Compare runs taken back to back, repeat them, and
read `/proc/loadavg` alongside; the first "before" table below was taken under
heavy load and overstates every cost.

## Frame time (1920x1080, release, RX 6800 XT)

2026-10-12, after the fixes below, at load about 12-17 (vsync: as shipped;
novsync: uncapped, for headroom). Tick = the fixed loop's time per tick
(lightyear, the sim and Avian); GPU = Bevy's instrumented passes.

| Situation | vsync fps / p99 ms | novsync fps / p99 ms | tick ms | GPU ms | Notes |
|---|---|---|---|---|---|
| gallery | 62 / 16.2 | 152 / 8.5 | 1.2 | 1.8-2.3 | 3000 visible meshes, 343k tris |
| field (with fauna) | 62 / 16.2 | 196 / 12.6 | 1.5-2.1 | 2.5-4.3 | 300 rocks, 9 limpets, 16 grazers |
| boarding, on our ship | 62 / 16.2 (x3) | 80-196 | 1.4 | 3.5-4.8 | |
| aboard the carrier, firing | 62 / 16.2 | 132 / 28 | 1.4 | 4.0-9.0 | 1300 visible meshes, 29 point lights |
| the derelict, lit | 62 / 16.3 | 246 / 6.1 | 0.9 | 2.2-5.7 | |
| co-op field, host / client | 62 / 16.3 both | 198 / 217 | 1.2-1.9 / 0.45 | 0.7-3.9 | |
| co-op boarding, host / client | 62 / 16.3 both | 162 / 156 | 1.7-1.9 / 0.5-0.8 | 1.3-3.3 | |

Before, 2026-10-12 (6 substeps, idle robots re-sent), novsync only, taken
under heavy load (25-45):

| Situation | fps / p95 / p99 ms | tick ms | GPU ms |
|---|---|---|---|
| gallery | 146 / 9.7 / 10.9 | 1.4 | 2.2 |
| field | 82 / 33 / 47 | 6.1 | 3.1 |
| boarding, on our ship | 49 / 64 / 106 | 7.1 | 3.9 |
| aboard the carrier | 74 / 50 / 108 | 5.9 | 5.5 |
| the derelict, lit | 204 / 7.2 / 9.8 | 1.4 | 2.9 |
| co-op field, host / client | 136 / 126 | 3.1 / 1.1 | 1.3 / 2.6 |
| co-op boarding, host / client | 143 / 165 | 2.6 / 0.7 | 1.5 / 1.8 |

Everything holds 60 fps with room to spare when the machine isn't loaded.
Under load the fixed loop spirals (a long frame queues several ticks into
the next, which runs long, and so on), worst where ticks cost most (the
field, boarding): that's what the "before" table's 49 fps and 100 ms p99 in
boarding were.

**Where a tick goes** (traced, field): Avian's step is most of it, 2.3 ms a
tick with 6 substeps over 300 dynamic rocks (its substep loop, integration,
and `update_solver_body_aabbs`: a rock's collider is up to three hulls of an
ico3 sphere, about 640 points each, and a hull's box is found from all its
points every step). Ours is small: `player_tick` 0.14 ms, `think` 0.05,
`recycle_field` 0.09, `track_motion` 0.04, the fauna less.

**Where a frame goes** (CPU, outside the tick, carrier): the render thread
(mostly waiting on the GPU and vsync), light clustering 0.9 ms (29 point
lights), the sun's shadow cascades about 1 ms (`specialize_shadows`,
`queue_shadows`, `check_dir_light_mesh_visibility` over 1690 shadow
casters), transform propagation 0.5, egui 0.5. On the GPU, the main opaque
pass (2-5 ms) is nearly all of it.

Checked and fine: every light has a range; only the sun (and a headlamp's
spot) casts shadows; flashes and spores despawn; nothing accumulates over a
run (entity, mesh and light counts hold steady in every situation).

### Fixes

| Change | Before | After | How measured |
|---|---|---|---|
| Avian substeps 6 -> 2 (`sim::PHYSICS_SUBSTEPS`) | tick 2.3 ms field, 1.9 boarding | 1.8, 1.4 | interleaved A/B, 2 reps each, `HULLBREAKERS_SUBSTEPS` |
| An avatar's transform written only when it moves | (re-propagated every model every frame) | idle ones not | by construction |
| Idle robots and creatures not re-sent | 43.7 kB/s field | 12.8 | below |

The substeps: Avian's default buys stiff joints and stacks, and this world
has neither (rocks and creatures in free flight that sometimes collide;
ropes and boots are our own code, outside the solver). The full suite
passes at 2.

## Bandwidth (to one client)

| Date | Situation | kB/s | Notes |
|---|---|---|---|
| 2026-10-12 | field, standing, before fauna | 6.3 | (items agent, `a_held_laser_is_cheap_on_the_wire`) |
| 2026-10-12 | field, standing, with fauna | 43.7 | 9 limpets 33.4, 16 grazers 4.8 |
| 2026-10-12 | field, standing, after | 12.8 | idle robots and creatures not re-sent (below) |
| 2026-10-12 | field, a held laser on top | +5.4 before, +0 after | |
| 2026-10-12 | boarding, standing on our ship, after | 37.8 | about 9 robots moving or aiming every tick |
| 2026-10-12 | co-op field, a real client (`sent_per_client`) | 15-20 | two players' own states, the field's fauna |
| 2026-10-12 | co-op boarding, a real client | 44-47 | |

What was wrong: every robot's `Player` was marked changed every tick (its
event log counts the tick; booted on a turning rock, its feet re-seat by a
few micrometres), and so was every robot's and creature's `Vitals` (the
hurt clock) and `PlayerTelemetry` (rewritten zero). A changed component is
re-sent. Now a robot whose state is within a millimetre of what was last
sent keeps its old change tick (`bots::quiet_idle_robots`), only the crew's
hurt clock is replicated (their HUD shows it), telemetry is written only
when it differs, and a grazer kept bolting isn't rewritten (`fauna::startle`).

Still big: a robot that moves sends its whole `Player` (loadout, event log
and all) at the send rate, about 170 B an update, for what a client draws
from its pose and a few flags. See "Bigger work".

## Bigger work (not done; numbers above)

- **Robots' replication.** A moving robot's whole `Player` goes at the send
  rate: boarding is 37-47 kB/s a client, most of it the carrier's crew
  turning and strafing (the turrets sweep all the time). A slim replicated
  pose for robots (mode, look, grapple; the loadout and event log only when
  they change) would cut that several-fold. It touches how robots are
  interpolated and drawn, so it wants its own change.
- **Rock colliders.** The field's tick is mostly Avian over 300 rocks, and
  a rock's box is found from every point of its hulls (about 640 each)
  every step. Fewer points per hull (ico2: 162) would cut that about 4x,
  but rocks are drawn from their colliders, so they'd look coarser: an art
  call. Alternatively draw from the fine shape and collide with a coarse
  one.
- **Shadow casters.** The sun's cascades cost about 1 ms of CPU a frame
  aboard the carrier, over 1690 casters, many of them small parts (robot
  pieces, affix parts, frond segments, gun parts). Marking small or
  interior-only meshes `NotShadowCaster` would cut it; it's a look call
  per kind of thing.
- **The fixed loop under load.** When a frame runs long the next runs
  several ticks; capping catch-up (Bevy's `Time<Fixed>` max delta) would
  stop the spiral at the cost of the sim running slow for a moment. It
  matters only on a loaded machine (or a slow one): worth deciding with the
  network in mind (the authority's tick is the clock).
