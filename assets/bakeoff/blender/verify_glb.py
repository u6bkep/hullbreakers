#!/usr/bin/env python3
"""Independent check of smg.glb: pure-stdlib GLB parser (no Blender, no pygltflib).

Usage: python3 verify_glb.py [smg.glb]
Exits non-zero on any failed check. Prints node tree, materials, tri counts, bounds.
"""
import json
import math
import struct
import sys

path = sys.argv[1] if len(sys.argv) > 1 else "smg.glb"
data = open(path, "rb").read()
magic, version, length = struct.unpack_from("<III", data, 0)
assert magic == 0x46546C67 and version == 2 and length == len(data), "bad GLB header"
jlen, jtype = struct.unpack_from("<II", data, 12)
assert jtype == 0x4E4F534A
gl = json.loads(data[20:20 + jlen])
off = 20 + jlen
blen, btype = struct.unpack_from("<II", data, off)
assert btype == 0x004E4942
BIN = data[off + 8: off + 8 + blen]

fails = []


def check(cond, msg):
    print(("  ok   " if cond else "  FAIL ") + msg)
    if not cond:
        fails.append(msg)


assert all("uri" not in b for b in gl["buffers"]), "external buffer"
check(all("uri" not in b for b in gl["buffers"]), "single embedded buffer, no external URIs")
check(not gl.get("images") and not gl.get("textures"), "no textures/images")

COMP = {5120: "b", 5121: "B", 5122: "h", 5123: "H", 5125: "I", 5126: "f"}
NCOMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}


def read_acc(i):
    a = gl["accessors"][i]
    bv = gl["bufferViews"][a["bufferView"]]
    n, c = a["count"], NCOMP[a["type"]]
    fmt = COMP[a["componentType"]]
    size = struct.calcsize(fmt)
    stride = bv.get("byteStride", size * c)
    base = bv.get("byteOffset", 0) + a.get("byteOffset", 0)
    return [struct.unpack_from("<" + fmt * c, BIN, base + k * stride) for k in range(n)]


# ---- matrices -------------------------------------------------------------
def mat_mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(4)) for j in range(4)] for i in range(4)]


