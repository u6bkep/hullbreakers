"""Finishing, export and the checks around it.

Finishing a part (all applied on export via export_apply):
  1. Hard edges: every edge of a flat face, and every edge of a curved surface sharper
     than its smooth_angle, is marked sharp (edge.smooth = False -> the 'sharp_edge'
     attribute). Mesh.set_sharp_from_angle(angle=) does exist in 5.2, on mesh instances
     (not bpy.types.Mesh), but it takes one angle per mesh; hb decides per face instead.
     Lathe seams made with smooth_seams (auto segments) are always smooth.
  2. Bevel: those hard edges (above 1 degree) get a bevel weight = primitive bevel / the
     part's widest bevel, and a Bevel modifier with limit_method WEIGHT and that widest
     width, so each primitive gets its own width. harden_normals keeps flat faces flat.
     An edge buried inside another primitive of the same part (deeper than its bevel, all
     along its length) stays hard but gets no bevel: nobody can see it, and its bevel would
     cost as much as a visible one. Only where every bevelled edge at both its ends is
     buried too, since mixed vertices bend normals. Other parts don't count: parts move.
  3. Weighted Normal modifier (face area, keep sharp) for the curved surfaces.
"""
import json
import math
import os

import bmesh
import bpy
from mathutils import Matrix
from mathutils.bvhtree import BVHTree

import verify_glb as V

from . import _state, materials
from .space import matrix_to_blender, to_blender

BEVEL_MIN_ANGLE = math.radians(1.0)


class ExportError(RuntimeError):
    pass


def _part_mesh(part, world_b, mat_cache, palette):
    bm = bmesh.new()
    flat_l = bm.faces.layers.int.new("hb_flat")
    bev_l = bm.faces.layers.float.new("hb_bevel")
    sm_l = bm.faces.layers.float.new("hb_smooth")
    seam_l = bm.edges.layers.int.new("hb_seam")
    inv = world_b.inverted()
    roles = []
    shells = []                                # per primitive: its verts and faces
    for pr in part.prims:
        vs = [bm.verts.new(inv @ to_blender(p)) for p in pr.verts]
        faces = []
        for idx, flat, role in pr.faces:
            try:
                f = bm.faces.new([vs[i] for i in idx])
            except ValueError as e:
                raise ExportError(f"{part.name}: bad {pr.kind} face: {e}") from None
            if role not in roles:
                roles.append(role)
            f.material_index = roles.index(role)
            f[flat_l] = int(flat)
            f[bev_l] = pr.bevel
            f[sm_l] = pr.smooth_angle
            f.smooth = True
            faces.append(f)
        for i, j in pr.seams:
            bm.edges.get((vs[i], vs[j]))[seam_l] = 1
        bmesh.ops.recalc_face_normals(bm, faces=faces)
        shells.append((vs, faces))
    bm.normal_update()
    for f in bm.faces:
        if f.calc_area() < 1e-12:
            raise ExportError(f"{part.name}: a zero-area face near {tuple(round(c, 4) for c in f.calc_center_median())}"
                              " (Blender coords); check for collinear or repeated points")

    widest = max((pr.bevel for pr in part.prims), default=0.0)
    bw = bm.edges.layers.float.new("bevel_weight_edge")
    for e in bm.edges:
        lf = e.link_faces
        if len(lf) != 2:
            raise ExportError(f"{part.name}: an edge has {len(lf)} faces; primitives must be closed shells")
        f1, f2 = lf
        ang = e.calc_face_angle(0.0)
        if e[seam_l] or (not f1[flat_l] and not f2[flat_l] and ang < min(f1[sm_l], f2[sm_l])):
            continue                           # smooth across a curved surface
        e.smooth = False                       # hard edge
        if widest > 0 and ang > BEVEL_MIN_ANGLE:
            e[bw] = min(f1[bev_l], f2[bev_l]) / widest
    if widest > 0 and len(shells) > 1:
        part.asset._buried_edges += _unbevel_buried(bm, bw, widest, shells)

    # Planes of the faces modelled flat, in the part's local glTF frame. Bevelling shrinks a
    # face within its own plane, so the verifier can find these planes in the .glb.
    planes = {}
    for f in bm.faces:
        if f[flat_l]:
            n = f.normal
            d = n.dot(f.calc_center_median())
            ng = (n.x, n.z, -n.y)              # Blender -> glTF axes
            key = tuple(round(c, 5) for c in ng) + (round(d, 6),)
            planes[key] = [round(c, 7) for c in ng] + [round(d, 7)]

    me = bpy.data.meshes.new(part.name)
    bm.to_mesh(me)
    bm.free()
    for nm in ("hb_flat", "hb_bevel", "hb_smooth", "hb_seam"):
        me.attributes.remove(me.attributes[nm])
    for role in roles:
        if role not in mat_cache:
            mat_cache[role] = materials.make(role, palette)
        me.materials.append(mat_cache[role])
    return me, widest, sorted(planes.values())


