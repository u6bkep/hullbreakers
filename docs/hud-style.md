# HUD style

The rules every HUD element follows. The tokens and the shared painters
are in `src/client/hud/style.rs`; use them, never a raw colour or size.

## The idea: a crew suit's visor

The HUD is what a work suit projects on the inside of its visor. Crew gear
is clean, human-scaled and practical (see [art-style.md](art-style.md)), so
the HUD is too: thin light lines and condensed caps, laid around the edge
of the view, with the centre left for the world. It has no opaque panels
and no rounded "app" boxes. It reads like an instrument, not a menu.

Two backgrounds it has to work over: black space, where a hairline glows,
and a sunlit hull or a bright rock, where pale ink vanishes. So:

- **Every block of readouts sits on a scrim**: a translucent dark plate
  (`SCRIM`) with cut corners, just big enough for what's on it.
- **Loose text in the world** (markers, labels, the prompt) gets a dark
  shadow one point down and right (`label_shadowed`).
- **Lines that matter get a glow**: a wider, faint stroke of the same
  colour under the line (`glow_line`), like light on a visor.

## Type

Two faces, both OFL, in `assets/fonts/` with their licences:

- **Rajdhani SemiBold** (`ui`) for words: labels, names, objectives. Labels
  are UPPERCASE with a little letter spacing (`caps`). Sentences (the
  debrief, the prompt) are sentence case.
- **Rajdhani Bold** (`ui_bold`) for titles and banners.
- **Share Tech Mono** (`num`) for every number that changes (ammo, health,
  distances, speeds, timers), so digits don't jitter as they count.

| Token | Size (pt) | Face | Use |
|---|---|---|---|
| `MICRO` | 12 | ui caps, spaced | tags, unit labels, hand/key labels |
| `SMALL` | 14 | ui / num | secondary readouts, marker labels |
| `BODY` | 17 | ui | objectives, prompts, the debrief |
| `TITLE` | 22 | ui_bold | card names, the debrief title |
| `NUMERAL` | 34 | num | the mag count |
| `DISPLAY` | 52 | ui_bold caps, spaced | banners (CARRIER DESTROYED, SUIT FAILURE) |

Sizes are points at 1080 lines. The HUD scales with the window's height
(`hud_zoom`: 0.8 at 720 lines up to 2 at 2160), so it covers the same share
of the screen at any resolution.

## Colour

Colour is not a faction code (art-style.md): the HUD never tints a robot
or paints anything "enemy red" on sight. Colour says what a mark *is
about*, and the hues are few, so each one means one thing:

| Token | Hex | Role |
|---|---|---|
| `INK` | `#e2f2f4` | neutral: text, frames, the crosshair |
| `INK_DIM` | `#8ea6ae` | labels, secondary values, inactive things |
| `INK_FAINT` | INK at 22% | rules, bar tracks, frame outlines |
| `SCRIM` | `#05080b` at 62% | the plate behind every block |
| `SUIT` | `#4fd6ff` | your own suit's systems: reload, regen, the active stance, nav (reference body, prograde, grapple range) |
| `FRIENDLY` | `#66e39e` | the crew and our ship, our hull |
| `HOSTILE` | `#ff5544` | known threats only: a ship the mission names as the enemy, where damage came from, critical health |
| `WARNING` | `#ffa12e` | caution: low ammo, low health, "get clear" |
| `OBJECTIVE` | `#ffd84a` | what to do next: objective markers and text |
| `SHIELD` | `#6ea8ff` | shields |
| `WEAK` | `#ff40c8` | **weak-spot hits only.** The one colour rule in the game (art-style.md). Nothing else in the HUD may use magenta or anything near it. |

Rarity accents (the weapon card's strip and name), in order: common
`INK_DIM`, uncommon `#7ee08a`, rare `#5aa9ff`, epic `#a98bff` (a blue
violet, well clear of `WEAK`), legendary `#ffb03a`. They only ever appear
on a card or in the menu.

Red is rare on purpose: it is what hurt you, or what's about to kill you.
If everything is red, nothing is.

## Lines and shapes

- **Weights**: `HAIR` 1 pt (frames, rules, tracks), `LINE` 1.5 pt
  (crosshair, markers, brackets), `BOLD` 2.5 pt (objective markers,
  damage arcs). Nothing thicker; mass comes from fills.
- **The cut corner** is the motif: plates have their top-left and
  bottom-right corners cut at 45° (`CUT` = 6 pt), with a short `SUIT`
  tick along the top edge from the cut. Nothing in the HUD has a rounded
  corner.
- **Brackets, not boxes**, for things in the world: four L-corners
  (`brackets`) round the reference body; a diamond for a ship; a
  chevroned diamond for an objective. Off screen, each becomes a
  notched arrow at the edge of the view, pointing at it, with its label.
- **Bars are segmented** (`seg_bar`): health in 10, hulls in 20, so a
  glance reads them as counts. A bar's track is `INK_FAINT`; what was just
  lost shows white and drains away.

## Motion

Nothing moves at rest. Motion only ever means "this just changed".

- **Values ease**, they don't jump: bars follow their value with a 0.1 s
  time constant (`ease`).
- **Losses linger**: the white trail of lost health holds 0.35 s, then
  drains at 1.5 bars a second.
- **Appear fast, leave slow**: 0.12 s in (`FADE_IN`), 0.4 s out
  (`FADE_OUT`).
- **Blinking** is for a state you must fix now (an empty mag, critical
  health): `pulse` at 1.6 Hz while it lasts, never otherwise.
- **Hits**: markers last 0.25 s (0.5 s for a kill); a damage arc 1.2 s.
- No sliding, bouncing or spinning.

## Layout

Reference frame 1920x1080 points; margins `EDGE` = 24 pt.

```
+---------------------------------------------------------------+
| [feed]           [ HULL ====  objective  CARRIER ==== ]        |
|                         timer                                  |
|                                                                |
|                    ( damage arcs ring )                        |
|                         +  [F] prompt                          |
|                     rope gauge                                 |
|                                                                |
| [suit: health,                              [stances 1 2 3 4]  |
|  regen, mode, rel v]                        [OFF card][MAIN]   |
+---------------------------------------------------------------+
```

- **Centre** (a circle of 12% of the height round the crosshair): only the
  crosshair, hit markers, the grapple ring and rope gauge, and the
  interact prompt to the right of it. Nothing else may live here.
- **Damage arcs** sit on a ring at 26% of the height.
- **Top centre**: the mission strip (objective, timer, the ships' bars).
  Banners and the debrief below it, in the upper third.
- **Top left**: the event feed (what your boots and rope just did).
- **Bottom left**: the suit block: health, regen, boots/EVA, assist,
  speed relative to the reference body, rope.
- **Bottom right**: the hands: a card per hand as you hold them (off hand
  left, main right) and the stance strip above.
- **The edge** of the screen is for off-screen arrows and the low-health
  vignette.

World markers can be anywhere but fade out as they cross the centre
circle, so they never sit on the crosshair.

## Adding an element

Pick its zone, use the tokens and painters, give it a scrim if it has
more than one line, check it over black space and over the lit hull at
1280x720 and 1920x1080, and keep the HUD's frame cost under 0.3 ms.
