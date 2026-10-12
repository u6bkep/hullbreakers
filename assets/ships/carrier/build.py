"""Carrier: the enemy ship of the boarding mission. Bevy space: +Y up, bow toward -Z,
origin = the layout origin (the middle of the lower deck's floor, amidships).

The game collides against the boxes in layout.json (generated from the design,
assets/ships/carrier/*.ron) and draws this model instead of them, so:
  - every surface a player walks on or bumps into is a layout box face, to within 2 cm;
    the structural boxes below are read straight from layout.json (and asserted, so a
    layout change fails the build instead of drifting);
  - interior details stand off a walkable face by 3 cm at most, below 2.3 m above a deck;
    overhead (ribs, conduits, light fixtures) and outside the hull they may go further;
  - no turrets and no shield button: the game draws those.
check_layout.py (beside this) loads layout.json and the exported .glb and checks all of it.

The hull's skin boxes are split into an inner 0.1 m layer, owned by the room it faces
(its paint), and an outer 0.2 m layer, owned by `exterior` (its paint). Parts are the
regions, for frustum culling; none moves.
"""
import json
import math
import os

import hb

a = hb.Asset(__file__)

PAINT, HAZ, DARK, BARE = "paint_primary", "paint_secondary", "metal_dark", "metal_bare"
OXIDE, BLACK = "polymer", "rubber"           # oxide-red primer; black hazard bands
SCREEN, LIGHT = "emissive", "emissive_secondary"

W, UP, TOP, L = 6.0, 3.3, 6.3, 30.0          # half width, upper deck, roof underside, half length
WALL_X = 1.9                                 # corridor wall inner faces, |x|
STAND = 0.03                                 # the most a detail stands off a walkable face

# ------------------------------------------------------------------ the layout
with open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "layout.json")) as f:
    LAYOUT = json.load(f)
BOXES = LAYOUT["boxes"]


def lbox(i):
    """(lo, hi) of unrotated layout box i."""
    b = BOXES[i]
    assert b["rotation"] == [0.0, 0.0, 0.0, 1.0], f"layout box {i} is rotated"
    c, s = b["center"], b["size"]
    return (tuple(round(c[k] - s[k] / 2, 4) for k in range(3)),
            tuple(round(c[k] + s[k] / 2, 4) for k in range(3)))


def expect(i, lo, hi, material="hull"):
    """Fail the build if layout box i isn't where this script assumes."""
    glo, ghi = lbox(i)
    if any(abs(p - q) > 1e-3 for p, q in zip(glo + ghi, tuple(lo) + tuple(hi))) or BOXES[i]["material"] != material:
        raise SystemExit(f"layout.json box {i} is {glo}..{ghi} ({BOXES[i]['material']}), the build assumes"
                         f" {lo}..{hi} ({material}): layout changed, update build.py")


# The skin and the furniture this script models by hand rather than straight from the file.
expect(0, (-6.3, -0.3, -30.3), (6.3, 0.0, 30.3))      # floor
expect(1, (-6.3, 6.3, -30.3), (6.3, 6.6, 30.3))       # roof
expect(2, (-6.3, 0.0, -30.0), (-6.0, 6.3, 30.0))      # port side
expect(3, (6.0, 0.0, -30.0), (6.3, 6.3, 30.0))        # starboard side
expect(4, (-6.0, 3.0, -30.0), (6.0, 3.3, 14.15))      # the deck between (to the bulkhead's hangar face)
expect(5, (-6.3, 0.0, -30.3), (6.3, 4.5, -30.0))      # bow, under the window slit
expect(6, (-6.3, 5.3, -30.3), (6.3, 6.3, -30.0))      # bow, over it
expect(7, (-6.3, 0.0, 30.0), (-3.0, 6.3, 30.3))       # stern, port of the hangar door
expect(8, (-3.0, 2.8, 30.0), (3.0, 6.3, 30.3))        # over it
expect(9, (3.0, 0.0, 30.0), (6.3, 6.3, 30.3))         # starboard of it
expect(43, (-2.0, 3.3, -27.4), (2.0, 4.3, -26.6), "trim")    # bridge console
expect(49, (-10.0, 1.5, 17.0), (-7.0, 4.5, 31.0))     # engine pods
expect(50, (7.0, 1.5, 17.0), (10.0, 4.5, 31.0))
expect(51, (-7.0, 2.7, 21.0), (-6.3, 3.3, 27.0), "trim")    # pylons
expect(52, (6.3, 2.7, 21.0), (7.0, 3.3, 27.0), "trim")
expect(53, (-1.5, 6.6, -17.0), (1.5, 7.6, 13.0), "trim")    # dorsal spine
expect(54, (-0.5, 7.6, -8.5), (0.5, 8.1, -7.5), "trim")     # dorsal turret base, on the spine
expect(55, (-0.5, -0.8, 3.5), (0.5, -0.3, 4.5), "trim")     # ventral turret base
CRATES = (44, 45, 46, 47, 48)
for i in CRATES:
    assert BOXES[i]["material"] == "trim"
TURRETS = [tuple(t) for t in LAYOUT["turrets"]]
assert TURRETS == [(0.0, 8.1, -8.0), (0.0, -0.8, 4.0)], TURRETS

