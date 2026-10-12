# Plan: keeping weapons, and persistent progression

The user's ruling (2026-10-12): "weapons should be kept permanently by the
player. this should go along with a persistent progression system of some
kind." Everything below the ruling is the lead's proposal; the user can
veto any of it.

## What persists, and where

- **A profile per player, on that player's own machine** (`profile.ron`
  beside `keybinds.ron`): their stash of weapons, their currency, their
  rank, and what they've unlocked. Co-op is with friends over direct IP, so
  we trust each client's profile, as Deep Rock does. There's no server-side
  account.
- **On joining**, the client sends its profile to the host. During a
  session the host is authoritative over it (the profile is part of what
  the host simulates). At the end of each mission, and when the player
  leaves, the host sends the updated profile back and the client saves it.
  If the host vanishes mid-mission, the client keeps the last profile it
  was sent.
- A scenario switch no longer resets players to the base kit: you keep what
  you carry.

## Keeping weapons

- **Every weapon you pick up is yours.** It goes into your inventory as now.
  At the end of a mission, everything you carry (and everything you own but
  left behind) is in your stash.
- **The weapons locker becomes your armory**: it opens your stash, and you
  choose what goes into your three weapon slots. Standard issue (an SMG and
  a rifle) is always available, free, so nobody is ever unarmed.
- **Death**: as now, what you carried drops where you died, and you can
  recover it during the mission. *Default chosen: gear you didn't recover
  goes back to your stash when the mission ends.* "Kept permanently" is
  read strictly, so the stake is losing the gun for the rest of the mission
  (the walk back to your body, under fire), never forever. The other option
  is "unrecovered gear is lost", which has higher stakes and is easy to
  switch to.
- **Other players' guns**: a pickup someone else dropped on death is theirs
  and can't be taken. Loot (crates, robot drops) goes to whoever takes it.
  If wanted later: dropping a gun of yours for a crewmate on purpose
  transfers it.
- **The stash has a cap** (e.g. 40 weapons), so it doesn't grow forever;
  past the cap you have to scrap.

## Progression

Three tracks, each tied to something the player does.

1. **Salvage (currency).** Earned from missions (base pay, plus bonuses
   from the scenario's report: objectives, speed, nobody left drifting) and
   from scrapping weapons you don't want (rarer guns are worth more).
   Spent at the workbench.
2. **The workbench (weapon upgrades)**, on our ship beside the armory:
   - **Tune**: spend salvage to raise a weapon's tier by one (Common, then
     Uncommon, then Rare...), which re-rolls nothing; it scales the rolled
     strengths up and, at Rare and Exotic, adds perks. Expensive at the top.
   - **Refit**: swap one part for another variant you have unlocked. Parts
     unlock by scrapping a gun that has them. This is the looter-shooter
     loop: you find a gun you like apart from its barrel, so you fit the
     barrel from a gun you scrapped.
   - Both change the weapon's stored description (a refit stores the part
     overrides next to the seed), so a weapon is still a few bytes.
3. **Rank (experience)**: earned per mission, more for wins and side
   objectives. Each rank unlocks something you can feel, not a percentage:
   a fourth weapon slot, more pack slots, a suit health tier, stronger
   jetpack thrust, a longer grapple reel, new paint, and later new frames
   (the shotgun and sniper of wave 4) so they arrive one by one.

## Building it

One agent, after the scenarios and HUD branches are merged (it touches
items, mission rewards, the net join handshake, the ship design for the
armory and workbench, and the menu). The order:

1. Profile type and file, with the handshake (send on join, the host keeps
   it, sent back at mission end).
2. Keeping weapons: the armory, the stash, death recovery, the scenario
   switch no longer resetting.
3. Salvage and mission pay, read from the scenario reports.
4. The workbench (tune, refit, scrap).
5. Rank and unlocks.

Tests for each, including netplay (two clients with different profiles,
end of mission, profiles back in each client's file).
