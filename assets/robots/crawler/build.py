"""Hull crawler: a low six-legged robot on magnetic feet that walks any surface (floor, wall,
ceiling, a ship's outer hull). Bevy space: +Y away from the surface, -Z forward, +X right.
Origin = the centre of the feet's plane, on the surface (y = 0 is the surface).

Read at range: a wide, flat, faceted oval carapace (slate blue, two ochre bands, a dark seam
behind the front plate), six bright-metal legs splayed wide with their knees at the shell's
crown, each on a round foot pad that glows cyan underneath, and a small wedge head in front.
Lights, loud to quiet: the four orange-red eyes (emissive); the cyan foot pads and the two seam
lights in the flanks (emissive_secondary); and the weak spot, a magenta core in a dark cowled
cup on the back behind a grille (emissive_core, SOCKET_weak at its centre). The cowl's rim is
tall at the front and low at the back, so the core is seen from above and above-behind, and
hidden by the cowl and head from the front and by the shell from below.

Moving parts: head tips about its local X at the neck; each leg_* swings about its local Y
(foot fore/aft) and lifts about its local Z, pivoted at its hip pin in the shell's lip."""
import math

import hb

a = hb.Asset(__file__)
PAINT, OCHRE, DARK, BARE, BLACK = "paint_primary", "paint_secondary", "metal_dark", "metal_bare", "polymer"
GLOW, LIGHT, CORE = "emissive", "emissive_secondary", "emissive_core"


def sym(*pts):
    """Points plus their mirror images across x = 0 (points on x = 0 once)."""
    out = []
    for x, y, z in pts:
        out.append((x, y, z))
        if x:
            out.append((-x, y, z))
    return out


def beam(a_, b_, half_w, half_h, k=1.0):
    """Hull points of a straight bar from a_ to b_: a rectangle across it at each end (half_h
    toward up, half_w across), the far one scaled by k."""
    d = [b_[i] - a_[i] for i in range(3)]
    n = math.sqrt(sum(c * c for c in d))
    d = [c / n for c in d]
    side = [d[2], 0.0, -d[0]]                                         # horizontal, square to d
    ns = math.sqrt(sum(c * c for c in side))
    side = [c / ns for c in side]
    upv = [d[1] * side[2] - d[2] * side[1], d[2] * side[0] - d[0] * side[2], d[0] * side[1] - d[1] * side[0]]
    if upv[1] < 0:
        upv = [-c for c in upv]
    out = []
    for p, s in ((a_, 1.0), (b_, k)):
        for u in (-1, 1):
            for v in (-1, 1):
                out.append(tuple(p[i] + upv[i] * u * half_h * s + side[i] * v * half_w * s for i in range(3)))
    return out


def pin(c, axis, r, half, n=8):
    """Hull points of an n-sided pin centred at c along the unit-ish `axis`."""
    ln = math.sqrt(sum(v * v for v in axis))
    ax = [v / ln for v in axis]
    ref = (0.0, 1.0, 0.0) if abs(ax[1]) < 0.9 else (1.0, 0.0, 0.0)
    e1 = [ax[1] * ref[2] - ax[2] * ref[1], ax[2] * ref[0] - ax[0] * ref[2], ax[0] * ref[1] - ax[1] * ref[0]]
    l1 = math.sqrt(sum(v * v for v in e1))
    e1 = [v / l1 for v in e1]
    e2 = [ax[1] * e1[2] - ax[2] * e1[1], ax[2] * e1[0] - ax[0] * e1[2], ax[0] * e1[1] - ax[1] * e1[0]]
    out = []
    for h in (-half, half):
        for i in range(n):
            t = 2 * math.pi * (i + 0.5) / n
            out.append(tuple(c[j] + ax[j] * h + r * (math.cos(t) * e1[j] + math.sin(t) * e2[j]) for j in range(3)))
    return out


# ---------------------------------------------------------------- carapace plan
# A flattened oval in plan (a superellipse, so the flanks run nearly straight) whose crown
# rises to 0.35 in the middle. Each plate is the hull of cross-sections at a few stations
# along z; neighbouring plates meet on a shared section, so their chamfers cut a panel seam.
W, ZC, R = 0.25, 0.03, 0.33           # half-width, plan centre z, half-length
LIP_LO, LIP_HI, BELLY = 0.185, 0.215, 0.15


def _t(z):
    return min(abs(z - ZC) / R, 0.999)


def width(z):
    return W * (1 - _t(z) ** 2.5) ** 0.4


def crown(z):
    return 0.24 + 0.11 * math.sqrt(1 - _t(z) ** 2)


