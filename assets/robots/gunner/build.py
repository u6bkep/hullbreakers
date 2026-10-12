"""Gunner: the baseline hostile robot. Bevy space: +Y up, -Z forward, +X right (its right
arm is the gun). Origin = between the feet on the ground. Side outlines are plane "zy":
points are (z, y).

Rigid parts, each pivoted at its joint, for procedural animation:
  pelvis -> torso -> head
         -> torso -> upper_arm_l -> forearm_l -> hand_l
         -> torso -> upper_arm_r -> gun_arm_r
  pelvis -> thigh_l -> shin_l -> foot_l   (and _r)
Hinges run along X. The child carries the knuckle (a cylinder on the pivot axis); the
parent carries the fork plates and the bare-metal axle bolt, so a rotation reads as a joint.
"""
import math

import hb

a = hb.Asset(__file__)

PAINT, ACCENT, DARK, BARE = "paint_primary", "paint_secondary", "metal_dark", "metal_bare"
SIDES = (("l", -1), ("r", 1))

LX = 0.18                       # leg line, |x|
HIP_Y, KNEE, ANKLE = 0.98, (0.56, -0.06), (0.16, 0.01)   # knee/ankle as (y, z)
AX = 0.44                       # arm line, |x|
SHOULDER_Y, ELBOW_Y = 1.58, 1.22
WAIST = (0, 1.10, 0)
NECK = (0, 1.62, -0.05)
SMOOTH = 40                     # curved bands meeting shallower than this stay smooth
EYE_Y, EYE_FRONT = 1.785, -0.305


def bar(p0, p1, w0, w1):
    """Quad in zy from p0 to p1 (z, y), half widths w0 at p0 and w1 at p1."""
    dz, dy = p1[0] - p0[0], p1[1] - p0[1]
    n = math.hypot(dz, dy)
    pz, py = -dy / n, dz / n
    return [(p0[0] + pz * w0, p0[1] + py * w0), (p1[0] + pz * w1, p1[1] + py * w1),
            (p1[0] - pz * w1, p1[1] - py * w1), (p0[0] - pz * w0, p0[1] - py * w0)]


def clevis(part, xc, piv, toward, r, length, inner, outer, bolt_r, mat=DARK):
    """Fork plates either side of a hinge along X at piv=(z, y): a round lobe of radius r
    round the pivot, running `length` toward the parent along `toward` (z, y). Plates span
    inner..outer from xc on each side; a bare axle bolt runs through both."""
    tz, ty = toward
    n = math.hypot(tz, ty)
    tz, ty = tz / n, ty / n
    phi = math.atan2(ty, tz)
    pts = []
    for k in range(5):                     # lobe: the half circle facing away from the parent, in 45 deg steps
        th = phi + math.pi / 2 + k * math.pi / 4
        pts.append((piv[0] + r * math.cos(th), piv[1] + r * math.sin(th)))
    pz, py = pts[-1][0] - piv[0], pts[-1][1] - piv[1]
    pts.append((piv[0] + pz + tz * length, piv[1] + py + ty * length))
    pts.append((piv[0] - pz + tz * length, piv[1] - py + ty * length))
    for sgn in (-1, 1):
        part.prism(pts, span=(xc + sgn * inner, xc + sgn * outer), mat=mat, smooth=True, smooth_angle=50)
    part.cylinder("x", (piv[1], piv[0]), bolt_r, (xc - outer - 0.014, xc + outer + 0.014), mat=BARE,
                  bevel=0.004)


# ---------------------------------------------------------------- pelvis
pelvis = a.part("pelvis", pivot=(0, HIP_Y, 0), bevel=0.012, smooth_angle=SMOOTH)
pelvis.box((-0.12, 0.87, -0.13), (0.12, 1.07, 0.13), mat=DARK)
pelvis.box((-0.17, 1.05, -0.15), (0.17, 1.10, 0.15), mat=DARK)                 # waist bearing plate
pelvis.prism([(-0.16, 1.06), (0.16, 1.06), (0.11, 0.86), (-0.11, 0.86)], plane="xy",
             span=(-0.175, -0.11), mat=PAINT)                                   # front armour
