# Hullbreakers

First-person co-op boarding-combat roguelite, early days (provisional name). Bevy 0.19 + Avian 0.7.

```
cargo run                                # host a game and play (UDP port 5888)
cargo run -- --connect 192.168.1.20      # join a host (HOST[:PORT])
cargo run -- --server [--port 5888]      # dedicated headless server
cargo test                               # movement, client-world and netplay tests
HULLBREAKERS_SHOTS=dir cargo run           # scripted screenshot tour, then exit
HULLBREAKERS_SCENARIO=boarding cargo run   # host straight into a scenario
HULLBREAKERS_SAVE=alt.ron cargo run        # another profile (default profile.ron)
```

## Scenarios

The crew's ship has a **mission wall** on the port side of the bridge: click
a button (F, within reach) to start a scenario. Every switch rebuilds the
world and puts everyone back on the deck. The game starts in the
**asteroid field**, and its button takes you back there.
`HULLBREAKERS_SCENARIO=boarding` (or `run`, `derelict`, `recovery`, `gallery`) starts in
another. When a mission is decided a banner says how; back in the field, the
**debrief** gives its report, every crew member's score card (time in vacuum,
longest swing on one rope, presses, deaths, and what they did) and awards.

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

- **Grapple run**: ten gates strung through a cluster of rocks, from just
  off our stern, out and back past our bow (about 430 m gate to gate). Fly
  them in order before the clock (4 minutes) runs out; your next gate glows
  cyan. No enemies: it's the grapple's show (the reel is far faster than the
  jetpack). The board at the top keeps everyone's progress and times, and
  the host remembers best times, the crew's and each runner's.

- **Dead ship**: a dark derelict lies 22 m off our stern, its airlock facing
  our cargo door. Cross over (the suit lamp is your light), pull the
  reactor's restart in engineering, then route the power to the vault at the
  bow through three junctions spread across the ship (F turns one to its
  next way). The **power board** in engineering shows which circuits are
  live, so one player can read it and call while others turn junctions; lit
  conduits on the floors and ceilings show where the power runs; doors
  latch open as power reaches them. The vault opens only when its two keys,
  on the bridge and in the port room, are turned within 1.5 s of each other
  (alone, key B holds 5 s: race to key A). Release the data core's clamp,
  take the core off its cradle (it goes in your pack), and get it and
  everyone back aboard in the 75 s before the tripped reactor goes.

- **Recovery**: a ship has broken up nearby and its crew is adrift in the
  wreckage, scattered in every direction: four suited survivors and a
  lifeboat with four aboard, each on a clock of air (the nearest run out
  first, from about 2:20). Their beacons ping in turn every 4 s (flashing
  amber on the pod, red when the air is nearly gone). Rope one and tow it
  into our hold; let go of the keys while towing and flight assist keeps
  station with what you tow instead of braking (so it doesn't run into
  you). A survivor is light; the lifeboat is 600 kg, so a second rope on it
  nearly doubles the pull. Win with everyone accounted for and at least 4
  of the 8 home; alone you can't reach them all, so split up.

- **Model gallery** (debug): every model under `assets/` in a row behind
  our ship, turning slowly, labelled with its path and size, sockets drawn
  as small axes (red +X, green +Y, blue forward), with every robot model's
  affix parts showing at once. Below it, a labelled row of live robots: each
  kind, and each with every affix it can have. They don't fight back. You
  start floating in front of it.

You start with the base kit (your suit and its grapple) and whatever your
profile says you carry (see **Progression**). Guns are in the **weapons
locker** on the starboard wall of our ship's aft hold: press it (F) to fill
your ammo and mags and open the **armory**, your stash. Anyone with no gun
at all is issued a standard SMG and rifle; standard issue is always free at
the armory. Die, and everything you carried stays where you fell, in one
glowing case on the deck (or drifting on, if you were in EVA): it's yours
alone to take back (look at it, F), and you come back aboard with the base
kit. A scenario change keeps what you carry.

