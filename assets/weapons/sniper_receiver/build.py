"""Sniper receiver: the body every precision rifle is built on, its parts mounted at its
sockets (the variants are in assets/weapons/parts). Bevy space: +Y up, -Z forward, +X
right. Origin = the main hand on the pistol grip (the SMG's grip). Side outlines are
plane "zy": points are (z, y). Bore axis y = 0.10.

Long and slim: a round action bedded in a chassis, the bolt handle out on the right, a
high rail on risers so a big scope's bell clears the handguard, a long free-float
handguard with a short foregrip under it (the off hand's place when it braces).
Sockets: SOCKET_barrel at the handguard's front cap, on the bore; SOCKET_mag at the top
of the magwell; SOCKET_sight on top of the rail; SOCKET_stock on the chassis's rear
face."""
import hb

a = hb.Asset(__file__)
BORE = 0.10
RW = 0.021                                    # chassis half width
ACT = 0.104                                   # the action's axis (y)

# --- receiver: chassis, action, rail ----------------------------------------------------
rec = a.part("receiver", bevel=0.003)
rec.prism([(0.068, 0.098), (0.068, 0.062), (0.058, 0.05), (-0.175, 0.05), (-0.205, 0.072),
           (-0.205, 0.098)], span=(-RW, RW), mat="paint_secondary")       # chassis, graphite
rec.lathe([(0.0195, 0.045), (0.0195, -0.198), (0.0165, -0.205)], axis="z", center=(0, ACT),
          mats=["paint_primary", "paint_primary"])                        # action, sand
rec.box((0.015, 0.097, -0.095), (0.0215, 0.117, -0.03), mat="metal_dark", bevel=0.0015)  # ejection port, right
rec.box((-0.019, 0.058, 0.066), (0.019, 0.12, 0.073), mat="metal_dark", bevel=0.0015)    # stock mount plate
rec.frame((-0.0165, 0.02, -0.162), (0.0165, 0.052, -0.098), wall=0.0035, axis="y",
          mat="metal_dark")                                             # magwell
for z in (0.035, -0.085, -0.225):                                       # rail risers
    rec.box((-0.008, 0.116, z - 0.022), (0.008, 0.141, z), mat="metal_dark", bevel=0.0015)
rec.box((-0.0105, 0.14, -0.275), (0.0105, 0.148, 0.045), mat="metal_dark", bevel=0.0015)  # the high rail
for s in (-1, 1):                                                       # status strips, both sides
    rec.box((s * (RW - 0.001), 0.062, -0.01), (s * (RW + 0.0015), 0.067, 0.05), mat="emissive",
            bevel=0.0005)
a.socket("SOCKET_eject", rec, at=(0.0215, 0.107, -0.0625), forward=(1, 0, 0))

# --- the bolt: its shroud at the back of the action, the handle out to the right ----------
bolt = a.part("bolt", rec, pivot=(0, ACT, 0.025), bevel=0.002)
bolt.lathe([(0.0165, 0.066), (0.0165, 0.052), (0.019, 0.046), (0.019, 0.04)], axis="z",
           center=(0, ACT), mats=["metal_dark"] * 3)                    # shroud
A, B = (0.012, ACT, 0.025), (0.046, 0.078, 0.033)                        # the handle's stem
bolt.hull([(x, y + dy, z + dz) for (x, y, z) in (A, B) for dy in (-0.0045, 0.0045) for dz in (-0.0045, 0.0045)],
          mat="metal_dark")
bolt.lathe([(0, 0.040), (0.007, 0.0405), (0.0115, 0.046), (0.0115, 0.058), (0.008, 0.063), (0, 0.0635)],
           axis="x", center=(0.076, 0.034), mat="metal_bare")           # the knob

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


# --- handguard: a long octagonal free-float tube --------------------------------------------
def octagon(z, hw, ylo, yhi, c):
    return [(sx * x, y, z) for sx in (-1, 1) for x, y in
            ((hw, ylo + c), (hw, yhi - c), (hw - c, ylo), (hw - c, yhi))]


hg = a.part("handguard", rec, pivot=(0, BORE, -0.205), bevel=0.003)
hg.hull(octagon(-0.202, 0.0225, 0.07, 0.13, 0.012) + octagon(-0.216, 0.0225, 0.07, 0.13, 0.012),
        mat="metal_dark")                                               # barrel-nut collar
hg.hull(octagon(-0.214, 0.019, 0.076, 0.124, 0.010) + octagon(-0.612, 0.019, 0.076, 0.124, 0.010),
        mat="paint_primary")
hg.hull(octagon(-0.61, 0.0165, 0.079, 0.121, 0.008) + octagon(-0.625, 0.0165, 0.079, 0.121, 0.008),
        mat="metal_dark")                                               # front cap
hg.box((-0.008, 0.069, -0.445), (0.008, 0.0775, -0.33), mat="metal_dark", bevel=0.0015)  # rail under it
for s in (-1, 1):
    for z0, z1 in ((-0.245, -0.36), (-0.455, -0.585)):                  # long cooling slots
        hg.box((s * 0.0172, 0.089, z1), (s * 0.0196, 0.101, z0), mat="metal_dark", bevel=0.0005)
    hg.box((s * 0.0182, 0.109, -0.585), (s * 0.0198, 0.113, -0.245), mat="emissive",
           bevel=0.0005)                                                # status strip
a.socket("SOCKET_barrel", hg, at=(0, BORE, -0.625))

fg = a.part("foregrip", hg, pivot=(0, 0.07, -0.40), bevel=0.004)
fg.box((-0.011, 0.062, -0.425), (0.011, 0.072, -0.375), mat="metal_dark", bevel=0.002)  # clamp
fg.prism([(-0.420, 0.064), (-0.382, 0.064), (-0.378, -0.018), (-0.385, -0.026), (-0.407, -0.026),
          (-0.414, -0.018)], span=(-0.0145, 0.0145), mat="polymer")
a.socket("SOCKET_foregrip", fg, at=(0, 0.03, -0.40))

a.socket("SOCKET_sight", rec, at=(0, 0.148, -0.115))
a.socket("SOCKET_mag", rec, at=(0, 0.05, -0.13))
a.socket("SOCKET_stock", rec, at=(0, 0.09, 0.073))
a.socket("SOCKET_grip", at=(0, 0, 0))
a.collider("COL_body", around=[rec, bolt])
a.collider("COL_barrel", around=[hg])
a.collider("COL_grip", around=[grip, guard])
a.collider("COL_foregrip", around=[fg])
a.export()