pelvis.prism([(-0.14, 1.06), (0.14, 1.06), (0.10, 0.90), (-0.10, 0.90)], plane="xy",
             span=(0.11, 0.16), mat=PAINT)                                      # rear armour
pelvis.cylinder("x", (HIP_Y, 0), 0.036, (-LX - 0.09, LX + 0.09), mat=BARE, bevel=0.004)  # hip axle
for _, s in SIDES:
    pelvis.cylinder("x", (HIP_Y, 0), 0.058, sorted((s * (LX + 0.068), s * (LX + 0.085))), mat=DARK,
                    bevel=0.005)                                                # axle end caps

# ---------------------------------------------------------------- torso
torso = a.part("torso", pelvis, pivot=WAIST, bevel=0.014, smooth_angle=SMOOTH)
torso.lathe([(0.125, 1.07), (0.125, 1.14), (0.15, 1.14), (0.15, 1.21)], axis="y", center=(0, 0),
            mats=[DARK, DARK, DARK], bevel=0.006)                               # waist turntable
torso.box((-0.24, 1.18, -0.17), (0.24, 1.63, 0.17), mat=DARK)                  # core frame
CHEST = [(-0.21, 1.31), (-0.285, 1.43), (-0.285, 1.58), (-0.22, 1.665), (0.12, 1.665), (0.17, 1.61),
         (0.17, 1.31)]
torso.prism(CHEST, span=(-0.335, 0.335), mat=PAINT)                            # chest shell
torso.box((-0.22, 1.20, -0.20), (0.22, 1.26, 0.11), mat=PAINT)                  # belly plates
torso.box((-0.25, 1.27, -0.215), (0.25, 1.33, 0.13), mat=PAINT)
torso.box((-0.30, 1.47, -0.297), (-0.07, 1.565, -0.275), mat=ACCENT, bevel=0.006)   # hazard chest plate
for k in range(3):                                                                  # vent slats
    y = 1.475 + k * 0.034
    torso.box((0.07, y, -0.303), (0.27, y + 0.02, -0.275), mat=DARK, bevel=0.005)
torso.lathe([(0.13, 1.6), (0.13, 1.69), (0.095, 1.69), (0.095, 1.70)], axis="y", center=(0, -0.05),
            mats=[DARK, DARK, DARK], bevel=0.005)                               # neck collar
# back: power pack. The weak spot is its coolant core, a glowing sphere sat in an open
# teal housing and caged by two guard bars, flanked by two yellow coolant canisters with
# sight-glass strips. The torso hides it from the front.
torso.box((-0.21, 1.24, 0.15), (0.21, 1.60, 0.30), mat=DARK)
CORE, CORE_R = (0, 1.42, 0.31), 0.14
torso.frame((-0.18, CORE[1] - 0.18, 0.27), (0.18, CORE[1] + 0.18, 0.37), 0.032, "z", mat=PAINT,
            bevel=0.008)                                                        # core housing
AP = 0.148                                                                      # aperture half width
for sx in (-1, 1):                                                              # corner gussets: octagonal port
    for sy in (-1, 1):
        cx, cy = sx * AP, CORE[1] + sy * AP
        torso.prism([(cx, cy), (cx - sx * 0.06, cy), (cx, cy - sy * 0.06)], plane="xy",
                    span=(0.29, 0.362), mat=DARK, bevel=0.004)
ring = 8
torso.lathe([(CORE_R * math.sin(math.pi * k / ring), CORE[2] - CORE_R * math.cos(math.pi * k / ring))
             for k in range(1, ring)], axis="z", center=(CORE[0], CORE[1]),
            mats=["emissive_core"] * (ring - 2), bevel=0)                      # the core (capped poles)
for x in (-0.06, 0.06):                                                         # guard bars
    torso.box((x - 0.011, CORE[1] - 0.165, 0.36), (x + 0.011, CORE[1] + 0.165, 0.455), mat=BARE,
              bevel=0.004)
