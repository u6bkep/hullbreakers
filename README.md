# space-game

First-person movement sandbox for a boarding-combat roguelite. Bevy 0.19 + Avian 0.7.

```
cargo run              # play
cargo test             # headless movement tests
SPACE_GAME_SHOTS=dir cargo run   # scripted screenshot tour, then exit
```

## Controls

| Input        | Mag boots        | EVA                   |
|--------------|------------------|-----------------------|
| Mouse        | look             | look (pitch past vertical tumbles the body) |
| WASD         | walk             | thrust                |
| Space / Ctrl | jump / —         | thrust up / down      |
| Shift        | sprint           | boost                 |
| Q / E        | —                | roll                  |
| F            | toggle mag boots | toggle mag boots (on = land on contact) |
| V            | toggle flight assist (on by default) ||
| T            | set reference body (what assist/brake match) to the one under the crosshair ||
| X (hold)     | —                | brake relative to last body touched/grappled |
| LMB          | fire / release grapple              ||
| RMB (hold)   | reel in                             ||
| R (hold)     | pay out rope                        ||
| Tab          | tuning panel (frees the mouse)      ||
| L            | debug: ship lurches forward         ||

Walk straight into a wall to climb onto it; walk off an edge to wrap around it.
Land feet-first, slowly (< `attach_max_speed`), with boots on, to stick.

## Architecture

- `src/sim/` is headless (would run on a server). Input arrives as a
  `PlayerCommand` component; nothing in `sim` touches rendering or devices.
- `src/client/` is input, camera, greybox rendering, the HUD, and the tuning panel.
- **Reference frames** (`sim/frame.rs`): a booted player is simulated in the
  local coordinates of the body underfoot, so a rolling ship is a stable floor.
  Boot adhesion and shear are checked against the body's acceleration at the
  feet (including centripetal), so hard maneuvers can rip you off. Every
  transition (jump, landing, rope yank, stepping between bodies) goes through
  `BodyFrame` so world momentum is continuous.
- **World frame**: the ship is (on average) at rest and the asteroid field
  streams past. Physics is Galilean-invariant, so this is the same as flying
  through a static field, but it keeps coordinates small.
- The player is a kinematic capsule driven by Avian's move-and-slide and
  spatial queries, not a rigid body. EVA contacts exchange impulses with rocks
  using their real mass and inertia.
- Camera: the eye is placed relative to the body's interpolated transform (no
  jitter against the deck); sim discontinuities (landing, edge wraps) are hidden
  with a decaying pose offset.

## Feedback

- Sounds are synthesized at startup (`client/sfx.rs`) and heard through the
  suit (non-spatial): footsteps, landing/impact clanks, release, jetpack hiss,
  grapple fire/attach/release, reel whine under load, rope twang, rip-off warning.
- Rope: grey and sagging when slack, straight pale gold when taut, orange to red
  under load with dashes running in while reeling; load gauge under the crosshair.
- Crosshair ring + distance when a surface is within grapple range.
- Camera shake on impacts and rip-offs, a dip on landing, FOV kick on boost.

## Tuning

All feel parameters are in `sim/tuning.rs` and appear in the Tab panel. "Save"
writes `tuning.ron` in the working directory, which is loaded at startup.
