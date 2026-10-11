# Hullbreakers

First-person co-op boarding-combat roguelite, early days (provisional name). Bevy 0.19 + Avian 0.7.

```
cargo run                                # host a game and play (UDP port 5888)
cargo run -- --connect 192.168.1.20      # join a host (HOST[:PORT])
cargo run -- --server [--port 5888]      # dedicated headless server
cargo test                               # movement, client-world and netplay tests
HULLBREAKERS_SHOTS=dir cargo run           # scripted screenshot tour, then exit
HULLBREAKERS_SCENARIO=boarding cargo run   # host straight into a scenario
```

## Scenarios

The crew's ship has a **mission wall** on the port side of the bridge: click
a button (F, within reach) to start a scenario. Every switch rebuilds the
world and puts everyone back on the deck. The game starts in the
**asteroid field**, and its button takes you back there. `HULLBREAKERS_SCENARIO=boarding` (or `gallery`) starts in another.

- **Boarding action**: our ship and an enemy **carrier** duel among rocks,
  the carrier weaving in to about 100 m and back out to 400 m every 60 s.
  Laser turrets on both ships fire slow, shielded beams: our hull lasts
  about 3.5 minutes. The carrier is 60 m long with two decks: get in
  through the hangar door at its stern, climb the ramp up the hangar's port
  wall, and fight forward along the upper deck to the bridge at the bow,
  where the shield console is. Our beams then finish it in seconds, so get
  clear: it breaks apart and throws boarders into space. Beams shove and
  burn anyone in EVA who crosses them: three hits kill. Lose our ship and
  the crew drifts a while, then returns to the field.

- **Model gallery** (debug): every model under `assets/` in a row behind
  our ship, turning slowly, labelled with its path and size, sockets drawn
  as small axes (red +X, green +Y, blue forward). You start floating in
  front of it.

You start (and respawn) with the base kit: your suit and its grapple. Guns
are in the **weapons locker** on the starboard wall of our ship's aft hold
(press it with F for an SMG and a rifle, full; again to top up). Die, and
what you carried stays where you fell, in a glowing case on the deck (or
drifting on, if you were in EVA): look at it and press F to take it back.

Armed, players carry an SMG and a rifle. Each hand holds one item and a stance
(1-4) sets what's in each; a one-handed gun with the other hand empty is
braced and shoots tighter. Bullets fly at realistic speeds and keep the
shooter's velocity, and firing in EVA pushes you back. Only the grapple's
hand works its rope; stow it and the rope holds its length.

The carrier is crewed by robots: seven **Gunners** posted from its hangar to
its bridge (walkers with rifle arms that keep their distance and strafe)
and six **Swarmers** flying escort (fast, fragile
drones that rush in and spray). Robots are players driven by a brain on the
host, so they move, aim and shoot by the crew's rules; they see along clear
sight lines, chase a target that ducks out of view for a few seconds, and
are destroyed at zero health. Each has a **weak spot**, a glowing magenta
core (the Gunner's backpack, the Swarmer's rear reactor) that takes 2.5×
damage: flank them. Hit markers show your hits (white), weak-spot hits
(magenta) and kills (an X); robots don't heal. The F1 panel can spawn a Gunner on our bridge
or three Swarmers off our port side, for trying out a fight.

Players have suit health (bottom of the screen). It comes back after a few
seconds unhurt; at zero you respawn aboard the home ship.

Co-op is host-authoritative: the host (or server) simulates everything and
replicates it. Its tuning is authoritative too; each player keeps their own
camera and debug settings.

### Headless harness (for agents and CI)

`tools/hl` runs the game in a GPU-accelerated headless sway (its own Wayland
socket, no seat, renders on `HL_RENDER_NODE`, default `/dev/dri/renderD128`),
drives it through a persistent virtual keyboard and pointer (`tools/hl-input`,
real US keycodes and relative motion), and captures frames with grim. Needs
`sway grim` and `xkbcli` (libxkbcommon).

```
tools/hl start && tools/hl game
tools/hl look 300 0 10 300      # turn right, spread over 300 ms
tools/hl down w; sleep 1.5; tools/hl up w
tools/hl click left             # grapple
tools/hl shot /tmp/frame.png
tools/hl stop
```

Games launched by `tools/hl` play into an ALSA null device, not your speakers.

### Performance runs

