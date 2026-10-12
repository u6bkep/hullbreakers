"""Weapon part variants: the swappable pieces a crew gun is assembled from (see
src/sim/items.rs `Parts`, and the receivers in assets/weapons/<frame>_receiver).

Each part is its own asset, assets/weapons/parts/<frame>_<slot>_<variant>/, whose
build.py only calls `build(__file__)`; the geometry is all here, so the variants of a
slot are drawn side by side and stay consistent. Bevy space, metres: +Y up, -Z
forward, +X right. A part's origin is the socket it mounts on: the receiver's
SOCKET_barrel / SOCKET_mag / SOCKET_stock / SOCKET_sight, or (a muzzle device or a
family's emitter) the barrel's SOCKET_muzzle. Barrels run along -Z with the bore on the origin; magazines
hang down -Y from the top of the magwell; stocks run back along +Z from the
receiver's rear face; sights sit on the rail's top at y = 0.

Every variant reads as what it does from its outline: a long barrel is long, a heavy
one is thick and finned, a drum is a drum, a quick mag is two taped together, a
compensator has ports cut in its top, a booster is a can with a flared end, a scope
is a tube with a big objective bell. Paint (paint_primary, paint_secondary) is on the
pieces a maker's paint job should reach; the rest is dark metal, bare metal and
polymer.
"""
import math
import os

import hb

PAINT, ACCENT, DARK, BARE, POLY, RUBBER, GLASS = (
    "paint_primary", "paint_secondary", "metal_dark", "metal_bare", "polymer", "rubber", "glass")
GLOW = "emissive"


class Part:
    """An hb part that keeps track of how far its primitives reach (for its collider)."""

    def __init__(self, part):
        self.p = part
        self.lo = [1e9] * 3
        self.hi = [-1e9] * 3

    def _reach(self, lo, hi):
        self.lo = [min(a, b) for a, b in zip(self.lo, lo)]
        self.hi = [max(a, b) for a, b in zip(self.hi, hi)]

    def box(self, lo, hi, **kw):
        self._reach([min(a, b) for a, b in zip(lo, hi)], [max(a, b) for a, b in zip(lo, hi)])
        self.p.box(lo, hi, **kw)

    def frame(self, lo, hi, wall, axis, **kw):
        self._reach(lo, hi)
        self.p.frame(lo, hi, wall, axis, **kw)

    def prism(self, points, plane="zy", span=(0, 0), **kw):
        u = [p[0] for p in points]
        v = [p[1] for p in points]
        if plane == "zy":
            self._reach([span[0], min(v), min(u)], [span[1], max(v), max(u)])
        else:                                              # "xy"
            self._reach([min(u), min(v), span[0]], [max(u), max(v), span[1]])
        self.p.prism(points, plane=plane, span=span, **kw)

    def lathe(self, profile, axis="z", center=(0, 0), **kw):
        r = max(p[0] for p in profile)
        t = [p[1] for p in profile]
        self._axial(axis, center, r, (min(t), max(t)))
        self.p.lathe(profile, axis=axis, center=center, **kw)

    def cylinder(self, axis, center, radius, span, **kw):
        self._axial(axis, center, radius, span)
        self.p.cylinder(axis, center, radius, span, **kw)

    def hull(self, points, **kw):
        self._reach([min(p[i] for p in points) for i in range(3)], [max(p[i] for p in points) for i in range(3)])
        self.p.hull(points, **kw)

    def _axial(self, axis, center, r, span):
        a, b = center
        lo, hi = min(span), max(span)
        if axis == "z":
            self._reach([a - r, b - r, lo], [a + r, b + r, hi])
        elif axis == "x":                                  # center is (y, z)
            self._reach([lo, a - r, b - r], [hi, a + r, b + r])
        else:                                              # "y": center is (x, z)
            self._reach([a - r, lo, b - r], [a + r, hi, b + r])

    def box_points(self):
        return [(x, y, z) for x in (self.lo[0], self.hi[0]) for y in (self.lo[1], self.hi[1])
                for z in (self.lo[2], self.hi[2])]


# ---------------------------------------------------------------------------------------
# Barrels: the bore on the origin, running along -Z. Each ends at its SOCKET_muzzle.

def smg_barrel(a, p, variant):
    if variant == "standard":
        # The issue shroud: a collar, a finned jacket, a crowned tip.
        p.lathe([(0, 0.0), (0.0225, 0.0), (0.0225, -0.018), (0.0185, -0.018), (0.0185, -0.122), (0.0145, -0.122),
                 (0.0145, -0.14), (0.0062, -0.14), (0.0062, -0.12), (0, -0.12)], axis="z",
                mats=[BARE] * 3 + [DARK] * 2 + [BARE] * 2 + [DARK] * 2)
        for x0 in (-0.0075, 0.0035):
            p.box((x0, 0.012, -0.104), (x0 + 0.004, 0.026, -0.024), mat=DARK)
        return -0.14
    if variant == "long":
        # A slim jacket run well out, vented along its top.
        p.lathe([(0, 0.0), (0.022, 0.0), (0.022, -0.016), (0.016, -0.016), (0.016, -0.272), (0.012, -0.272),
                 (0.012, -0.292), (0.005, -0.292), (0.005, -0.28), (0, -0.28)], axis="z",
                mats=[BARE] * 3 + [PAINT] * 2 + [BARE] * 2 + [DARK] * 2)
        for k in range(6):
            z = -0.05 - k * 0.036
            p.box((-0.006, 0.0135, z - 0.018), (0.006, 0.0172, z), mat=DARK, bevel=0)
        return -0.292
    if variant == "short":
        # A stub: hardly past the receiver.
        p.lathe([(0, 0.0), (0.0235, 0.0), (0.0235, -0.016), (0.0195, -0.016), (0.0195, -0.048), (0.0155, -0.048),
                 (0.0155, -0.06), (0.0065, -0.06), (0.0065, -0.052), (0, -0.052)], axis="z",
                mats=[BARE] * 3 + [DARK] * 2 + [BARE] * 2 + [DARK] * 2)
        return -0.06
    if variant == "heavy":
        # Thick, with heat-sink fins standing all round it.
        p.lathe([(0, 0.0), (0.027, 0.0), (0.027, -0.02), (0.022, -0.02), (0.022, -0.132), (0.018, -0.132),
                 (0.018, -0.152), (0.0075, -0.152), (0.0075, -0.14), (0, -0.14)], axis="z",
                mats=[DARK] * 3 + [PAINT] * 2 + [BARE] * 2 + [DARK] * 2)
        for k in range(5):
            z = -0.03 - k * 0.02
            p.box((-0.031, -0.031, z - 0.006), (0.031, 0.031, z), mat=DARK, bevel=0)
        return -0.152
    raise ValueError(variant)


