# Plan: wave 3 (2026-10-12)

The user's brief for the day: make the HUD good-looking rather than
demo-grade, add scenarios beyond the one boarding fight (puzzles, things
that need several players, not only combat), add enemy variety, build
weapon systems (looter-shooter generation that looks different, weapons as
items) and a real inventory (extra guns, ammo, mission items).

Agents work in parallel, each in its own git worktree and branch. The lead
(the session that wrote this) merges them into `main`. To keep merges
cheap, each agent owns a set of files (below) and touches others only where
its brief says so, and only as much as it has to.

## Shared rules for every agent

- Read `README.md`, `docs/art-style.md` and this file first, plus
  `docs/assets.md` if you build models.
- Visual checks go through `tools/hl` (headless sway). Never launch the game
  on the user's desktop. Use your own `HL_STATE` (under your worktree's
  `target/`) and your own `--port` (below), since several harnesses run at
  once.
- Commit on your branch with clear messages, ending with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. Don't push, and
  don't merge `main` in. Never commit `tuning.ron` changes.
- Delete old code when reworking. Don't keep two versions alive.
- Run the tests that relate to your change, and the whole suite
  (`cargo test`) once at the end.
- **TheBook.md.** When a library (or one of our subsystems) gets in your
  way, add an entry the moment it happens (see the file for the format).
- Game design calls you have to make: make them, then list them in your
  report as "defaults chosen without asking", so the user can veto them.
- Report back with what's done, what isn't, what you verified and how, and
  anything the lead will have to resolve when merging.

## Ports and harness state

| Agent | Port | `HL_STATE` |
|---|---|---|
| Items and inventory | 5981 | `target/hl-items` |
| HUD | 5982 | `target/hl-hud` |
| Enemies | 5983 | `target/hl-enemies` |
| Scenarios | 5984 | `target/hl-scenarios` |

## The fixed points between agents

These are settled now so agents can build against each other without
waiting.

### Items (owned by the items agent; others code against this shape)

- New module `sim::items`. `Item` is one of:
  - `Weapon(Weapon)`: a gun as an object. `Weapon` has a `frame` (the
    existing `GunKind`: the archetype, with its base stats), a `seed`, a
    `rarity`, and the rounds in its mag. Everything else (rolled stats,
    name, parts) derives from those deterministically, so a weapon costs
    a few bytes on the wire and in the predicted player state.
  - `Ammo { kind, rounds }`: loose ammo, by ammo kind. Reserves live in the
    inventory, not in the gun.
  - `Cargo(CargoKind)`: mission items (a power cell, a data core, a black
    box). Bulky cargo takes both hands while carried, so whoever carries it
    can't shoot or grapple, and the team escorts them.
- `Weapon::look() -> WeaponLook`, where `WeaponLook { frame, parts, paint }`
  picks one variant per part slot (barrel, magazine, stock, sight, muzzle).
  The client builds the visible gun from this.
- Robots keep plain guns: a robot's weapon is a `GunKind` with its base
  stats, without generation.
- The inventory replaces `Loadout::guns`. Stances still refer to weapon
  *slots* by position (so `stances.ron` stays meaningful), and the items in
  those slots can change.
- Pickups carry `Vec<Item>` instead of `Vec<Gun>`.

### HUD (owned by the HUD agent)

- The HUD agent owns `src/client/hud.rs` (and any `src/client/hud/`
  modules it splits it into). Everyone else keeps out, except for:
- The hand cards draw from a small view struct (`HandView`: name, rarity,
  mag, reserve, reload progress, tags) built by one function from the
  player's state. The items agent changes only that function when the
  inventory lands, plus any new HUD it needs, such as a pickup prompt.

## The agents

1. **Items and inventory** (`sim/weapons.rs`, new `sim/items.rs`,
   `sim/pickups.rs`, the Loadout tab in `client/menu.rs`; `sim/player.rs`
   and `sim/bots.rs` only where items flow, such as pickups, the locker and
   drops on death). Weapons as items, seeded generation of stats and names,
   rarity, ammo pools, inventory with weapon slots plus a pack, dropping and
   swapping, loot (robot drops, crates on the carrier), and an inventory
   screen to manage it.
2. **HUD** (`client/hud.rs`, fonts, a `docs/hud-style.md`). An actual visual
   design: a style guide, then a pass over every HUD element, plus the
   missing pieces (damage direction, objective marker, low-health
   feedback).
3. **Enemies** (`sim/bots.rs`, new robot models, robot entries in
   `client/animate.rs` and `client/combat_fx.rs`). Dynamic variations on
   the existing robots (affixes), at least one new kind of enemy, and robot
   death effects.
4. **Scenarios** (`sim/mission.rs`, a new devices module, new ship or
   site designs under `assets/ships`). A layer of interactive devices
   (switches, doors, timed locks, gates, sockets) and new scenarios built
   from them, not all of them fights.

Wave 4 (after these merge): weapon part models (the art for `WeaponLook`),
new weapon archetypes and families (shotgun, sniper with zoom, plasma, arc),
and mission items wired into the scenarios.

## Status at the end of the day (2026-10-12)

Waves 3 and 4 are merged into `main` (316 tests). Beyond the plan above,
the day added persistent progression ([plan-progression.md](plan-progression.md)),
the shotgun, sniper and the plasma, arc and laser families
([plan-weapons.md](plan-weapons.md)), the hull crawler, field fauna,
damage control and the escort, device press prediction, the
`AuthorityLog`, smoother corrections ([proposal-remote-forces.md](proposal-remote-forces.md),
option A), slim robot replication and a perf audit ([perf.md](perf.md)).
Solo playtest notes: [playtest-solo.md](playtest-solo.md). Co-op is checked
with `tools/coop-smoke`.

Waiting on the user:
- Remote forces: option C for blasts or keep A; B1 (predict ropes others
  hold) next; B2 when wrangling returns?
- Look calls in perf.md: rock hull coarseness, which shadow casters to drop.
- Unrecovered gear returns to the stash (`RECOVER_UNCLAIMED`): keep?
- A human solo playtest of boarding (the scripts couldn't win the fight).