def local_matrix(n):
    if "matrix" in n:
        m = n["matrix"]
        return [[m[c * 4 + r] for c in range(4)] for r in range(4)]
    t = n.get("translation", [0, 0, 0])
    x, y, z, w = n.get("rotation", [0, 0, 0, 1])
    s = n.get("scale", [1, 1, 1])
    r = [[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
         [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
         [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]]
    return [[r[i][0] * s[0], r[i][1] * s[1], r[i][2] * s[2], t[i]] for i in range(3)] + [[0, 0, 0, 1]]


def xf(m, p):
    return tuple(m[i][0] * p[0] + m[i][1] * p[1] + m[i][2] * p[2] + m[i][3] for i in range(3))


nodes = gl["nodes"]
name_of = [n.get("name", f"<{i}>") for i, n in enumerate(nodes)]
idx = {nm: i for i, nm in enumerate(name_of)}
parent = {}
for i, n in enumerate(nodes):
    for c in n.get("children", []):
        parent[c] = i
world = {}


def walk(i, pm, depth):
    m = mat_mul(pm, local_matrix(nodes[i]))
    world[i] = m
    n = nodes[i]
    kind = f"mesh '{gl['meshes'][n['mesh']]['name']}'" if "mesh" in n else "empty"
    extra = ""
    if "rotation" in n or "scale" in n:
        extra = f" rot={n.get('rotation')} scale={n.get('scale')}"
    print("  " + "  " * depth + f"{name_of[i]}  [{kind}]  world_pos=({m[0][3]:+.3f},{m[1][3]:+.3f},{m[2][3]:+.3f}){extra}")
    for c in n.get("children", []):
        walk(c, m, depth + 1)


I4 = [[1 if i == j else 0 for j in range(4)] for i in range(4)]
print("\n== hierarchy (glTF space: +Y up, -Z forward) ==")
for r in gl["scenes"][gl.get("scene", 0)]["nodes"]:
    walk(r, I4, 0)

print("\n== required nodes ==")
parts = ["receiver", "grip", "barrel_shroud", "magazine", "sight_rail", "trigger_guard"]
sockets = ["SOCKET_grip", "SOCKET_muzzle", "SOCKET_mag", "SOCKET_sight", "SOCKET_barrel"]
for p in parts:
    check(p in idx and "mesh" in nodes[idx[p]], f"part node '{p}' exists with a mesh")
for s in sockets:
    check(s in idx and "mesh" not in nodes[idx[s]], f"socket '{s}' exists, no mesh")
check("COL_body" in idx and "mesh" in nodes[idx["COL_body"]], "COL_body exists with a mesh")
check(len(set(name_of)) == len(name_of), "node names unique")
check(parent.get(idx["magazine"]) is not None and "mesh" not in nodes[parent[idx["magazine"]]],
      "magazine is its own node (parent is an empty, so it can animate independently)")

g = world[idx["SOCKET_grip"]]
check(all(abs(g[i][3]) < 1e-6 for i in range(3)), "SOCKET_grip at scene origin")

print("\n== materials ==")
want = {
    "paint_primary": dict(rough=0.55),
    "metal_dark": dict(metal=1.0, rough=0.4),
    "metal_bare": dict(metal=1.0),
    "polymer": dict(metal=0.0, rough_min=0.7),
    "emissive_accent": dict(emissive=True),
}
mats = {m["name"]: m for m in gl.get("materials", [])}
for nm, m in mats.items():
    pbr = m.get("pbrMetallicRoughness", {})
    print(f"  {nm}: base={[round(v, 3) for v in pbr.get('baseColorFactor', [1, 1, 1, 1])]}"
          f" metallic={pbr.get('metallicFactor', 1.0)} roughness={pbr.get('roughnessFactor', 1.0)}"
          f" emissive={m.get('emissiveFactor')} ext={m.get('extensions')}")
for nm, w in want.items():
    m = mats.get(nm)
    check(m is not None, f"material '{nm}' present")
    if not m:
        continue
    pbr = m.get("pbrMetallicRoughness", {})
    mf, rf = pbr.get("metallicFactor", 1.0), pbr.get("roughnessFactor", 1.0)
    if "metal" in w:
        check(abs(mf - w["metal"]) < 1e-3, f"{nm} metallic == {w['metal']} (got {mf})")
    if "rough" in w:
        check(abs(rf - w["rough"]) < 0.051, f"{nm} roughness ~{w['rough']} (got {rf:.3f})")
    if "rough_min" in w:
        check(rf >= w["rough_min"], f"{nm} rough (got {rf:.3f})")
    if w.get("emissive"):
        ef = m.get("emissiveFactor", [0, 0, 0])
        st = m.get("extensions", {}).get("KHR_materials_emissive_strength", {}).get("emissiveStrength", 1.0)
        check(max(ef) > 0 and st > 1.0, f"{nm} emissive {ef} x strength {st} (KHR_materials_emissive_strength)")
check(set(mats) <= set(want) | {"collider"}, f"no stray materials (have {sorted(mats)})")
if "KHR_materials_emissive_strength" in mats.get("emissive_accent", {}).get("extensions", {}):
    check("KHR_materials_emissive_strength" in gl.get("extensionsUsed", []), "extension declared in extensionsUsed")

print("\n== meshes / triangles / bounds ==")
total = 0
lo, hi = [1e9] * 3, [-1e9] * 3
render_lo, render_hi = [1e9] * 3, [-1e9] * 3
bad_normals = 0
for i, n in enumerate(nodes):
    if "mesh" not in n:
        continue
    mesh = gl["meshes"][n["mesh"]]
    tris = 0
    used = []
    for prim in mesh["primitives"]:
        mode = prim.get("mode", 4)
        assert mode == 4, "non-triangle primitive"
        cnt = gl["accessors"][prim["indices"]]["count"] if "indices" in prim else \
            gl["accessors"][prim["attributes"]["POSITION"]]["count"]
        tris += cnt // 3
        used.append(gl["materials"][prim["material"]]["name"] if "material" in prim else None)
        check("NORMAL" in prim["attributes"], f"{name_of[i]}: primitive has NORMALs") if name_of[i] != "COL_body" else None
        if "NORMAL" in prim["attributes"]:
            for nv in read_acc(prim["attributes"]["NORMAL"]):
                if abs(math.sqrt(sum(c * c for c in nv)) - 1) > 1e-3:
                    bad_normals += 1
        for p in read_acc(prim["attributes"]["POSITION"]):
            wp = xf(world[i], p)
            for k in range(3):
                lo[k], hi[k] = min(lo[k], wp[k]), max(hi[k], wp[k])
                if name_of[i] != "COL_body":
                    render_lo[k], render_hi[k] = min(render_lo[k], wp[k]), max(render_hi[k], wp[k])
    if name_of[i] != "COL_body":
        total += tris
    print(f"  {name_of[i]:15s} {tris:5d} tris  materials={used}")
print(f"  TOTAL (excluding COL_body): {total} triangles")
check(2000 <= total <= 6000, f"triangle budget 2k-6k (got {total})")
check(bad_normals == 0, f"all normals unit length ({bad_normals} bad)")
col = gl["meshes"][nodes[idx["COL_body"]]["mesh"]]
check(all(p.get("material") is None or gl["materials"][p["material"]]["name"] == "collider"
          for p in col["primitives"]), "COL_body has no material (or 'collider')")

size = [render_hi[k] - render_lo[k] for k in range(3)]
print(f"  render bounds min=({render_lo[0]:+.3f},{render_lo[1]:+.3f},{render_lo[2]:+.3f})"
      f" max=({render_hi[0]:+.3f},{render_hi[1]:+.3f},{render_hi[2]:+.3f})  size={[round(s, 3) for s in size]}")
check(0.40 <= size[2] <= 0.45, f"length along Z in 0.40-0.45 m (got {size[2]:.3f})")
check(size[2] > size[1] > size[0], "longest along Z, then Y (height), then X (width)")

print("\n== orientation ==")
mz = world[idx["SOCKET_muzzle"]]
check(mz[2][3] < -0.1, f"SOCKET_muzzle is forward (-Z): z={mz[2][3]:+.3f}")
check(abs(mz[0][3]) < 1e-4, "muzzle on centreline (x=0)")
sg = world[idx["SOCKET_sight"]]
check(sg[1][3] > 0.1, f"SOCKET_sight is up (+Y): y={sg[1][3]:+.3f}")
mg = world[idx["SOCKET_mag"]]
check(mg[2][3] < 0 and mg[1][3] > 0, f"SOCKET_mag forward of grip, above it (y={mg[1][3]:+.3f}, z={mg[2][3]:+.3f})")
check(render_lo[1] < -0.07, f"grip/mag hang below origin (min y {render_lo[1]:+.3f})")
for s in sockets:
    n = nodes[idx[s]]
    check(n.get("rotation", [0, 0, 0, 1]) == [0, 0, 0, 1] or all(abs(a - b) < 1e-6 for a, b in zip(n["rotation"], [0, 0, 0, 1])),
          f"{s} has identity rotation (local -Z = forward, +Y = up)")
# Right side (+X) carries the ejection port: receiver verts with x > 0.0235 exist only on +X
rec = nodes[idx["receiver"]]
xs = [xf(world[idx["receiver"]], p)[0] for prim in gl["meshes"][rec["mesh"]]["primitives"]
      for p in read_acc(prim["attributes"]["POSITION"])]
check(max(xs) > 0.0235 and min(xs) > -0.0235, "ejection port block is on +X (right side)")

print("\n== flat shading ==")
# Group each part's triangles into planes (same geometric normal + same offset). A plane
# whose effective width (area / longest extent) is >= 6 mm is a real flat face; bevel
# strips are ~1-2 mm wide and are meant to shade smoothly. Every vertex normal on a real
# flat face must equal the face normal, or it would show a smoothing gradient.
def tri_check(node_name):
    n = nodes[idx[node_name]]
    planes = {}
    for prim in gl["meshes"][n["mesh"]]["primitives"]:
        P = read_acc(prim["attributes"]["POSITION"])
        N = read_acc(prim["attributes"]["NORMAL"])
        I = [t[0] for t in read_acc(prim["indices"])]
        for t in range(0, len(I), 3):
            a, b, c = (P[I[t]], P[I[t + 1]], P[I[t + 2]])
            u = [b[k] - a[k] for k in range(3)]
            v = [c[k] - a[k] for k in range(3)]
            cr = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
            ln = math.sqrt(sum(x * x for x in cr))
            if ln < 1e-12:
                continue
            g = tuple(x / ln for x in cr)
            key = (tuple(round(x, 3) for x in g), round(sum(g[k] * a[k] for k in range(3)), 4))
            pl = planes.setdefault(key, {"g": g, "pts": [], "normals": [], "area": 0.0})
            pl["area"] += 0.5 * ln
            for k in range(3):
                pl["pts"].append(P[I[t + k]])
                pl["normals"].append(N[I[t + k]])
    nflat, worst = 0, 0.0
    for pl in planes.values():
        g = pl["g"]
        # in-plane basis
        ref = (1, 0, 0) if abs(g[0]) < 0.9 else (0, 1, 0)
        e1 = [g[1] * ref[2] - g[2] * ref[1], g[2] * ref[0] - g[0] * ref[2], g[0] * ref[1] - g[1] * ref[0]]
        l1 = math.sqrt(sum(x * x for x in e1)); e1 = [x / l1 for x in e1]
        e2 = [g[1] * e1[2] - g[2] * e1[1], g[2] * e1[0] - g[0] * e1[2], g[0] * e1[1] - g[1] * e1[0]]
        s1 = [sum(p[k] * e1[k] for k in range(3)) for p in pl["pts"]]
        s2 = [sum(p[k] * e2[k] for k in range(3)) for p in pl["pts"]]
        # effective width = area / longest extent: bevel strips are ~1-2 mm wide
        if pl["area"] / max(max(s1) - min(s1), max(s2) - min(s2)) < 0.006:
            continue
        nflat += 1
        for nv in pl["normals"]:
            d = sum(g[q] * nv[q] for q in range(3))
            worst = max(worst, math.degrees(math.acos(max(-1.0, min(1.0, d)))))
    return nflat, worst


for nm in ["receiver", "grip", "trigger_guard", "magazine", "sight_rail"]:
    cnt, worst = tri_check(nm)
    check(cnt > 0 and worst < 1.0, f"{nm}: {cnt} flat faces, max vertex-normal deviation {worst:.3f} deg")

print()
if fails:
    print(f"{len(fails)} CHECK(S) FAILED")
    sys.exit(1)
print("ALL CHECKS PASSED")
