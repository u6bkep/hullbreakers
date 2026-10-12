"""Rifle receiver: the body every rifle is built on, its parts mounted at its sockets
(the variants are in assets/weapons/parts). Bevy space: +Y up, -Z forward, +X right.
Origin = the main hand on the pistol grip (the SMG's grip). Side outlines are plane
"zy": points are (z, y). Bore axis y = 0.10.

Long and lean where the SMG is a box: an octagonal faceted handguard with its vertical
foregrip (the off hand's place when it braces), an optic rail. Sockets: SOCKET_barrel
at the handguard's front cap, on the bore; SOCKET_mag at the top of the magwell;
SOCKET_sight on top of the rail; SOCKET_stock on the receiver's rear face."""
import hb

a = hb.Asset(__file__)
BORE = 0.10
RW = 0.023                                    # receiver half width

# --- receiver -------------------------------------------------------------------------
rec = a.part("receiver", bevel=0.003)
rec.prism([(0.065, 0.076), (-0.195, 0.076), (-0.195, 0.125),
           (0.045, 0.125), (0.065, 0.112)], span=(-RW, RW), mat="paint_primary")      # upper, bone
rec.prism([(0.063, 0.08), (0.063, 0.058), (0.055, 0.05), (-0.165, 0.05), (-0.188, 0.07),
           (-0.188, 0.08)], span=(-RW + 0.002, RW - 0.002), mat="paint_secondary")  # lower, slate
rec.box((-0.010, 0.124, -0.17), (0.010, 0.131, 0.035), mat="metal_dark", bevel=0.0015)  # optic rail
rec.frame((-0.017, 0.015, -0.152), (0.017, 0.052, -0.098), wall=0.0035, axis="y",
          mat="metal_dark")                                             # magwell
rec.box((0.021, 0.09, -0.08), (0.0255, 0.115, -0.02), mat="metal_dark", bevel=0.0015)  # ejection port, right
rec.cylinder("x", (0.108, -0.155), 0.0065, (-0.034, -0.021), mat="metal_bare")       # charging knob, left
rec.box((-0.019, 0.062, 0.058), (0.019, 0.12, 0.065), mat="metal_dark", bevel=0.0015)  # stock mount plate
for s in (-1, 1):                                                       # status strips, both sides
    rec.box((s * (RW - 0.001), 0.060, -0.015), (s * (RW + 0.0015), 0.065, 0.045),
            mat="emissive_secondary", bevel=0.0005)
a.socket("SOCKET_eject", rec, at=(0.0255, 0.1025, -0.05), forward=(1, 0, 0))

# --- grip and trigger (the SMG's: same hand, same glove) --------------------------------
grip = a.part("grip", rec, bevel=0.004)
grip.prism([(-0.036, 0.056), (-0.018, -0.068), (-0.008, -0.082), (0.034, -0.082), (0.042, -0.072),
            (0.022, 0.036), (0.040, 0.044), (0.046, 0.052), (0.046, 0.068), (-0.036, 0.068)],
           span=(-0.015, 0.015), mat="polymer")

guard = a.part("trigger_guard", rec, bevel=0.002)
guard.prism([(-0.078, 0.054), (-0.078, 0.012), (-0.068, 0.0), (-0.024, 0.0), (-0.024, 0.007),
             (-0.064, 0.007), (-0.071, 0.015), (-0.071, 0.054)], span=(-0.007, 0.007), mat="paint_secondary")
guard.prism([(-0.040, 0.052), (-0.047, 0.052), (-0.050, 0.032), (-0.046, 0.018), (-0.041, 0.020),
             (-0.043, 0.034)], span=(-0.003, 0.003), mat="metal_bare")  # trigger blade

# --- handguard: octagonal, faceted -------------------------------------------------------
def octagon(z, hw, ylo, yhi, c):
    return [(sx * x, y, z) for sx in (-1, 1) for x, y in
            ((hw, ylo + c), (hw, yhi - c), (hw - c, ylo), (hw - c, yhi))]

HG = (0, BORE, -0.195)
hg = a.part("handguard", rec, pivot=HG, bevel=0.003)
hg.hull(octagon(-0.195, 0.0215, 0.068, 0.127, 0.012) + octagon(-0.207, 0.0215, 0.068, 0.127, 0.012),
        mat="metal_dark")                                               # barrel-nut collar
hg.hull(octagon(-0.205, 0.021, 0.070, 0.125, 0.011) + octagon(-0.455, 0.019, 0.075, 0.122, 0.010),
        mat="paint_primary")                                            # tapers toward the muzzle
hg.hull(octagon(-0.45, 0.0165, 0.078, 0.119, 0.009) + octagon(-0.47, 0.0165, 0.078, 0.119, 0.009),
        mat="metal_dark")                                               # front cap
for s in (-1, 1):
    hg.box((s * 0.019, 0.104, -0.36), (s * 0.0222, 0.108, -0.22), mat="emissive_secondary",
           bevel=0.0005)                                                # status strip
    for z in (-0.39, -0.41, -0.43):                                     # cooling slots
        hg.box((s * 0.0185, 0.088, z - 0.011), (s * 0.0205, 0.099, z), mat="metal_dark", bevel=0.0005)
a.socket("SOCKET_barrel", hg, at=(0, BORE, -0.47))

fg = a.part("foregrip", hg, pivot=(0, 0.07, -0.30), bevel=0.004)
fg.box((-0.012, 0.062, -0.326), (0.012, 0.074, -0.274), mat="metal_dark", bevel=0.002)  # clamp
fg.prism([(-0.318, 0.064), (-0.284, 0.064), (-0.279, -0.028), (-0.286, -0.036), (-0.306, -0.036),
          (-0.313, -0.028)], span=(-0.0145, 0.0145), mat="polymer")
a.socket("SOCKET_foregrip", fg, at=(0, 0.02, -0.298))

a.socket("SOCKET_sight", rec, at=(0, 0.131, -0.07))
a.socket("SOCKET_mag", rec, at=(0, 0.05, -0.125))
a.socket("SOCKET_stock", rec, at=(0, 0.09, 0.065))
a.socket("SOCKET_grip", at=(0, 0, 0))
a.collider("COL_body", around=[rec])
a.collider("COL_barrel", around=[hg])
a.collider("COL_grip", around=[grip, guard])
a.collider("COL_foregrip", around=[fg])
a.export()