def rifle_barrel(a, p, variant):
    # The rifle's barrel leaves the handguard's front cap; a stub runs back into it.
    if variant == "standard":
        p.lathe([(0.0105, 0.03), (0.0105, -0.105), (0.0145, -0.105), (0.0145, -0.118), (0.005, -0.118),
                 (0.005, -0.11), (0, -0.11)], axis="z", mats=[DARK, BARE, BARE, DARK, DARK, DARK])
        return -0.118
    if variant == "long":
        # Long and thin, with a gas block a third of the way out.
        p.lathe([(0.0095, 0.03), (0.0095, -0.30), (0.0135, -0.30), (0.0135, -0.315), (0.0045, -0.315),
                 (0.0045, -0.305), (0, -0.305)], axis="z", mats=[DARK, BARE, BARE, DARK, DARK, DARK])
        p.box((-0.0125, -0.014, -0.125), (0.0125, 0.014, -0.10), mat=PAINT)
        p.box((-0.006, 0.014, -0.122), (0.006, 0.022, -0.103), mat=DARK, bevel=0)
        return -0.315
    if variant == "short":
        p.lathe([(0.0115, 0.03), (0.0115, -0.035), (0.0155, -0.035), (0.0155, -0.048), (0.0055, -0.048),
                 (0.0055, -0.04), (0, -0.04)], axis="z", mats=[DARK, BARE, BARE, DARK, DARK, DARK])
        return -0.048
    if variant == "heavy":
        # A bull barrel, fluted (octagonal), with a heat collar.
        p.lathe([(0.016, 0.03), (0.016, -0.13), (0.02, -0.13), (0.02, -0.145), (0.006, -0.145),
                 (0.006, -0.135), (0, -0.135)], axis="z", segments=8,
                mats=[DARK, BARE, BARE, DARK, DARK, DARK])
        p.lathe([(0.021, -0.03), (0.021, -0.06)], axis="z", mat=PAINT, segments=8)
        return -0.145
    raise ValueError(variant)


def _rib(p, z0, z1, y, posts):
    """A shotgun's vent rib: a strip along the barrel's top at height `y`, on posts."""
    p.box((-0.0035, y, z1), (0.0035, y + 0.003, z0), mat=DARK, bevel=0)
    for k in range(posts):
        z = z0 - 0.008 - k * (z0 - z1 - 0.016) / max(posts - 1, 1)
        p.box((-0.0025, y - 0.007, z - 0.004), (0.0025, y, z + 0.004), mat=DARK, bevel=0)


def shotgun_barrel(a, p, variant):
    # A wide bore; like the rifle's, a stub runs back into the heat shield's front cap.
    if variant == "standard":
        p.lathe([(0.0165, 0.03), (0.0165, -0.094), (0.0205, -0.094), (0.0205, -0.11), (0.0115, -0.11),
                 (0.0115, -0.10), (0, -0.10)], axis="z", mats=[DARK, BARE, BARE, DARK, DARK, DARK])
        _rib(p, 0.0, -0.104, 0.0215, 5)
        p.box((-0.0018, 0.0245, -0.103), (0.0018, 0.0285, -0.099), mat=BARE, bevel=0)   # bead
        return -0.11
    if variant == "long":
        # The vent rib run long, a barrel band partway out.
        p.lathe([(0.016, 0.03), (0.016, -0.234), (0.0205, -0.234), (0.0205, -0.25), (0.0115, -0.25),
                 (0.0115, -0.24), (0, -0.24)], axis="z", mats=[DARK, BARE, BARE, DARK, DARK, DARK])
        p.cylinder("z", (0, 0), 0.0195, (-0.115, -0.135), mat=PAINT)
        _rib(p, 0.0, -0.244, 0.021, 9)
        p.box((-0.0018, 0.024, -0.243), (0.0018, 0.028, -0.239), mat=BARE, bevel=0)
        return -0.25
    if variant == "short":
        # A stub with a toothed breaching crown.
        p.lathe([(0.0175, 0.03), (0.0175, -0.016), (0.0225, -0.016), (0.0225, -0.042), (0.012, -0.042),
                 (0.012, -0.034), (0, -0.034)], axis="z", mats=[DARK, DARK, BARE, DARK, DARK, DARK])
        for k in range(6):                                           # teeth round the crown
            c, s = math.cos(k * math.pi / 3 + math.pi / 6), math.sin(k * math.pi / 3 + math.pi / 6)
            p.hull([(r * c - t * s, r * s + t * c, z) for r in (0.0145, 0.0215) for t in (-0.0035, 0.0035)
                    for z in (-0.04, -0.05)], mat=BARE, bevel=0)
        return -0.05
    if variant == "heavy":
        # Over-and-under: a second barrel slung beneath, clamped to the first.
        p.lathe([(0.0175, 0.03), (0.0175, -0.114), (0.0215, -0.114), (0.0215, -0.13), (0.012, -0.13),
                 (0.012, -0.12), (0, -0.12)], axis="z", mats=[DARK, BARE, BARE, DARK, DARK, DARK])
        p.lathe([(0.016, 0.0), (0.016, -0.11), (0.02, -0.11), (0.02, -0.124), (0.011, -0.124),
                 (0.011, -0.116), (0, -0.116)], axis="z", center=(0, -0.037),
                mats=[DARK, BARE, BARE, DARK, DARK, DARK])
        for z0 in (-0.018, -0.088):                                  # clamp bands
            p.box((-0.0205, -0.056, z0 - 0.014), (0.0205, 0.0205, z0), mat=PAINT)
        _rib(p, 0.0, -0.124, 0.022, 5)
        return -0.13
    raise ValueError(variant)


