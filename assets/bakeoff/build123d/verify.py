"""Stage 3: independent verification of smg.glb.
Uses a hand-rolled GLB chunk parser (struct + json, no pygltflib) for the JSON
structure, and trimesh's glTF loader for geometry/scene-graph transforms."""
import json
import struct
import sys

import numpy as np
import trimesh

path = sys.argv[1]
fails = []
def check(cond, msg):
    print(("  ok   " if cond else "  FAIL ") + msg)
    if not cond:
        fails.append(msg)

# ---------------------------------------------------------------- raw JSON
data = open(path, "rb").read()
magic, ver, length = struct.unpack_from("<4sII", data, 0)
check(magic == b"glTF" and ver == 2 and length == len(data), f"GLB header (v{ver}, {length} bytes)")
jlen, jtype = struct.unpack_from("<II", data, 12)
gj = json.loads(data[20:20 + jlen])
blen, btype = struct.unpack_from("<II", data, 20 + jlen)
check(btype == 0x004E4942 and "uri" not in gj["buffers"][0], "single embedded BIN chunk, no external URIs")
bin_ = data[28 + jlen: 28 + jlen + blen]

nodes = gj["nodes"]
names = [n.get("name") for n in nodes]
PARTS = ["receiver", "grip", "barrel_shroud", "magazine", "sight_rail", "trigger_guard"]
SOCKETS = ["SOCKET_grip", "SOCKET_muzzle", "SOCKET_mag", "SOCKET_sight", "SOCKET_barrel"]
print("nodes:")
for n in PARTS + SOCKETS + ["COL_body", "smg"]:
    check(names.count(n) == 1, f"node '{n}' present exactly once")
root_i = gj["scenes"][gj.get("scene", 0)]["nodes"]
check(len(root_i) == 1 and names[root_i[0]] == "smg", "scene has single root 'smg'")
root = nodes[root_i[0]]
check(not any(k in root for k in ("translation", "rotation", "scale", "matrix")), "root transform is identity")
kids = {names[c] for c in root["children"]}
check(kids == set(PARTS + SOCKETS + ["COL_body"]), "root children = 6 parts + 5 sockets + COL_body (flat hierarchy)")
for s in SOCKETS:
    n = nodes[names.index(s)]
    check("mesh" not in n and not n.get("children"), f"{s} is an empty node")
for p in PARTS + ["COL_body"]:
    n = nodes[names.index(p)]
    check("mesh" in n and gj["meshes"][n["mesh"]].get("name") == p, f"{p} has its own mesh named '{p}'")

print("materials:")
mats = {m["name"]: m for m in gj["materials"]}
want = {"paint_primary": (None, 0.55), "metal_dark": (1.0, 0.4), "metal_bare": (1.0, None),
        "polymer": (0.0, None), "emissive_accent": (None, None)}
for name, (metal, rough) in want.items():
    m = mats.get(name)
    check(m is not None, f"material '{name}' exists")
    if not m:
        continue
    pbr = m["pbrMetallicRoughness"]
    print(f"         {name}: base={np.round(pbr['baseColorFactor'],3).tolist()} metallic={pbr.get('metallicFactor',1)} "
          f"rough={pbr.get('roughnessFactor',1)} emissive={m.get('emissiveFactor')} ext={m.get('extensions')}")
    if metal is not None:
        check(abs(pbr.get("metallicFactor", 1.0) - metal) < 1e-6, f"{name} metallic = {metal}")
    if rough is not None:
        check(abs(pbr.get("roughnessFactor", 1.0) - rough) < 1e-6, f"{name} roughness = {rough}")
    check("baseColorTexture" not in pbr and "metallicRoughnessTexture" not in pbr, f"{name} untextured")
check(mats["polymer"]["pbrMetallicRoughness"]["roughnessFactor"] >= 0.7, "polymer is rough")
em = mats["emissive_accent"]
check(max(em.get("emissiveFactor", [0])) > 0 and
      em.get("extensions", {}).get("KHR_materials_emissive_strength", {}).get("emissiveStrength", 1) > 1,
      "emissive_accent has emissiveFactor + KHR_materials_emissive_strength > 1")
check("KHR_materials_emissive_strength" in gj.get("extensionsUsed", []), "extension declared in extensionsUsed")
check(not gj.get("textures") and not gj.get("images"), "no textures/images in file")
col_mesh = gj["meshes"][nodes[names.index("COL_body")]["mesh"]]
check(all(p.get("material") is None or gj["materials"][p["material"]]["name"] == "collider"
          for p in col_mesh["primitives"]), "COL_body has no material")
used = {gj["materials"][p["material"]]["name"] for m in gj["meshes"] for p in m["primitives"] if "material" in p}
check(used == set(want), f"all five materials used ({sorted(used)})")

