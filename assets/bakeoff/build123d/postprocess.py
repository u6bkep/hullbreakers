"""Stage 2: turn build123d's raw glTF into the game asset (pure Python, numpy + pygltflib).

What build123d / OCCT's writer cannot do, and this script therefore does:
  1. Materials: OCCT names them mat_0..N with metallic=1, roughness=1, doubleSided,
     no emissive. Map each back to its named material by baseColor, set real PBR
     values and KHR_materials_emissive_strength.
  2. Primitives: OCCT writes one primitive per B-rep face (~340 draw calls). Merge
     into one primitive per material per part.
  3. Hierarchy: build123d wraps every leaf shape in its own child node carrying the
     shape's TopLoc (our moves/rotations were NOT baked into the geometry). Flatten
     each part's subtree into a single mesh on the part node, baking leaf transforms.
  4. Sockets: build123d has no empty-node concept (an empty Compound can't even be
     created); add SOCKET_* nodes here from layout.json.
  5. COL_body: rename the mesh, no material.
Normals are kept exactly as OCCT computed them (per B-rep face, so face edges are
hard and cylinders/fillets are smooth). TEXCOORD_0 (surface-parameter UVs) dropped.
"""
import json
import sys
from pathlib import Path

import numpy as np
import pygltflib as G

work, out_path = Path(sys.argv[1]), Path(sys.argv[2])
layout = json.loads((work / "layout.json").read_text())
src = G.GLTF2().load(str(work / "raw.glb"))
blob = src.binary_blob()

PARTS = ["receiver", "grip", "barrel_shroud", "magazine", "sight_rail", "trigger_guard"]
MATERIALS = {   # linear baseColor, metallic, roughness, emissive
    "paint_primary":   dict(c=(0.448, 0.147, 0.022), m=0.0, r=0.55),
    "metal_dark":      dict(c=(0.022, 0.025, 0.030), m=1.0, r=0.40),
    "metal_bare":      dict(c=(0.342, 0.355, 0.393), m=1.0, r=0.28),
    "polymer":         dict(c=(0.004, 0.004, 0.0045), m=0.0, r=0.85),
    "emissive_accent": dict(c=(0.033, 0.692, 1.0), m=0.0, r=0.4,
                            e=(0.033, 0.692, 1.0), es=8.0),
}

def srgb_to_linear(c):
    c = np.asarray(c)
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)

key_lin = {n: srgb_to_linear(c) for n, c in layout["material_keys"].items()}

def name_for_material(i):
    bc = np.array(src.materials[i].pbrMetallicRoughness.baseColorFactor[:3])
    name, err = min(((n, np.abs(bc - k).max()) for n, k in key_lin.items()), key=lambda t: t[1])
    assert err < 1e-3, f"material {i} colour {bc} matches no key"
    return name

COMP = {5120: np.int8, 5121: np.uint8, 5122: np.int16, 5123: np.uint16, 5125: np.uint32, 5126: np.float32}
NCOMP = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}

def read(acc_i):
    a = src.accessors[acc_i]
    bv = src.bufferViews[a.bufferView]
    dt = np.dtype(COMP[a.componentType])
    n = NCOMP[a.type]
    off = (bv.byteOffset or 0) + (a.byteOffset or 0)
    stride = bv.byteStride or dt.itemsize * n
    raw = np.frombuffer(blob, dtype=np.uint8, count=stride * (a.count - 1) + dt.itemsize * n, offset=off)
    arr = np.lib.stride_tricks.as_strided(raw, shape=(a.count, n * dt.itemsize), strides=(stride, 1))
    return np.ascontiguousarray(arr).view(dt).reshape(a.count, n) if n > 1 else \
        np.ascontiguousarray(arr).view(dt).reshape(a.count)

def quat_to_m(q):
    x, y, z, w = q
    return np.array([[1 - 2*(y*y+z*z), 2*(x*y-z*w), 2*(x*z+y*w)],
                     [2*(x*y+z*w), 1 - 2*(x*x+z*z), 2*(y*z-x*w)],
                     [2*(x*z-y*w), 2*(y*z+x*w), 1 - 2*(x*x+y*y)]])

def local_matrix(n):
    if n.matrix:
        return np.array(n.matrix, dtype=float).reshape(4, 4).T
    M = np.eye(4)
    S = np.diag(n.scale) if n.scale else np.eye(3)
    R = quat_to_m(n.rotation) if n.rotation else np.eye(3)
    M[:3, :3] = R @ S
    if n.translation:
        M[:3, 3] = n.translation
    return M