def sniper_barrel(a, p, variant):
    # Fluted: the body is a hard-faceted hexagon, the muzzle end round; a stub runs back
    # into the handguard's front cap.
    def fluted(r, z1, crown, end, mats=(BARE, BARE, BARE, DARK, DARK, DARK)):
        p.lathe([(r, 0.03), (r, z1)], axis="z", mat=DARK, segments=6)
        p.lathe([(0.8 * r, z1 + 0.005), (0.8 * r, end + 0.015), (crown, end + 0.015), (crown, end),
                 (0.0045, end), (0.0045, end + 0.008), (0, end + 0.008)], axis="z", mats=list(mats))

    if variant == "standard":
        fluted(0.0115, -0.17, 0.0125, -0.22)
        return -0.22
    if variant == "long":
        # Longer still, with a painted thread guard on the crown.
        fluted(0.0115, -0.30, 0.0125, -0.36, mats=(BARE, BARE, PAINT, PAINT, DARK, DARK))
        return -0.36
    if variant == "short":
        fluted(0.012, -0.035, 0.013, -0.07)
        return -0.07
    if variant == "heavy":
        # A bull barrel, thick and fluted, with heat collars.
        p.lathe([(0.0165, 0.03), (0.0165, -0.20)], axis="z", mat=DARK, segments=8)
        for z0 in (-0.04, -0.13):
            p.lathe([(0.0195, z0), (0.0195, z0 - 0.022)], axis="z", mat=PAINT, segments=8)
        p.lathe([(0.0135, -0.195), (0.0135, -0.215), (0.017, -0.215), (0.017, -0.23), (0.0055, -0.23),
                 (0.0055, -0.222), (0, -0.222)], axis="z", mats=[BARE, BARE, BARE, DARK, DARK, DARK])
        return -0.23
    raise ValueError(variant)


# ---------------------------------------------------------------------------------------
# Muzzle devices: on the barrel's muzzle, running along -Z.

def muzzle(a, p, variant, k):
    """k scales for the frame (the rifle's bore is thinner than the SMG's tip)."""
    if variant == "compensator":
        # A block with ports cut across its top: the gas goes up.
        w, h, L = 0.017 * k, 0.017 * k, 0.052 * k
        p.box((-w, -h, 0.0), (w, 0.35 * h, -L), mat=BARE)
        p.box((-0.75 * w, 0.0, -0.002 * k), (0.75 * w, 0.8 * h, -L + 0.002 * k), mat=DARK, bevel=0)   # inside
        edges = [0.0, -0.009, -0.017, -0.026, -0.034, -0.043, -0.052]
        for i in range(0, len(edges) - 1, 2):                        # top bars between the ports
            p.box((-w, 0.35 * h, edges[i] * k), (w, h, edges[i + 1] * k), mat=BARE)
        for s in (-1, 1):                                            # side walls the ports open between
            xa, xb = sorted((s * w, s * 0.8 * w))
            p.box((xa, 0.35 * h, 0.0), (xb, h, -L), mat=BARE)
        return -L
    if variant == "booster":
        # A can that flares at its end, with grip rings.
        r, L = 0.016 * k, 0.07 * k
        p.lathe([(0, 0.0), (r, 0.0), (r, -0.01 * k), (0.85 * r, -0.014 * k), (0.85 * r, -0.05 * k),
                 (1.25 * r, -0.058 * k), (1.25 * r, -L), (0.5 * r, -L), (0.5 * r, -L + 0.006 * k),
                 (0, -L + 0.006 * k)], axis="z", mats=[DARK, DARK, DARK, PAINT, DARK, BARE, DARK, DARK, DARK])
        for z in (-0.022, -0.032, -0.042):
            p.cylinder("z", (0, 0), 0.95 * r, (z * k, (z - 0.004) * k), mat=BARE, bevel=0)
        return -L
    raise ValueError(variant)


# ---------------------------------------------------------------------------------------
# Magazines: hanging down -Y from the top of the magwell.

def smg_mag(a, p, variant):
    W = 0.0125
    if variant == "standard":
        p.prism([(0.0195, 0.008), (-0.0195, 0.008), (-0.026, -0.12), (0.012, -0.12)], span=(-W, W), mat=DARK)
        p.box((-0.0155, -0.132, -0.031), (0.0155, -0.118, 0.016), mat=BARE)
        p.box((0.012, -0.10, -0.008), (0.0135, -0.03, 0.004), mat=GLOW, bevel=0.0005)
        return
    if variant == "extended":
        # Nearly twice as long, raked forward, two count windows.
        p.prism([(0.0195, 0.008), (-0.0195, 0.008), (-0.038, -0.215), (0.002, -0.215)], span=(-W, W), mat=DARK)
        p.box((-0.0155, -0.228, -0.043), (0.0155, -0.213, 0.006), mat=BARE)
        for y0 in (-0.10, -0.18):
            p.box((0.012, y0, -0.012 - (y0 + 0.1) * 0.08), (0.0135, y0 + 0.055, -0.0 - (y0 + 0.1) * 0.08),
                  mat=GLOW, bevel=0.0005)
        return
    if variant == "drum":
        # A short neck into a fat drum.
        p.box((-W, -0.05, -0.019), (W, 0.008, 0.019), mat=DARK)
        C = (-0.105, -0.03)                                         # drum axis (y, z)
        p.cylinder("x", C, 0.062, (-0.022, 0.022), mat=PAINT)
        p.cylinder("x", C, 0.05, (-0.025, 0.025), mat=DARK, bevel=0.002)
        p.cylinder("x", C, 0.016, (-0.029, 0.029), mat=BARE, bevel=0.002)
        p.box((0.0255, C[0] - 0.006, C[1] - 0.04), (0.027, C[0] + 0.006, C[1] - 0.02), mat=GLOW, bevel=0.0005)
        return
    if variant == "quick":
        # Two mags taped side by side, the spare a little lower, with a pull loop.
        p.prism([(0.0195, 0.008), (-0.0195, 0.008), (-0.026, -0.12), (0.012, -0.12)], span=(-W, W), mat=DARK)
        p.prism([(0.0195, -0.02), (-0.0195, -0.02), (-0.026, -0.15), (0.012, -0.15)], span=(W + 0.002, 3 * W + 0.002),
                mat=DARK)
        p.box((-0.0155, -0.132, -0.031), (0.0155, -0.118, 0.016), mat=BARE)
        for y in (-0.045, -0.095):                                  # tape
            p.box((-W - 0.0015, y - 0.009, -0.023), (3 * W + 0.0035, y + 0.009, 0.019), mat=ACCENT, bevel=0)
        p.frame((-0.01, -0.152, -0.03), (0.01, -0.132, 0.0), wall=0.004, axis="x", mat=RUBBER, bevel=0)  # pull loop
        p.box((0.012, -0.10, -0.008), (0.0135, -0.03, 0.004), mat=GLOW, bevel=0.0005)
        return
    raise ValueError(variant)