a.socket("SOCKET_weak", torso, at=CORE)
for x in (-0.245, 0.245):
    torso.lathe([(0.045, 1.23), (0.06, 1.26), (0.06, 1.57), (0.045, 1.60)], axis="y", center=(x, 0.27),
                mats=[ACCENT] * 3, bevel=0.006)                                 # coolant canisters
    torso.box((x - 0.011, 1.31, 0.322), (x + 0.011, 1.52, 0.336), mat="emissive_secondary",
              bevel=0.002)                                                      # sight glass
for _, s in SIDES:                                                              # shoulder hubs
    torso.cylinder("x", (SHOULDER_Y, 0), 0.09, sorted((s * 0.30, s * 0.37)), mat=DARK,
                   bevel=0.008)

# ---------------------------------------------------------------- head
head = a.part("head", torso, pivot=NECK, bevel=0.012, smooth_angle=SMOOTH)
head.cylinder("y", (0, -0.05), 0.06, (1.62, 1.72), mat=DARK, bevel=0.006)   # neck
head.prism([(-0.215, 1.685), (-0.255, 1.74), (-0.255, 1.835), (-0.205, 1.875), (0.03, 1.875), (0.08, 1.83),
            (0.08, 1.71), (0.03, 1.68)], span=(-0.155, 0.155), mat=PAINT)       # sensor housing
head.box((-0.11, 1.675, -0.245), (0.11, 1.71, -0.09), mat=DARK, bevel=0.006)    # chin
head.prism([(-0.315, 1.862), (-0.30, 1.895), (-0.13, 1.895), (-0.13, 1.862)], span=(-0.13, 0.13),
           mat=ACCENT, bevel=0.007)                                             # brow hood over the eye
head.lathe([(0.092, -0.22), (0.092, -0.268), (0.08, -0.278), (0.07, -0.278), (0, -0.278)], axis="z",
           center=(0, EYE_Y), mats=[DARK, DARK, DARK, DARK], bevel=0.003)   # eye bezel
head.lathe([(0.066, -0.27), (0.066, -0.29), (0.05, EYE_FRONT), (0, EYE_FRONT)], axis="z",
           center=(0, EYE_Y), mats=["emissive"] * 3, bevel=0.0015)  # lens: a proud puck, seen from the side too
for _, s in SIDES:                                                              # side sensor pods
    head.lathe([(0.05, s * 0.15), (0.05, s * 0.185), (0.03, s * 0.185), (0.03, s * 0.195)], axis="x",
               center=(1.79, -0.06), mats=[DARK, DARK, BARE], bevel=0.004)
    head.box((s * 0.15, 1.70, 0.0), (s * 0.165, 1.85, 0.07), mat=ACCENT, bevel=0.004)   # side panels
head.box((-0.085, 1.795, 0.074), (0.085, 1.811, 0.088), mat="emissive_secondary",
         bevel=0.002)                                                           # rear status strip
a.socket("SOCKET_eye", head, at=(0, EYE_Y, EYE_FRONT), forward=(0, 0, -1))

# ---------------------------------------------------------------- arms
uppers = {}
for side, s in SIDES:
    x = s * AX
    up = a.part(f"upper_arm_{side}", torso, pivot=(x, SHOULDER_Y, 0), bevel=0.012, smooth_angle=SMOOTH)
    up.cylinder("x", (SHOULDER_Y, 0), 0.078, sorted((s * 0.37, s * 0.51)), mat=DARK,
                bevel=0)                                                        # shoulder knuckle (ends hidden)
    up.prism([(-0.16, 1.50), (-0.175, 1.62), (-0.11, 1.72), (0.09, 1.72), (0.155, 1.64), (0.145, 1.50)],
             span=sorted((s * 0.345, s * 0.555)), mat=ACCENT)                   # pauldron
    up.cylinder("x", (SHOULDER_Y + 0.025, -0.01), 0.042, sorted((s * 0.545, s * 0.57)), mat=BARE,
                bevel=0.004)                                                    # shoulder bolt
    up.box((x - 0.05, 1.27, -0.055), (x + 0.05, 1.52, 0.055), mat=DARK)
    up.box((x - 0.068, 1.31, -0.078), (x + 0.068, 1.50, 0.07), mat=PAINT)
    clevis(up, x, (0, ELBOW_Y), (0, 1), 0.064, 0.11, 0.052, 0.074, 0.028)
    uppers[side] = up

