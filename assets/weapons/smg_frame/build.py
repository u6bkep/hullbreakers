"""SMG frame (the pipeline bake-off gun). Bevy space: +Y up, -Z forward, +X right.
Origin = the hand on the grip. Side outlines are plane "zy": points are (z, y)."""
import hb

a = hb.Asset(__file__)
RW = 0.022                                    # receiver half width

rec = a.part("receiver", bevel=0.003)
rec.prism([(0.12, 0.065), (0.105, 0.05), (-0.13, 0.05), (-0.16, 0.075), (-0.16, 0.122),
           (-0.13, 0.13), (0.10, 0.13), (0.12, 0.112)], span=(-RW, RW), mat="paint_primary")
rec.prism([(0.128, 0.07), (0.115, 0.07), (0.115, 0.108), (0.128, 0.104)],
          span=(-0.016, 0.016), mat="metal_dark")                       # rear buffer cap
rec.frame((-0.017, 0.022, -0.132), (0.017, 0.052, -0.078), wall=0.0035, axis="y",
          mat="metal_dark")                                             # magwell
rec.box((0.020, 0.09, -0.075), (0.025, 0.118, -0.02), mat="metal_dark")  # ejection port, right side
for s in (-1, 1):                                                       # status strips
    rec.box((s * (RW - 0.001), 0.064, 0.01), (s * (RW + 0.0015), 0.07, 0.09), mat="emissive", bevel=0.0005)
a.socket("SOCKET_eject", rec, at=(0.025, 0.104, -0.0475), forward=(1, 0, 0))

grip = a.part("grip", rec, bevel=0.004)
grip.prism([(-0.036, 0.056), (-0.018, -0.068), (-0.008, -0.082), (0.034, -0.082), (0.042, -0.072),
            (0.022, 0.036), (0.040, 0.044), (0.046, 0.052), (0.046, 0.068), (-0.036, 0.068)],
           span=(-0.015, 0.015), mat="polymer")

guard = a.part("trigger_guard", rec, bevel=0.002)
guard.prism([(-0.078, 0.054), (-0.078, 0.012), (-0.068, 0.0), (-0.024, 0.0), (-0.024, 0.007),
             (-0.064, 0.007), (-0.071, 0.015), (-0.071, 0.054)], span=(-0.007, 0.007), mat="paint_primary")
guard.prism([(-0.040, 0.052), (-0.047, 0.052), (-0.050, 0.032), (-0.046, 0.018), (-0.041, 0.020),
             (-0.043, 0.034)], span=(-0.003, 0.003), mat="metal_bare")  # trigger blade

rail = a.part("sight_rail", rec, pivot=(0, 0.13, 0), bevel=0.0015)
rail.box((-0.009, 0.128, -0.10), (0.009, 0.136, 0.085), mat="metal_dark")
for k in range(6):
    z = 0.075 - k * 0.03
    rail.box((-0.011, 0.134, z - 0.014), (0.011, 0.141, z), mat="metal_dark")
a.socket("SOCKET_sight", rail, at=(0, 0.141, -0.005))

BARREL = (0, 0.10, -0.16)                    # receiver front face, on the bore axis
a.socket("SOCKET_barrel", rec, at=BARREL)
barrel = a.part("barrel_shroud", "SOCKET_barrel", pivot=BARREL, bevel=0.002)
barrel.lathe([(0, -0.156), (0.022, -0.156), (0.022, -0.174), (0.019, -0.174), (0.019, -0.278),
              (0.015, -0.278), (0.015, -0.30), (0.0062, -0.30), (0.0062, -0.28), (0, -0.28)],
             axis="z", center=(0, 0.10),
             mats=["metal_bare"] * 3 + ["metal_dark"] * 2 + ["metal_bare"] * 2 + ["metal_dark"] * 2)
for x0 in (-0.0075, 0.0035):                                            # heat-sink fins
    barrel.box((x0, 0.112, -0.264), (x0 + 0.004, 0.126, -0.184), mat="metal_dark")
a.socket("SOCKET_muzzle", barrel, at=(0, 0.10, -0.30))

MAG = (0, 0.05, -0.105)                      # top of the magwell
a.socket("SOCKET_mag", rec, at=MAG)
mag = a.part("magazine", "SOCKET_mag", pivot=MAG, bevel=0.0025)
mag.prism([(-0.0855, 0.058), (-0.1245, 0.058), (-0.131, -0.07), (-0.093, -0.07)],
          span=(-0.0125, 0.0125), mat="metal_dark")
mag.box((-0.0155, -0.082, -0.136), (0.0155, -0.068, -0.089), mat="metal_bare")       # base plate
mag.box((0.012, -0.05, -0.113), (0.0135, 0.02, -0.101), mat="emissive", bevel=0.0005)  # ammo window

a.socket("SOCKET_grip", at=(0, 0, 0))
a.collider("COL_body", around=[rec, barrel, rail])
a.collider("COL_grip", around=[grip, guard])
a.collider("COL_mag", around=[mag], parent=mag)
a.export()