RIFLE_MAG = [(0.021, 0.008), (0.019, -0.06), (0.003, -0.122), (-0.043, -0.122), (-0.023, -0.06), (-0.021, 0.008)]


def rifle_mag(a, p, variant):
    W = 0.012
    if variant == "standard":
        p.prism(RIFLE_MAG, span=(-W, W), mat=POLY)
        p.box((-0.0145, -0.132, -0.046), (0.0145, -0.12, 0.006), mat=BARE)
        for s in (-1, 1):
            p.box((s * 0.0115, -0.054, -0.006), (s * 0.0133, -0.014, 0.004), mat=GLOW, bevel=0.0005)
        return
    if variant == "extended":
        # The curve carried on: a banana mag.
        p.prism([(0.021, 0.008), (0.019, -0.06), (0.003, -0.122), (-0.022, -0.2), (-0.068, -0.2),
                 (-0.043, -0.122), (-0.023, -0.06), (-0.021, 0.008)], span=(-W, W), mat=POLY)
        p.hull([(x, y, z) for x in (-0.0145, 0.0145) for (z, y) in
                ((-0.02, -0.198), (-0.07, -0.198), (-0.024, -0.21), (-0.074, -0.21))], mat=BARE)
        for s in (-1, 1):
            p.box((s * 0.0115, -0.054, -0.006), (s * 0.0133, -0.014, 0.004), mat=GLOW, bevel=0.0005)
        return
    if variant == "drum":
        p.box((-W, -0.05, -0.02), (W, 0.008, 0.02), mat=POLY)
        C = (-0.11, -0.015)
        p.cylinder("x", C, 0.07, (-0.024, 0.024), mat=PAINT)
        p.cylinder("x", C, 0.056, (-0.027, 0.027), mat=DARK, bevel=0.002)
        p.cylinder("x", C, 0.018, (-0.031, 0.031), mat=BARE, bevel=0.002)
        p.box((0.0275, C[0] - 0.006, C[1] - 0.045), (0.029, C[0] + 0.006, C[1] - 0.022), mat=GLOW, bevel=0.0005)
        return
    if variant == "quick":
        p.prism(RIFLE_MAG, span=(-W, W), mat=POLY)
        p.prism([(z, y - 0.025) for z, y in RIFLE_MAG], span=(W + 0.002, 3 * W + 0.002), mat=POLY)
        p.box((-0.0145, -0.132, -0.046), (0.0145, -0.12, 0.006), mat=BARE)
        for y in (-0.03, -0.09):
            p.box((-W - 0.0015, y - 0.009, -0.03), (3 * W + 0.0035, y + 0.009, 0.02), mat=ACCENT, bevel=0)
        p.frame((-0.01, -0.152, -0.04), (0.01, -0.132, -0.01), wall=0.004, axis="x", mat=RUBBER, bevel=0)
        return
    raise ValueError(variant)


def _box_mag(p, depth, rake, x0, x1, top=0.028, ribs=(), windows=(), y0=0.006):
    """A straight box magazine from y0 down to -depth, its bottom `rake` forward of its top,
    between x0 and x1: the body, a bare base plate, side ribs at the heights in `ribs`, and
    count windows (y_hi, y_lo) glowing on its right side."""
    def edges(y):                                           # its front and back z at height y
        t = (y0 - y) / (y0 + depth)
        return -top - (rake + 0.006) * t, top - rake * t

    yb = -depth
    f, b = edges(yb)
    p.prism([(top, y0), (-top, y0), (f, yb), (b, yb)], span=(x0, x1), mat=POLY)
    p.box((x0 - 0.0025, yb - 0.012, f - 0.003), (x1 + 0.0025, yb + 0.002, b + 0.003), mat=BARE)
    for y in ribs:                                          # the shells' steps, both sides
        f, b = edges(y - 0.006)
        for xa, xb in ((x0 - 0.0015, x0 + 0.001), (x1 - 0.001, x1 + 0.0015)):
            p.box((xa, y - 0.006, f + 0.004), (xb, y, b - 0.014), mat=POLY, bevel=0.0005)
    for yh, yl in windows:
        b = min(edges(yh)[1], edges(yl)[1])
        p.box((x1 - 0.001, yl, b - 0.011), (x1 + 0.0015, yh, b - 0.004), mat=GLOW, bevel=0.0005)