# The ramp: layout box 16, turned about x. Its top runs from (z 27, y 0) up to (z 15, y 3.3).
RB = BOXES[16]
_th = 2 * math.atan2(RB["rotation"][0], RB["rotation"][3])
_c, _s = RB["center"], RB["size"]
_Y = (math.cos(_th), math.sin(_th))           # local +Y in (y, z)
_Z = (-math.sin(_th), math.cos(_th))          # local +Z in (y, z)


def ramp_pt(sy, sz):
    """(z, y) of the ramp box's edge at local y = sy * half, z = sz * half."""
    y = _c[1] + sy * _s[1] / 2 * _Y[0] + sz * _s[2] / 2 * _Z[0]
    z = _c[2] + sy * _s[1] / 2 * _Y[1] + sz * _s[2] / 2 * _Z[1]
    return (z, y)


RAMP_X = (_c[0] - _s[0] / 2, _c[0] + _s[0] / 2)        # -6.0 .. -3.6
TOP_LO, TOP_HI = ramp_pt(1, 1), ramp_pt(1, -1)         # (27, 0), (15, 3.3)
BOT_LO, BOT_HI = ramp_pt(-1, 1), ramp_pt(-1, -1)
assert abs(TOP_LO[0] - 27) < 1e-3 and abs(TOP_LO[1]) < 1e-3 and abs(TOP_HI[0] - 15) < 1e-3 \
    and abs(TOP_HI[1] - UP) < 1e-3, (TOP_LO, TOP_HI)
SLOPE_N = (_Y[1], _Y[0])                                # top normal in (z, y)
SLOPE_D = ((TOP_LO[0] - TOP_HI[0]), (TOP_LO[1] - TOP_HI[1]))
_sl = math.hypot(*SLOPE_D)
SLOPE_D = (SLOPE_D[0] / _sl, SLOPE_D[1] / _sl)          # down the slope, unit, in (z, y)


def ramp_top(s, off=0.0):
    """(z, y) at distance s down the ramp's top from its upper end, `off` above it."""
    return (TOP_HI[0] + SLOPE_D[0] * s + SLOPE_N[0] * off, TOP_HI[1] + SLOPE_D[1] * s + SLOPE_N[1] * off)


# ------------------------------------------------------------------ helpers
def box(part, lo, hi, mat, bevel=None):
    part.box(lo, hi, mat=mat, bevel=bevel)


def layout_box(part, i, mat, bevel=None):
    lo, hi = lbox(i)
    assert BOXES[i]["material"] == "hull", i
    part.box(lo, hi, mat=mat, bevel=bevel)


def clip(poly, a, b, c):
    """Clip a polygon [(u, v)] to a*u + b*v <= c."""
    out = []
    n = len(poly)
    for k in range(n):
        p, q = poly[k], poly[(k + 1) % n]
        fp, fq = a * p[0] + b * p[1] - c, a * q[0] + b * q[1] - c
        if fp <= 0:
            out.append(p)
        if (fp < 0 < fq) or (fq < 0 < fp):
            t = fp / (fp - fq)
            out.append((p[0] + t * (q[0] - p[0]), p[1] + t * (q[1] - p[1])))
    clean = []
    for p in out:
        if not clean or math.dist(p, clean[-1]) > 1e-4:
            clean.append(p)
    if len(clean) > 1 and math.dist(clean[0], clean[-1]) <= 1e-4:
        clean.pop()
    return clean


def area(poly):
    return 0.5 * abs(sum(poly[i][0] * poly[(i + 1) % len(poly)][1] - poly[(i + 1) % len(poly)][0] * poly[i][1]
                         for i in range(len(poly))))


def rect(part, plane, u0, u1, v0, v1, span, mat, bevel=0):
    part.prism([(u0, v0), (u1, v0), (u1, v1), (u0, v1)], plane=plane, span=span, mat=mat, bevel=bevel)


def stripes(part, plane, u0, u1, v0, v1, span, out=1, width=0.3, flip=False):
    """Diagonal hazard stripes over the rectangle u0..u1 x v0..v1 in `plane`: a black
    plate filling `span`, with yellow stripes laid in its outer skin (the end of span
    toward `out`, +1 or -1). Only the yellow ones are prisms, half the cost of both."""
    lo, hi = sorted(span)
    t = min(0.01, (hi - lo) / 2)
    back, skin = ((lo, hi - t), (hi - t, hi)) if out > 0 else ((lo + t, hi), (lo, lo + t))
    rect(part, plane, u0, u1, v0, v1, back, BLACK)
    sgn = -1 if flip else 1
    base = [(u0, v0), (u1, v0), (u1, v1), (u0, v1)]
    vals = [sgn * u + v for u, v in base]
    lo_, hi_ = min(vals), max(vals)
    ph = 0.0371 * width                      # an odd phase: no stripe plane lands on a chamfer by chance
    k = 2 * math.floor((lo_ - ph) / (2 * width))
    while ph + k * width < hi_:
        c0, c1 = ph + k * width, ph + (k + 1) * width
        poly = clip(clip(base, sgn, 1, c1), -sgn, -1, -c0)
        if len(poly) >= 3 and area(poly) > 1e-5:
            part.prism(poly, plane=plane, span=skin, mat=HAZ, bevel=0)
        k += 2