BURIED_SAMPLES = 9                             # points tested along an edge, ends included


def _unbevel_buried(bm, bw, widest, shells):
    """Zero the bevel weight of every edge that lies inside another primitive's closed
    shell, deeper than the edge's own bevel width, at every sample point along it (ends
    included). The bevel stays within its width of the edge, so it would be hidden.
    Primitives are closed shells with outward normals (recalc_face_normals), so a point is
    inside one when the nearest surface point's normal faces away from it."""
    trees = []
    for vs, faces in shells:
        lo = [min(v.co[k] for v in vs) for k in range(3)]
        hi = [max(v.co[k] for v in vs) for k in range(3)]
        index = {v: n for n, v in enumerate(vs)}
        tree = BVHTree.FromPolygons([v.co.copy() for v in vs], [[index[v] for v in f.verts] for f in faces])
        trees.append((set(vs), lo, hi, tree))

    def buried_in(tree, lo, hi, pts, depth):
        if any(p[k] < lo[k] + depth or p[k] > hi[k] - depth for p in pts for k in range(3)):
            return False
        for p in pts:
            loc, nrm, _, dist = tree.find_nearest(p)
            if loc is None or dist <= depth or (p - loc).dot(nrm) >= 0:
                return False
        return True

    buried = set()
    for e in bm.edges:
        if e[bw] <= 0:
            continue
        depth = e[bw] * widest + 1e-5
        a, b = e.verts[0].co, e.verts[1].co
        pts = [a.lerp(b, t / (BURIED_SAMPLES - 1)) for t in range(BURIED_SAMPLES)]
        for own, lo, hi, tree in trees:
            if e.verts[0] not in own and buried_in(tree, lo, hi, pts, depth):
                buried.add(e)
                break
    # A vertex where a bevelled edge meets an unbevelled one makes the Bevel modifier bend
    # the unbevelled faces' normals there. Only unbevel an edge when every bevelled edge at
    # both its ends is buried too (repeated until nothing changes).
    changed = True
    while changed:
        changed = False
        for e in list(buried):
            if any(o[bw] > 0 and o not in buried for v in e.verts for o in v.link_edges):
                buried.discard(e)
                changed = True
    for e in buried:
        e[bw] = 0.0
    return len(buried)


def _collider_mesh(col, world_b):
    bm = bmesh.new()
    inv = world_b.inverted()
    for p in col.points:
        bm.verts.new(inv @ to_blender(p))
    res = bmesh.ops.convex_hull(bm, input=bm.verts, use_existing_faces=False)
    drop = list({g for g in res["geom_interior"] + res["geom_unused"] if isinstance(g, bmesh.types.BMVert)})
    bmesh.ops.delete(bm, geom=drop, context="VERTS")
    loose = [v for v in bm.verts if not v.link_faces]
    bmesh.ops.delete(bm, geom=loose, context="VERTS")
    if len(bm.faces) < 4:
        bm.free()
        raise ExportError(f"{col.name}: its points are flat or collinear; a hull needs volume")
    bmesh.ops.triangulate(bm, faces=bm.faces)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new(col.name)
    bm.to_mesh(me)
    bm.free()
    return me


def _new_object(name, data):
    ob = bpy.data.objects.new(name, data)
    if ob.name != name:
        raise ExportError(f"Blender renamed object '{name}' to '{ob.name}' (a name clash)")
    if data is not None and data.name != name:
        raise ExportError(f"Blender renamed mesh '{name}' to '{data.name}' (a name clash)")
    bpy.context.scene.collection.objects.link(ob)
    return ob