# left: a forearm and a work clamp
xl = -AX
WRIST = (-0.13, 0.93)                                                           # (z, y)
fore = a.part("forearm_l", uppers["l"], pivot=(xl, ELBOW_Y, 0), bevel=0.012, smooth_angle=SMOOTH)
fore.cylinder("x", (ELBOW_Y, 0), 0.058, (xl - 0.05, xl + 0.05), mat=DARK, bevel=0.006)
fore.prism(bar((0.0, ELBOW_Y - 0.04), (WRIST[0] + 0.01, WRIST[1] + 0.03), 0.06, 0.05),
           span=(xl - 0.042, xl + 0.042), mat=DARK)                             # strut
fore.prism(bar((-0.015, ELBOW_Y - 0.08), (WRIST[0] + 0.025, WRIST[1] + 0.075), 0.075, 0.065),
           span=(xl - 0.072, xl + 0.072), mat=PAINT)                            # shell
fore.prism(bar((-0.03, ELBOW_Y - 0.105), (WRIST[0] + 0.035, WRIST[1] + 0.105), 0.055, 0.048),
           span=(xl - 0.086, xl - 0.07), mat=ACCENT, bevel=0.005)               # outer hazard plate
hand = a.part("hand_l", fore, pivot=(xl, WRIST[1], WRIST[0]), bevel=0.008, smooth_angle=SMOOTH)
hand.cylinder("y", (xl, WRIST[0]), 0.05, (WRIST[1] - 0.05, WRIST[1] + 0.02), mat=DARK,
              bevel=0.005)                                                      # wrist rotator
hand.box((xl - 0.058, 0.79, -0.19), (xl + 0.058, 0.89, -0.075), mat=DARK)      # palm block
for x0 in (xl - 0.048, xl + 0.013):                                            # two clamp jaws
    hand.prism([(-0.175, 0.81), (-0.135, 0.81), (-0.145, 0.72), (-0.185, 0.67), (-0.21, 0.69),
                (-0.20, 0.76)], span=(x0, x0 + 0.035), mat=BARE, bevel=0.006)
hand.prism([(-0.11, 0.81), (-0.08, 0.81), (-0.085, 0.74), (-0.12, 0.69), (-0.14, 0.705), (-0.125, 0.76)],
           span=(xl - 0.019, xl + 0.019), mat=BARE, bevel=0.006)                # thumb jaw

# right: the gun arm, pointing forward from the elbow
xr = AX
BORE = ELBOW_Y + 0.015
MUZZLE_Z = -0.96
gun = a.part("gun_arm_r", uppers["r"], pivot=(xr, ELBOW_Y, 0), bevel=0.012, smooth_angle=SMOOTH)
gun.cylinder("x", (ELBOW_Y, 0), 0.058, (xr - 0.05, xr + 0.05), mat=DARK, bevel=0.006)
gun.box((xr - 0.046, ELBOW_Y - 0.06, -0.12), (xr + 0.046, ELBOW_Y + 0.06, 0.0), mat=DARK)   # elbow block
Y0 = ELBOW_Y
gun.prism([(-0.08, Y0 - 0.095), (-0.08, Y0 + 0.08), (-0.12, Y0 + 0.115), (-0.43, Y0 + 0.115),
           (-0.50, Y0 + 0.07), (-0.50, Y0 - 0.06), (-0.37, Y0 - 0.105), (-0.12, Y0 - 0.105)],
          span=(xr - 0.088, xr + 0.088), mat=PAINT)                             # receiver housing
gun.prism([(-0.14, Y0 + 0.115), (-0.15, Y0 + 0.135), (-0.38, Y0 + 0.135), (-0.40, Y0 + 0.115)],
          span=(xr - 0.055, xr + 0.055), mat=DARK, bevel=0.006)                 # top rail
gun.prism([(-0.13, Y0 - 0.05), (-0.13, Y0 + 0.08), (-0.36, Y0 + 0.08), (-0.41, Y0 - 0.05)],
          span=(xr + 0.084, xr + 0.10), mat=ACCENT, bevel=0.005)                # outer hazard plate