def door_frame(part, normal, face, s, u0, u1, v0, v1, w=0.18, mat=HAZ, hazard=False, light=0.0, d=STAND):
    """A frame of plates standing `d` off a wall around the doorway u0..u1 (along the
    wall) x v0..v1 (y). normal: the wall's normal axis ('x' or 'z'); face: its face
    coordinate; s: +1/-1, the side the frame stands out to. The doorway itself stays
    exactly the layout's gap. hazard: diagonal stripes instead of plain plates; light: a
    lit strip this wide along the opening's edge (sides and top)."""
    plane = "xy" if normal == "z" else "zy"
    span = tuple(sorted((face, face + s * d)))
    pieces = [(u0 - w, u0, v0, v1), (u1, u1 + w, v0, v1), (u0 - w, u1 + w, v1, v1 + w)]
    if light:
        rect(part, plane, u0 - light, u0, v0, v1, span, LIGHT)
        rect(part, plane, u1, u1 + light, v0, v1, span, LIGHT)
        rect(part, plane, u0 - light, u1 + light, v1, v1 + light, span, LIGHT)
        pieces = [(u0 - w, u0 - light, v0, v1 + light), (u1 + light, u1 + w, v0, v1 + light),
                  (u0 - w, u1 + w, v1 + light, v1 + w)]
    for k, (a0, a1, b0, b1) in enumerate(pieces):
        if hazard:
            stripes(part, plane, a0, a1, b0, b1, span, out=s, flip=(k == 1))
        else:
            rect(part, plane, a0, a1, b0, b1, span, mat)


def lamp(part, x, y, z, ceil, hx=0.35, hz=0.22):
    """A ceiling fixture over the game's point light at (x, y, z): its lit face 5 cm above."""
    box(part, (x - hx, y + 0.08, z - hz), (x + hx, ceil, z + hz), DARK, bevel=0)
    box(part, (x - hx + 0.05, y + 0.05, z - hz + 0.05), (x + hx - 0.05, y + 0.08, z + hz - 0.05), LIGHT, bevel=0)


def portal(part, z, floor, ceil, xin=WALL_X, gusset=0.4, depth=0.25, hw=0.12, mat=DARK):
    """A rib across a corridor at z: pilasters STAND proud up to 2.3 m above the floor,
    knee gussets and a beam overhead."""
    knee = floor + 2.3
    for s in (-1, 1):
        box(part, (s * xin, floor, z - hw), (s * (xin - STAND), knee, z + hw), mat, bevel=0)
        part.prism([(s * xin, knee), (s * xin, ceil), (s * (xin - gusset), ceil)], plane="xy",
                   span=(z - hw, z + hw), mat=mat)
    box(part, (-xin, ceil - depth, z - hw), (xin, ceil, z + hw), mat)


def wall_band(part, x_face, s, zs, y0, y1, mat, d=0.015):
    """A band standing d off the wall face x = x_face (toward s) over each z range."""
    for z0, z1 in zs:
        box(part, (x_face, y0, z0), (x_face + s * d, y1, z1), mat, bevel=0)


def cabinet(part, axis, face, s, u0, u1, v0, v1, light=True):
    """A wall-flush equipment cabinet: a 2 cm plate with a 1 cm status strip on it."""
    plane = "xy" if axis == "z" else "zy"
    rect(part, plane, u0, u1, v0, v1, tuple(sorted((face, face + s * 0.02))), DARK)
    if light:
        um = (u0 + u1) / 2
        rect(part, plane, um - 0.25, um + 0.25, v1 - 0.22, v1 - 0.15,
             tuple(sorted((face + s * 0.02, face + s * 0.03))), LIGHT)


def pipe(part, axis, center, r, span, mat=DARK):
    """Overhead pipe: a deliberate hexagon, low-poly."""
    part.cylinder(axis, center, r, span, mat=mat, segments=6, bevel=0)


# ================================================================== exterior
ext = a.part("exterior", bevel=0.04)
# The outer 0.2 m of the skin (no bevel: its faces are the collision; the armour carries
# the chamfers). Its corners are filled out to the full 12.6 x 60.6 m box.
box(ext, (-6.3, -0.3, -30.3), (6.3, -0.1, 30.3), DARK, bevel=0)                 # belly
box(ext, (-6.3, 6.4, -30.3), (6.3, 6.6, 30.3), DARK, bevel=0)                   # roof
for s in (-1, 1):
    box(ext, (s * 6.1, -0.1, -30.3), (s * 6.3, 6.4, 30.3), DARK, bevel=0)       # sides
box(ext, (-6.1, -0.1, -30.3), (6.1, 4.5, -30.1), DARK, bevel=0)                 # bow under the slit
box(ext, (-6.1, 5.3, -30.3), (6.1, 6.4, -30.1), DARK, bevel=0)                  # bow over it
box(ext, (-6.1, -0.1, 30.1), (-3.0, 6.4, 30.3), DARK, bevel=0)                  # stern
box(ext, (3.0, -0.1, 30.1), (6.1, 6.4, 30.3), DARK, bevel=0)
box(ext, (-3.0, 2.8, 30.1), (3.0, 6.4, 30.3), DARK, bevel=0)