def shotgun_mag(a, p, variant):
    # Shell boxes: fat, their sides stepped by the shells stacked in them.
    W = 0.019
    if variant == "standard":
        _box_mag(p, 0.10, 0.006, -W, W, ribs=(-0.025, -0.05, -0.075), windows=((-0.02, -0.085),))
        return
    if variant == "extended":
        # Half again as long, raked forward.
        _box_mag(p, 0.16, 0.016, -W, W, ribs=(-0.025, -0.05, -0.075, -0.10, -0.125),
                 windows=((-0.02, -0.07), (-0.09, -0.14)))
        return
    if variant == "drum":
        # A short neck into a big shell drum.
        p.box((-W, -0.045, -0.026), (W, 0.006, 0.026), mat=POLY)
        C = (-0.105, -0.012)                                        # drum axis (y, z)
        p.cylinder("x", C, 0.072, (-0.026, 0.026), mat=PAINT)
        p.cylinder("x", C, 0.058, (-0.029, 0.029), mat=DARK, bevel=0.002)
        p.cylinder("x", C, 0.02, (-0.033, 0.033), mat=BARE, bevel=0.002)
        p.box((0.0285, C[0] - 0.007, C[1] - 0.05), (0.0305, C[0] + 0.007, C[1] - 0.026), mat=GLOW, bevel=0.0005)
        return
    if variant == "quick":
        # Two shell boxes taped side by side, the spare a little lower, with a pull loop.
        _box_mag(p, 0.10, 0.006, -W, W, ribs=(-0.025, -0.05, -0.075), windows=((-0.02, -0.085),))
        _box_mag(p, 0.13, 0.006, W + 0.002, 3 * W + 0.002, y0=-0.024, ribs=(-0.06, -0.085, -0.11))
        for y in (-0.035, -0.08):                                  # tape
            p.box((-W - 0.0025, y - 0.009, -0.026), (3 * W + 0.0045, y + 0.009, 0.012), mat=PAINT, bevel=0)
        p.frame((-0.012, -0.135, -0.03), (0.012, -0.112, 0.0), wall=0.0045, axis="x", mat=RUBBER, bevel=0)
        return
    raise ValueError(variant)


def sniper_mag(a, p, variant):
    # Short straight boxes: a precision rifle's ten rounds.
    W = 0.0115
    if variant == "standard":
        _box_mag(p, 0.075, 0.002, -W, W, top=0.026, windows=((-0.012, -0.06),))
        return
    if variant == "extended":
        _box_mag(p, 0.15, 0.004, -W, W, top=0.026, windows=((-0.012, -0.06), (-0.08, -0.13)))
        return
    if variant == "drum":
        p.box((-W, -0.04, -0.024), (W, 0.006, 0.024), mat=POLY)
        C = (-0.088, -0.01)
        p.cylinder("x", C, 0.055, (-0.02, 0.02), mat=PAINT)
        p.cylinder("x", C, 0.044, (-0.023, 0.023), mat=DARK, bevel=0.002)
        p.cylinder("x", C, 0.015, (-0.027, 0.027), mat=BARE, bevel=0.002)
        p.box((0.0225, C[0] - 0.006, C[1] - 0.038), (0.0245, C[0] + 0.006, C[1] - 0.02), mat=GLOW, bevel=0.0005)
        return
    if variant == "quick":
        _box_mag(p, 0.075, 0.002, -W, W, top=0.026, windows=((-0.012, -0.06),))
        _box_mag(p, 0.1, 0.002, W + 0.002, 3 * W + 0.002, top=0.026, y0=-0.025)
        for y in (-0.025, -0.06):
            p.box((-W - 0.0025, y - 0.008, -0.03), (3 * W + 0.0045, y + 0.008, 0.008), mat=PAINT, bevel=0)
        p.frame((-0.01, -0.105, -0.03), (0.01, -0.085, 0.0), wall=0.004, axis="x", mat=RUBBER, bevel=0)
        return
    raise ValueError(variant)


# ---------------------------------------------------------------------------------------
# Stocks: back along +Z from the receiver's rear face.

def smg_stock(a, p, variant):
    if variant == "standard":
        # A folding wire stock: two rods to a butt plate.
        p.box((-0.015, -0.032, 0.0), (0.015, 0.022, 0.022), mat=DARK)
        for s in (-1, 1):
            p.cylinder("z", (s * 0.011, 0.012), 0.0045, (0.018, 0.2), mat=BARE, bevel=0)
            p.hull([(s * 0.011 + dx, y + dy, z) for dx in (-0.0045, 0.0045) for dy in (-0.0045, 0.0045)
                    for (z, y) in ((0.018, -0.024), (0.2, -0.062))], mat=BARE)
        p.box((-0.019, -0.078, 0.196), (0.019, 0.022, 0.208), mat=RUBBER)
        return
    if variant == "light":
        # Hardly anything: an end cap with a sling loop.
        p.box((-0.017, -0.034, 0.0), (0.017, 0.032, 0.03), mat=POLY)
        p.frame((-0.004, -0.03, 0.03), (0.004, 0.0, 0.058), wall=0.005, axis="x", mat=BARE, bevel=0)
        return
    if variant == "heavy":
        # A solid stock with a cheek rest and a thick pad.
        p.prism([(0.0, 0.036), (0.0, -0.036), (0.09, -0.062), (0.215, -0.078), (0.215, 0.036), (0.09, 0.04)],
                span=(-0.017, 0.017), mat=PAINT)
        p.box((-0.015, 0.036, 0.07), (0.015, 0.054, 0.19), mat=ACCENT)
        p.box((-0.019, -0.082, 0.213), (0.019, 0.058, 0.232), mat=RUBBER)
        return
    raise ValueError(variant)


