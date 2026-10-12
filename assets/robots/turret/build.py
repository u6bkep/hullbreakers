"""Hull Turret: a robot gun bolted to the outside of the carrier's hull. Bevy space: +Y out
of the hull, -Z forward (where the gun points at rest), +X right. Origin = the centre of
its mounting face, on the hull. Side outlines are plane "zy": points are (z, y).

Rigid parts:
  base -> head -> barrel
The whole robot turns about +Y in the game, so the base (a faceted drum on a round flange,
with the yoke arms) is one body. The head pitches about X at the trunnions; the twin
barrels slide back +Z into their sleeves as recoil.

Clearances (the game pitches the head, the base doesn't): the head's rear half stays
within 0.31 m of the trunnion axis and the weak-spot housing starts 0.33 m from it, so
the head can pitch up through 90 deg. Its underside stays above the saddle (0.50) for any
pitch; the front corners reach the saddle past about 45 deg nose-down.
"""
import math

import hb

a = hb.Asset(__file__)

PAINT, ACCENT, DARK, BARE = "paint_primary", "paint_secondary", "metal_dark", "metal_bare"
LIGHT, CORE_MAT, EYE = "emissive_secondary", "emissive_core", "emissive"
SIDES = (("l", -1), ("r", 1))

PIVOT_Y = 0.85                  # trunnion axis (along X), z = 0
BORE_Y, BORE_X = 0.885, 0.235     # the twin barrels' axes: (+-BORE_X, BORE_Y)
LAMP_Y = 0.99                   # searchlight axis, x = 0
LENS_FRONT = -0.48
MUZZLE_Z = -0.88
BREECH_Z = -0.22
CORE, CORE_R = (0.0, 0.60, 0.48), 0.14
ARM_X0, ARM_X1 = 0.34, 0.46     # yoke arms, |x|

# ---------------------------------------------------------------- base
base = a.part("base", bevel=0.016, smooth_angle=40)
base.cylinder("y", (0, 0), 0.53, (0.0, 0.07), mat=DARK, bevel=0.012)            # mounting flange
for k in range(10):                                                             # flange bolts
    th = 2 * math.pi * (k + 0.5) / 10
    base.cylinder("y", (0.495 * math.sin(th), -0.495 * math.cos(th)), 0.022, (0.07, 0.09), mat=BARE,
                  segments=8, bevel=0)
DRUM = [(0.46, 0.06), (0.46, 0.40), (0.40, 0.46), (0, 0.46)]
base.lathe(DRUM, axis="y", center=(0, 0), mats=[PAINT] * 3, segments=12, smooth_angle=20)  # armoured drum
base.lathe([(0.468, 0.24), (0.468, 0.31)], axis="y", center=(0, 0), mat=ACCENT, segments=12,
           smooth_angle=20, bevel=0.008)                                        # hazard band
base.cylinder("y", (0, 0), 0.385, (0.455, 0.472), mat=LIGHT, bevel=0)          # status ring under the collar
base.cylinder("y", (0, 0), 0.36, (0.44, 0.50), mat=DARK, bevel=0.01)           # turntable collar
base.box((-ARM_X1, 0.44, -0.22), (ARM_X1, 0.50, 0.22), mat=DARK, bevel=0.01)    # yoke saddle
ARM = [(0.21, 0.46), (0.15, PIVOT_Y)]
for k in range(1, 4):
    th = k * math.pi / 4
    ARM.append((0.15 * math.cos(th), PIVOT_Y + 0.15 * math.sin(th)))
ARM += [(-0.15, PIVOT_Y), (-0.21, 0.46)]
for _, s in SIDES:
    base.prism(ARM, span=sorted((s * ARM_X0, s * ARM_X1)), mat=DARK, smooth=True, smooth_angle=50)  # yoke arm
    base.cylinder("x", (PIVOT_Y, 0), 0.075, sorted((s * ARM_X1, s * (ARM_X1 + 0.03))), mat=BARE,
                  bevel=0.008)                                                  # trunnion cap
    base.box((s * ARM_X1 - 0.01 * (s < 0), 0.55, -0.025), (s * ARM_X1 + 0.01 * (s > 0), 0.73, 0.025),
             mat=LIGHT, bevel=0)                                                # red status strip
# Weak spot: the coolant core, a glowing sphere in a housing on the back of the drum. The
# housing's front wall (sloped back at the top to clear the head) hides it from the front;
# it shows through the open rear, caged by two bars, and through a slot in the top.
HX, HY0, HY1, HZ0, HZ1 = 0.21, 0.38, 0.82, 0.34, 0.66
base.hull([(x, y, z) for x in (-HX, HX) for y, z in ((HY0, 0.30), (HY1, 0.33), (HY0, 0.36), (HY1, 0.37))],
          mat=DARK)                                                             # front wall
