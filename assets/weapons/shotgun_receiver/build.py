"""Shotgun receiver: the body every combat shotgun is built on, its parts mounted at its
sockets (the variants are in assets/weapons/parts). Bevy space: +Y up, -Z forward, +X
right. Origin = the main hand on the pistol grip (the SMG's grip). Side outlines are
plane "zy": points are (z, y). Bore axis y = 0.10.

Short, wide and chunky where the rifle is lean: a tall receiver with a shell saddle on
its left, a vent-ribbed heat shield over the bore, and under it a tube with the pump
slide on it (the off hand's place: SOCKET_foregrip at the pump's centre). Sockets:
SOCKET_barrel at the heat shield's front cap, on the bore; SOCKET_mag at the top of the
magwell; SOCKET_sight on top of the rail; SOCKET_stock on the receiver's rear face."""
import hb

a = hb.Asset(__file__)
BORE = 0.10
RW = 0.031                                    # receiver half width
TUBE = 0.056                                  # the pump tube's axis (y)

# --- receiver -------------------------------------------------------------------------
rec = a.part("receiver", bevel=0.003)
rec.prism([(0.075, 0.085), (-0.205, 0.085), (-0.205, 0.148), (0.05, 0.148),
           (0.075, 0.13)], span=(-RW, RW), mat="paint_primary")                     # upper, olive
rec.prism([(0.072, 0.09), (0.072, 0.06), (0.06, 0.045), (-0.17, 0.045), (-0.198, 0.07),
           (-0.198, 0.09)], span=(-RW + 0.003, RW - 0.003), mat="paint_secondary")  # lower, near-black
rec.box((-0.011, 0.147, -0.19), (0.011, 0.155, 0.045), mat="metal_dark", bevel=0.0015)  # optic rail
for z in (0.02, -0.04, -0.10, -0.16):                                   # its cross slots
    rec.box((-0.0115, 0.151, z - 0.006), (0.0115, 0.1555, z), mat="metal_dark", bevel=0)
rec.frame((-0.026, 0.012, -0.175), (0.026, 0.048, -0.105), wall=0.004, axis="y",
          mat="metal_dark")                                             # magwell
rec.box((RW - 0.001, 0.094, -0.085), (RW + 0.004, 0.128, -0.02), mat="metal_dark",
        bevel=0.0015)                                                   # ejection port, right
rec.box((-0.024, 0.06, 0.074), (0.024, 0.128, 0.08), mat="metal_dark", bevel=0.0015)  # stock mount plate
for s in (-1, 1):                                                       # status strips, both sides
    rec.box((s * (RW - 0.004), 0.062, -0.02), (s * (RW - 0.0015), 0.068, 0.05),
            mat="emissive", bevel=0.0005)
# Shell saddle on the left: a plate holding four spare shells, brass down.
rec.box((-RW - 0.006, 0.086, -0.146), (-RW + 0.001, 0.142, -0.032), mat="paint_secondary",
        bevel=0.002)
for z in (-0.054, -0.077, -0.100, -0.123):
    rec.cylinder("y", (-RW - 0.0155, z), 0.0095, (0.091, 0.14), mat="polymer", bevel=0.0015)
    rec.cylinder("y", (-RW - 0.0155, z), 0.0099, (0.081, 0.091), mat="metal_bare", bevel=0.001)
a.socket("SOCKET_eject", rec, at=(RW + 0.004, 0.111, -0.0525), forward=(1, 0, 0))

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


# --- heat shield over the bore, the pump tube under it -----------------------------------
def octagon(z, hw, ylo, yhi, c):
    return [(sx * x, y, z) for sx in (-1, 1) for x, y in
            ((hw, ylo + c), (hw, yhi - c), (hw - c, ylo), (hw - c, yhi))]


hg = a.part("handguard", rec, pivot=(0, BORE, -0.205), bevel=0.003)
hg.hull(octagon(-0.203, 0.026, 0.074, 0.134, 0.012) + octagon(-0.217, 0.026, 0.074, 0.134, 0.012),
        mat="metal_dark")                                               # barrel collar
hg.hull(octagon(-0.215, 0.023, 0.083, 0.128, 0.011) + octagon(-0.395, 0.022, 0.084, 0.126, 0.010),
        mat="paint_primary")                                            # heat shield
hg.box((-0.005, 0.126, -0.385), (0.005, 0.133, -0.225), mat="metal_dark", bevel=0.0015)  # vent rib
for s in (-1, 1):
    for k in range(5):                                                  # vent ports
        z = -0.245 - k * 0.028
        hg.box((s * 0.0205, 0.095, z - 0.016), (s * 0.0232, 0.116, z), mat="metal_dark", bevel=0.0005)
    hg.box((s * 0.0205, 0.088, -0.385), (s * 0.0236, 0.093, -0.235), mat="emissive",
           bevel=0.0005)                                                # status strip
hg.cylinder("z", (0, TUBE), 0.0135, (-0.198, -0.395), mat="metal_dark")  # pump tube
hg.prism([(-0.39, 0.128), (-0.39, 0.048), (-0.398, 0.04), (-0.405, 0.04), (-0.405, 0.124),
          (-0.40, 0.128)], span=(-0.021, 0.021), mat="metal_dark")      # front cap: shield to tube
a.socket("SOCKET_barrel", hg, at=(0, BORE, -0.405))

# --- the pump: a ribbed slide on the tube --------------------------------------------------
PZ = (-0.238, -0.352)
pump = a.part("pump", hg, pivot=(0, TUBE, -0.295), bevel=0.003)
pump.hull(octagon(PZ[0] - 0.006, 0.027, 0.031, 0.08, 0.011) + octagon(PZ[1] + 0.006, 0.027, 0.031, 0.08, 0.011),
          mat="polymer")
for z0 in (PZ[0], PZ[1] + 0.008):                                       # the lips the hand sits between
    pump.hull(octagon(z0, 0.031, 0.026, 0.08, 0.013) + octagon(z0 - 0.008, 0.031, 0.026, 0.08, 0.013),
              mat="paint_secondary")
for k in range(5):                                                      # grip ribs
    z = -0.258 - k * 0.017
    pump.hull(octagon(z, 0.0295, 0.0285, 0.078, 0.012) + octagon(z - 0.006, 0.0295, 0.0285, 0.078, 0.012),
              mat="rubber", bevel=0)
a.socket("SOCKET_foregrip", pump, at=(0, TUBE, -0.295))

a.socket("SOCKET_sight", rec, at=(0, 0.155, -0.07))
a.socket("SOCKET_mag", rec, at=(0, 0.046, -0.14))
a.socket("SOCKET_stock", rec, at=(0, 0.09, 0.08))
a.socket("SOCKET_grip", at=(0, 0, 0))
a.collider("COL_body", around=[rec])
a.collider("COL_barrel", around=[hg])
a.collider("COL_grip", around=[grip, guard])
a.collider("COL_pump", around=[pump])
a.export()