# Chines: rails along the four long edges, and posts up the bow and stern corners.
for s in (-1, 1):
    for y0, y1 in ((-0.45, -0.15), (6.45, 6.75)):
        box(ext, (s * 6.1, y0, -30.45), (s * 6.45, y1, 30.45), DARK)
    for z0, z1 in ((-30.45, -30.1), (30.1, 30.45)):         # (clear of the window slit's ends)
        box(ext, (s * 6.0, -0.45, z0), (s * 6.45, 6.75, z1), DARK)

# Flank armour: two rows of blue-grey plates with dark seams; oxide plates fore and aft.
ROWS = ((0.25, 2.95), (3.35, 6.05))
NPL = 8
pz = [-29.9 + k * (59.8 / NPL) for k in range(NPL + 1)]
for s in (-1, 1):
    for r, (y0, y1) in enumerate(ROWS):
        for k in range(NPL):
            mat = OXIDE if (r == 1 and k == 0) or (r == 0 and k == NPL - 1) else PAINT
            box(ext, (s * 6.3, y0, pz[k] + 0.08), (s * 6.42, y1, pz[k + 1] - 0.08), mat, bevel=0.03)
    # a running light in the deck seam, forward
    box(ext, (s * 6.3, 3.08, -24.0), (s * 6.37, 3.22, -14.0), LIGHT, bevel=0)

# Ring frames: the structure showing through the skin at the bulkheads, standing proud of
# the armour so the outline steps; a running light on each one's flanks.
for z in (-24.0, -12.0, 0.0, 14.0):
    ext.frame((-6.65, -0.7, z - 0.35), (6.65, 7.0, z + 0.35), 0.4, "z", mat=DARK)
    for s in (-1, 1):
        box(ext, (s * 6.65, 2.9, z - 0.08), (s * 6.69, 3.4, z + 0.08), LIGHT, bevel=0)

# Roof and belly plates (the spine sits on the roof ones).
NR = 6
rz = [-29.9 + k * (59.8 / NR) for k in range(NR + 1)]
for s in (-1, 1):
    for k in range(NR):
        box(ext, (s * 0.1, 6.6, rz[k] + 0.08), (s * 5.9, 6.7, rz[k + 1] - 0.08), PAINT, bevel=0.03)
        box(ext, (s * 0.1, -0.4, rz[k] + 0.08), (s * 5.9, -0.3, rz[k + 1] - 0.08), PAINT, bevel=0.03)
for z in (-20.0, 20.0):                                                         # belly lights
    box(ext, (-0.3, -0.44, z - 0.1), (0.3, -0.4, z + 0.1), LIGHT, bevel=0)

# Bow: an oxide jaw under the window slit and a brow over it, so the slit reads as the
# ship's eye; running lights at the brow's tips. Neither covers the slit (y 4.5..5.3).
ext.prism([(-30.3, 4.45), (-31.0, 3.9), (-31.25, 2.6), (-31.1, 0.6), (-30.6, -0.42), (-30.3, -0.42)],
          plane="zy", span=(-5.6, 5.6), mat=OXIDE, bevel=0.05)
ext.prism([(-30.3, 5.32), (-31.15, 5.5), (-31.15, 5.85), (-30.6, 6.72), (-30.3, 6.72)],
          plane="zy", span=(-6.0, 6.0), mat=PAINT, bevel=0.05)
for s in (-1, 1):
    box(ext, (s * 5.35, 5.58, -31.19), (s * 5.95, 5.78, -31.1), LIGHT, bevel=0)
    box(ext, (s * 6.1, 0.4, -30.48), (s * 6.35, 4.2, -30.45), LIGHT, bevel=0)   # bow corner posts

# Stern: the hangar door's outer frame, hazard-striped, lit round the opening; a lit sill;
# an oxide field over the door; armour either side.
FD = (30.3, 30.65)                                                               # frame depth
ext.prism([(-3.75, -0.3), (-3.0, -0.3), (-3.0, 2.8), (3.0, 2.8), (3.0, -0.3), (3.75, -0.3), (3.75, 3.55),
           (-3.75, 3.55)], plane="xy", span=FD, mat=DARK)
door_frame(ext, "z", FD[1], 1, -3.0, 3.0, -0.3, 2.8, w=0.75, hazard=True, light=0.1, d=0.02)
box(ext, (-3.0, -0.27, 30.3), (3.0, -0.12, 30.33), LIGHT, bevel=0)              # sill light
box(ext, (-3.6, 3.75, 30.3), (3.6, 6.2, 30.42), OXIDE, bevel=0.03)
for s in (-1, 1):
    box(ext, (s * 3.95, 0.25, 30.3), (s * 5.85, 6.2, 30.42), PAINT, bevel=0.03)

# Dorsal spine (layout box 53): strakes flare its base, lights at its ends, the dorsal
# turret's base (layout box 54) on top.
box(ext, *lbox(53), DARK)
box(ext, (-1.3, 7.6, -16.75), (1.3, 7.68, 12.75), PAINT, bevel=0.03)
for s in (-1, 1):
    ext.prism([(s * 1.5, 6.6), (s * 2.3, 6.6), (s * 1.5, 7.25)], plane="xy", span=(-16.7, 12.7), mat=PAINT)
