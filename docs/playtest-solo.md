# Solo playtest of the scenarios (2026-10-12)

One player, a fresh profile, 1280x720 in the headless harness. Every leg was
played by real inputs: walking, EVA thrust, the grapple and reel, shooting,
and F/G at devices, from the mission wall onward. Nothing was teleported or
scripted into the game state. The inputs came from per-leg scripts that steer
by the client's own view of the world. That view is written by
`HULLBREAKERS_TELEMETRY=<file>` (src/client/telemetry.rs), and
`tools/hl-aim <telemetry> <target> [--go]` turns the mouse toward a point,
marker or ship-local spot. Harness times include the tooling's own fumbling,
so the human estimates are my judgement.

| Scenario | Finished? | Harness time | Likely human time | Verdict |
|---|---|---|---|---|
| Grapple run | yes | 1:34 (board) | 0:45–1:00 | Too easy; the grapple is never needed |
| Dead ship | yes (after fixes) | 1:38 | 3–4 min first time | Good once the two blockers were fixed |
| Recovery | **no** (3 tries) | — | ? | The tow ran away (fixed); getting a pod in through the door is fiddly |
| Escort | yes | 8:27 (two deaths, much fumbling) | 3–4 min | Was a death loop solo (fixed); otherwise fair |
| Boarding | **no** (3 tries, never got aboard) | — | ? | Doesn't look soloable as tuned |

## Fixes made

- **Our ship is parked beside static sites** (`ship::parked_home_ship`, used by
  the dead ship, escort, grapple run and recovery). It used to roll 2°/s
  beside a derelict at rest: after a few minutes it had rolled ~105°, so a
  player stepping off arrived rolled relative to the derelict, and the EVA
  capsule, which follows the view, lay sideways and didn't fit the 1.8 m
  airlock. The player had to roll level (E) first. Field and gallery keep
  the drift.
- **Dead ship: key B holds 10 s for a lone player** (was 5). The route from
  B to A is 27 m with 7 turns through four doorways. A perfect sprint is
  about 2 s; a first-timer needs 6–10 s. My tooling made it in 5.1 s with
  the 10 s hold and failed at 5 s.
- **A tow no longer runs away** (player.rs, `fire_grapple`). Hooking a
  towable body (a survivor or lifeboat) made it the flight assist's
  reference body, so the speed limit rode along with the load. A solo tow
  of the lifeboat reached 22 m/s and couldn't be stopped. Now the assist
  keeps flying relative to what you were flying by. Test:
  `a_tow_keeps_the_assist_flying_by_our_ship` (fails without the fix).