def _build_scene(asset):
    mat_cache = {}
    intent = {}
    world_b = {}
    for node in asset.nodes.values():
        wb = matrix_to_blender(node.world_g)
        world_b[node.name] = wb
        if node.kind == "part":
            if not node.prims:
                raise ExportError(f"part '{node.name}' has no geometry")
            me, widest, planes = _part_mesh(node, wb, mat_cache, asset.palette)
            ob = _new_object(node.name, me)
            if widest > 0:
                bv = ob.modifiers.new("bevel", "BEVEL")
                bv.width = widest
                bv.segments = node.bevel_segments
                bv.limit_method = "WEIGHT"
                bv.harden_normals = True
                bv.miter_outer = "MITER_ARC"
                bv.use_clamp_overlap = True
            wn = ob.modifiers.new("weighted_normal", "WEIGHTED_NORMAL")
            wn.mode = "FACE_AREA"
            wn.weight = 50
            wn.keep_sharp = True
            intent[node.name] = {"bevel": widest, "flat_planes": planes}
        elif node.kind == "collider":
            ob = _new_object(node.name, _collider_mesh(node, wb))
            ob.display_type = "WIRE"
        else:
            ob = _new_object(node.name, None)
            ob.empty_display_type = "ARROWS"
            ob.empty_display_size = 0.02
        node.obj = ob
        if node.parent is not None:
            # Parent with an identity parent-inverse and a parent-local basis. Setting
            # obj.parent alone keeps whatever matrix_parent_inverse is there, and the
            # operator path bakes a hidden one: either way, plausible but wrong positions.
            ob.parent = node.parent.obj
            ob.matrix_parent_inverse = Matrix.Identity(4)
            ob.matrix_basis = world_b[node.parent.name].inverted() @ wb
        else:
            ob.matrix_basis = wb
    bpy.context.view_layer.update()
    for node in asset.nodes.values():
        err = max(abs(a - b) for ra, rb in zip(node.obj.matrix_world, world_b[node.name]) for a, b in zip(ra, rb))
        if err > 1e-6:
            raise ExportError(f"{node.name}: ended up {err:.2e} away from where it was placed (parenting bug)")
    return intent


def _preflight(asset):
    """Everything the exporter could silently drop or add."""
    scene = bpy.context.scene
    problems = []
    roles = V.load_roles()
    for m in bpy.data.materials:
        if m.name in roles and roles[m.name].get("palette") and m.name not in asset.palette:
            problems.append(f"'{m.name}' is used, so asset.json palette must give its colour (from the brief)")
    ours = {n.name for n in asset.nodes.values()}
    for ob in bpy.data.objects:
        if ob.name not in ours:
            problems.append(f"object '{ob.name}' wasn't made by hb (would be exported)")
        if ob.type not in ("MESH", "EMPTY"):
            problems.append(f"object '{ob.name}' is a {ob.type}")
        if ob.hide_viewport or ob.hide_render or ob.hide_get() or ob.hide_select:
            problems.append(f"object '{ob.name}' is hidden")
        if ob.name not in scene.collection.objects:
            problems.append(f"object '{ob.name}' isn't linked in the scene")
    for name in ours:
        if name not in bpy.data.objects:
            problems.append(f"node '{name}' has no object")
    if len(bpy.data.scenes) != 1:
        problems.append(f"{len(bpy.data.scenes)} scenes")
    if bpy.data.cameras or bpy.data.lights:
        problems.append("the scene has cameras or lights (previews render after export)")
    if problems:
        raise ExportError("refusing to export:\n  " + "\n  ".join(problems))