### Progression

What you own and earn is kept in `profile.ron` beside the game, on your own
machine (Deep Rock style: co-op is between friends, so a client's profile is
trusted). On joining, it goes to the host; the host keeps it while you play
and sends back a snapshot whenever your gear changes (every few seconds while
only ammo does), which your machine saves. A missing file starts a fresh
profile; an unparseable one is kept as `profile.ron.bad` and a fresh one
started; one that can't be read at all is left alone (a fresh profile, not
saved). `HULLBREAKERS_SAVE=path` keeps it elsewhere; `tools/hl` gives each
game it runs its own. Mission cargo (a data core) stays in its mission.

- **Your guns are yours, for good.** Every gun you pick up is yours. Store
  the ones you aren't carrying in the **armory** (up to 40). What you leave
  where you die, or put down and nobody takes, is back in your stash when the
  mission's over. A gun you put down (G) for a crewmate is theirs once they
  take it; the case you leave where you die, nobody else can.
- **Salvage** pays for upgrades. A decided mission pays each crew member
  (more for a win, for each thing the scenario credits you with, for
  everyone getting home, and a quarter more for a quick win); walking away
  undecided pays nothing. Scrapping a gun pays too, more for rarer ones.
- **The workbench**, beside the locker: **tune** a gun up a tier of rarity
  (its parts stay; rare and up add perks), **refit** a part with one you've
  unlocked by scrapping a gun that had it, **repaint** it in a crew paint, or
  **scrap** it.
- **Rank** comes with experience from missions, and each brings something you
  can feel: paints (2, 5, 7, 9), a fourth weapon slot (3), two more pack
  slots (4, 8), suit plating, +25 health (5, 9), a stronger jetpack (6), a
  faster winch (7), and new frames to come (10).

### Weapons, loot and the inventory

Guns are items, Borderlands-style: each is a **frame** (SMG or rifle), a
**maker**, five **parts** and a **rarity**, all rolled from a seed, so two
guns of a kind play differently. Every part trades one thing for another
(a long barrel: faster, tighter shots but a slower rate; a drum: twice the
mag, a much slower reload; a light stock: quick to switch to, wild
unbraced). Makers lean their guns their own way: Kessler (precise, slower),
Brakk (heavy, crude), Hoshi (fast and light), Corvid (deep mags, loose).
Rarity runs Standard issue (grey), Common (white), Uncommon (green), Rare
(blue, one perk) and Exotic (orange, two perks); perks are things like
Recycler (every fourth shot is free) and Last word (the last round in the
mag hits three times as hard). A gun's name is its maker, the maker's model
(rarer guns have higher marks) and a word for one of its parts or perks:
"Kessler KG-3 Longshot", "Hoshi Type II Hornet", "Corvid Scrap #4 Miser".
A gun looks like what it rolled: it's drawn from its frame's receiver with
each of its parts on (a long barrel is long, a drum is a drum, a quick mag
is two taped together, a compensator is ported, a scope has a big objective
bell), in its maker's paint, in your hands, your crewmates' and on pickups.
The model gallery has racks of every part and a row of rolled guns.

Robots drop things when destroyed (mostly ammo, sometimes a gun, better from
tougher robots), and the enemy carrier has **loot crates** aboard, better
the deeper in they are (the best on the bridge). A pickup glows in the
colour of the best thing in it, with its best gun turning on the lid; look
at it for what's inside and how it compares with your guns, and press F to
take everything that fits.