def rifle_stock(a, p, variant):
    if variant == "standard":
        # The skeletal issue stock.
        p.box((-0.018, -0.036, -0.007), (0.018, 0.034, 0.02), mat=DARK)
        p.prism([(0.015, 0.034), (0.125, 0.034), (0.125, 0.013), (0.015, 0.018)], span=(-0.014, 0.014), mat=ACCENT)
        p.prism([(0.015, -0.032), (0.015, -0.012), (0.125, -0.05), (0.125, -0.07)], span=(-0.009, 0.009), mat=ACCENT)
        p.prism([(0.12, -0.078), (0.12, 0.038), (0.141, 0.038), (0.141, -0.078)], span=(-0.016, 0.016), mat=ACCENT)
        p.box((-0.017, -0.082, 0.14), (0.017, 0.042, 0.15), mat=RUBBER, bevel=0.002)
        return
    if variant == "light":
        # A bare telescoping tube and a small pad.
        p.cylinder("z", (0, 0.004), 0.017, (-0.005, 0.022), mat=DARK)
        p.cylinder("z", (0, 0.004), 0.011, (0.02, 0.13), mat=BARE)
        p.box((-0.014, -0.05, 0.128), (0.014, 0.03, 0.14), mat=RUBBER, bevel=0.002)
        return
    if variant == "heavy":
        # Full and solid, with a raised cheek piece.
        p.prism([(0.0, 0.036), (0.0, -0.036), (0.09, -0.07), (0.175, -0.088), (0.175, 0.044), (0.06, 0.046)],
                span=(-0.019, 0.019), mat=PAINT)
        p.box((-0.016, 0.044, 0.045), (0.016, 0.064, 0.15), mat=ACCENT)
        p.box((-0.021, -0.092, 0.173), (0.021, 0.068, 0.19), mat=RUBBER, bevel=0.002)
        return
    raise ValueError(variant)


def shotgun_stock(a, p, variant):
    if variant == "standard":
        # A solid polymer stock with a thick recoil pad.
        p.box((-0.021, -0.03, 0.0), (0.021, 0.037, 0.016), mat=DARK)
        p.prism([(0.012, 0.037), (0.012, -0.03), (0.08, -0.058), (0.15, -0.078), (0.15, 0.039), (0.06, 0.04)],
                span=(-0.0195, 0.0195), mat=ACCENT)
        p.box((-0.0215, -0.084, 0.148), (0.0215, 0.045, 0.172), mat=RUBBER)
        p.frame((-0.004, -0.088, 0.112), (0.004, -0.062, 0.136), wall=0.0045, axis="x", mat=BARE, bevel=0)
        return
    if variant == "light":
        # Hardly anything: an end cap with a sling loop.
        p.box((-0.023, -0.03, 0.0), (0.023, 0.038, 0.032), mat=POLY)
        p.frame((-0.0045, -0.028, 0.032), (0.0045, 0.004, 0.062), wall=0.0055, axis="x", mat=BARE, bevel=0)
        return
    if variant == "heavy":
        # Deep and solid, a shell holder on its left, a thick pad.
        p.prism([(0.0, 0.038), (0.0, -0.032), (0.09, -0.072), (0.19, -0.09), (0.19, 0.04), (0.08, 0.042)],
                span=(-0.021, 0.021), mat=PAINT)
        p.box((-0.018, 0.04, 0.07), (0.018, 0.058, 0.172), mat=ACCENT)
        p.box((-0.0235, -0.098, 0.188), (0.0235, 0.062, 0.218), mat=RUBBER)
        p.box((-0.028, -0.034, 0.068), (-0.0205, 0.024, 0.162), mat=ACCENT, bevel=0.0015)
        for z in (0.082, 0.104, 0.126, 0.148):                          # spare shells, brass down
            p.cylinder("y", (-0.0365, z), 0.0095, (-0.029, 0.021), mat=POLY, bevel=0.0015)
            p.cylinder("y", (-0.0365, z), 0.0099, (-0.039, -0.029), mat=BARE, bevel=0.001)
        return
    raise ValueError(variant)


def sniper_stock(a, p, variant):
    if variant == "standard":
        # A chassis stock: a top beam with a raised cheek riser, a raked bottom beam with a
        # grip spur under it for the rear hand, a butt plate and pad.
        p.box((-0.018, -0.032, -0.006), (0.018, 0.032, 0.016), mat=DARK)
        p.box((-0.012, 0.012, 0.012), (0.012, 0.032, 0.178), mat=ACCENT)
        p.box((-0.0135, 0.032, 0.06), (0.0135, 0.052, 0.16), mat=PAINT)
        for z in (0.07, 0.15):                                          # its posts
            p.box((-0.006, 0.031, z - 0.004), (0.006, 0.042, z + 0.004), mat=BARE, bevel=0)
        p.prism([(0.012, -0.03), (0.012, -0.01), (0.178, -0.058), (0.178, -0.08)], span=(-0.009, 0.009), mat=ACCENT)
        p.prism([(0.11, -0.06), (0.145, -0.07), (0.142, -0.104), (0.128, -0.104), (0.116, -0.078)],
                span=(-0.009, 0.009), mat=ACCENT)                       # grip spur
        p.box((-0.016, -0.09, 0.172), (0.016, 0.056, 0.188), mat=ACCENT)
        p.box((-0.018, -0.094, 0.186), (0.018, 0.058, 0.198), mat=RUBBER, bevel=0.002)
        return
    if variant == "light":
        # A bare tube and a small pad.
        p.cylinder("z", (0, 0.004), 0.017, (-0.005, 0.022), mat=DARK)
        p.cylinder("z", (0, 0.004), 0.011, (0.02, 0.17), mat=BARE)
        p.box((-0.014, -0.05, 0.168), (0.014, 0.034, 0.18), mat=RUBBER, bevel=0.002)
        return
    if variant == "heavy":
        # Full and solid, with a high cheek piece and a monopod leg under its heel.
        p.prism([(0.0, 0.034), (0.0, -0.032), (0.10, -0.07), (0.19, -0.086), (0.19, 0.04), (0.06, 0.04)],
                span=(-0.019, 0.019), mat=PAINT)
        p.box((-0.016, 0.04, 0.05), (0.016, 0.068, 0.17), mat=ACCENT)
        p.box((-0.021, -0.09, 0.188), (0.021, 0.074, 0.205), mat=RUBBER, bevel=0.002)
        p.cylinder("y", (0, 0.152), 0.009, (-0.095, -0.07), mat=DARK)
        p.cylinder("y", (0, 0.152), 0.0055, (-0.128, -0.094), mat=BARE)
        p.box((-0.013, -0.136, 0.142), (0.013, -0.126, 0.162), mat=RUBBER, bevel=0.002)
        return
    raise ValueError(variant)