`tools/stress` builds the game (release and dev), runs scenarios (`gallery`,
`field`, `boarding`, `firefight`: 30 robots fighting a player who never
stops firing; `coop`: host plus client) in a harness of its own on port
5977, with vsync on and off, and prints a table: frame time percentiles,
fixed-tick time, GPU utilisation and power, CPU use, and counts. A traced
build (`--features profile-trace`) gives per-system time. `--ab` prices
rendering features by switching them off (`tools/stress --help`). The game
side is `HULLBREAKERS_PROFILE=<dir>` (`src/client/profile.rs`), off by default.

Run `tools/hl game` again (e.g. `tools/hl game --connect 127.0.0.1`) for more
instances; sway tiles them side by side (`tools/hl start 2560x720` gives each
a 1280-wide half). `tools/hl focus <title>` picks which one gets input; the
client's title is `Hullbreakers — HOST:PORT`.

## Controls

| Input        | Mag boots        | EVA                   |
|--------------|------------------|-----------------------|
| Mouse        | look             | look (pitch past vertical tumbles the body) |
| WASD         | walk             | thrust                |
| Space / C    | jump / —         | thrust up / down      |
| Shift        | sprint           | boost                 |
| Q / E        | —                | roll                  |
| B            | toggle mag boots | toggle mag boots (on = land on contact) |
| V            | toggle flight assist (on by default) ||
| T            | set reference body (what assist/brake match) to the one under the crosshair ||
| X (hold)     | —                | brake relative to last body touched/grappled |
| F            | press the button under the crosshair (within reach) ||
| LMB / RMB    | use the main / off hand's item (gun: fire; grapple: fire or let go) ||
| Ctrl+LMB / Ctrl+RMB (hold) | that hand's alternate: for the grapple, reel in ||
| Alt+LMB / Alt+RMB (hold)   | that hand's second alternate: for the grapple, pay out ||
| Mouse4 / Mouse5 (hold) | reel in / pay out, whichever hand holds the grapple ||
| R            | reload (main hand first; a dry gun reloads itself) ||
| 1-4          | stance: SMG + grapple, rifle + grapple, rifle braced, grapple alone ||
| F1           | tuning panel (frees the mouse)      ||
| Escape       | free the mouse                      ||
| L            | debug: ship lurches forward         ||

These are the defaults. To remap them, write a `keybinds.ron` next to the game
listing only the actions you change, each with up to two bindings. A binding
is a key or mouse button with optional modifiers (Shift, Ctrl, Alt, Tab,
CapsLock):

```ron
{ "ReelIn": ["RMB", "Ctrl+LMB"], "ToggleBoots": ["G"], "Interact": ["F", "MMB"] }
```

The action names are in `src/client/bindings.rs`. When chords share a button,
the most specific one whose modifiers are held wins, so Ctrl+LMB reels without
also firing. Modifiers that no chord on that button uses are ignored, so Shift
(sprint) plus a click still fires.

Walk straight into a wall to climb onto it; walk off an edge to wrap around it.
Land feet-first, slowly (< `attach_max_speed`), with boots on, to stick.

## Architecture

- `src/sim/` is headless (would run on a server); nothing in it touches
  rendering or devices. Each tick is a function of state, input, and the tick
  number, so it can be re-run (network rollback) and reproduces bit-for-bit:
  - **Input** (`sim/input.rs`) is absolute: look is a running total and button
    presses are counters. The sim applies the difference since its
    `InputCursor`, so a repeated input is a no-op and a skipped one catches up.
  - **Events** (`sim/events.rs`) are tick-stamped entries in each player's
    `EventLog`, not messages. A re-simulated tick rewrites its own entries.
    The client presents each one once (`client/events.rs`).
  - Time-dependent behaviour (ship wobble, lurch, turret fire, scripted
    flight) derives from `SimTick`. Orders to a drifting ship arrive as
    `ShipCommand` messages.
  - **Scenarios** (`sim/mission.rs`) are the authority's: button presses are
    player events (`Pressed`) that the `Director` acts on after the players
    move; a switch despawns the world, spawns the scenario's, and respawns
    every player on deck (`Respawned`, a view relocation). `Mission` is
    replicated for the HUD.
  - **Combat** (`sim/combat.rs`): which turrets fire is a function of the
    tick, and where a beam stops is a function of the ships' poses and the
    target's shield, so every peer traces the same `Beams` each tick and a
    client predicts being shoved by one. Only damage is the authority's.