DRUM = (Y0 - 0.17, -0.27)                                                       # (y, z)
gun.cylinder("x", DRUM, 0.088, (xr - 0.06, xr + 0.06), mat=DARK, bevel=0.008)   # ammo drum
gun.cylinder("x", DRUM, 0.045, (xr - 0.076, xr + 0.076), mat=BARE, bevel=0.004)  # drum hub
gun.box((xr - 0.04, DRUM[0] + 0.04, -0.34), (xr + 0.04, Y0 - 0.09, -0.19), mat=DARK, bevel=0.006)  # feed
gun.lathe([(0.072, -0.50), (0.072, -0.74), (0.05, -0.74), (0.033, -0.76), (0.033, -0.88), (0.05, -0.88),
           (0.05, MUZZLE_Z), (0.018, MUZZLE_Z), (0.018, MUZZLE_Z + 0.012), (0, MUZZLE_Z + 0.012)],
          axis="z", center=(xr, BORE),
          mats=[DARK, DARK, DARK, BARE, DARK, DARK, DARK, DARK, DARK], bevel=0.005)  # shroud, barrel, brake
for k in range(4):                                                              # shroud cooling fins
    z = -0.52 - k * 0.055
    gun.box((xr - 0.014, BORE + 0.06, z - 0.03), (xr + 0.014, BORE + 0.098, z), mat=DARK, bevel=0.004)
for y in (Y0 + 0.045, Y0 + 0.005, Y0 - 0.035):                                 # power lights, outer face
    gun.box((xr + 0.084, y - 0.011, -0.115), (xr + 0.095, y + 0.011, -0.093), mat="emissive_secondary",
            bevel=0.002)
a.socket("SOCKET_muzzle", gun, at=(xr, BORE, MUZZLE_Z), forward=(0, 0, -1))

# ---------------------------------------------------------------- legs
for side, s in SIDES:
    x = s * LX
    thigh = a.part(f"thigh_{side}", pelvis, pivot=(x, HIP_Y, 0), bevel=0.012, smooth_angle=SMOOTH)
    thigh.cylinder("x", (HIP_Y, 0), 0.085, (x - 0.066, x + 0.066), mat=DARK, bevel=0.008)  # hip knuckle
    thigh.box((x - 0.05, 0.62, -0.10), (x + 0.05, 0.95, 0.055), mat=DARK)
    thigh.prism([(-0.11, 0.925), (0.09, 0.925), (0.085, 0.74), (0.035, 0.675), (-0.12, 0.675), (-0.14, 0.81)],
                span=(x - 0.098, x + 0.098), mat=PAINT)                         # thigh armour
    clevis(thigh, x, (KNEE[1], KNEE[0]), (0.06, 0.42), 0.072, 0.13, 0.066, 0.09, 0.032)

    shin = a.part(f"shin_{side}", thigh, pivot=(x, KNEE[0], KNEE[1]), bevel=0.012, smooth_angle=SMOOTH)
    shin.cylinder("x", (KNEE[0], KNEE[1]), 0.068, (x - 0.064, x + 0.064), mat=DARK,
                  bevel=0)                                                      # knee knuckle (ends hidden)
    shin.prism([(-0.115, 0.635), (-0.16, 0.60), (-0.165, 0.505), (-0.13, 0.455), (-0.09, 0.465), (-0.09, 0.63)],
               span=(x - 0.068, x + 0.068), mat=ACCENT, bevel=0.008)            # knee cap
    shin.box((x - 0.05, 0.22, -0.065), (x + 0.05, 0.52, 0.06), mat=DARK)
    shin.prism([(-0.125, 0.49), (-0.14, 0.36), (-0.095, 0.25), (0.035, 0.25), (0.07, 0.33), (0.06, 0.49)],
               span=(x - 0.09, x + 0.09), mat=PAINT)                            # shin armour
    shin.cylinder("y", (x, 0.09), 0.034, (0.41, 0.52), mat=DARK, bevel=0.005)  # damper body
    shin.cylinder("y", (x, 0.09), 0.021, (0.27, 0.42), mat=BARE, bevel=0.003)  # damper rod
    clevis(shin, x, (ANKLE[1], ANKLE[0]), (0, 1), 0.06, 0.12, 0.058, 0.08, 0.026)

    foot = a.part(f"foot_{side}", shin, pivot=(x, ANKLE[0], ANKLE[1]), bevel=0.012, smooth_angle=SMOOTH)
    foot.cylinder("x", (ANKLE[0], ANKLE[1]), 0.056, (x - 0.056, x + 0.056), mat=DARK,
                  bevel=0.006)                                                  # ankle knuckle
    foot.box((x - 0.104, 0.0, -0.26), (x + 0.104, 0.02, 0.155), mat=BARE, bevel=0.005)  # mag plate
    foot.box((x - 0.112, 0.016, -0.265), (x + 0.112, 0.058, 0.16), mat=DARK, bevel=0.01)  # magnet block
    foot.box((x - 0.116, 0.026, -0.262), (x + 0.116, 0.036, 0.157), mat="emissive_secondary",
             bevel=0.002)                                                       # mag-boot glow seam
    foot.prism([(0.14, 0.05), (-0.17, 0.05), (-0.17, 0.125), (-0.10, 0.125), (-0.065, 0.10), (0.11, 0.10),
                (0.14, 0.08)], span=(x - 0.095, x + 0.095), mat=PAINT)          # boot upper
    foot.prism([(-0.165, 0.05), (-0.28, 0.05), (-0.29, 0.08), (-0.255, 0.14), (-0.165, 0.14)],
               span=(x - 0.102, x + 0.102), mat=ACCENT)                         # toe cap
    foot.box((x - 0.08, 0.04, 0.09), (x + 0.08, 0.12, 0.18), mat=DARK, bevel=0.01)  # heel coil housing
    a.socket(f"SOCKET_foot_{side}", foot, at=(x, 0, -0.05), forward=(0, -1, 0), up=(0, 0, -1))

    a.collider(f"COL_thigh_{side}", around=[thigh], parent=thigh)
    a.collider(f"COL_shin_{side}", around=[shin, foot], parent=shin)

