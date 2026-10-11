"""Blender materials from the shared role library (tools/assets/materials.json).

The values come from verify_glb.resolve_material, the same function the verifier checks
against, so a material can't drift between what is built and what is checked.
"""
import bpy

import verify_glb as V


def make(role, palette):
    """Create the Blender material for a role, named exactly `role`."""
    want = V.resolve_material(role, palette)
    if role in bpy.data.materials:
        raise RuntimeError(f"material '{role}' already exists; hb creates each role once")
    m = bpy.data.materials.new(role)
    if m.name != role:
        raise RuntimeError(f"Blender renamed material '{role}' to '{m.name}'")
    bsdf = next(n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED")
    bsdf.inputs["Base Color"].default_value = (*want["base"], 1.0)
    bsdf.inputs["Metallic"].default_value = want["metallic"]
    bsdf.inputs["Roughness"].default_value = want["roughness"]
    bsdf.inputs["Alpha"].default_value = want["alpha"]
    if want["strength"] > 0:
        # Strength > 1 makes the exporter write KHR_materials_emissive_strength. It normalises
        # the colour (brightest channel 1) and scales the strength to keep colour x strength.
        bsdf.inputs["Emission Color"].default_value = (*want["emissive"], 1.0)
        bsdf.inputs["Emission Strength"].default_value = want["strength"]
    if want["alpha"] < 1.0:
        m.surface_render_method = "BLENDED"   # exported as alphaMode BLEND
    # Off by default in Blender, and the exporter writes doubleSided: true unless it's on.
    m.use_backface_culling = True
    m.diffuse_color = (*want["base"], want["alpha"])
    return m
