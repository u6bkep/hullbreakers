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
| 2026-10-12 | co-op boarding, a real client, robots as their views | 43.4 -> 18.0 | back to back, same build otherwise; packets 82 -> 62/s |
| 2026-10-12 | co-op field, a real client, robots as their views | 11.6 -> 11.4 | its limpets were already quiet |
| 2026-10-12 | boarding, standing on our ship (`netplay` test) | 37.8 -> 12.9 | asserted under 20 |

What was wrong: every robot's `Player` was marked changed every tick (its
event log counts the tick; booted on a turning rock, its feet re-seat by a
few micrometres), and so was every robot's and creature's `Vitals` (the
hurt clock) and `PlayerTelemetry` (rewritten zero). A changed component is
re-sent. Now a robot whose state is within a millimetre of what was last
sent keeps its old change tick (`bots::quiet_idle_robots`, since replaced:
see below), only the crew's
hurt clock is replicated (their HUD shows it), telemetry is written only
when it differs, and a grazer kept bolting isn't rewritten (`fauna::startle`).

**Robots as what's drawn of them** (`net::robots`). A moving robot sent its
whole `Player` (216 B on the wire) every tick, for what a client draws and
hears. Now `Player` isn't replicated for robots at all; the authority sends
a `RobotView` (mode, rope, boots, beams; quantised to mm, cm/s and 1/32767:
57 B) when it moves by more than that, and a `RobotLog` (the last half
second of its event log) when there's a new entry. A remote client builds
the robot's `Player` from them, so everything that draws, hears or aims at
robots reads it as before; the host reads the real one.

Checked on a client at 100 ms (2026-10-12): `tools/coop-smoke` with
`COOP_ONLY=watch_gunners,watch_grappler,watch_crawler,watch_turret,watch_marksman,field_limpet,gallery_shielded,gallery_volatile`
(host and client views side by side), and in `netplay`: still robots sit
within 5 mm of the server's, every shot reaches the client's log
(`a_robots_shots_reach_a_client`).