# ---------------------------------------------------------------- affixes
# Variations the game shows only on a robot that has them (it hides every affix_ part
# otherwise): affix_<tag>_<what>, each on the part it rides. Their lights are
# emissive_secondary; the game tints them per affix.
LIGHT = "emissive_secondary"
BOLT = dict(mat=BARE, bevel=0)

# Armoured: thick dark slabs bolted over the chest, head, shoulders, thighs and shins. Bulkier
# and darker; the back (and its core) stays bare.
plate = a.part("affix_armour_chest", torso, pivot=WAIST, bevel=0.014)
plate.prism([(-0.26, 1.40), (-0.21, 1.355), (0.21, 1.355), (0.26, 1.40), (0.26, 1.60), (0.21, 1.648),
             (-0.21, 1.648), (-0.26, 1.60)], plane="xy", span=(-0.345, -0.284), mat=DARK)
for x in (-0.215, 0.215):
    for y in (1.40, 1.605):
        plate.cylinder("z", (x, y), 0.016, (-0.36, -0.34), segments=8, **BOLT)
helmet = a.part("affix_armour_head", head, pivot=NECK, bevel=0.012)
helmet.prism([(-0.325, 1.878), (-0.29, 1.948), (0.05, 1.948), (0.095, 1.905), (0.095, 1.878)],
             span=(-0.172, 0.172), mat=DARK)
for _, s in SIDES:
    helmet.box((s * 0.158, 1.70, -0.235), (s * 0.205, 1.88, 0.05), mat=DARK)           # cheek guards
