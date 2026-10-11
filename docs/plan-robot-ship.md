# Plan: the robot ship (boarding v2)

Goal: the boarding scenario's raider becomes a larger, slower enemy ship full
of robots. To board it, the crew needs guns, the robots need to fight back, and
both need health. Design background: the combat brainstorm of 2026-10-11 (in
project memory) and [art-style.md](art-style.md).

## Status (2026-10-11)
Stages 1–3 are done: health and death, hands and guns, robots (Gunner and
Swarmer; models built and in game). Not yet: the dropped-inventory pickup
(stage 1), procedural animation of the robot models, the carrier (stage 4).
Known limit: robots have no pathfinding; a Gunner that loses its target
walks straight at it and sidesteps to line up with it, which gets it through
a doorway but not round a corner.

## Stages

1. **Health and death.**
   - Players get health: one bar, which regenerates after a few seconds
     without taking damage.
   - At zero you die and respawn on the home ship. What you carried stays
     where you died, as a pickup.
   - Damage comes from a generic `Damage` event, so beams, weapons and later
     hazards share one path.
2. **Hands, stances and the first two guns.**
   - Players hold a `Loadout`: up to four stances, each with a main-hand item
     (LMB) and an off-hand item (RMB), or one two-handed item.
   - The grapple is a suit system whose launcher occupies a hand. When it's
     stowed, the rope is frozen.
   - The first guns are an SMG and a rifle, both ballistic with realistic
     speeds; shots inherit the shooter's velocity. Mags hold hammerspace
     reserves (the usual mag-and-reload). A hand reloads itself when it runs
     dry; R reloads everything.
   - Per-hand alternate functions use chords: Ctrl+button and Alt+button. For
     the grapple these are reel in and pay out; Mouse4/5 also reel.
   - Netcode: a shot is a tick-stamped `Fired` event from the shooter's
     predicted sim. Everyone draws tracers from the event log. Only the
     authority resolves hits. No lag compensation in v1, since robots are
     slow. Revisit if hits on moving targets feel unfair.
3. **Robots on the player controller.**
   - A robot is a player body driven by an AI writing its `PlayerInput` on the
     authority. It gets boots, EVA, grapple, weapons and health for free,
     replicates like a remote player, and uses the same rules as the crew.
   - It has its own tuning (mass, speed) and its own model.
   - First two: the **Gunner** (rifle, keeps its distance, strafes, uses
     cover) and the **Swarmer** (fast, fragile, rushes, short spray).
4. **The carrier.**
   - A new hull kind: bigger (around 60 m, two decks with rooms and
     corridors), flying a slower, wider path.
   - The shield console is deep inside. Robots are posted on board and
     respond to boarders.
   - It replaces the raider in the boarding scenario.
5. **Models.** Gunner, Swarmer, the SMG and rifle as held items, and carrier
   modules, built by agents through the asset pipeline ([assets.md](assets.md)).

Each stage ends playable and tested before the next starts.

## Defaults chosen without asking (easy to change)
- Default stance 1: SMG in the main hand (LMB), grapple in the off hand (RMB),
  following the shooter convention. Stance 2: rifle plus grapple.
- Player health 100; regeneration starts after 6 s without damage.