def gather(node_i, M, acc):
    """Collect (material, positions, normals, indices) of node subtree in the frame M maps from."""
    n = src.nodes[node_i]
    if n.mesh is not None:
        for p in src.meshes[n.mesh].primitives:
            assert p.mode in (None, 4)
            pos = read(p.attributes.POSITION).astype(float)
            nrm = read(p.attributes.NORMAL).astype(float)
            idx = read(p.indices).astype(np.uint32)
            pos = pos @ M[:3, :3].T + M[:3, 3]
            nrm = nrm @ np.linalg.inv(M[:3, :3])  # inverse-transpose
            nrm /= np.linalg.norm(nrm, axis=1, keepdims=True)
            acc.append((p.material, pos, nrm, idx))
    for c in n.children or []:
        gather(c, M @ local_matrix(src.nodes[c]), acc)

# --------------------------------------------------------------- build output
out = G.GLTF2(asset=G.Asset(version="2.0", generator="build123d 0.13 + OCCT RWGltf + postprocess.py"))
out.extensionsUsed = ["KHR_materials_emissive_strength"]
buf = bytearray()

def add_view(data, target):
    while len(buf) % 4:
        buf.append(0)
    out.bufferViews.append(G.BufferView(buffer=0, byteOffset=len(buf), byteLength=len(data), target=target))
    buf.extend(data)
    return len(out.bufferViews) - 1

def add_acc(arr, ctype, atype, target, minmax=False):
    v = add_view(arr.tobytes(), target)
    a = G.Accessor(bufferView=v, componentType=ctype, count=len(arr), type=atype)
    if minmax:
        a.min = arr.min(axis=0).tolist()
        a.max = arr.max(axis=0).tolist()
    out.accessors.append(a)
    return len(out.accessors) - 1

mat_index = {}
for name, d in MATERIALS.items():
    m = G.Material(name=name, doubleSided=False,
                   pbrMetallicRoughness=G.PbrMetallicRoughness(
                       baseColorFactor=[*d["c"], 1.0], metallicFactor=d["m"], roughnessFactor=d["r"]))
    if "e" in d:
        m.emissiveFactor = list(d["e"])
        m.extensions = {"KHR_materials_emissive_strength": {"emissiveStrength": d["es"]}}
    mat_index[name] = len(out.materials)
    out.materials.append(m)

def build_mesh(name, src_node):
    acc = []
    gather(src_node, np.eye(4), acc)   # relative to the part node itself
    by_mat = {}
    for m, pos, nrm, idx in acc:
        key = None if m is None else name_for_material(m)
        by_mat.setdefault(key, []).append((pos, nrm, idx))
    prims = []
    for key, chunks in by_mat.items():
        P, N, I, base = [], [], [], 0
        for pos, nrm, idx in chunks:
            P.append(pos); N.append(nrm); I.append(idx + base); base += len(pos)
        P = np.concatenate(P).astype(np.float32)
        N = np.concatenate(N).astype(np.float32)
        I = np.concatenate(I).astype(np.uint32 if base > 65535 else np.uint16)
        attrs = G.Attributes(POSITION=add_acc(P, 5126, "VEC3", 34962, minmax=True),
                             NORMAL=add_acc(N, 5126, "VEC3", 34962))
        ind = add_acc(I, 5125 if I.dtype == np.uint32 else 5123, "SCALAR", 34963)
        prims.append(G.Primitive(attributes=attrs, indices=ind, mode=4,
                                 material=None if key is None else mat_index[key]))
    out.meshes.append(G.Mesh(name=name, primitives=prims))
    return len(out.meshes) - 1

src_root = next(i for i, n in enumerate(src.nodes) if n.name == "smg")
src_by_name = {src.nodes[c].name: c for c in src.nodes[src_root].children}  # part nodes only
assert local_matrix(src.nodes[src_root]).round(9).tolist() == np.eye(4).tolist(), "root not identity"
root = G.Node(name="smg", children=[])
out.nodes.append(root)
for name in PARTS + ["COL_body"]:
    si = src_by_name[name]
    t = layout["attach"][name]
    # sanity: part node translation from build123d must equal our attach point
    assert np.allclose(local_matrix(src.nodes[si])[:3, 3], t, atol=1e-6), name
    node = G.Node(name=name, mesh=build_mesh(name, si),
                  translation=[float(v) for v in t] if any(t) else None)
    out.nodes.append(node)
    root.children.append(len(out.nodes) - 1)
for name, t in layout["sockets"].items():
    out.nodes.append(G.Node(name=name, translation=[float(v) for v in t] if any(t) else None))
    root.children.append(len(out.nodes) - 1)

out.scenes = [G.Scene(name="smg", nodes=[0])]
out.scene = 0
out.buffers = [G.Buffer(byteLength=len(buf))]
out.set_binary_blob(bytes(buf))
out.save_binary(str(out_path))
print("wrote", out_path)
