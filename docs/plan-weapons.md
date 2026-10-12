# Plan: wave 4's weapons (frames, families, zoom)

The ratified roles (memory): splash for crowd control, a sniper for range, the
workhorse rifle, the high-rate SMG sidearm. Dual wielding is first class;
two-handed actions (zoom) need the grapple stowed; LMB fires, RMB is zoom or
the secondary function. Families differ in how they act on the world, not only
in numbers. Arc jumps the traditional way, never through hulls or ropes.

## Frames

| Frame | Role | Hands | Ammo | What it does |
|---|---|---|---|---|
| SMG | sidearm | one | light | as now |
| Rifle | workhorse | one (braced with the other hand free) | heavy | as now; a scope part zooms (x2.5) |
| **Shotgun** | crowd control | one (braced with the other hand free) | shells | 8 pellets a shot in a wide cone; heavy pellets: a strong shove on whatever's floating (Swarmers fly off), and on you in EVA |
| **Sniper** | range | two: fires one-handed only from the hip (a huge cone) | heavy | 1400 m/s, 90 damage, 5 rounds; zooms (x5, x6 with a scope); its round goes through the first body it hits into a second |

## Families

**A family is rolled on the SMG and the rifle only** (35% of those in loot), and
stored with the gun (`Weapon::family`, a serde default of ballistic, so every
saved gun is exactly what it was). The shotgun and sniper are ballistic
archetypes already: their point is the pellet cone and the long shot, and a
plasma sniper or an arc shotgun would blur the roles the user ratified.
Rolling the family on the two general-purpose frames gives each role family
variety (a plasma SMG is a different sidearm from an arc SMG) without
multiplying frames, models or roles. A family shows as an emitter on the muzzle
(replacing the muzzle device), in its tracer and its sound; its name says so
on the HUD ("Rare plasma rifle").

| Family | Travel | Hits | Momentum | Ammo | Notes |
|---|---|---|---|---|---|
| Ballistic | bullets at real speeds (380–1400 m/s), inherit the shooter's velocity | first thing in the way | bullet mass x speed: pushes rocks, shoves what floats | mags and pools | sniper rounds pierce one body |
| Plasma | slow bolts (12% of the frame's speed: 45 m/s SMG, 100 m/s rifle), so the shooter's own velocity matters most | splash: 2.5 m, falling off, only what the blast can see (no damage through a hull) | heavy (6x the momentum of the frame's bullet): a strong push at the blast, both on bodies and on anything floating in the radius | energy cells | half the rate, 1.6x damage a bolt |
| Arc | instant, short (35 m) | the first thing in the crosshair, then jumps to up to 2 more within 7 m of it, each in clear sight of the last (a ray: hulls and decks stop it; ropes aren't in the way of anything and aren't conductors) | none | energy cells | ignores weak spots: it's the crowd weapon of the general frames; 0.6x per jump |
| Laser | instant, a continuous beam (fires every tick it's held), 400 m | the first thing in the way | none | none: heat. Each tick of beam heats it; at full it overheats and won't fire until it's cooled to a third; cools when not firing. The heat is the gun's (`Weapon::heat`), so a hot laser stays hot in the other hand or the pack, and cools there | dead accurate (no cone), low damage per tick. The beam is held state (`HandState::beam`), not a `Fired` entry a tick: the log keeps a `BeamStarted` and a `BeamStopped` per burst, and the authority hits from the beam state every tick (a held beam measured 71.8 kB/s on the wire as log entries, 7.1 kB/s as state, 6.3 standing) |

## Hands, stances, zoom

- Zoom is on the *other* hand's button: a gun that zooms (a sniper, or any rifle
  or sniper with a scope part) in one hand, the other hand empty, the other
  hand's button held = zoomed (the stance with a gun braced is the zoom stance).
  With the grapple in the other hand there's no zoom (the grapple's button is
  the grapple's), so zooming means stowing it: the ratified rule falls out of
  the stance scheme rather than needing its own.
- Zoomed: the cone is the braced cone x0.3, the view narrows (x2.5 / x5 / x6)
  and mouse look slows by the same factor. Zoom is predicted state
  (`Loadout::zoom`), so it shapes the shots the authority flies.
- The sniper unbraced fires from the hip: a 6° cone.

## Ranks and frames

Loot can be any frame at any rank (it's found, not issued): a rolled gun is a
shotgun or sniper 35% of the time (evenly), else the SMG or rifle it always
drew. What rank opens is
standard issue at the armory (`progression::frames(rank)`): the SMG and rifle
from the start, the shotgun at rank 4, the sniper at rank 10 (the ladder's old
"new frames (to come)" step). The ladder says so.

## Frozen seeds

Nothing that decides an existing gun changes: `build()` and the stat
arithmetic for SMG and rifle ballistic guns are untouched (the pinned rolls
test). New frames use the same tables. Loot rolls keep their old draws for the
frame and rarity, and add new salts for "a new frame instead" and the family.
Robots' kinds never roll. Ammo pools grow from 2 kinds to 4 (shells, cells);
saved inventories with 2 read as before (the rest empty).

The pinned-rolls test kept its pinned data; only its loop now walks
`GunKind::FIRST` (the two frames it pinned) instead of `CREW`, which grew. A
second pin (`wave_4_guns_stay_the_guns_they_were`) freezes the shotgun, the
sniper and the families as they shipped.