def section(z, k=1.0):
    """The carapace's cross-section at z (x, y, z points), its outline scaled by k about the
    lip's middle: lip, shoulder facet, crown facet, underside."""
    w, yt = width(z), crown(z)
    ys = LIP_HI + 0.62 * (yt - LIP_HI)
    c = 0.5 * (LIP_LO + LIP_HI)
    pts = [(w, LIP_LO), (w, LIP_HI), (0.72 * w, ys), (0.32 * w, yt)]
    return sym(*[(k * x, c + k * (y - c), z) for x, y in pts])


body = a.part("body", pivot=(0.0, 0.22, 0.0))


def plate(zs, mat, k=1.0, bevel=None):
    body.hull([p for z in zs for p in section(z, k)], mat=mat, bevel=bevel)


plate((-0.27, -0.165, -0.075), PAINT)              # front plate (pronotum)
plate((-0.090, -0.040), DARK, k=0.93)                    # the dark seam behind it
plate((-0.055, -0.015), OCHRE)                           # ochre band
plate((-0.015, 0.09, 0.20), PAINT)                 # back plate
plate((0.20, 0.235), OCHRE)                              # ochre band
plate((0.235, 0.29, 0.33), PAINT)                        # tail
# Belly: a dark keel under the shell, between the hips.
body.hull([(x, y, z) for z in (-0.22, 0.03, 0.28) for x, y in
           ((0.62 * width(z), 0.17), (-0.62 * width(z), 0.17), (0.42 * width(z), 0.10), (-0.42 * width(z), 0.10))],
          mat=DARK)
# Two ochre bars across the keel: markings for the underside, seen on a ceiling.
for zb in (-0.11, 0.15):
    body.box((-0.075, 0.092, zb - 0.022), (0.075, 0.104, zb + 0.022), mat=OCHRE, bevel=0)

# Seam lights: a cyan strip down each flank in the dark seam.
for s in (-1, 1):
    w = width(-0.065)
    xa, xb = sorted((s * 0.62 * w, s * 0.985 * w))
    body.box((xa, 0.19, -0.070), (xb, 0.255, -0.060), mat=LIGHT, bevel=0)

# ---------------------------------------------------------------- weak spot
# A faceted cowl around the core: a ring of slate segments whose rim is one plane, tall at the
# front (it hides the core from ahead) and low at the back (the core shows over it from
# above-behind); the core glows in it under three grille bars lying on the rim plane.
WK = (0.0, 0.36, 0.12)
CORE_R, CUP_IN = 0.072, 0.088


def rim(z):
    return 0.415 - 0.22 * (z - WK[2])


N = 10
HOOD_OUT, HOOD_BASE, HOOD_Y = 0.11, 0.15, 0.29
for i in range(N):
    pts = []
    for th in ((i - 0.5) * 2 * math.pi / N, (i + 0.5) * 2 * math.pi / N):
        sx, cz = math.sin(th), -math.cos(th)
        for r in (CUP_IN, HOOD_OUT):
            pts.append((r * sx, rim(WK[2] + r * cz), WK[2] + r * cz))
        for r in (CUP_IN, HOOD_BASE):
            pts.append((r * sx, HOOD_Y, WK[2] + r * cz))
    body.hull(pts, mat=PAINT, bevel=0)
ring = [(CORE_R * math.sqrt(1 - (t / 3) ** 2), WK[1] + CORE_R * t / 3) for t in range(-3, 4)]
body.lathe([(0, WK[1] - CORE_R)] + ring[1:-1] + [(0, WK[1] + CORE_R)], axis="y", center=(WK[0], WK[2]), mat=CORE,
           segments=12, bevel=0)
for zb in (WK[2] - 0.05, WK[2], WK[2] + 0.05):
    xw = math.sqrt(CUP_IN ** 2 - (zb - WK[2]) ** 2) + 0.01
    body.hull([(x, rim(z) + dy, z) for x in (-xw, xw) for z in (zb - 0.008, zb + 0.008) for dy in (0.0, 0.012)],
              mat=BLACK, bevel=0)
a.socket("SOCKET_weak", body, at=WK)

# Vent outlet on the tail's spine, where it steams when it overheats.
VENT = (0.0, 0.305, 0.30)
body.box((-0.025, 0.28, 0.285), (0.025, VENT[1], 0.315), mat=BLACK, bevel=0)
a.socket("SOCKET_vent", body, at=VENT, forward=(0, 1, 0), up=(0, 0, 1))

# ---------------------------------------------------------------- head
NECK = (0.0, 0.24, -0.24)
head = a.part("head", body, pivot=NECK)
head.hull(sym((0.11, 0.15, -0.20), (0.11, 0.33, -0.20), (0.06, 0.365, -0.20), (0.065, 0.20, -0.39),
              (0.065, 0.335, -0.39), (0.035, 0.35, -0.39), (0.06, 0.13, -0.28)), mat=PAINT)
