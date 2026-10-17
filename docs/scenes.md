# Scenes

A scene is where the crew jumps in: our ship at rest, sites set round it
with something to do at each, loot adrift between them, and a rock field
grown round it all. The code is `src/sim/scene.rs` (the planner, pure) and
`src/sim/scenarios/scene.rs` (building and judging it). Scenes are the
"Scene" button on the mission wall, or `HULLBREAKERS_SCENE=<seed>[:<size>]`.

The arc's rulings (2026-10-16): scenes only for now (the run that strings
them together, fuel and such, come later); size is a lever; everything is
seeded; the lobby is the asteroid field. Designs and their content come
from agents; the planner chooses and connects.

## One seed, everything derived

`SceneSeed { seed, size }` → `plan(seed)` → `ScenePlan`: the sites (which
design, where, how turned, what to do there, crewed or not), which one is
the main objective, the loot adrift, the threat, the jump window. Every
peer computes the same plan, and the rock field is grown on every peer
from the seed (`FieldLayout::Scene`, the race's path): nothing of the
layout is sent. Change `plan` and every seed's scene changes; that's fine
until seeds are shared as runs.

Sizes, in `SceneSize`:

| Size | Sites | Reach from our ship | Rocks | Jump window |
|---|---|---|---|---|
| tight | 1–2 | 60–110 m | 400 | 90 s |
| wide | 2–3 | 200–450 m | 1500 | 180 s |
| vast | 3–4 | 0.8–1.6 km | 5000 | 360 s |

The size is rolled from the seed unless spelt (`7:vast`).

## How a scene plays

- The board (the mission panel) names the scene and lists every
  encounter: `MAIN:` one, `side:` the rest, ticked when done.
- Markers: the main's are goals; the sides' are waypoints.
- When the main is done, the jump window opens: a countdown to be aboard.
  The crew may jump early from the wall (the "Asteroid field" button). The
  scene is **won** if the main is done at the jump, whoever's aboard (the
  debrief says who was left drifting: `homecoming`); **lost** (abandoned)
  if the crew jumps without it.
- Pay (`progression::pay_for`): the win, the main, each side done, plus the
  usual credits and everyone-home bonus.

## Encounters

What a site can host, declared in its design. The scenario is generic over
them: it reads devices, pickups, robots and pods; the design supplies the
puzzle.

| Encounter | What it needs of the design | Done when |
|---|---|---|
| `Salvage(cargo, released_by)` | an `objective` marker (where the cargo lies); if `released_by: Some(id)`, a device `id` that going live sets the cargo out (a clamp at the end of a device chain) | the cargo is in our hold or held by someone aboard |
| `Restore(device)` | a device `device` | that device is live |
| `Clear` | `crewed: true` and posts | no robots left aboard |
| `Rescue(pods)` | nothing (survivors are scattered 50–140 m round the site on staggered air) | every survivor is home or lost |

`cargo` is a `CargoKind` (`PowerCell`, bulky, both hands; `DataCore` or
`BlackBox`, into the pack). Add kinds in `sim/items.rs` as sites need them.

The main objective is a prize (a salvage or a rescue) when any site in the
catalogue offers one; sides are anything.

## Making a site

A site is a design (docs/ships.md) with a `scene:` block:

```ron
Design(
    name: "DERELICT",
    hostile: false,
    model: None,
    shield: (...), turrets: [],
    modules: [(module: "derelict/hull")],
    scene: Some(Site(
        encounters: [Salvage(cargo: DataCore, released_by: Some("core_clamp")), Restore(device: "reactor")],
        crewed: false,          // robots at its posts, at the scene's threat
        clearance: None,        // m from its origin that rocks and other sites keep; default its hull's reach + 25
    )),
)
```

What the planner relies on:

- **`entry`** marker: the site is turned so its entry faces our ship (the
  way in is the side we see from the deck). Put it at the airlock or the
  breach, out from the origin.
- **`objective`** marker for a salvage: a floor spot the cargo lies on.
- **`loot…`** markers get crates (better nearer the objective), as on any
  ship.
- **Posts** if `crewed`: who stands there is rolled from the threat
  (`bots::compose_crew`). The scene's threat is 1–3, eased for a small crew.
- **Devices** are the puzzle: wire whatever chain the site's story wants
  and name the device an encounter refers to. The dead ship's chain
  (reactor → junctions read off a board → two keys → vault → clamp) is the
  model; see docs/ships.md "Devices".
- Keep the site's own reach modest: a 40 m hull is a site; a 300 m station
  is a scene of its own (later).

`cargo run --bin ship_check <id>` checks the design and the `scene` block
(the devices and markers it names exist; a crewed site has posts; an entry).
The design tests run the check on every design.

To see a site in a scene: pick a seed whose plan uses it
(`plan()` in a test, or try a few), then
`HULLBREAKERS_SCENE=<seed>:tight` on the headless harness (`tools/hl`).
`tests/scene.rs` builds every size for a few seeds and plays a salvage
through to the jump.

## Site ideas for the catalogue

What the planner can't do is make geometry or stories; these are for
agents to build, each a design with devices, posts and a `scene:` block.

- **Wreck**: a broken hull (half a ship, decks open to space), survivors
  adrift round it (`Rescue`), a black box deep in what's left (`Salvage`).
- **Outpost / relay**: a small crewed station; `Clear`, or `Restore` its
  transmitter through a breaker chain.
- **Cargo pod cluster**: pods strung on a spine; a `PowerCell` in one, the
  rest crates; `Clear` of crawlers and turrets on its hull.
- **Mining rig** on a big rock: `Restore` its drill (route power, reset the
  tripped breakers), `Salvage` its ore core.
- **Patrol hulk**: a hostile gunship gone dark, crew still aboard, `Clear`.

## Open

- A scene can't yet be lost before its main is done (no clocks on the
  crew). Those belong to the run (fuel, air), later.
- The jump is the wall's field button. A helm, and piloting to the next
  node, is the run's work.
- Fauna in scene fields (the field scenario's limpets and grazers ride the
  streaming field's rocks; scene rocks are grown still).
- Sizes beyond the table: a scene that is one huge site.
