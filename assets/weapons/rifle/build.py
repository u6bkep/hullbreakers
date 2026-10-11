"""Crew rifle: the precision gun beside the SMG. Bevy space: +Y up, -Z forward, +X right.
Origin = the main hand on the pistol grip (same grip as the SMG). Side outlines are
plane "zy": points are (z, y). Bore axis y = 0.10.

Shape language: long and lean where the SMG is a box. An octagonal faceted handguard,
an open skeletal stock, a box-section optic that is a real open sleeve (the zoom camera
sits at its rear pane and looks down it)."""
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

# --- skeletal stock ----------------------------------------------------------------------
STOCK = (0, 0.09, 0.065)                     # receiver rear face: where a fold hinge would be
stock = a.part("stock", rec, pivot=STOCK, bevel=0.003)
stock.box((-0.018, 0.054, 0.058), (0.018, 0.124, 0.085), mat="metal_dark")              # hinge block
stock.prism([(0.08, 0.124), (0.19, 0.124), (0.19, 0.103), (0.08, 0.108)],
            span=(-0.014, 0.014), mat="paint_secondary")                                 # cheek strut
stock.prism([(0.08, 0.058), (0.08, 0.078), (0.19, 0.040), (0.19, 0.020)],
            span=(-0.009, 0.009), mat="paint_secondary")                                 # lower strut
stock.prism([(0.185, 0.012), (0.185, 0.128), (0.206, 0.128), (0.206, 0.012)],
            span=(-0.016, 0.016), mat="paint_secondary")                                 # butt frame
stock.box((-0.017, 0.008, 0.205), (0.017, 0.132, 0.215), mat="rubber", bevel=0.002)      # butt pad
a.socket("SOCKET_stock", stock, at=(0, 0.07, 0.215))

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

fg = a.part("foregrip", hg, pivot=(0, 0.07, -0.30), bevel=0.004)
fg.box((-0.012, 0.062, -0.326), (0.012, 0.074, -0.274), mat="metal_dark", bevel=0.002)  # clamp
fg.prism([(-0.318, 0.064), (-0.284, 0.064), (-0.279, -0.028), (-0.286, -0.036), (-0.306, -0.036),
          (-0.313, -0.028)], span=(-0.0145, 0.0145), mat="polymer")
a.socket("SOCKET_foregrip", fg, at=(0, 0.02, -0.298))

# --- barrel and muzzle brake -------------------------------------------------------------
barrel = a.part("barrel", hg, pivot=(0, BORE, -0.46), bevel=0.002)
barrel.lathe([(0.0105, -0.44), (0.0105, -0.575)], axis="z", center=(0, BORE), mats=["metal_dark"])
barrel.lathe([(0.015, -0.572), (0.015, -0.625), (0.0065, -0.625), (0.0065, -0.613), (0, -0.613)],
             axis="z", center=(0, BORE), segments=6,
             mats=["metal_bare", "metal_bare", "metal_dark", "metal_dark"])  # hex brake
a.socket("SOCKET_muzzle", barrel, at=(0, BORE, -0.625))

# --- optic: an open box sleeve, eye axis y = 0.16 ----------------------------------------
EYE = 0.16
opt = a.part("optic", rec, pivot=(0, 0.131, -0.07), bevel=0.002)
for z0 in (-0.045, -0.11):                                               # mounts onto the rail
    opt.box((-0.011, 0.128, z0), (0.011, 0.139, z0 + 0.015), mat="metal_dark", bevel=0.0015)
opt.frame((-0.019, 0.137, -0.012), (0.019, 0.183, 0.020), wall=0.005, axis="z",
          mat="metal_dark", bevel=0.0015)                                # ocular housing
opt.frame((-0.015, 0.141, -0.125), (0.015, 0.179, -0.012), wall=0.004, axis="z",
          mat="paint_secondary", bevel=0.0015)                           # body
opt.frame((-0.021, 0.135, -0.172), (0.021, 0.185, -0.125), wall=0.0045, axis="z",
          mat="metal_dark", bevel=0.0015)                                # objective hood
opt.box((-0.0138, 0.1422, 0.012), (0.0138, 0.1778, 0.014), mat="glass", bevel=0)       # rear pane
opt.box((-0.0163, 0.1397, -0.150), (0.0163, 0.1803, -0.148), mat="glass", bevel=0)     # front pane
opt.box((-0.007, 0.178, -0.085), (0.007, 0.187, -0.055), mat="metal_dark", bevel=0.0015)  # top turret
opt.cylinder("x", (EYE, -0.07), 0.008, (0.014, 0.0235), mat="metal_dark")              # side turret
opt.box((-0.0205, 0.170, 0.002), (-0.0175, 0.176, 0.012), mat="emissive", bevel=0)      # power light, left
a.socket("SOCKET_sight", opt, at=(0, EYE, 0.015))

# --- magazine: raked, curving forward ----------------------------------------------------
MAG = (0, 0.05, -0.125)                      # top of the magwell
a.socket("SOCKET_mag", rec, at=MAG)
mag = a.part("magazine", "SOCKET_mag", pivot=MAG, bevel=0.0025)
mag.prism([(-0.104, 0.058), (-0.106, -0.01), (-0.122, -0.072), (-0.168, -0.072), (-0.148, -0.01),
           (-0.146, 0.058)], span=(-0.012, 0.012), mat="polymer")
mag.box((-0.0145, -0.082, -0.171), (0.0145, -0.070, -0.119), mat="metal_bare")        # base plate
for s in (-1, 1):                                                       # round-count windows
    mag.box((s * 0.0115, -0.004, -0.131), (s * 0.0133, 0.036, -0.121), mat="emissive", bevel=0.0005)

a.socket("SOCKET_grip", at=(0, 0, 0))
a.collider("COL_body", around=[rec, opt])
a.collider("COL_barrel", around=[hg, barrel])
a.collider("COL_grip", around=[grip, guard])
a.collider("COL_foregrip", around=[fg])
a.collider("COL_stock", around=[stock])
a.collider("COL_mag", around=[mag], parent=mag)
a.export()
