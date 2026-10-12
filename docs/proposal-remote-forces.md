# Proposal: forces from outside the predicted player

*Status: a proposal for the user to rule on. Nothing here is built. It
touches ratified netcode: host-authoritative, the client predicts its own
player, everything else interpolated or extrapolated, coupled-body
prediction deferred until measured, and presentation reading tick-stamped
event logs without gating on rollback.*

## The problem

More and more pushes on a player come from outside their own input. These
include shot shoves (strong now, with the shotgun and plasma), volatile
robots' blasts, Grappler yanks, a crewmate's rope, beams on EVA players, and
towing. The client's prediction knows none of them except the beams. Beams
are a pure function of the tick and the ships' poses, so they're already
predicted (0 rollbacks/s standing on the duelling ship). For the rest the
client finds out a round trip later, rolls back, and the camera blends over
the jump (`ViewCorrection`, 0.12 s).

## Measured (netplay rig, 100 ms simulated latency, 40 kg player)

The tests are `measure_*` in `tests/netplay.rs`. They're ignored by default;
run them with `cargo test --test netplay measure_ -- --ignored --nocapture`.
A correction is how far the predicted player jumps off its own straight-line
flight on a rollback frame.

| Push | Rollbacks | Worst correction | Total |
|---|---|---|---|
| Shotgun shell, all 8 pellets (100 N·s, 2.5 m/s) | 1 | 0.55 m | 0.55 m |
| Volatile Grappler blast at 3 m (450 N·s, 11 m/s) | 1 | 2.62 m | 2.62 m |
| Rope yank: a robot winches 12 m → 4 m at 15 m/s | 14 in 3 s | 3.00 m | 6.20 m |
| Reeling a light rock on your own rope (like towing) | 2.0 /s while reeling | (existing test) | |

Two shapes emerge:

- **A discrete kick costs one rollback.** Its correction is about
  Δv × 0.23 s: the round trip plus the 50 ms send interval. A shotgun hit is
  mild; a big blast moves you 2–3 m in a blink.
- **A sustained force costs a stream of rollbacks**: the yank's 14, and the
  tow's 2/s. While it lasts, every update disagrees.

CPU isn't the issue. A rollback re-simulates one player for about 14 ticks.

## Options

### A. Accept corrections; present them better (no netcode change)

- Scale the camera's blend with the correction's size: about 0.12 s for half
  a metre, up to 0.3 s for metres.
- Start the cosmetic reaction at once, from what the client already has:
  - a hit or blast in the replicated `HitLog` or event log gives a camera
    kick, a sound and a flash, at the client's first knowledge of it;
  - so the felt jolt and the corrected position line up.

| | |
|---|---|
| Bandwidth | none |
| CPU | none |
| Code | small (`client/camera.rs`, `client/events.rs`) |
| Feel | kicks feel right. A big blast still visibly slides you 2–3 m, though under a blast's flash and shake. Rope yanks stay rubbery: 14 corrections in 3 s, each up to 3 m. |

### B. Predict forces whose source the client already has

1. **Ropes held by someone else.** The far end's player state (robot or
   crewmate) is already replicated, rope included (target, length, tension).
   - The client solves that rope on its own player each tick, using the far
     end's motion extrapolated to the predicted tick (its last known
     velocity, as bodies are extrapolated today).
   - The first round trip of a yank is still a surprise, so expect about one
     rollback per hook.
   - After that the rope is predicted, and only the holder's changes of mind
     (reel or pay out) mispredict.
   - Expected for the yank above: about 14 rollbacks → 1–3.
2. **The body on your own rope** (towing, reeling a rock). Predict that one
   body's recoil as well: a single coupled body with no AI, its motion kept
   in the rollback history. This is the "coupled-body prediction" the
   memory file defers until measured; the measurement is the 2.0/s above.

| | |
|---|---|
| Bandwidth | none (everything used is already replicated) |
| CPU | a rope solve per incoming rope per tick, plus re-simulating one body on rollback |
| Code | moderate. B1 is the existing `receive_rope` path fed by a replicated far end. B2 needs that body's motion in lightyear's prediction history. |
| Feel | ropes, the most frequent and longest-lasting case, stop fighting you |

### C. Impulses stamped for a future tick

When the authority decides a kick at tick T, it doesn't apply it at T. It
applies it at T + D, where D is the target player's lead (about the round
trip), and sends it at once as a stamped entry the target's own simulation
reads. Both peers apply it on the same tick, so it can't mispredict. This is
the event-log model applied to forces: data stamped with a tick, which a
re-simulation applies again.

| | |
|---|---|
| Bandwidth | about 16 bytes per kick |
| CPU | none |
| Code | small to moderate: a per-player queue of scheduled impulses, D from lightyear's sync |
| Feel | every kick lands D late for its target, about 200 ms at 100 ms latency and 0 on the host. The hit flash plays at once, the shove after. A late shove reads as weight on a blast, and as lag on a shotgun point blank. Doesn't help ropes, which are continuous. |

### D. Full coupled-body prediction (rejected for now)

- Predict robots, crewmates and nearby bodies too.
- Robots' brains run only on the authority, so the client would extrapolate
  their inputs. Mispredictions then move onto those entities and become
  more frequent.
- CPU becomes N bodies × 14 ticks per rollback, and the rollback code must
  handle many entities.
- The highest cost for the least certain gain; it's what the ratified
  deferral was protecting against.

## Option A, built (2026-10-12)

- **Blending.** Corrections now blend out on their own, apart from view
  relocations. They use a critically damped spring (`client::camera::blend_out`):
  - it starts from rest, so nothing jumps the frame a correction lands;
  - its time grows with the correction's size, from `smooth_time` (0.12 s)
    up to 0.35 s.
- **Felt reaction** (`client::push_feel`), started as the client learns of
  the push:
  - for a correction over 15 cm: a head kick the way you were pushed (it
    eases in over about 4 frames), shake, and a thump;
  - for a new entry in your `HurtLog`: the same, away from where the hit
    came from.
- **Measured** with the drawn eye's path replayed through both blends (the
  `measure_*` tests). The corrections themselves are unchanged, by design.
  Worst per-frame jerk, i.e. one frame's step against the last:

| Push | Old blend | New blend | Floor (the new velocity's own step) |
|---|---|---|---|
| Shotgun shell | 0.107 m | 0.053 m | 0.042 m |
| Blast (11 m/s) | 0.48 m | 0.21 m | 0.19 m |
| Rope yank | 0.53 m | 0.22 m | – |

- **The cost:** the drawn eye now trails where you are for about 0.6 s after
  a big correction (up to about 1.4 m behind, for a blast), rather than
  catching up in 0.3 s with a lurch.
- **Frame sequence:** `docs/remote-forces-a.png`, plotted from the CSVs
  the measurements write to `target/remote-forces/`.

## Recommendation

1. **A now.** It's cheap and fixes how kicks feel, which is most of their
   cost.
2. **B1 next.** Ropes held by others are the one force measured as a
   rollback storm, and the source is already on the client. Measure again
   afterwards; the target is ≤ 3 rollbacks per yank.
3. **B2 when wrangling or towing becomes a main activity.** That is the
   deferred coupled-body prediction, scoped to one body.
4. **C only if A isn't enough for big blasts in playtests.** It trades a
   snap for lateness.
5. **Not D.**

Questions for the user:

- Is a blast that slides you under its flash acceptable (A), or should
  blasts land a beat late but never slide (C)?
- Should B2 wait for wrangling to come back?