- **Escort: the freighter lies 33 m off our stern** (was 18 m). Its grappler,
  standing in the freighter's airlock, reached into our hold and as far as
  the respawn spot on our deck. I died three times running: twice
  boarding through its airlock, then once inside our own hold, unarmed
  after respawning, yanked across to the gunners ("yanked off by rope
  (14000 N)" on the HUD). From 33 m it can't reach, and the aft-deck
  gunners can be sniped from our hold door (two kills, no damage taken).
- **Debrief**: the Home/Adrift word shows only for homecoming scenarios. It
  said ADRIFT after a won grapple run (src/client/hud/mission.rs, the HUD
  agent's file; a 4-line change).

## Per scenario

### Grapple run: done, 1:34

- **HUD:** "Fly the gates in order — 3:59 left", status "<callsign> to gate
  1", and a marker on the next gate. The debrief said COURSE CLEAR with the
  pay line. All clear.
- **Pace:** a boosted jetpack (20 m/s) did each gate in about 3 s, and the
  grapple was never needed. About 25 s of the 1:34 was tooling start-up and
  about 20 s was a rock trap (below).
- **Trap:** flying into a rock with boots on lands you and sticks you to
  it; you need jump plus B. New players will hit this.
- **Recommendation:** make the course need the grapple (rocks across the
  straight lines, longer legs or a speed cap). 4:00 is generous.

### Dead ship: done, 1:38 (second run, with the fixes)

| Leg | Time | Objective shown |
|---|---|---|
| Mission wall to aboard | 15.5 s (crossing clean with our ship parked) | "Board the derelict and restart its reactor (engineering, aft)", Airlock and Reactor markers |
| Reactor | 2.3 s | "Route power to the vault: read the power board, set the junctions"; Board, J-A, J-B, J-C markers |
| Junctions (J-A ×2, J-B ×2, J-C ×1) | 14 s | "Turn key B, then race to key A: alone, B holds 10 s", key markers |
| Key B to key A | 5.1 s | "Release the data core in the vault" |
| Clamp, core into the pack | 22 s | "Reactor overload — take the data core and get back to the ship!", "Overload in 1:13", "Aboard: 0 of 1", Ship marker |
| Back to our hold | 30 s (44 s to spare) | "Salvage complete" |

- **Stale marker:** in the first run, an "AIRLOCK 7 m" marker stayed on the
  HUD after the step changed (likely the HUD's fade). For the HUD agent.
- **Boots in EVA:** boots on in EVA kept sticking me to walls (the stern
  wall, the door jamb, the roof). Walking at a doorway climbed me onto the
  roof. The same trap as the grapple run.

### Recovery: not finished (3 tries)

- **HUD:** "Find the survivors by their beacons and tow them into the hold",
  with "Home: 0 of 8 (need 4)" and an air line per pod. Beacons show as
  pinging markers. Clear.
- **Reaching pods is quick:** out to Survivor A at 125 m and hooked in
  13–16 s.
- **The tow ran away** (fixed above). Before the fix, that cost the first
  two tries.
- **Getting the pod in through our 3 m door is the real problem.** The pod
  swings on the rope and catches the door frame or the starboard engine
  nacelle. Twice it ended up wedged between the nacelle and the hull side,
  with me pinned beside it (the rope and the nook), while its air ran out.
- **Air:** A's 2:20 leaves room for one careful trip. The lifeboat (four
  aboard, 5:00 of air, and on its own enough to win) drifts out to about
  230 m.
- **Recommendation (game design, not fixed):** count a pod as home once it's
  in a generous catch zone at the open door (say up to 6 m astern of it,
  within its width), since the survivors climb in. Or widen the door.
  Solo, the lifeboat first is the winning line; the objective could hint
  at it ("the lifeboat holds four").

### Escort: done, 8:27 harness (realistic 3–4 min)

- **Start:** a fresh profile starts **unarmed**. The HUD says "UNARMED ·
  WEAPONS LOCKER, AFT HOLD" (good). In the locker you click "Take an issue
  SMG/Rifle". After a death the same applies, and your dropped kit is
  marked "YOUR KIT".
- **Before the freighter moved:** three deaths, the last to the grappler's
  yank (fixed above).
- **After the move:**
  - two aft-deck gunners sniped from our hold door: 52 s, no damage;
  - across in 5 s;
  - the two hall robots cleared in 8 s from the aft edge of the breach.
- **Legs:**
  - Across the breach: jetpack. My lever presses for the gangway didn't
    take, three times. The same lever kind works in game (I flipped our
    turret breaker and back), and a sim test from my standing spots passes.
    So I put it down to my aim and reach, not a bug.
  - The bay: the hall key gives 3 s to get in. Getting out alone with the
    cell: set it down by the door (G), turn the bay key, then step into
    the doorway, which holds it open, and reach the cell from there (F).
    That works well once seen. Nothing tells you about the doorway trick
    except the README.
  - With the cell, I left over the breach by jetpack (no gangway needed)
    and crossed to our hold: "Power restored".
- **HUD:** "Board the freighter and bring the power cell from its bay to
  our hold", then "Carry the power cell to our hold: hands full, so no
  guns, grapple or switches". Status lines: cell, robots aboard, gangway.
  The hands-full panel is clear.
- **Recommendation:** the status could add "stand in the doorway to hold it
  open" while the door is shut and the cell is in the bay.

### Boarding: not finished (3 tries, never aboard)

- **HUD:** our hull %, the clock and the carrier's hull (SHIELDED), with
  "Board the carrier and disable its shield on the bridge". Markers:
  Carrier, Hangar, Shield console. Clear.
- **Our hull went at about 3:00–3:20 every time.** A hit knocked out the
  main bus at about 2:00 ("DAMAGE: Main bus knocked out"), dropping our
  shield. That happened even with the only crew member at the door.
- **The carrier's passes** (distance from our ship, sampled each second):
  it closed from ~370 m to ~150–160 m at about 60 s intervals. I never
  saw it nearer than ~150 m, though `duel_near` is 100 m.
- **Chasing by jetpack doesn't work:** after 60 s of boosting at it I was
  still 380 m off. The two ships' relative motion matches the jetpack's
  top speed.
- **The rope does reach:** I hooked the carrier at 120 m on a pass
  (`grapple_range` is 200 m). But I was dead 4 s into the reel and
  respawned on our deck. My guess is the beams: EVA between the two ships
  is the beams' line, and three hits kill. I didn't confirm the cause.
- **Recommendation (design call):** as tuned, a lone player can't board.
  To give solo a chance:
  - an earlier, closer first pass;
  - beams that hold fire, or miss, along a boarding rope;
  - more hull time.

  Even aboard, the carrier holds 7 gunners, 3 turrets, 2 grapplers and 6
  swarmers, against two gunners and a grappler that killed me three times
  on the freighter.

## Other notes

- **Boots climb everything:** boots-on walking climbs crates and walls,
  even onto the ceiling. Our hold's crate at (2.4, 6) caught me several
  times on the way to the locker. Boots-on landing in EVA sticks you to
  the nearest surface (I landed on our hull's underside once).
- **Mission wall buttons need precise aim** at 1.4 m: it took 2–3 aim
  passes.
- **Kit:** it is kept across scenario switches; a death drops it.