for side, s in SIDES:
    x = s * AX
    pad = a.part(f"affix_armour_pauldron_{side}", uppers[side], pivot=(x, SHOULDER_Y, 0), bevel=0.014)
    pad.prism([(-0.195, 1.60), (-0.135, 1.775), (0.10, 1.775), (0.175, 1.67), (0.165, 1.60)],
              span=sorted((s * 0.33, s * 0.58)), mat=DARK)
    x = s * LX
    tp = a.part(f"affix_armour_thigh_{side}", f"thigh_{side}", pivot=(x, HIP_Y, 0), bevel=0.012)
    tp.box((x - 0.108, 0.70, -0.185), (x + 0.108, 0.915, -0.135), mat=DARK)
    xa, xb = sorted((x + s * 0.096, x + s * 0.128))
    tp.box((xa, 0.70, -0.135), (xb, 0.915, 0.075), mat=DARK)                          # outer side plate
    sp = a.part(f"affix_armour_shin_{side}", f"shin_{side}", pivot=(x, KNEE[0], KNEE[1]), bevel=0.012)
    sp.prism([(-0.19, 0.48), (-0.205, 0.36), (-0.165, 0.27), (-0.13, 0.27), (-0.14, 0.48)],
             span=(x - 0.095, x + 0.095), mat=DARK)

# Shielded: an emitter dish on a stalk on its right shoulder, its rim glowing.
emit = a.part("affix_shield_emitter", torso, pivot=WAIST, bevel=0.006)
EM = (0.215, -0.02)                                                                     # (x, z)
emit.cylinder("y", EM, 0.03, (1.64, 1.68), mat=DARK, bevel=0.004)
emit.cylinder("y", EM, 0.014, (1.67, 1.80), segments=8, **BOLT)
emit.lathe([(0.02, 1.79), (0.075, 1.815), (0.082, 1.83), (0.035, 1.835), (0, 1.84)], axis="y", center=EM,
           mats=[DARK, DARK, DARK, DARK], bevel=0)
emit.lathe([(0.084, 1.816), (0.084, 1.83)], axis="y", center=EM, mat=LIGHT, bevel=0)

# Overclocked: finned radiators behind the shoulders, glowing between the fins, and two
# exhaust stacks on the backpack, where it vents.
oc = a.part("affix_overclock_radiators", torso, pivot=WAIST, bevel=0.006)
for _, s in SIDES:
    x0, x1 = sorted((s * 0.305, s * 0.312))
    oc.box((x0, 1.25, 0.165), (x1, 1.51, 0.295), mat=LIGHT, bevel=0)                    # the glow behind the fins
    for k in range(6):
        y = 1.25 + k * 0.05
        xa, xb = sorted((s * 0.31, s * 0.375))
        oc.box((xa, y, 0.16), (xb, y + 0.014, 0.30), mat=BARE, bevel=0)
    xa, xb = sorted((s * 0.305, s * 0.38))
    oc.box((xa, 1.235, 0.15), (xb, 1.25, 0.31), mat=DARK, bevel=0)
stacks = a.part("affix_overclock_stacks", torso, pivot=WAIST, bevel=0.006)
for side, s in SIDES:
    c = (s * 0.12, 0.235)
    stacks.lathe([(0.04, 1.58), (0.04, 1.62), (0.033, 1.64), (0.033, 1.83), (0.042, 1.85), (0.042, 1.88)],
                 axis="y", center=c, mats=[DARK, DARK, DARK, DARK, DARK])
    stacks.cylinder("y", c, 0.03, (1.86, 1.876), mat=LIGHT, bevel=0)
    a.socket(f"SOCKET_vent_{side}", torso, at=(c[0], 1.88, c[1]), forward=(0, 1, 0), up=(0, 0, 1))

# Volatile: a bandolier of hazard-yellow charges across its belly, and a fat canister slung
# under its backpack; warning bands on each.
vol = a.part("affix_volatile_charges", torso, pivot=WAIST, bevel=0.008)
vol.box((-0.23, 1.29, -0.252), (0.23, 1.31, -0.218), mat=DARK, bevel=0)                 # strap
for x in (-0.14, 0.0, 0.14):
    vol.box((x - 0.05, 1.19, -0.31), (x + 0.05, 1.30, -0.235), mat=ACCENT)
    vol.box((x - 0.052, 1.235, -0.313), (x + 0.052, 1.252, -0.233), mat=LIGHT, bevel=0)
can = a.part("affix_volatile_canister", torso, pivot=WAIST, bevel=0.008)
can.lathe([(0.03, -0.215), (0.055, -0.20), (0.055, 0.20), (0.03, 0.215)], axis="x", center=(1.165, 0.245),
          mats=[ACCENT, ACCENT, ACCENT])