# ---------------------------------------------------------------- geometry via trimesh
print("geometry (trimesh):")
scene = trimesh.load(path, force="scene", process=False)
# trimesh splits each multi-material glTF mesh into one geometry node per primitive
# (named <node>_<hash>); regroup them under the glTF node name.
tri = {}
for node in scene.graph.nodes_geometry:
    T, gname = scene.graph[node]
    part = next(p for p in PARTS + ["COL_body"] if node == p or node.startswith(p + "_"))
    m = scene.geometry[gname]
    if part in tri:
        T0, m0 = tri[part]
        assert np.allclose(T0, T)
        m = trimesh.util.concatenate([m0, m])
    tri[part] = (T, m)
total = sum(len(m.faces) for n, (T, m) in tri.items() if not n.startswith("COL_"))
for n, (T, m) in sorted(tri.items()):
    print(f"         {n:14s} {len(m.faces):5d} tris")
print(f"         visible total {total} tris (+{len(tri['COL_body'][1].faces)} collider)")
check(2000 <= total <= 6000, "visible triangle count within 2k-6k")

def world(n):
    T, m = tri[n]
    return trimesh.transform_points(m.vertices, T)
allv = np.vstack([world(n) for n in tri if not n.startswith("COL_")])
lo, hi = allv.min(0), allv.max(0)
print(f"         bounds min {np.round(lo,4).tolist()} max {np.round(hi,4).tolist()} size {np.round(hi-lo,4).tolist()}")
check(0.40 <= hi[2] - lo[2] <= 0.45, f"length along Z = {hi[2]-lo[2]:.3f} m (0.40-0.45)")
check(hi[2] - lo[2] > hi[1] - lo[1] > hi[0] - lo[0], "longest along Z, then Y, narrowest X")
check(abs(lo[0] + hi[0]) < 1e-4, "symmetric about X=0")

def socket(s):
    return np.array(scene.graph.get(s)[0])[:3, 3]
check(np.allclose(socket("SOCKET_grip"), 0), "SOCKET_grip at origin")
mz = socket("SOCKET_muzzle")
check(abs(mz[2] - lo[2]) < 0.002, f"SOCKET_muzzle at the -Z extreme (z={mz[2]:.4f}, min z={lo[2]:.4f}) -> gun points -Z")
check(socket("SOCKET_sight")[1] > socket("SOCKET_barrel")[1] > 0, "sight above barrel axis above grip -> +Y up")
check(socket("SOCKET_mag")[2] < 0 and socket("SOCKET_barrel")[2] < socket("SOCKET_mag")[2], "mag ahead of grip, barrel ahead of mag")
mag_w = world("magazine")
check(mag_w[:, 1].min() < -0.05, "magazine hangs below the grip origin")
check(np.allclose(np.array(scene.graph.get("magazine")[0])[:3, 3], socket("SOCKET_mag")), "magazine node origin == SOCKET_mag (animate out along local -Y)")
check(np.allclose(np.array(scene.graph.get("barrel_shroud")[0])[:3, 3], socket("SOCKET_barrel")), "barrel_shroud node origin == SOCKET_barrel")
cw = world("COL_body")
check(trimesh.Trimesh(tri["COL_body"][1].vertices, tri["COL_body"][1].faces).is_convex, "COL_body is convex")

# ---------------------------------------------------------------- normals from raw buffers
print("normals:")
COMP = {5123: np.uint16, 5125: np.uint32, 5126: np.float32}
def acc(i):
    a = gj["accessors"][i]; bv = gj["bufferViews"][a["bufferView"]]
    nc = {"SCALAR": 1, "VEC3": 3}[a["type"]]
    arr = np.frombuffer(bin_, COMP[a["componentType"]], a["count"] * nc, bv.get("byteOffset", 0) + a.get("byteOffset", 0))
    return arr.reshape(-1, nc) if nc > 1 else arr
flat = smooth = bad = n_tri = nonunit = 0
for m in gj["meshes"]:
    for p in m["primitives"]:
        if "NORMAL" not in p["attributes"]:
            check(False, f"{m['name']} primitive has NORMAL"); continue
        P = acc(p["attributes"]["POSITION"]); N = acc(p["attributes"]["NORMAL"]); I = acc(p["indices"]).reshape(-1, 3)
        fn = np.cross(P[I[:, 1]] - P[I[:, 0]], P[I[:, 2]] - P[I[:, 0]])
        fl = np.linalg.norm(fn, axis=1); keep = fl > 1e-12
        fn = fn[keep] / fl[keep, None]; vn = N[I[keep]]
        dots = np.einsum("tij,tj->ti", vn, fn)
        bad += int((dots.mean(1) <= 0).sum())
        is_flat = (dots > 0.9999).all(1)
        flat += int(is_flat.sum()); smooth += int((~is_flat).sum()); n_tri += int(keep.sum())
        nonunit += int((np.abs(np.linalg.norm(N, axis=1) - 1) > 1e-3).sum())
check(nonunit == 0, f"all stored normals unit length ({nonunit} not)")
check(bad == 0, f"winding agrees with stored normals on all {n_tri} triangles ({bad} disagree)")
print(f"         {flat} triangles shade exactly flat (vertex normal == face normal), {smooth} use varying normals (cylinders/fillets)")

print("\nRESULT:", "PASS" if not fails else f"{len(fails)} FAIL(S)")
sys.exit(1 if fails else 0)