You carry three **weapon slots** (four from rank 3; the stances use these, by number), a
**pack** of six (up to ten with rank) for spares and small cargo, **ammo** by kind (light rounds
for SMGs, heavy for rifles; a reload draws from it), and bulky **cargo**
such as a power cell, which takes both hands: whoever carries it can't
shoot or grapple, so the team escorts them. G puts down what's in hand
(cargo first, else the main hand's gun). The **inventory** (I) shows it all:
click an item, then a slot to move it there (or swap); right-click drops
it; the panel beside compares the chosen gun with yours.

Each hand holds one item and a stance (1-4) sets what's in each; a one-handed
gun with the other hand empty is braced and shoots tighter. Bullets fly at realistic speeds and keep the
shooter's velocity, and firing in EVA pushes you back. Only the grapple's
hand works its rope; stow it and the rope holds its length.

The carrier is crewed by robots. Robots are players driven by a brain on
the host, so they move, aim and shoot by the crew's rules; they see along
clear sight lines, chase a target that ducks out of view for a few seconds,
call to each other, and are destroyed at zero health, coming apart into
tumbling, darkened pieces. Each has a **weak spot**, a glowing magenta core
that takes 2.5× damage: flank them. Hit markers show your hits (white),
weak-spot hits (magenta) and kills (an X); robots don't heal. Four kinds:

- **Gunners** (walkers with rifle arms that keep their distance and strafe),
  posted from the hangar to the bridge. Weak spot: the backpack.
- **Swarmers** flying escort: fast, fragile drones that rush in and spray.
  Weak spot: the reactor between the rear thrusters.
- **Grapplers**: heavy walkers that harpoon you (they draw back first, with
  a ratchet), reel you off your boots and let go, flinging you out of cover
  or off the hull. Weak spot: the winch's power cell on the back.
- **Hull turrets** bolted to the carrier's engine pods and keel, covering the
  approach to the hangar: a searchlight sweeps for anyone crossing, and the
  slow shells lead you, so change your velocity. Weak spot: the coolant
  core at the back of the base.

Robots can have **affixes**, each shown on the model: **Armoured** (dark
slabs; body hits do 30%, the weak spot is soft), **Shielded** (a shell that
takes everything until broken, then comes back if left alone), **Overclocked**
(radiators and stacks; fast and frantic, its lights heat up, then it stops
to vent steam: while venting every hit is a weak-spot hit), **Volatile**
(hazard-yellow charges; it explodes when killed, hurting and shoving
everything near it, robots too, so blasts chain), **Marksman** (Gunners: a
long barrel and scope; a laser and a swelling glint before each slow, heavy
shot, its aim locking just before it fires: move then) and **Leader**
(Gunners: an antenna mast and crest; robots near it aim and fire better, their
lights pulsing gold with its beacon, until it dies). A ship is crewed for a
**threat** level that sets how many affixes, and which kinds man which
posts (`sim::bots::crew`); the boarding action's carrier is threat 3: two
Grapplers, three turrets, a Leader on the bridge and six affixes. The model
gallery has a row of every kind with each of its affixes, dormant but
shootable.

The F1 panel can spawn a Gunner on our bridge or three Swarmers off our
port side, for trying out a fight.

Players have suit health (bottom left). It comes back after a few seconds
unhurt; at zero you respawn aboard the home ship, and the HUD marks where
your kit was left.

### The HUD

The HUD is a suit visor projection (design: [docs/hud-style.md](docs/hud-style.md),
code: `src/client/hud/`). Around a clear centre: the scenario strip at the
top (the objective, the clock, the scenario's status lines, and in a ship
fight both hulls and the carrier's shield), the event feed top left, your
suit (health, regen, boots, assist, speed against the reference body,
rope) bottom left, and a card per hand with the stances bottom right
(weapon names and rarity; what's in a pickup in a card by the crosshair).
In the world it marks our ship and hostile ones, brackets the reference
body, and marks the scenario's markers (goals, gates, devices, the way
home) plus the carrier's hangar, the mission wall after a win, the weapons
locker while you're unarmed aboard, and your dropped kit; off screen
they're arrows at the edge. Arcs round the centre point toward whatever
just hurt you (the authority records where each hurt came from), the edge of the
screen reddens as your health runs low, and dying shows SUIT FAILURE. It
scales with the window's height. `HULLBREAKERS_SCENARIO=boarding
HULLBREAKERS_SHOTS_SHIP=hud HULLBREAKERS_SHOTS=dir` screenshots it in each
of its states.

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
| F            | press the button, or take what's in the pickup, under the crosshair (within reach) ||
| G            | put down what's in hand (cargo, else the main hand's gun) ||
| LMB / RMB    | use the main / off hand's item (gun: fire; grapple: fire or let go) ||
| Ctrl+LMB / Ctrl+RMB (hold) | that hand's alternate: for the grapple, reel in ||
| Alt+LMB / Alt+RMB (hold)   | that hand's second alternate: for the grapple, pay out ||
| Mouse4 / Mouse5 (hold) | reel in / pay out, whichever hand holds the grapple ||
| R            | reload from your ammo (main hand first; a dry gun reloads itself) ||
| 1-4          | stance: by default weapon slot 1 + grapple, slot 2 + grapple, slot 2 braced, grapple alone ||
| I            | the menu, on your inventory         ||
| Escape       | the menu (frees the mouse)          ||
| F1           | tuning panel (frees the mouse)      ||
| L            | debug: ship lurches forward         ||
| K            | debug (host): a loot crate at your feet ||

The menu (Escape) has its pages. **Inventory** shows what you carry, lets you
sort and drop it, and lets you choose which weapon slot (or the grapple) each
stance puts in each hand; the stances are saved to `stances.ron`. Aboard our
ship, **Armory** (your stash) and **Workbench** are there too (pressing the
locker or the bench opens them).
**Controls** remaps every action: click a binding and press the key, button or
chord you want. Right-click clears a binding. The game goes on while the menu
is open.

The controls above are the defaults. The menu saves your changes to a
`keybinds.ron` next to the game, which you can also write by hand. It lists
only the actions you change, each with up to two bindings. A binding is a key
or mouse button with optional modifiers (Shift, Ctrl, Alt, Tab, CapsLock):

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
  - **Scenarios** are the authority's, one module each under
    `sim/scenarios/` with a common shape: build a world, judge it (systems
    that run only while it's on), and say when to go back to the field.
    `sim/mission.rs` is what they share: button presses are player events
    (`Pressed`) that the `Director` acts on after the players move; a switch
    despawns the world, spawns the scenario's, and respawns every player on
    deck (`Respawned`, a view relocation). `Mission` is replicated for the
    HUD: the objective, a status board, markers (label, place, kind, for an
    objective-marker list), the banner, the report and the debrief, and the
    report as numbers (`results`: `won`, `seconds`, and each scenario's
    own, such as `survivors_home`) for whatever pays out on them. Markers
    name their body by `BodyId` (given by the authority, replicated with the
    body) and their player by deck slot, never by entity, so a client that
    hasn't received the body yet finds nothing rather than something
    wrong.
  - **Devices** (`sim/devices.rs`) are placed by designs as data (see
    [docs/ships.md](docs/ships.md)) and wired by conditions over each other.
    They're pressed through the same interact path as buttons; the
    authority applies presses and settles every device once a tick, and the
    state (`Devices`, on the ship) is replicated. Shut doors are part of
    the ship's collider on every peer.
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
- Crosshair ring + distance when a surface is within grapple range; the
  interact prompt (key and label) beside the crosshair when a click would
  press something.
- Camera shake on impacts and rip-offs, a dip on landing, FOV kick on boost.

## Tuning

All feel parameters are in `sim/tuning.rs` and appear in the F1 panel. "Save"
writes `tuning.ron` in the working directory, which is loaded at startup.

## Working notes

- [TheBook.md](TheBook.md) is a ledger of friction: every time a library or
  one of our subsystems gets in the way, it gets an entry, so that a
  pattern of small costs shows up in review before it's a big one.
- Plans live in `docs/plan-*.md`; parked ideas in `docs/spitball.md`.
