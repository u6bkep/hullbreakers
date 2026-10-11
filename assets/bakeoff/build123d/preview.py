"""Stage 4: offscreen previews with pyrender (EGL). Reads the .glb with pygltflib and
feeds the stored normals straight to pyrender, so what you see is the file's shading
(trimesh's loader would re-merge vertices / recompute normals)."""
import os
os.environ.setdefault("PYOPENGL_PLATFORM", "egl")
import sys
from pathlib import Path

import numpy as np
import pygltflib as G
import pyrender
from PIL import Image

glb, outdir = Path(sys.argv[1]), Path(sys.argv[2])
outdir.mkdir(parents=True, exist_ok=True)
g = G.GLTF2().load(str(glb))
blob = g.binary_blob()
COMP = {5123: np.uint16, 5125: np.uint32, 5126: np.float32}
NC = {"SCALAR": 1, "VEC3": 3}

def read(i):
    a = g.accessors[i]; bv = g.bufferViews[a.bufferView]
    arr = np.frombuffer(blob, COMP[a.componentType], a.count * NC[a.type], (bv.byteOffset or 0) + (a.byteOffset or 0))
    return arr.reshape(a.count, NC[a.type]) if NC[a.type] > 1 else arr

def mats(n):
    M = np.eye(4)
    if n.translation: M[:3, 3] = n.translation
    return M

def build_scene(show_collider=False):
    sc = pyrender.Scene(bg_color=[0.13, 0.14, 0.16, 1.0], ambient_light=[0.12, 0.12, 0.13])
    root = g.nodes[g.scenes[0].nodes[0]]
    for c in root.children:
        n = g.nodes[c]
        if n.mesh is None:
            continue
        is_col = n.name.startswith("COL_")
        if is_col != show_collider and not (show_collider and not is_col):
            continue
        prims = []
        for p in g.meshes[n.mesh].primitives:
            if is_col:
                mat = pyrender.MetallicRoughnessMaterial(baseColorFactor=[0.2, 1.0, 0.3, 0.35],
                                                         alphaMode="BLEND", metallicFactor=0, roughnessFactor=1)
            else:
                m = g.materials[p.material]; pbr = m.pbrMetallicRoughness
                es = (m.extensions or {}).get("KHR_materials_emissive_strength", {}).get("emissiveStrength", 1.0)
                mat = pyrender.MetallicRoughnessMaterial(
                    baseColorFactor=pbr.baseColorFactor, metallicFactor=pbr.metallicFactor,
                    roughnessFactor=pbr.roughnessFactor,
                    emissiveFactor=np.minimum(np.array(m.emissiveFactor or [0, 0, 0]) * es, 1.0))
            prims.append(pyrender.Primitive(positions=read(p.attributes.POSITION), normals=read(p.attributes.NORMAL),
                                            indices=read(p.indices).reshape(-1, 3), material=mat, mode=4))
        sc.add(pyrender.Mesh(prims), pose=mats(n))
    return sc

def look_at(eye, target, up=(0, 1, 0)):
    eye, target, up = map(np.asarray, (eye, target, up))
    f = target - eye; f /= np.linalg.norm(f)
    r = np.cross(f, up); r /= np.linalg.norm(r)
    u = np.cross(r, f)
    M = np.eye(4); M[:3, 0], M[:3, 1], M[:3, 2], M[:3, 3] = r, u, -f, eye
    return M

C = np.array([0.0, 0.03, -0.12])   # roughly the gun's centre
VIEWS = {
    "left":        C + [-0.5, 0.0, 0.0],
    "right":       C + [0.5, 0.0, 0.0],
    "three_quarter_front": C + [0.33, 0.18, -0.36],
    "three_quarter_rear":  C + [-0.3, 0.15, 0.4],
    "top":         C + [0.0, 0.5, 0.001],
    "front":       C + [0.0, 0.05, -0.5],
}
r = pyrender.OffscreenRenderer(1200, 800)
for show_col, views in ((False, VIEWS), (True, {"collider_left": VIEWS["left"]})):
    for name, eye in views.items():
        sc = build_scene(show_col)
        cam = sc.add(pyrender.PerspectiveCamera(yfov=np.radians(38), aspectRatio=1.5), pose=look_at(eye, C))
        sc.add(pyrender.DirectionalLight(color=[1, 0.97, 0.92], intensity=4.0), pose=look_at(C + [0.5, 0.8, -0.3], C))
        sc.add(pyrender.DirectionalLight(color=[0.6, 0.7, 1.0], intensity=1.5), pose=look_at(C + [-0.6, 0.2, 0.5], C))
        sc.add(pyrender.PointLight(color=[1, 1, 1], intensity=1.2), pose=look_at(eye, C))
        col, _ = r.render(sc)
        Image.fromarray(col).save(outdir / f"{name}.png")
        print("preview", outdir / f"{name}.png")
r.delete()

# close-ups for shading checks
r = pyrender.OffscreenRenderer(1200, 800)
for name, eye, tgt in (("closeup_shroud", [0.12, 0.13, -0.33], [0, 0.085, -0.25]),
                       ("closeup_mag_bottom", [0.12, -0.15, -0.0], [0, -0.07, -0.086])):
    sc = build_scene(False)
    sc.add(pyrender.PerspectiveCamera(yfov=np.radians(38), aspectRatio=1.5), pose=look_at(np.array(eye), np.array(tgt)))
    sc.add(pyrender.DirectionalLight(color=[1, 0.97, 0.92], intensity=4.0), pose=look_at(C + [0.5, 0.8, -0.3], C))
    sc.add(pyrender.PointLight(color=[1, 1, 1], intensity=0.5), pose=look_at(np.array(eye), np.array(tgt)))
    col, _ = r.render(sc)
    Image.fromarray(col).save(outdir / f"{name}.png")
r.delete()