# ---------------------------------------------------------------------------------------
# Sights: on top of the rail, y = 0.

def sight(a, p, variant, length):
    """`length`: how far the rail runs either side of the socket (m)."""
    L = length
    if variant == "iron":
        # A notched rear sight and a front post with ears.
        p.box((-0.01, 0.0, L - 0.025), (0.01, 0.007, L - 0.005), mat=DARK)
        for s in (-1, 1):                                            # the notch between two posts
            xa, xb = sorted((s * 0.0035, s * 0.0095))
            p.box((xa, 0.006, L - 0.02), (xb, 0.024, L - 0.012), mat=DARK, bevel=0)
        p.box((-0.009, 0.0, -L + 0.004), (0.009, 0.007, -L + 0.022), mat=DARK)
        p.box((-0.002, 0.006, -L + 0.009), (0.002, 0.026, -L + 0.015), mat=BARE, bevel=0)
        for s in (-1, 1):                                            # its ears
            xa, xb = sorted((s * 0.006, s * 0.0085))
            p.box((xa, 0.006, -L + 0.006), (xb, 0.024, -L + 0.018), mat=DARK, bevel=0)
        return
    if variant == "reflex":
        # An open window on a mount, its glowing dot hanging in it.
        p.box((-0.011, 0.0, -0.025), (0.011, 0.008, 0.02), mat=DARK)
        p.frame((-0.019, 0.007, -0.016), (0.019, 0.046, 0.004), wall=0.0045, axis="z", mat=PAINT, bevel=0.0015)
        p.box((-0.0145, 0.0115, -0.012), (0.0145, 0.0415, -0.0105), mat=GLASS, bevel=0)
        p.box((-0.0015, 0.0255, -0.0105), (0.0015, 0.0285, -0.0095), mat=GLOW, bevel=0)
        return
    if variant == "scope":
        # A tube scope: eyepiece, a turret on top, a big objective bell, lenses.
        Y = 0.034
        for z in (0.035, -0.04):                                    # rings onto the rail
            p.box((-0.009, 0.0, z - 0.009), (0.009, Y - 0.012, z + 0.009), mat=DARK, bevel=0)
        p.lathe([(0.016, 0.085), (0.019, 0.075), (0.019, 0.055), (0.0135, 0.045), (0.0135, -0.06),
                 (0.023, -0.085), (0.023, -0.11)], axis="z", center=(0, Y),
                mats=[DARK, DARK, DARK, PAINT, DARK, DARK])
        p.cylinder("y", (0, -0.005), 0.009, (Y + 0.012, Y + 0.026), mat=DARK)
        p.cylinder("x", (Y, -0.005), 0.008, (0.012, 0.024), mat=DARK)
        p.cylinder("z", (0, Y), 0.0155, (0.084, 0.086), mat=GLASS, bevel=0)
        p.cylinder("z", (0, Y), 0.02, (-0.109, -0.111), mat=GLASS, bevel=0)
        p.lathe([(0.0232, -0.1), (0.0232, -0.104)], axis="z", center=(0, Y), mat=GLOW, bevel=0)
        return
    raise ValueError(variant)


def long_scope(a, p):
    """The sniper's scope: the rifle's, grown: a thicker, longer tube on taller rings, a
    big objective bell with a sunshade, tall turrets and a parallax knob."""
    Y = 0.05
    for z in (0.05, -0.075):                                        # rings onto the rail
        p.box((-0.011, 0.0, z - 0.011), (0.011, Y - 0.014, z + 0.011), mat=DARK, bevel=0)
    p.lathe([(0.019, 0.135), (0.0235, 0.12), (0.0235, 0.088), (0.0175, 0.074), (0.0175, -0.10),
             (0.033, -0.14), (0.033, -0.185), (0.033, -0.235)], axis="z", center=(0, Y),
            mats=[DARK, DARK, DARK, PAINT, DARK, DARK, PAINT])                # the sunshade painted
    p.cylinder("y", (0, -0.01), 0.0125, (Y + 0.014, Y + 0.037), mat=DARK)          # elevation turret
    p.cylinder("x", (Y, -0.01), 0.011, (0.014, 0.034), mat=DARK)                   # windage
    p.cylinder("x", (Y, -0.01), 0.013, (-0.03, -0.014), mat=DARK)                  # parallax knob
    p.cylinder("z", (0, Y), 0.0165, (0.134, 0.137), mat=GLASS, bevel=0)
    p.cylinder("z", (0, Y), 0.029, (-0.234, -0.237), mat=GLASS, bevel=0)
    p.lathe([(0.0333, -0.15), (0.0333, -0.155)], axis="z", center=(0, Y), mat=GLOW, bevel=0)


# ---------------------------------------------------------------------------------------
# Emitters: a weapon family's device on the barrel's muzzle (like a muzzle device),
# running along -Z; `k` scales for the frame. Each glows in its family's colour where
# the shooter can see it from behind.