box(ext, *lbox(54), BARE)                                                       # dorsal turret base
box(ext, (-0.8, 7.2, -17.03), (0.8, 7.35, -17.0), LIGHT, bevel=0)
box(ext, (-0.8, 7.2, 13.0), (0.8, 7.35, 13.03), LIGHT, bevel=0)
box(ext, *lbox(55), BARE)                                                       # ventral turret base

# ================================================================== engines
eng = a.part("engines", bevel=0.04)
for s in (-1, 1):
    lo, hi = lbox(49 if s < 0 else 50)
    box(eng, lo, hi, DARK)                                                      # the pod's box
    xo, xi = s * 10.0, s * 7.0                                                  # outer, inner faces
    box(eng, (xo, 1.7, 17.4), (xo + s * 0.12, 4.3, 30.6), PAINT, bevel=0.03)   # cowl
    box(eng, (s * 7.2, 4.5, 17.4), (s * 9.8, 4.62, 30.6), PAINT, bevel=0.03)
    box(eng, (s * 7.2, 1.38, 17.4), (s * 9.8, 1.5, 30.6), PAINT, bevel=0.03)
    box(eng, (xo, 1.6, 27.6), (xo + s * 0.17, 4.4, 28.4), OXIDE, bevel=0.03)   # oxide band
    box(eng, (s * 7.1, 4.5, 27.6), (s * 9.9, 4.67, 28.4), OXIDE, bevel=0.03)
    box(eng, (s * 7.1, 1.33, 27.6), (s * 9.9, 1.5, 28.4), OXIDE, bevel=0.03)
    box(eng, (xo + s * 0.12, 3.95, 22.0), (xo + s * 0.16, 4.1, 27.2), LIGHT, bevel=0)   # running light
    # intake: a frame round the pod's front with three vanes
    eng.frame((s * 7.2, 1.7, 16.6), (s * 9.8, 4.3, 17.05), 0.25, "z", mat=DARK)
    for y in (2.3, 2.9, 3.5):
        box(eng, (s * 7.45, y, 16.75), (s * 9.55, y + 0.15, 17.0), BARE, bevel=0)
    # nozzle: an octagonal bell with a glowing throat
    eng.lathe([(1.0, 31.0), (1.3, 32.3), (1.18, 32.3), (0.85, 31.35), (0.0, 31.35)], axis="z",
              center=(s * 8.5, 3.0), mats=[DARK, BARE, LIGHT, LIGHT], segments=8, bevel=0.03)
    # pylon to the hull (covers layout box 51/52, which stops 0.2 m short of the hull)
    eng.hull([(s * 6.25, 2.35, 19.6), (s * 6.25, 3.65, 19.6), (s * 6.25, 2.35, 28.4), (s * 6.25, 3.65, 28.4),
              (s * 7.05, 2.6, 20.4), (s * 7.05, 3.4, 20.4), (s * 7.05, 2.6, 27.6), (s * 7.05, 3.4, 27.6)], mat=DARK)

# ================================================================== hangar (z 14..30)
hg = a.part("hangar", bevel=0.015)
box(hg, (-W, -0.1, 14.0), (W, 0.0, 30.0), DARK, bevel=0)                       # floor
box(hg, (-W, TOP, 14.0), (W, TOP + 0.1, 30.0), DARK, bevel=0)                  # ceiling
for s in (-1, 1):
    box(hg, (s * W, 0.0, 14.0), (s * (W + 0.1), TOP, 30.0), PAINT, bevel=0)    # side walls
box(hg, (-W - 0.1, -0.1, 30.0), (-3.0, TOP + 0.1, 30.1), PAINT, bevel=0)       # stern, inside
box(hg, (3.0, -0.1, 30.0), (W + 0.1, TOP + 0.1, 30.1), PAINT, bevel=0)
box(hg, (-3.0, 2.8, 30.0), (3.0, TOP + 0.1, 30.1), PAINT, bevel=0)
box(hg, (-3.0, -0.1, 30.0), (3.0, 0.0, 30.3), DARK, bevel=0)                   # door sill
for i in range(10, 16):                                                         # forward bulkhead
    layout_box(hg, i, DARK)
# hazard frames on the doors, both sides of the stern door lit round the opening
door_frame(hg, "z", 30.0, -1, -3.0, 3.0, 0.0, 2.8, w=0.3, hazard=True, light=0.06)
door_frame(hg, "z", 14.15, 1, -1.0, 1.0, 0.0, 2.4, w=0.25, hazard=True)
door_frame(hg, "z", 14.15, 1, -5.8, -3.8, UP, UP + 2.4, w=0.2, hazard=True)
door_frame(hg, "z", 13.85, -1, -1.0, 1.0, 0.0, 2.4)                            # lower deck side
# floor: a striped apron inside the door and a lane to the lower deck
stripes(hg, "xz", -3.0, 3.0, 28.6, 29.4, (0.0, 0.02), width=0.4)
for s in (-1, 1):
    box(hg, (s * 1.0, 0.0, 14.2), (s * 1.1, 0.01, 28.6), HAZ, bevel=0)


def ramp_deck(z):
    """Height of the ramp's walking surface over the port wall at z (0 aft of it)."""
    if z <= TOP_HI[0]:
        return UP
    return max(0.0, UP * (TOP_LO[0] - z) / (TOP_LO[0] - TOP_HI[0]))


