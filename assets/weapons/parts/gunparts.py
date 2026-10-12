"""Weapon part variants: the swappable pieces a crew gun is assembled from (see
src/sim/items.rs `Parts`, and the receivers in assets/weapons/<frame>_receiver).

Each part is its own asset, assets/weapons/parts/<frame>_<slot>_<variant>/, whose
build.py only calls `build(__file__)`; the geometry is all here, so the variants of a
slot are drawn side by side and stay consistent. Bevy space, metres: +Y up, -Z
forward, +X right. A part's origin is the socket it mounts on: the receiver's
SOCKET_barrel / SOCKET_mag / SOCKET_stock / SOCKET_sight, or (a muzzle device) the
barrel's SOCKET_muzzle. Barrels run along -Z with the bore on the origin; magazines
hang down -Y from the top of the magwell; stocks run back along +Z from the
receiver's rear face; sights sit on the rail's top at y = 0.

Every variant reads as what it does from its outline: a long barrel is long, a heavy
one is thick and finned, a drum is a drum, a quick mag is two taped together, a
compensator has ports cut in its top, a booster is a can with a flared end, a scope
is a tube with a big objective bell. Paint (paint_primary, paint_secondary) is on the
pieces a maker's paint job should reach; the rest is dark metal, bare metal and
polymer.
"""
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


# ---------------------------------------------------------------------------------------

BEVEL = {"barrel": 0.0015, "mag": 0.0025, "stock": 0.003, "sight": 0.0015, "muzzle": 0.002}


def build(script):
    name = os.path.basename(os.path.dirname(os.path.abspath(script)))
    frame, slot, variant = name.split("_", 2)
    a = hb.Asset(script)
    p = Part(a.part(slot, bevel=BEVEL[slot]))
    if slot == "barrel":
        end = (smg_barrel if frame == "smg" else rifle_barrel)(a, p, variant)
        a.socket("SOCKET_muzzle", p.p, at=(0, 0, end), forward=(0, 0, -1))
    elif slot == "muzzle":
        muzzle(a, p, variant, 1.0 if frame == "smg" else 0.85)
    elif slot == "mag":
        (smg_mag if frame == "smg" else rifle_mag)(a, p, variant)
    elif slot == "stock":
        (smg_stock if frame == "smg" else rifle_stock)(a, p, variant)
    elif slot == "sight":
        sight(a, p, variant, 0.085 if frame == "smg" else 0.095)
    else:
        raise ValueError(slot)
    a.collider("COL_part", points=p.box_points())
    a.export()