def emitter(a, p, variant, k):
    if variant == "plasma":
        # A magnetic coil can: stacked rings round a glowing chamber, two painted tie bars.
        p.cylinder("z", (0, 0), 0.0195 * k, (0.0, -0.011 * k), mat=DARK)
        p.cylinder("z", (0, 0), 0.0165 * k, (-0.008 * k, -0.074 * k), mat=GLOW)     # the chamber
        for i in range(4):                                          # the coils, the chamber glowing between
            z0 = -0.019 * k - i * 0.0155 * k
            p.cylinder("z", (0, 0), 0.024 * k, (z0, z0 - 0.0065 * k), mat=DARK, bevel=0)
        p.cylinder("z", (0, 0), 0.0205 * k, (-0.071 * k, -0.081 * k), mat=BARE)
        p.cylinder("z", (0, 0), 0.0065 * k, (-0.08 * k, -0.084 * k), mat=GLOW, bevel=0)
        for s in (-1, 1):
            xa, xb = sorted((s * 0.0205 * k, s * 0.0255 * k))
            p.box((xa, -0.0045 * k, -0.004 * k), (xb, 0.0045 * k, -0.078 * k), mat=PAINT)
        return
    if variant == "arc":
        # Two forked prongs off a painted yoke, insulator rings on each, a glowing spark
        # gap at the yoke and the arc jumping between the tips.
        p.lathe([(0.018 * k, 0.0), (0.018 * k, -0.02 * k), (0.013 * k, -0.025 * k)], axis="z",
                mats=[DARK, DARK])
        p.hull([(sx * x, y, z) for sx in (-1, 1) for (x, z) in ((0.014 * k, -0.012 * k), (0.03 * k, -0.036 * k))
                for y in (-0.009 * k, 0.009 * k)], mat=PAINT)
        p.cylinder("z", (0, 0), 0.0095 * k, (-0.03 * k, -0.042 * k), mat=GLOW, bevel=0)  # spark gap

        def prong_x(z):                                             # a prong's centre at z (>0 side)
            return (0.024 + 0.004 * (-z / k - 0.034) / 0.056) * k

        t = 0.005 * k
        for s in (-1, 1):
            ends = ((s * prong_x(-0.034 * k), -0.032 * k), (s * prong_x(-0.09 * k), -0.09 * k))
            p.hull([(x + dx, dy, z) for (x, z) in ends for dx in (-t, t) for dy in (-t, t)], mat=BARE)
            tip = ((s * prong_x(-0.09 * k), -0.088 * k), (s * 0.016 * k, -0.103 * k))
            p.hull([(x + dx, dy, z) for (x, z) in tip for dx in (-t, t) for dy in (-t, t)], mat=BARE)
            g = 0.0058 * k
            p.hull([(s * 0.016 * k + dx, dy, z) for dx in (-g, g) for dy in (-g, g)
                    for z in (-0.1 * k, -0.111 * k)], mat=GLOW, bevel=0)
            for z in (-0.046 * k, -0.066 * k):                      # insulators
                x = s * prong_x(z)
                p.cylinder("z", (x, 0), 0.0095 * k, (z, z - 0.01 * k), mat=POLY, segments=8, bevel=0)
        arc = [(-0.011, 0.0, -0.106), (-0.004, 0.005, -0.109), (0.004, -0.005, -0.103), (0.011, 0.0, -0.106)]
        for (x0, y0, z0), (x1, y1, z1) in zip(arc, arc[1:]):          # the arc, zigzag
            q = 0.0016 * k
            p.hull([(x * k, y * k + dy, z * k + dz) for (x, y, z) in ((x0, y0, z0), (x1, y1, z1))
                    for dy in (-q, q) for dz in (-q, q)], mat=GLOW, bevel=0)
        return
    if variant == "laser":
        # A lens housing: square cooling fins down its length, a glowing conduit along its
        # top, a painted bezel and the lens glowing deep in it.
        p.lathe([(0.0165 * k, 0.0), (0.0165 * k, -0.012 * k), (0.0135 * k, -0.015 * k), (0.0135 * k, -0.066 * k)],
                axis="z", mats=[DARK, DARK, DARK])
        for i in range(5):
            z0 = -0.018 * k - i * 0.0095 * k
            p.box((-0.022 * k, -0.022 * k, z0), (0.022 * k, 0.022 * k, z0 - 0.0045 * k), mat=BARE, bevel=0)
        p.box((-0.003 * k, 0.012 * k, -0.012 * k), (0.003 * k, 0.0245 * k, -0.064 * k), mat=GLOW, bevel=0)
        p.lathe([(0.0135 * k, -0.062 * k), (0.0205 * k, -0.068 * k), (0.0205 * k, -0.088 * k),
                 (0.0145 * k, -0.088 * k), (0.0145 * k, -0.082 * k), (0, -0.082 * k)], axis="z",
                mats=[PAINT, PAINT, PAINT, DARK, DARK])
        p.cylinder("z", (0, 0), 0.0135 * k, (-0.081 * k, -0.0845 * k), mat=GLOW, bevel=0)  # the lens
        return
    raise ValueError(variant)


# ---------------------------------------------------------------------------------------

BEVEL = {"barrel": 0.0015, "mag": 0.0025, "stock": 0.003, "sight": 0.0015, "muzzle": 0.002, "emitter": 0.002}

# Per frame: its barrels, magazines and stocks; how muzzle devices and emitters scale to
# its muzzle; how far its rail runs either side of SOCKET_sight.
BARRELS = {"smg": smg_barrel, "rifle": rifle_barrel, "shotgun": shotgun_barrel, "sniper": sniper_barrel}
MAGS = {"smg": smg_mag, "rifle": rifle_mag, "shotgun": shotgun_mag, "sniper": sniper_mag}
STOCKS = {"smg": smg_stock, "rifle": rifle_stock, "shotgun": shotgun_stock, "sniper": sniper_stock}
MUZZLE_K = {"smg": 1.0, "rifle": 0.85, "shotgun": 1.3, "sniper": 0.85}
RAIL = {"smg": 0.085, "rifle": 0.095, "shotgun": 0.10, "sniper": 0.15}


def build(script):
    name = os.path.basename(os.path.dirname(os.path.abspath(script)))
    frame, slot, variant = name.split("_", 2)
    a = hb.Asset(script)
    p = Part(a.part(slot, bevel=BEVEL[slot]))
    if slot == "barrel":
        end = BARRELS[frame](a, p, variant)
        a.socket("SOCKET_muzzle", p.p, at=(0, 0, end), forward=(0, 0, -1))
    elif slot == "muzzle":
        muzzle(a, p, variant, MUZZLE_K[frame])
    elif slot == "emitter":
        emitter(a, p, variant, MUZZLE_K[frame])
    elif slot == "mag":
        MAGS[frame](a, p, variant)
    elif slot == "stock":
        STOCKS[frame](a, p, variant)
    elif slot == "sight":
        if frame == "sniper" and variant == "scope":
            long_scope(a, p)
        else:
            sight(a, p, variant, RAIL[frame])
    else:
        raise ValueError(slot)
    a.collider("COL_part", points=p.box_points())
    a.export()