can.cylinder("x", (1.165, 0.245), 0.058, (-0.03, 0.03), mat=LIGHT, bevel=0)

# Marksman: a long barrel run out past the muzzle, and a scope on the top rail whose lens
# glows like its eye. The game's aiming laser starts at SOCKET_aim.
mb = a.part("affix_marksman_barrel", gun, pivot=(xr, ELBOW_Y, 0), bevel=0.005)
mb.lathe([(0.034, -0.94), (0.034, -1.26), (0.026, -1.27), (0.026, -1.34), (0.044, -1.34), (0.044, -1.43),
          (0.016, -1.43), (0.016, -1.42), (0, -1.42)], axis="z", center=(xr, BORE),
         mats=[DARK, DARK, BARE, DARK, DARK, DARK, DARK, DARK])
ms = a.part("affix_marksman_scope", gun, pivot=(xr, ELBOW_Y, 0), bevel=0.005)
SC = (xr, BORE + 0.205)                                                                 # scope axis (x, y)
for z in (-0.2, -0.34):
    ms.box((xr - 0.02, BORE + 0.12, z - 0.025), (xr + 0.02, BORE + 0.18, z + 0.025), mat=DARK, bevel=0)
ms.lathe([(0.03, -0.10), (0.036, -0.12), (0.036, -0.40), (0.046, -0.43), (0.046, -0.47)], axis="z", center=SC,
         mats=[DARK, DARK, DARK, DARK])
ms.cylinder("z", SC, 0.036, (-0.476, -0.466), mat="emissive", bevel=0)
a.socket("SOCKET_aim", gun, at=(xr, BORE, -1.43), forward=(0, 0, -1))

# Leader: a tall antenna mast on its back with a beacon on top and a crossbar, and a hazard-
# yellow crest on its head: the one to pick out of a crowd.
mast = a.part("affix_leader_mast", torso, pivot=WAIST, bevel=0.008)
MC = (0.0, 0.205)                                                                       # (x, z)
mast.box((-0.055, 1.585, 0.15), (0.055, 1.665, 0.26), mat=DARK)
mast.cylinder("y", MC, 0.02, (1.66, 2.20), segments=8, **BOLT)
mast.box((-0.13, 2.0, 0.19), (0.13, 2.02, 0.22), mat=DARK, bevel=0)
for x in (-0.13, 0.13):
    mast.box((x - 0.012, 1.95, 0.195), (x + 0.012, 2.03, 0.215), mat=LIGHT, bevel=0)
mast.lathe([(0.026, 2.19), (0.04, 2.21), (0.04, 2.25), (0.022, 2.275), (0, 2.28)], axis="y", center=MC,
           mats=[LIGHT, LIGHT, LIGHT, LIGHT], bevel=0)
a.socket("SOCKET_beacon", torso, at=(MC[0], 2.235, MC[1]))
crest = a.part("affix_leader_crest", head, pivot=NECK, bevel=0.006)
crest.prism([(-0.21, 1.885), (-0.14, 2.0), (0.04, 2.05), (0.085, 1.885)], span=(-0.022, 0.022), mat=ACCENT)

# Torso and pelvis hulls from their outlines (the lathes' many near-coplanar points make
# Blender's hull wobble past the verifier's convexity tolerance).
a.collider("COL_torso", parent=torso, points=[(x, y, z) for z, y in CHEST for x in (-0.335, 0.335)]
           + [(x, y, z) for x in (-0.25, 0.25) for y in (1.18, 1.33) for z in (-0.215, 0.13)]
           + [(x, y, z) for x in (-0.305, 0.305) for y in (1.24, 1.60) for z in (0.30, 0.33)]
           + [(x, y, z) for x in (-0.18, 0.18) for y in (1.24, 1.60) for z in (0.455,)])
a.collider("COL_pelvis", parent=pelvis, points=[(x, y, z) for x in (-0.17, 0.17) for y in (0.86, 1.10)
                                                for z in (-0.175, 0.16)])
a.collider("COL_head", around=[head], parent=head)
a.export()