for _, s in SIDES:
    base.box((s * (HX - 0.04), HY0, HZ0), (s * HX, HY1, HZ1), mat=PAINT)       # side walls
base.box((-HX, HY0, HZ0), (HX, HY0 + 0.04, HZ1), mat=PAINT)                    # floor
base.box((-HX, HY1 - 0.04, HZ0), (HX, HY1, 0.50), mat=PAINT)                   # roof, open at the back
base.hull([(x, y, z) for x in (-0.15, 0.15) for z, y in ((0.32, HY0), (HZ1 - 0.02, HY0), (0.44, 0.14),
                                                         (0.32, 0.14))], mat=DARK)  # gusset onto the drum
ring = 8
base.lathe([(CORE_R * math.sin(math.pi * k / ring), CORE[2] - CORE_R * math.cos(math.pi * k / ring))
            for k in range(1, ring)], axis="z", center=(CORE[0], CORE[1]), mats=[CORE_MAT] * (ring - 2),
           bevel=0)                                                             # the core
for x in (-0.06, 0.06):                                                         # guard bars
    base.box((x - 0.012, HY0 + 0.02, HZ1 - 0.02), (x + 0.012, HY1 - 0.02, HZ1 + 0.01), mat=BARE, bevel=0)
    base.box((x - 0.012, HY1 - 0.03, 0.49), (x + 0.012, HY1 - 0.005, HZ1 + 0.01), mat=BARE, bevel=0)
a.socket("SOCKET_weak", base, at=CORE)

# ---------------------------------------------------------------- head
head = a.part("head", base, pivot=(0, PIVOT_Y, 0), bevel=0.015, smooth_angle=40)
HEAD = [(-0.34, 0.70), (-0.34, 1.12), (-0.26, 1.19), (0.0, 1.16), (0.16, 1.115), (0.27, 0.99),
        (0.30, 0.82), (0.24, 0.66), (0.12, 0.57), (-0.20, 0.59)]
head.prism(HEAD, span=(-0.30, 0.30), mat=PAINT)                                # shell
for _, s in SIDES:
    head.cylinder("x", (PIVOT_Y, 0), 0.12, sorted((s * 0.28, s * 0.36)), mat=DARK, bevel=0)  # trunnion
    head.lathe([(0.08, -0.30), (0.08, -0.40), (0.07, -0.42), (0.07, -0.54)], axis="z",
               center=(s * BORE_X, BORE_Y), mats=[DARK] * 3, bevel=0.008)       # barrel sleeve
head.cylinder("z", (0, LAMP_Y), 0.18, (-0.445, -0.30), mat=DARK, bevel=0.012)  # searchlight housing
head.lathe([(0.15, -0.43), (0.15, -0.455), (0.11, -0.475), (0, LENS_FRONT)], axis="z", center=(0, LAMP_Y),
           mats=[EYE] * 3, bevel=0)                                             # lens: a proud dome
head.box((-0.19, 1.155, -0.50), (0.19, 1.205, -0.27), mat=ACCENT, bevel=0.01)  # hazard brow over the lamp
head.box((-0.10, 0.90, 0.27), (0.10, 0.925, 0.295), mat=LIGHT, bevel=0)       # rear status strip
a.socket("SOCKET_eye", head, at=(0, LAMP_Y, LENS_FRONT), forward=(0, 0, -1))

# ---------------------------------------------------------------- barrel
barrel = a.part("barrel", head, pivot=(0, BORE_Y, BREECH_Z), bevel=0.008)
for _, s in SIDES:
    barrel.lathe([(0.055, BREECH_Z), (0.055, -0.72), (0.08, -0.72), (0.08, -0.78), (0.058, -0.78),
                  (0.058, -0.80), (0.08, -0.80), (0.08, MUZZLE_Z), (0.036, MUZZLE_Z), (0.036, MUZZLE_Z + 0.025),
                  (0, MUZZLE_Z + 0.025)], axis="z", center=(s * BORE_X, BORE_Y),
                 mats=[DARK, BARE, BARE, BARE, BARE, BARE, BARE, BARE, DARK, DARK])  # barrel, muzzle brake
a.socket("SOCKET_muzzle", barrel, at=(0, BORE_Y, MUZZLE_Z), forward=(0, 0, -1))

# ---------------------------------------------------------------- affixes
# Variations the game shows only on a robot that has them (it hides every affix_ part
# otherwise): affix_<tag>_<what>, each on the part it rides. Lights are emissive_secondary;
# the game tints them per affix.
BOLT = dict(mat=BARE, bevel=0)