# Ochre brow: a V over the eyes.
head.hull(sym((0.078, 0.325, -0.30), (0.078, 0.352, -0.30), (0.07, 0.348, -0.408), (0.07, 0.33, -0.408),
              (0.0, 0.322, -0.408), (0.0, 0.342, -0.408), (0.0, 0.372, -0.30)), mat=OCHRE, bevel=0.005)
# The four eyes: dark rims, glowing domes.
FACE = -0.392
EYES = [(-0.023, 0.308, 0.02), (0.023, 0.308, 0.02), (-0.05, 0.27, 0.014), (0.05, 0.27, 0.014)]
for x, y, r in EYES:
    rim_r = r + 0.006
    head.lathe([(rim_r, FACE + 0.01), (rim_r, FACE - 0.008), (r, FACE - 0.008), (0, FACE - 0.008 - 0.6 * r)],
               axis="z", center=(x, y), mats=[BLACK, BLACK, GLOW],
               segments=8, bevel=0)
a.socket("SOCKET_eye", head, at=(0.0, 0.305, -0.42))
# Spray nozzle: a dark housing under the chin, two short barrels with bare tips.
head.box((-0.05, 0.145, -0.30), (0.05, 0.21, -0.41), mat=DARK)
MZ = (0.0, 0.175, -0.475)
for x in (-0.024, 0.024):
    head.cylinder("z", (x, MZ[1]), 0.016, (-0.40, -0.46), mat=DARK, segments=8, bevel=0)
    head.cylinder("z", (x, MZ[1]), 0.013, (-0.46, MZ[2]), mat=BARE, segments=8, bevel=0)
a.socket("SOCKET_muzzle", head, at=MZ)

# ---------------------------------------------------------------- legs
# tag, side, hip z, knee z, foot z. Each leg rises from a hip pin under the shell's lip to a
# knee at the crown's height and drops to a round magnetic pad on the surface.
LEGS = [("fl", -1, -0.13, -0.25, -0.30), ("ml", -1, 0.02, 0.01, 0.0), ("rl", -1, 0.18, 0.26, 0.30),
        ("fr", 1, -0.13, -0.25, -0.30), ("mr", 1, 0.02, 0.01, 0.0), ("rr", 1, 0.18, 0.26, 0.30)]
PAD_R = 0.062
for tag, s, zh, zk, zf in LEGS:
    hip = (s * (width(zh) - 0.02), 0.20, zh)
    knee = (s * 0.415, 0.30, zk)
    ankle = (s * 0.44, 0.055, zf)
    leg = a.part(f"leg_{tag}", body, pivot=hip)
    leg.hull(pin(hip, (0, 1, 0), 0.034, 0.034), mat=BLACK, bevel=0)
    leg.hull(beam(hip, knee, 0.03, 0.03, 0.75), mat=BARE)
    d = (knee[0] - hip[0], 0.0, knee[2] - hip[2])
    leg.hull(pin(knee, (-d[2], 0.0, d[0]), 0.03, 0.03), mat=BLACK, bevel=0)
    leg.hull(beam(knee, ankle, 0.024, 0.022, 0.6), mat=DARK)
    f = [knee[i] + 0.12 * (ankle[i] - knee[i]) for i in range(3)]
    g = [knee[i] + 0.36 * (ankle[i] - knee[i]) for i in range(3)]
    leg.hull(beam(f, g, 0.028, 0.026, 0.92), mat=OCHRE, bevel=0)
    leg.cylinder("y", (ankle[0], ankle[2]), 0.022, (0.035, 0.07), mat=BLACK, segments=8, bevel=0)
    leg.cylinder("y", (ankle[0], ankle[2]), PAD_R, (0.012, 0.042), mat=DARK, segments=8)
    leg.cylinder("y", (ankle[0], ankle[2]), PAD_R - 0.006, (0.0, 0.014), mat=LIGHT, segments=8, bevel=0)

# ---------------------------------------------------------------- affixes
# Variations the game shows only on a robot that has them (it hides every affix_ part
# otherwise): affix_<tag>_<what>, each on the part it rides. Their lights are
# emissive_secondary; the game tints them per affix.


def upper(z, g):
    """The lip-top, shoulder and crown points of section(z), pushed out by g."""
    w, yt = width(z), crown(z)
    ys = LIP_HI + 0.62 * (yt - LIP_HI)
    return [(w + g, LIP_HI), (0.72 * w + 0.6 * g, ys + g), (0.32 * w, yt + g)]