- `src/client/` is input, camera, greybox rendering, the HUD, and the tuning
  panel. It presents the `LocalPlayer` and writes only `LocalInput`.
- `src/net/` is lightyear: `--host` (listen server, the default), `--connect`,
  `--server`. Replicated state is the sim's own: generating components that
  every peer expands locally (`ShipHull` builds the hull, `Asteroid { seed }`
  the rock's shape), each body's `Motion`, and `Player` state, whose event
  log carries what happened.
- **Prediction**: a client simulates its own player ahead of the server
  (`PlayerSimPlugin`: a player with `PlayerInput` is simulated wherever it
  is) against bodies posed at the same tick from their motion, and rolls
  back when the server's state differs beyond tolerance (`player_diverged`).
  The camera blends over corrections (`ViewCorrection`). Other players are
  interpolated. `HULLBREAKERS_LAG=ms[,loss%]` simulates a bad link on a client;
  clients log their rollback rate every 10 s.
- **Netplay tests** (`tests/netplay.rs`) run a server and a client in one
  process over loopback UDP with 100 ms simulated latency, on lightyear's
  mock clock (a few seconds for all of them). They measure rollback rates
  (walking 0/s, bouncing off rocks one per contact, reeling a small rock in
  ~2.7/s since the rock's recoil isn't predicted) and check that forcing a
  rollback on every update changes nothing.
- **Extrapolated bodies** (`sim/motion.rs`, `net/extrapolate.rs`): a body is
  sent as its free flight at a tick (center of mass, rotation, velocities),
  which every peer evaluates at its own tick, so clients see bodies where
  they are now. The authority re-sends a body's motion only when the body
  strays from it by more than a centimeter (a collision, an impulse, the
  ship's scripted manoeuvres); clients blend over the correction. Rocks get
  isotropic inertia so free tumbling is torque-free and exactly predictable.
  Measured: ~16 KB/s to one client (was ~400 KB/s streaming poses), about 3
  motion updates/s for the whole field.
- **Scripted flight** (`sim/flight.rs`): a ship in a fight flies a path that
  is a pure function of the tick. Every peer poses it from its `Flight`
  (sent once), as a static body moved before each physics step with the
  path's velocities, so it is exactly where the authority has it and a
  client standing on it predicts itself without corrections (measured: 0
  rollbacks/s at 100 ms). Hits shake it, gently: a shake is an
  acceleration at the boots.
- **Body mass** (`BodyInertia`) is derived from the collider identically on
  every peer, so clients have real reference frames without simulating.
- **Reference frames** (`sim/frame.rs`): a booted player is simulated in the
  local coordinates of the body underfoot, so a rolling ship is a stable floor.
  Boot adhesion and shear are checked against the body's acceleration at the
  feet (including centripetal), so hard maneuvers can rip you off. Every
  transition (jump, landing, rope yank, stepping between bodies) goes through
  `BodyFrame` so world momentum is continuous.
- **World frame**: in the field, the ship is (on average) at rest and the
  asteroid field streams past. Physics is Galilean-invariant, so this is the same as flying
  through a static field, but it keeps coordinates small.
- The player is a kinematic capsule driven by Avian's move-and-slide and
  spatial queries, not a rigid body. EVA contacts exchange impulses with rocks
  using their real mass and inertia.
- Camera: the eye is placed relative to the body's interpolated transform (no
  jitter against the deck); events that relocate the view (landing, edge
  wraps) are smoothed with a decaying pose offset.

## Feedback

- Sounds are synthesized at startup (`client/sfx.rs`) and heard through the
  suit (non-spatial): footsteps, landing/impact clanks, release, jetpack hiss,
  grapple fire/attach/release, reel whine under load, rope twang, rip-off warning.
- Rope: grey and sagging when slack, straight pale gold when taut, orange to red
  under load with dashes running in while reeling; load gauge under the crosshair.
- Crosshair ring + distance when a surface is within grapple range.
- Camera shake on impacts and rip-offs, a dip on landing, FOV kick on boost.

## Tuning

All feel parameters are in `sim/tuning.rs` and appear in the F1 panel. "Save"
writes `tuning.ron` in the working directory, which is loaded at startup.