# Armoured: a gun shield round the lamp and barrels (a glacis over the top, cheek plates
# outboard of the barrels, a chin slab under them; open at the front), and skirts round
# the drum. Thick dark slabs, bolted.
shield = a.part("affix_armour_shield", head, pivot=(0, PIVOT_Y, 0), bevel=0.015)
shield.hull([(x, y, z) for x in (-0.34, 0.34) for z, y in ((-0.56, 1.215), (-0.54, 1.27), (-0.14, 1.31),
                                                           (-0.08, 1.27), (-0.08, 1.18))], mat=DARK)  # glacis
CHEEK = [(-0.26, 0.68), (-0.26, 1.20), (-0.55, 1.215), (-0.62, 0.95), (-0.60, 0.68)]
for _, s in SIDES:
    shield.prism(CHEEK, span=sorted((s * 0.30, s * 0.34)), mat=DARK)
    for y, z in ((1.10, -0.32), (0.76, -0.32), (1.10, -0.52), (0.76, -0.54)):
        shield.cylinder("x", (y, z), 0.022, sorted((s * 0.335, s * 0.355)), segments=8, **BOLT)
shield.prism([(-0.30, 0.66), (-0.30, 0.80), (-0.60, 0.80), (-0.64, 0.73), (-0.60, 0.66)], span=(-0.34, 0.34),
             mat=DARK)                                                          # chin slab

skirts = a.part("affix_armour_skirts", base, bevel=0.015)
for deg in (0, 55, -55, 100, -100):
    phi = math.radians(deg)
    d = (math.sin(phi), -math.cos(phi))                                         # outward, (x, z)
    t = (math.cos(phi), math.sin(phi))                                          # along the slab

    def at(r, y, w):
        return (d[0] * r + t[0] * w, y, d[1] * r + t[1] * w)

    skirts.hull([at(r, y, w) for r, y in ((0.465, 0.09), (0.54, 0.09), (0.465, 0.42), (0.515, 0.42))
                 for w in (-0.14, 0.14)], mat=DARK)
    for w in (-0.09, 0.09):                                                     # square bolt heads
        skirts.hull([at(r, y, w + dw) for r in (0.515, 0.535) for y in (0.315, 0.345) for dw in (-0.015, 0.015)],
                    **BOLT)

# Shielded: an emitter dish on a mast clamped to the side of the core housing, its rim glowing.
emit = a.part("affix_shield_emitter", base, bevel=0.008)
EM = (0.27, 0.56)                                                               # (x, z)
emit.box((HX, 0.50, EM[1] - 0.05), (EM[0] + 0.035, 0.62, EM[1] + 0.05), mat=DARK)   # clamp
emit.cylinder("y", EM, 0.028, (0.55, 1.14), mat=BARE, segments=8, bevel=0)     # mast
emit.lathe([(0.035, 1.12), (0.13, 1.21), (0.13, 1.235), (0.05, 1.235)], axis="y", center=EM,
           mats=[DARK, DARK, DARK])                                             # dish
emit.cylinder("y", EM, 0.134, (1.212, 1.232), mat=LIGHT, bevel=0)              # glowing rim
emit.lathe([(0.04, 1.23), (0.04, 1.26), (0.018, 1.28), (0, 1.28)], axis="y", center=EM,
           mats=[LIGHT] * 3, bevel=0)                                           # emitter nub

# ---------------------------------------------------------------- colliders
RING = [(math.sin(2 * math.pi * k / 12), -math.cos(2 * math.pi * k / 12)) for k in range(12)]
a.collider("COL_base", parent=base,
           points=[(0.53 * c, y, 0.53 * s) for c, s in RING for y in (0.0, 0.07)]
           + [(0.46 * c, 0.40, 0.46 * s) for c, s in RING]
           + [(0.36 * c, 0.50, 0.36 * s) for c, s in RING]
           + [(x, y, z) for x in (-HX, HX) for y in (HY0, HY1) for z in (0.37, HZ1)])
for side, s in SIDES:
    a.collider(f"COL_yoke_{side}", parent=base,
               points=[(x, y, z) for z, y in ARM for x in (s * ARM_X0, s * ARM_X1)])
a.collider("COL_head", parent=head,
           points=[(x, y, z) for z, y in HEAD for x in (-0.30, 0.30)]
           + [(x, y, z) for x in (-0.18, 0.18) for y in (LAMP_Y - 0.18, LAMP_Y + 0.18) for z in (LENS_FRONT,)])
a.export()