# ribs: wall pilasters, knee gussets above everyone's head, girders across the ceiling
for z in (16.5, 22.0, 27.6):
    for s in (-1, 1):
        deck = ramp_deck(z) if s < 0 else 0.0
        knee = max(5.3, deck + 2.35)
        box(hg, (s * W, 0.0, z - 0.15), (s * (W - STAND), knee, z + 0.15), DARK, bevel=0)
        hg.prism([(s * W, knee), (s * W, TOP), (s * (W - 0.6), TOP)], plane="xy", span=(z - 0.15, z + 0.15),
                 mat=DARK)
    box(hg, (-W, 5.85, z - 0.15), (W, TOP, z + 0.15), DARK)
for s in (-1, 1):
    box(hg, (s * 3.3, 6.0, 14.15), (s * 3.7, TOP, 30.0), DARK, bevel=0)       # cable trunking
    wall_band(hg, s * W, -s, [(14.3, 29.9)], 1.0, 1.6, OXIDE)                  # oxide band
for z in (18.0, 26.0):
    lamp(hg, 0.0, 5.9, z, TOP, hx=0.45, hz=0.3)

# ================================================================== ramp
rp = a.part("ramp", bevel=0.015)
RAMP_POLY = [TOP_LO, TOP_HI, BOT_HI, (BOT_HI[0] + (BOT_LO[0] - BOT_HI[0]) * BOT_HI[1] / (BOT_HI[1] - BOT_LO[1]), 0.0)]
rp.prism(RAMP_POLY, plane="zy", span=RAMP_X, mat=DARK)                          # the ramp, cut at the floor
layout_box(rp, 17, DARK)                                                        # the landing
rp.prism(RAMP_POLY, plane="zy", span=(RAMP_X[1], RAMP_X[1] + 0.01), mat=HAZ, bevel=0)   # yellow edge
lo17, hi17 = lbox(17)
box(rp, (RAMP_X[1], lo17[1], lo17[2]), (RAMP_X[1] + 0.01, hi17[1], hi17[2]), HAZ, bevel=0)
box(rp, (RAMP_X[1] - 0.1, hi17[1], lo17[2]), (RAMP_X[1], hi17[1] + 0.01, hi17[2]), HAZ, bevel=0)
rlen = math.dist(TOP_LO, TOP_HI)
s = 0.35
while s < rlen - 0.2:                                                           # cleats across the slope
    p0, p1 = ramp_top(s), ramp_top(s + 0.06)
    q1, q0 = ramp_top(s + 0.06, 0.02), ramp_top(s, 0.02)
    rp.prism([p0, p1, q1, q0], plane="zy", span=(RAMP_X[0] + 0.05, RAMP_X[1] - 0.12), mat=BARE, bevel=0)
    s += 0.55
# the yellow edge line on top, and a rail on the wall carrying it
rp.prism([ramp_top(0.0), ramp_top(rlen), ramp_top(rlen, 0.01), ramp_top(0.0, 0.01)], plane="zy",
         span=(RAMP_X[1] - 0.1, RAMP_X[1]), mat=HAZ, bevel=0)
rail = [ramp_top(0.0, -0.3), ramp_top(rlen - 0.9, -0.3), ramp_top(rlen - 0.9, -0.55), ramp_top(0.0, -0.55)]
rail = clip(rail, 0, -1, 0)
rp.prism(rail, plane="zy", span=(-W, -W + STAND), mat=OXIDE, bevel=0)


# ================================================================== decks
def corridor(part, floor, ceil, z0, z1, doors, ribs, lamps_z):
    """The corridor between x = +-2 from z0 to z1: rails, wall bands, ribs, conduits."""
    for s in (-1, 1):
        box(part, (s * 0.55, floor, z0), (s * 0.65, floor + 0.01, z1), BARE, bevel=0)   # robot rails
        runs, z = [], z0
        for d0, d1 in sorted(doors):
            runs.append((z, d0))
            z = d1
        runs.append((z, z1))
        for face, side in ((WALL_X, -1), (2.1, 1)):
            wall_band(part, s * face, s * side, runs, floor + 1.0, floor + 1.15, OXIDE)
            wall_band(part, s * face, s * side, runs, floor, floor + 0.15, DARK, d=0.02)
        for d0, d1 in doors:
            for face, side in ((WALL_X, -1), (2.1, 1)):
                door_frame(part, "x", s * face, s * side, d0, d1, floor, floor + 2.2)
    for z in ribs:
        portal(part, z, floor, ceil)
    box(part, (1.2, ceil - 0.2, z0), (1.55, ceil, z1), DARK, bevel=0)           # trunking
    pipe(part, "z", (-1.3, ceil - 0.15), 0.07, (z0, z1))
    for z in lamps_z:
        lamp(part, 0.0, ceil - 0.3, z, ceil)