# Armoured: thick dark plates bolted over the front plate and the back plate's flanks, and a
# helmet over the head. The spine and the cowl stay bare, so the core is still seen.
arm = a.part("affix_armour_carapace", body, pivot=(0.0, 0.22, 0.0), bevel=0.008)
arm.hull([p for z in (-0.255, -0.17, -0.09) for g in (0.0, 0.026) for p in sym(*[(x, y, z) for x, y in upper(z, g)])],
         mat=DARK)
for s in (-1, 1):
    pts = []
    for z in (0.0, 0.19):
        for g in (0.0, 0.026):
            (x0, y0), (x1, y1), _ = upper(z, g)
            pts += [(s * x0, y0, z), (s * x1, y1, z), (s * x0, LIP_LO, z)]
    arm.hull(pts, mat=DARK)
    for z in (0.02, 0.17):
        xb = width(z) + 0.026
        xa, xc = sorted((s * (xb - 0.004), s * (xb + 0.008)))
        arm.cylinder("x", (0.205, z), 0.011, (xa, xc), mat=BARE, segments=8, bevel=0)
helm = a.part("affix_armour_head", head, pivot=NECK, bevel=0.008)
helm.hull(sym((0.12, 0.33, -0.20), (0.12, 0.36, -0.20), (0.07, 0.392, -0.23), (0.095, 0.37, -0.36),
              (0.095, 0.345, -0.36), (0.0, 0.385, -0.36)), mat=DARK)
for s in (-1, 1):
    xa, xb = sorted((s * 0.105, s * 0.125))
    helm.box((xa, 0.17, -0.21), (xb, 0.33, -0.36), mat=DARK)

# Shielded: an emitter dish on a stub on the front plate, its rim glowing.
emit = a.part("affix_shield_emitter", body, pivot=(0.0, 0.22, 0.0), bevel=0)
EM = (0.0, -0.13)                                                        # (x, z)
emit.cylinder("y", EM, 0.022, (0.32, 0.35), mat=DARK, segments=8)
emit.cylinder("y", EM, 0.009, (0.34, 0.40), mat=BARE, segments=8)
emit.lathe([(0.012, 0.395), (0.048, 0.408), (0.052, 0.416), (0.02, 0.42), (0, 0.422)], axis="y", center=EM,
           mat=DARK, segments=8)
emit.lathe([(0.054, 0.406), (0.054, 0.416)], axis="y", center=EM, mat=LIGHT, segments=8)

# Overclocked: radiator fins either side of the vent on the tail, glowing between them.
oc = a.part("affix_overclock_radiators", body, pivot=(0.0, 0.22, 0.0), bevel=0)
for s in (-1, 1):
    xa, xb = sorted((s * 0.035, s * 0.135))
    oc.box((xa, 0.27, 0.245), (xb, 0.30, 0.315), mat=LIGHT)
    for k in range(4):
        x0, x1 = sorted((s * (0.04 + k * 0.026), s * (0.05 + k * 0.026)))
        oc.box((x0, 0.27, 0.25), (x1, 0.36 - k * 0.012, 0.31), mat=BARE)

# Volatile: a fat ochre canister strapped across the tail, a glowing warning band round it.
vol = a.part("affix_volatile_canister", body, pivot=(0.0, 0.22, 0.0), bevel=0.006)
CAN = (0.215, 0.375)                                                     # (y, z)
vol.lathe([(0.025, -0.135), (0.042, -0.12), (0.042, 0.12), (0.025, 0.135)], axis="x", center=CAN,
          mats=[OCHRE, OCHRE, OCHRE], segments=8)
vol.cylinder("x", CAN, 0.045, (-0.02, 0.02), mat=LIGHT, segments=8, bevel=0)
for x in (-0.08, 0.08):
    vol.box((x - 0.012, CAN[0] - 0.048, 0.32), (x + 0.012, CAN[0] + 0.048, CAN[1] + 0.008), mat=BLACK, bevel=0)

# ---------------------------------------------------------------- colliders
# One hull for the shell, cowl and head; one slab through the legs.
a.collider("COL_body", points=sym((0.25, 0.10, 0.03), (0.25, 0.22, 0.03), (0.14, 0.10, -0.27), (0.14, 0.29, -0.27),
                                  (0.14, 0.10, 0.33), (0.14, 0.29, 0.33), (0.08, 0.35, 0.03), (0.11, 0.47, 0.0),
                                  (0.11, 0.42, 0.23), (0.075, 0.14, -0.475), (0.075, 0.36, -0.40),
                                  (0.11, 0.37, -0.21)))
a.collider("COL_legs", points=sym((0.49, 0.0, -0.37), (0.49, 0.0, 0.37), (0.49, 0.05, -0.37), (0.49, 0.05, 0.37),
                                  (0.37, 0.40, -0.26), (0.37, 0.40, 0.27), (0.20, 0.17, -0.37), (0.20, 0.17, 0.37)))
a.export()