def _postflight(asset, path):
    """Read the .glb back and compare it with what was built."""
    glb = V.Glb(path)
    problems = []
    names = set(glb.names)
    want = set(asset.nodes)
    if names != want:
        if want - names:
            problems.append(f"dropped by the exporter: {sorted(want - names)}")
        if names - want:
            problems.append(f"added by the exporter: {sorted(names - want)}")
    if [glb.names[r] for r in glb.roots] != [asset.name]:
        problems.append(f"scene roots are {[glb.names[r] for r in glb.roots]}, expected ['{asset.name}']")
    for node in asset.nodes.values():
        i = glb.index.get(node.name)
        if i is None:
            continue
        has_mesh = "mesh" in glb.nodes[i]
        if has_mesh != (node.kind in ("part", "collider")):
            problems.append(f"{node.name}: mesh {'present' if has_mesh else 'missing'}")
        parent = glb.parent.get(i)
        want_parent = node.parent.name if node.parent else None
        if (glb.names[parent] if parent is not None else None) != want_parent:
            problems.append(f"{node.name}: parent in file is not {want_parent}")
        # hb's Bevy space is the file's glTF space: the world matrices must agree.
        got = glb.world.get(i)
        if got is not None:
            err = max(abs(got[r][c] - node.world_g[r][c]) for r in range(4) for c in range(4))
            if err > 1e-5:
                problems.append(f"{node.name}: world transform in file is {err:.2e} off what was placed")
    if problems:
        raise ExportError(f"{path} doesn't match the scene:\n  " + "\n  ".join(problems))
    return glb


def export(asset):
    if asset._exported:
        raise RuntimeError("export() called twice")
    asset._exported = True
    glb_path = os.path.join(asset.dir, f"{asset.name}.glb")
    intent_path = os.path.join(asset.dir, f"{asset.name}.intent.json")
    for p in (glb_path, intent_path):      # never leave a stale file for the verifier to pass
        if os.path.exists(p):
            os.remove(p)

    intent = _build_scene(asset)
    _preflight(asset)
    bpy.ops.export_scene.gltf(
        filepath=glb_path,
        export_format="GLB",
        export_yup=True,
        export_apply=True,             # bake bevel + weighted normals
        export_normals=True,
        export_tangents=False,
        export_texcoords=True,         # nothing has UVs yet; harmless, and right once textures arrive
        export_materials="EXPORT",
        export_vertex_color="NONE",
        export_attributes=False,
        export_cameras=False,
        export_lights=False,
        export_extras=False,
        export_animations=False,
        export_skins=False,
        export_morph=False,
        export_gn_mesh=False,
        export_gpu_instances=False,
        export_hierarchy_flatten_objs=False,
        export_hierarchy_full_collections=False,
        export_shared_accessors=False,
        export_draco_mesh_compression_enable=False,
        export_meshopt_compression_enable=False,
        export_use_gltfpack=False,
        # Filters that would silently leave things out. All off; _preflight already
        # refuses anything they could have caught.
        use_selection=False,
        use_visible=False,
        use_renderable=False,
        use_active_collection=False,
        use_active_scene=False,
        use_mesh_edges=False,
        use_mesh_vertices=False,
    )
    if not os.path.isfile(glb_path):
        raise ExportError(f"the exporter wrote nothing to {glb_path}")
    glb = _postflight(asset, glb_path)

    with open(intent_path, "w") as f:
        json.dump({
            "_doc": "Generated by tools/assets/hb on export; don't edit. Planes (part-local glTF:"
                    " nx, ny, nz, d) of every face modelled flat; verify_glb.py checks they shade flat.",
            "asset": asset.name,
            "parts": intent,
        }, f, indent=1)
        f.write("\n")

    total = 0
    print(f"hb: exported {glb_path}")
    for node in asset.nodes.values():
        if node.kind in ("part", "collider"):
            t = glb.triangles(glb.index[node.name])
            if node.kind == "part":
                total += t
            print(f"hb:   {node.name:20s} {t:6d} tris{'  (collider)' if node.kind == 'collider' else ''}")
    lo, hi = asset.kind_spec["tris"]
    print(f"hb:   {'TOTAL':20s} {total:6d} tris (budget {lo}-{hi})")
    print(f"hb:   detail {asset.detail * 1000:.1f} mm (1 px at {asset.kind_spec['near']} m): {asset._dropped_bevels}"
          f" primitive bevels under it dropped; {asset._buried_edges} buried edges not bevelled")
    _state.exported = glb_path

    if _state.preview:
        from . import preview
        preview.render(asset)
    return glb_path