def room(part, s, floor, ceil, z0, z1, beams, lamp_z, cabinets):
    """A side room between the corridor wall (|x| 2.1) and the hull (|x| 6)."""
    xi, xo = s * 2.1, s * W
    for z in beams:
        box(part, (min(xi, xo), ceil - 0.25, z - 0.12), (max(xi, xo), ceil, z + 0.12), DARK)
        for x, sg in ((xo, -s), (xi, s)):
            part.prism([(x, floor + 2.35), (x, ceil), (x + sg * 0.35, ceil)], plane="xy",
                       span=(z - 0.12, z + 0.12), mat=DARK)
    pipe(part, "z", (s * 5.5, ceil - 0.2), 0.09, (z0, z1))
    lamp(part, s * 4.0, ceil - 0.3, lamp_z, ceil)
    for c0, c1 in cabinets:
        cabinet(part, "x", xo, -s, c0, c1, floor + 0.1, floor + 2.1)


# ------------------------------------------------------------------ lower deck (y 0..3, z -30..14)
lo_ = a.part("lower_deck", bevel=0.015)
CEIL_LO = 3.0
box(lo_, (-W, -0.1, -L), (W, 0.0, 14.0), DARK, bevel=0)                        # floor
box(lo_, (-W, CEIL_LO, -L), (W, CEIL_LO + 0.1, 14.15), DARK, bevel=0)          # ceiling
for s in (-1, 1):
    box(lo_, (s * W, 0.0, -L), (s * (W + 0.1), UP, 14.0), PAINT, bevel=0)     # (up behind the deck's edge)
box(lo_, (-W - 0.1, -0.1, -L - 0.1), (W + 0.1, UP, -L), PAINT, bevel=0)       # bow (to the deck above)
for i in range(18, 30):
    layout_box(lo_, i, PAINT)
corridor(lo_, 0.0, CEIL_LO, -24.0, 13.85, [(-7.0, -5.4), (7.0, 8.6)],
         ribs=(-22.0, -19.0, -13.0, -10.0, -1.5, 2.0, 4.5, 11.5), lamps_z=(-16.0, -4.0, 8.0))
for s in (-1, 1):
    room(lo_, s, 0.0, CEIL_LO, -24.0, 13.85, beams=(-18.0, -6.0, 3.0, 10.5), lamp_z=-12.0,
         cabinets=((-21.0, -19.0), (-9.0, -7.5), (8.5, 10.0)))
    lamp(lo_, s * 4.0, 2.7, 6.0, CEIL_LO)
# the forward room: machinery on the bow wall, pipes overhead
for x0, x1 in ((-4.6, -1.2), (1.2, 4.6)):
    cabinet(lo_, "z", -L, 1, x0, x1, 0.2, 2.1)
for z in (-28.6, -25.4):
    pipe(lo_, "x", (CEIL_LO - 0.22, z), 0.1, (-W, W), mat=BARE)
lamp(lo_, 0.0, 2.7, -27.0, CEIL_LO)

# ------------------------------------------------------------------ upper deck (y 3.3..6.3, z -22..14)
up_ = a.part("upper_deck", bevel=0.015)
box(up_, (-W, UP - 0.2, -22.0), (W, UP, 14.15), DARK, bevel=0)                 # floor (to the landing)
box(up_, (-W, TOP, -22.0), (W, TOP + 0.1, 14.0), DARK, bevel=0)                # ceiling
for s in (-1, 1):
    box(up_, (s * W, UP, -22.0), (s * (W + 0.1), TOP, 14.0), PAINT, bevel=0)
for i in range(33, 43):
    layout_box(up_, i, PAINT)
corridor(up_, UP, TOP, -21.85, 13.85, [(-13.0, -11.4), (3.0, 4.6)],
         ribs=(-20.0, -17.0, -9.0, -5.5, 0.5, 6.5, 12.0), lamps_z=(-14.0, -2.0, 9.0))
for s in (-1, 1):
    room(up_, s, UP, TOP, -21.85, 13.85, beams=(-16.0, -3.0, 2.5, 12.0), lamp_z=-8.0,
         cabinets=((-19.0, -17.0), (-1.5, 1.0), (10.0, 12.0)))
    lamp(up_, s * 4.0, 6.0, 8.0, TOP)
door_frame(up_, "z", 13.85, -1, -5.8, -3.8, UP, UP + 2.4, w=0.18)               # ramp door, deck side
door_frame(up_, "z", -21.85, 1, -1.0, 1.0, UP, UP + 2.2, w=0.25)                # bridge door, corridor side
box(up_, (-0.6, UP + 2.5, -21.85), (0.6, UP + 2.6, -21.84), LIGHT, bevel=0)    # lit sign over it

# ------------------------------------------------------------------ bridge (y 3.3..6.3, z -30..-22)
br = a.part("bridge", bevel=0.015)
box(br, (-W, UP - 0.2, -L), (W, UP, -22.0), DARK, bevel=0)                     # floor
box(br, (-W, TOP, -L), (W, TOP + 0.1, -22.0), DARK, bevel=0)                   # ceiling
for s in (-1, 1):
    box(br, (s * W, UP, -L), (s * (W + 0.1), TOP, -22.0), PAINT, bevel=0)
box(br, (-W - 0.1, UP, -L - 0.1), (W + 0.1, 4.5, -L), PAINT, bevel=0)          # bow, under the slit
box(br, (-W - 0.1, 5.3, -L - 0.1), (W + 0.1, TOP + 0.1, -L), PAINT, bevel=0)                     # over it
for i in range(30, 33):
    layout_box(br, i, DARK)                                                     # bulkhead