**Fast robots by a moving ship** (2026-10-12, `netplay::robot_smoothness`).
The carrier's robots in EVA, flying with it at 27 m/s, were drawn 7.6-7.8 m
from their place by it on a client, and stepped unevenly (jolts up to
527 mm a frame). Two causes. A client interpolated them 1.2 send intervals
behind the authority's latest (lightyear's default; the authority sends
every third tick), which wasn't enough: the next sample was often not in
when it was needed, so lightyear held the last one (it won't extrapolate)
and then caught up. Now 2 send intervals (`INTERPOLATION_SEND_INTERVALS`,
40 ms more behind): mean jolt 56-87 mm -> 4 mm in the world. And a robot was
interpolated in the world, so drawn where it was an interpolation delay
ago, while its ship is drawn where it is now (from its extrapolated motion):
at 27 m/s, metres behind. Now a robot in EVA is posed in its reference
body's frame on the wire (`RobotView::frame`), blended there, and drawn on
the body as drawn (`DrawnIn`), as one on a deck already was: 0.13-0.17 m
from its place by the ship (its own motion relative to the ship over the
delay), jolts under 25 mm. Asserted: under 0.6 m and 100 mm.

What's left in co-op is mostly the other crew members: each one's whole
`Player` (loadout, input cursor, event log) goes every tick, since the log
counts the tick. Splitting that the same way is harder (their own client
predicts from the full state, and the others interpolate it), and at 11 kB/s
for two players it's not yet worth it.

## Big fields (the race, 2026-10-14)

The grapple run's field is a `RockField` grown from a seed on every peer
(its anchors static, the rest dynamic and asleep until knocked; it was all
static and sent rock by rock when the first numbers below were taken),
sized by `HULLBREAKERS_RACE_ROCKS` (default 6000), measured with
`tools/stress race` (the player flown round the line at 40 m/s) and
`coop-race`. Load 15-20.

**The sim alone** (`tests/rock_scaling.rs`, ignored; headless, release, mean
ms per tick, 0.35-0.42 with no rocks):

| Rocks | Static | Dynamic, asleep | Kinematic, spinning | Dynamic (the field's kind) |
|---|---|---|---|---|
| 300 | 0.42 | 0.47 | 0.64 | 0.62 |
| 2000 | 0.70 | 0.73 | 1.58 | 1.60 |
| 8000 | 0.96 | 1.06 | 3.13 | 3.59 |

A static rock costs Avian nearly nothing a tick; a moving one about 0.4 µs
(its solver body, its box, the broad phase). Substeps barely matter at 300
rocks (0.62 / 0.65 / 0.74 ms at 1 / 2 / 6). A slowly spinning rock that's
allowed to sleep does, and stops turning: set `SleepingDisabled` on rocks
meant to turn. Note the sim alone at 300 dynamic rocks is 0.6 ms, against
the field's 1.5-2.1 ms in game above: the rest is the game's (lightyear,
fauna, the frame around the tick), not the rocks.

**In game** (1920x1080, release, uncapped):

| Rocks | fps | frame p99 ms | tick ms | visible meshes | visible tris |
|---|---|---|---|---|---|
| 1000 | 256 | 6.1 | 1.22 | 427 | 96k |
| 2000 | 250 | 5.9 | 1.31 | 635 | 179k |
| 4000 | 238 | 6.5 | 1.51 | 1065 | 346k |
| 8000 | 208 | 7.5 | 1.94 | 1899 | 672k |
| 16000 | 162 | 10.0 | 2.93 | 4055 | 1.5M |
| 32000 | 105 | 15.6 | 4.74 | 7868 | 3.0M |

Drawing and simulating a field isn't the limit at any size we'd want. What
is, is **making one**: each rock's hulls take about 0.2 ms to build, and a
scenario's thousands were built one by one in the frame they arrived. The
host stalled 3.5 s at 8000 rocks starting the race (traced), and a joining
client as long, long enough to time out. Fixed: the hulls are built in
parallel (`asteroids::shape_asteroids`). And the field reached a client as
one burst, overflowing its UDP receive buffer (see TheBook, lightyear):
paced now (`net::SEND_QUOTA`, 1 MB/s; not in the netplay rig, whose clock
is mocked). And the race's rocks are immovable to the sim, not only to
Avian: as dynamic mass on a static body, a bump left a rock a velocity
nothing cleared, re-sent to clients as drift (`Asteroid::fixed`).

**Co-op, joining a race in progress** (localhost, both fixes):

| Rocks | client stall on joining | the whole field there |
|---|---|---|
| 2000 | 0.6 s | yes |
| 6000 (the race's) | 0.64 s | yes; then 14.8 kB/s to it, the host flying the line |
| 8000 | 0.85 s | yes |
| 16000 | 1.5 s | yes |
| 32000 | 3 s, then timed out | no |

So about 16000 rocks was the most a co-op field could be while it was sent
(over a real link, slower: a rock is about 160 B on the wire, so 6000 were a
second at the pace).

**Grown, not sent** (2026-10-14, later). The field reaches a client as its
`RockField` (a layout, a seed, a count: a few bytes on the course's site)
and the client grows the same rocks itself. Each rock carries a Replicon
`Signature` from the field and its index, so when the authority does
replicate one (the first time a player stands on it, ropes it or rides it,
or it's knocked adrift: `net::replicate_touched_rocks`) it maps onto the
client's own copy, and a player's reference to it resolves to the rock the
client's prediction already chose. Measured in the netplay rig: the race's
start costs a client 10 kB (was about 960 kB), a rope to a grown anchor
rolls nothing back, and a knocked rock drifts on the client's copy. What a
joining client waits on now is its own CPU: the hulls (about 0.2 ms each
across its cores) and the meshes, built a batch of `SHAPE_PER_FRAME` (200)
rocks a frame so the frame stays alive, under a veil with a bar
(`client::hud::loading`), which also covers joining a host. Traced (32,000
rocks, release): a batch of 1000 was 33 ms to build and about 90 ms to
apply (Avian's collider-tree and mass hooks, our mesh observer), and a
budget per *tick* bounded nothing, since the fixed loop caught up with 24
ticks inside three frames (one 2.5 s long): hence per frame.

## Bigger work (not done; numbers above)

- **Rock colliders.** Withdrawn (2026-10-14): a rock's hulls aren't 640
  points each but about 100 (parry drops the interior points of the noisy
  sphere), ico2 gives 96, and it moved the tick 5% (within noise). The
  field's tick isn't mostly the rocks either (see Big fields). REJECTED:
  coarser rocks, which would cost the look for nothing measurable.
- **Shadow casters.** The sun's cascades cost about 1 ms of CPU a frame
  aboard the carrier, over 1690 casters, many of them small parts (robot
  pieces, affix parts, frond segments, gun parts). Marking small or
  interior-only meshes `NotShadowCaster` would cut it; it's a look call
  per kind of thing.
- **The fixed loop under load: decided against a cap** (2026-10-12). When a
  frame runs long the next runs several ticks to catch up. That catch-up is
  what keeps the authority's tick on the wall clock, and the authority's
  tick is every peer's clock: each client runs its own timeline ahead of
  the authority's estimated tick (lightyear's sync slews its speed for small
  errors and shifts it by whole ticks for large ones). An authority that
  dropped time under load would fall behind every client's estimate, and
  each would slew or snap back (rollbacks, a jump in its own player's view)
  until it re-estimated. A client that capped would fight its own sync,
  which would only speed it up again or skip it ahead. So there's no cap
  beyond Bevy's own: `Time<Virtual>`'s 250 ms max delta (at most 15 ticks a
  frame) already bounds the spiral, and below that, keeping up is the
  point. The fix for a loaded or slow machine is a cheaper tick (see rock
  colliders). One case could take a tighter cap: an authority with no
  remote clients (playing alone), where no other clock is watching; it
  isn't done, since a client can join at any time and it would be a second
  timing mode to keep right.
