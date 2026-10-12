# Ships

Ships are data. A ship is a **design** that places **modules**; a module is a
piece of ship with its own geometry, fittings and crew. Everything here is in
`assets/ships/`, and every `.ron` there is built into the game (`build.rs`), so
adding a ship is adding files. The code that reads them is `src/sim/design.rs`.

Hand-designed ships place one or a few modules. Generated ships will place many,
chosen and joined by a generator (not built yet; see "Next" below).

## Coordinates

Metres. +Y up, the bow toward −Z, +X to starboard. Decks are horizontal: the
robots walk floors, never walls, and the floor plans and checks assume it. A
module is authored in its own frame; a design places it with `at` (a move) after
`turn` (degrees about +Y, counterclockwise seen from above).

## A design: `assets/ships/<ship>/design.ron`

```ron
Design(
    name: "CARRIER",          // what the HUD calls it
    hostile: true,            // an enemy: hostile hull, red on the HUD
    model: Some("ships/carrier/carrier.glb"),  // drawn in place of the boxes, or None
    shield: (center: (0.0, 3.2, 0.0), radii: (11.0, 8.0, 36.0)),  // an ellipsoid
    turrets: [(0.0, 8.1, -8.0), (0.0, -0.8, 4.0)],  // laser mounts; each gets a base box
    modules: [(module: "carrier/hull"), (module: "parts/airlock", at: (0.0, 0.0, 30.0), turn: 180.0)],
)
```

The ship's id is its directory name (`carrier`). Module names are paths under
`assets/ships/` without `.ron`.

## A module: any other `.ron`

```ron
Module(
    parts: [...],       // geometry, below
    controls: [(action: DisableShields, center: (0.0, 4.36, -27.0), half_size: (0.35, 0.06, 0.3))],
    cabins: [(center: (...), size: (...))],   // interior volumes, for light probes
    lights: [(0.0, 2.7, -27.0), ...],         // ceiling lights
    posts: [(robot: Gunner, at: (0.0, 0.0, -2.0), facing: 180.0),   // feet on a floor
            (robot: Swarmer, at: (18.0, 6.0, -20.0))],              // station outside
    markers: {"entry": (0.0, 0.0, 30.0), "objective": (0.0, 3.8, -27.0)},
)
```

Parts (material is `Hull`, the default, or `Trim` for fittings and furniture):

| Part | What |
|---|---|
| `Box(center, size, material)` | a box, by centre and full size |
| `Span(lo, hi, material)` | a box, corner to corner |
| `WallAcross(z, x: (a, b), y: (a, b), thickness, doors: [(x0, x1, top)])` | a wall across the ship at `z`, with doorways (`top` is the opening's top, absolute y) |
| `WallAlong(x, z: (a, b), y: (a, b), thickness, doors: [(z0, z1, top)])` | likewise, along the ship at `x` |
| `Ramp(foot, top, width, thickness)` | a ramp whose walking surface runs from `foot` to `top` (middles of its ends) |

Boxes keep their order (parts in order, modules in order, turret bases last).
A model's `build.py` may rely on it, so add parts at the end of a module.

Markers are named points. Play uses `entry` (where boarders come in) and
`objective` (what they're there for) on enemy ships. Every marker whose name
starts with `loot` (`loot_bridge`, ...) gets a loot crate on that floor spot,
rolled better the nearer it is to the `objective` than the `entry`
(`sim/pickups.rs`). Others are free (the carrier's `upper_deck` is a point on
its upper floor, for tests).

## What play needs: the check

`cargo run --bin ship_check [ship ...]` checks designs and draws each deck as a
floor plan (one character per half metre: `.` reachable from the entry, `,` a
place you can stand but can't get to, `G` a walker's post, `E` the entry, `O` the
objective, `C` a control, `*` a light; the bow at the top). It reports the
walk from entry to objective. A test (`design::tests`) runs the check on every
design, so a ship that falls short fails the build's tests.

The check uses the robots' own walkable grid (`src/sim/nav.rs`), for the biggest
walker (a Gunner, 0.42 m radius, 2 m tall), so "a walker can get there" means a
Gunner can. It checks:

- every walker's post is a place to stand, and can be walked to from the entry;
- every flyer's station is clear of the hull;
- every control has somewhere to stand within 2 m of it (one boarders can reach,
  on an enemy);
- an enemy has an `entry` a walker can stand at, an `objective`, and a
  `DisableShields` control;
- the crew's ship has a button for every scenario;
- every `loot` marker is on a floor that can be walked to from the entry.

What the grid takes as walkable: a floor with 2 m clear above it, no wall within
0.42 m, floor under most of the walker, and steps between places of at most
0.3 m over smooth floor (a ramp, not the ramp's side). In practice: **doorways at
least 1.2 m wide and 2.2 m tall, ramps under about 25°, nothing walkers must
climb.**

## A ship brief: what the game mode asks of a design

Agents design ships from a brief that gives the game's needs and leaves the
ship to them. A boarding target's brief, for example:

- **Role:** a boarding target; the objective is its shield console (or reactor,
  or whatever the mission is).
- **Size:** length range (the carrier is 60 m), decks (1–3), 3 m inside each deck.
- **The walk:** about how long entry to objective takes on foot, unopposed
  (`ship_check` reports it; the carrier's is 59 m, 20 s). Fights stretch it many times.
- **Spaces:** at least one entry (a hangar, an airlock), two for a crew to split;
  the objective room, deep; an engineering bay (for a later puzzle); others as the
  ship's story wants (armoury, cargo, quarters).
- **Routes:** at least one loop or alternative route; some rooms long enough
  (12–35 m) for Gunners' fighting range, with cover; vertical links by ramp.
- **Crew:** how many walkers and flyers, spread along the routes.
- **Outside:** surfaces to grapple and land on along the approach; turret mounts
  top and bottom; a shield that encloses it all.
- **Look:** a silhouette readable at 400 m that shows which way it's heading;
  colours free except magenta (weak spots only); the ship triangle budget.

The workflow: write the design and modules, run `ship_check` until it passes and
the plans look right, then `layout_dump` it and model it (see "Ships" in
[assets.md](assets.md)).

## Next: ships from components

The format places modules with transforms, which is what a generator needs to
assemble ships from pre-designed pieces. Still to design, with the game in mind:
which pieces exist (rooms, corridors, hangars, decks, hull sections), how they
join (named connection points with sizes, so doorways line up), the rules a
generator follows (the objective deep, entries outside, loops), and how a
generated ship reaches other peers (its seed, or its list of placements, in
`ShipHull`). A generated ship's model would then be the pieces' models, placed
the same way.