door_frame(br, "z", -22.15, -1, -1.0, 1.0, UP, UP + 2.2, w=0.25, hazard=True)
# the window slit: a sill and a header
box(br, (-W, 4.25, -L), (W, 4.5, -L + STAND), DARK, bevel=0)
box(br, (-W, 5.3, -L), (W, 5.55, -L + STAND), DARK, bevel=0)
# screens: the bow wall under the slit, and banks on the side walls (2 cm frame, 1 cm glass)
for x0, x1 in ((-5.4, -3.2), (-1.1, 1.1), (3.2, 5.4)):
    rect(br, "xy", x0 - 0.08, x1 + 0.08, 3.55, 4.18, (-L, -L + 0.02), DARK)
    rect(br, "xy", x0, x1, 3.63, 4.1, (-L + 0.02, -L + STAND), SCREEN)
for s in (-1, 1):
    for z0, z1 in ((-28.8, -27.0), (-25.0, -23.2)):
        rect(br, "zy", z0 - 0.08, z1 + 0.08, 3.92, 4.98, tuple(sorted((s * W, s * (W - 0.02)))), DARK)
        rect(br, "zy", z0, z1, 4.0, 4.9, tuple(sorted((s * (W - 0.02), s * (W - STAND)))), SCREEN)
    wall_band(br, s * W, -s, [(-29.9, -22.15)], 3.3, 3.45, DARK, d=0.02)
# guide lights in the floor from the door to the console
for s in (-1, 1):
    box(br, (s * 0.75, UP, -26.55), (s * 0.82, UP + 0.01, -22.2), LIGHT, bevel=0)
# the crown: oxide girders overhead, fore-and-aft and across
for s in (-1, 1):
    box(br, (s * 2.3, 5.75, -L), (s * 2.6, TOP, -22.15), OXIDE)
for z in (-29.3, -23.0):
    box(br, (-W, 5.75, z - 0.15), (W, TOP, z + 0.15), OXIDE)
    for s in (-1, 1):
        br.prism([(s * W, 5.65), (s * W, TOP), (s * (W - 0.5), TOP)], plane="xy", span=(z - 0.15, z + 0.15),
                 mat=OXIDE)
lamp(br, 0.0, 6.0, -26.0, TOP, hx=0.45, hz=0.3)

# ================================================================== console (layout box 43)
co = a.part("console", bevel=0.015)
clo, chi = lbox(43)
box(co, clo, chi, DARK)
CT = chi[1]                                                                     # its top, y 4.3
box(co, (clo[0] + 0.05, CT, clo[2] + 0.05), (chi[0] - 0.05, CT + 0.01, chi[2] - 0.05), OXIDE, bevel=0)
for x0, x1 in ((-1.85, -0.65), (0.65, 1.85)):
    box(co, (x0, CT + 0.01, -27.3), (x1, CT + 0.02, -26.72), SCREEN, bevel=0)
bx, bz = LAYOUT["controls"][0]["center"][0], LAYOUT["controls"][0]["center"][2]
stripes(co, "xz", bx - 0.5, bx + 0.5, bz - 0.38, bz + 0.38, (CT + 0.005, CT + 0.02), width=0.12)
box(co, (clo[0] + 0.15, 4.12, chi[2]), (chi[0] - 0.15, 4.2, chi[2] + 0.01), LIGHT, bevel=0)   # status strip
box(co, (clo[0] + 0.15, 3.3, chi[2]), (chi[0] - 0.15, 3.42, chi[2] + 0.02), BLACK, bevel=0)   # kick plate
for s in (-1, 1):
    x = chi[0] if s > 0 else clo[0]
    box(co, (x, 3.45, clo[2] + 0.1), (x + s * 0.015, 4.15, chi[2] - 0.1), OXIDE, bevel=0)

# ================================================================== crates
cr = a.part("crates", bevel=0.02)
for n, i in enumerate(CRATES):
    lo, hi = lbox(i)
    box(cr, lo, hi, OXIDE if n % 2 == 0 else PAINT)
    # two bare straps round it, over the top and down the long sides
    long_x = (hi[0] - lo[0]) > (hi[2] - lo[2])
    for f in (0.25, 0.75):
        if long_x:
            x = lo[0] + f * (hi[0] - lo[0])
            box(cr, (x - 0.04, hi[1], lo[2] - 0.015), (x + 0.04, hi[1] + 0.015, hi[2] + 0.015), BARE, bevel=0)
            for z0, z1 in ((lo[2] - 0.015, lo[2]), (hi[2], hi[2] + 0.015)):
                box(cr, (x - 0.04, lo[1], z0), (x + 0.04, hi[1], z1), BARE, bevel=0)
        else:
            z = lo[2] + f * (hi[2] - lo[2])
            box(cr, (lo[0] - 0.015, hi[1], z - 0.04), (hi[0] + 0.015, hi[1] + 0.015, z + 0.04), BARE, bevel=0)
            for x0, x1 in ((lo[0] - 0.015, lo[0]), (hi[0], hi[0] + 0.015)):
                box(cr, (x0, lo[1], z - 0.04), (x1, hi[1], z + 0.04), BARE, bevel=0)

a.export()
