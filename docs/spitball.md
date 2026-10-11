# Spitball

A parking lot for ideas we might want later but aren't building now. Nothing
here is a decision. When an idea gets picked up, move it into the real plan
and note it here as taken. When one is tried and dropped, leave it here with
why, so nobody rediscovers it blind.

Inspirations so far: **Jump Space** (crewed ships, boarding actions) and
**Deep Rock Galactic** (co-op contracts, classes, extraction pressure).

## What's already distinctive

These are the hooks to keep leaning on.

- **Everything has mass.** Ropes pull both ways, and pulls are shared by mass.
- **You move like a climber.** Magnetic boots on any body, a jetpack, and a
  grapple you can swing and reel on. Playtest 1 moved the tuning to faster,
  lighter and more fluid.
- **You hear through what you touch.** Sound reaches you only through the
  structure you're standing on, so vacuum is silent.
- **Big moving things are cheap to network.** A body only costs bandwidth
  when something disturbs its flight.

## Game loop shapes

- **Contract crew (Deep Rock).** Hub ship, then a contract board, then a site
  with a main and side objectives, then extract before the window closes,
  then upgrades. A "deep contract" chains three sites with no resupply.
- **Jump run (Jump Space, FTL).** A sector map of nodes. Ship damage, fuel
  and injuries carry over. Losing the ship ends the run; unlocks persist.
- **Haul and grow.** The ship is built from what you drag home. A bigger ship
  carries more, but has heavier inertia of its own.
- **Leading candidate:** contract missions as the nodes of a jump run.

## Mission ideas

- **Boarding action**: *taken; see the first scenario.*
- **Asteroid ring race.** Grapple your way across an asteroid ring to the far
  side. Could be a race between players, against a timer, or against the
  ring's own rotation.
- **Wrangling.** Lasso ore-rich rocks and haul them into the ship's intake.
  Big rocks need several ropes. A side objective: bring one in with no
  impacts. *See the wrangling playtest notes below before building this.*
- **Dead ship.** Stop a tumbling derelict's spin (rope it to a heavy rock, or
  bolt on thrusters), then board it and pull its cores.
- **Hot core.** Rope-escort an unstable reactor core home. It detonates above
  some impact speed.
- **Ride the stream.** Surf the asteroid stream to reach something at its
  end before the stream carries it away.
- **Drill site.** Plant a drill on a rock and defend it while it works (Deep
  Rock's Point Extraction).
- **Recovery.** Crew drifting in a debris field with low oxygen. Find them by
  beacon and reel them out.

## Systems

### Costs and clocks (deferred on purpose; goals come first)

- **Jetpack propellant.** Walking and swinging are free; thrust costs fuel.
  This rewards the fluid movement style.
- **Oxygen.** A soft mission clock.
- **Suit integrity / health.** Impacts above a relative-speed threshold hurt.
  A downed player drifts, and teammates reel them home to revive them, which
  turns the player grapple into the revive mechanic.
- **Extraction window.** A solar flare or a closing jump window forces
  everyone back aboard.

### Classes (Deep Rock's four roles, each bending the physics)

- **Rigger.** Fixed cables between bodies that work as ziplines, tethers and
  traversal routes.
- **Cutter.** A mining laser that splits rocks into smaller real bodies and
  breaches hulls.
- **Engineer.** Stick-anywhere thrusters, anchors and turrets. Push a rock by
  bolting an engine to it.
- **Gunner.** A heavy weapon whose recoil really pushes you; the team's
  anchor point.
- Open question: fixed classes, or open loadouts?

### Resistance on a boarding action

Two routes, both deferred until the boarding scenario gives them a place to
live (decided 2026-10-10).

- **Defenders.** Turrets that track EVA players, crawlers, boarders. Needs the
  enemy system built out first.
- **Puzzles.** Jump Space has an engineering panel where tetris-like pieces
  get shuffled around to power ship components. The user thinks this idea can
  be pushed much further. Directions to explore:
  - Power routing that is physical: the pieces are real objects in the ship,
    carried or roped around, rather than a 2D panel.
  - Panels spread across the ship, so one player reads the state while
    another acts, talking through the hull.
  - On a boarding action, the enemy's panel is the lock: re-route their power
    to drop the shield rather than pressing one button.
  - On our own ship, a hit knocks pieces loose and someone has to fix it mid
    fight.

### Enemies

- **Hull crawlers.** They walk on magnetic feet using the players' own booted
  movement. You hear them through the hull before you see them.
- **Drone swarms** in open space.
- **Grapplers** that yank you off the hull, using our yank mechanic against
  you.
- **Boarders.** The enemy plays our own boarding game back at us.

### Our ship

- A walkable interior with stations: helm, winch/crane, turret, fabricator.
  Anyone can fly it.
- Ship modules as meta progression.
- Ship-to-ship combat where the guns are slow and boarding is the fast way
  to win (the first scenario).

### Talking

- Short-range suit radio, plus clearer audio through shared structure.
- Press helmets together to talk privately.

### Meta progression

- Credits, per-class unlocks, ship modules, cosmetics.
- Deep Rock-style hub social stuff (a bar, dancing, kicking a barrel around)
  much later, if ever.

## Small details for fun

- The ship's PA says something dry when a mission starts or ends.
- A laser hit you can hear only if you're standing on the hull it hits.
- A sticky-note "DO NOT PRESS" on a button that does something harmless.
- A hull counter: "days since last decompression: 0".
- A crew score card at the end of a mission: who pressed the button, longest
  swing, time in vacuum.

## Playtest notes

### Wrangling (impromptu test, after playtest 1)

- **Rocks are far too massive to play with**, even with the jetpack turned all
  the way up. A wrangling mission needs a sweep of rock masses and densities
  (and maybe jetpack and winch force) to find a range that feels like play.
- **Real rope physics on a mass gets high-energy fast.** Tugging something on
  a rope quickly turns into an oscillation that's hard to damp by hand. It
  needs an assist mechanic, like flight assist but for the rope: for example,
  a winch that damps relative velocity along the cable, or a "tow mode" that
  matches the load's velocity to yours.
- **Networking note:** reeling a light rock already causes about 2.7
  rollbacks per second at 100 ms, because the client doesn't predict the
  rock's recoil. Wrangling as a main activity makes coupled-body prediction a
  priority.
