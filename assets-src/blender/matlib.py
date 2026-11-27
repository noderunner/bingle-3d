"""Blender materials from the client's own material data (`look::MaterialSpec`, as an environment's
or an appearance's TOML file holds it), so that Cycles renders a thing as the client draws it: the
previews, and the picture of the garden its light is made from (`garden_light.py`).

Only what Cycles needs is built. The glTF files are exported without materials: the client reads
the same data."""

import os
import tomllib

import bpy

import common

# Each wood's size and oil finish, as in `src/look.rs`.
WOODS = {
    "teak": (1.0, (1.0, 0.93, 0.84)),
    "walnut": (1.0, (0.58, 0.44, 0.35)),
    "cherry": (1.0, (0.78, 0.55, 0.46)),
    "maple": (1.0, (1.0, 1.0, 1.0)),
    "felt": (0.285, (1.0, 1.0, 1.0)),
}
BOARD_WOOD = "teak"


def environment(name):
    with open(os.path.join(common.ASSETS, "environments", f"{name}.toml"), "rb") as f:
        return tomllib.load(f)


def srgb_to_linear(c):
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def image(path, color):
    img = bpy.data.images.load(os.path.join(common.ASSETS, path), check_existing=True)
    img.colorspace_settings.name = "sRGB" if color else "Non-Color"
    return img


def build(name, spec):
    """Gives the material of this name the look the data describes. The mesh's own texture
    coordinates are used (a wood's are in metres, with its relief on a second set)."""
    spec = dict(spec)
    mat = bpy.data.materials.get(name) or bpy.data.materials.new(name)
    mat.use_nodes = True
    nodes, links = mat.node_tree.nodes, mat.node_tree.links
    nodes.clear()
    out = nodes.new("ShaderNodeOutputMaterial")
    bsdf = nodes.new("ShaderNodeBsdfPrincipled")
    links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])

    tint = list(spec.get("tint", [1.0, 1.0, 1.0]))
    metres = spec.get("metres", 1.0)
    albedo_file, normal_file, arm_file = spec.get("albedo"), spec.get("normal"), spec.get("arm")
    detail = False
    if "wood" in spec:
        wood = BOARD_WOOD if spec["wood"] == "board" else spec["wood"]
        metres, finish = WOODS[wood]
        tint = [t * f for t, f in zip(tint, finish)]
        albedo_file = f"textures/wood/{wood}_albedo.jpg"
        normal_file = f"textures/wood/{wood}_normal.jpg"
        arm_file = f"textures/wood/{wood}_arm.jpg"
        detail = True

    def texture(path, color, uv_map=None):
        node = nodes.new("ShaderNodeTexImage")
        node.image = image(path, color)
        coords = nodes.new("ShaderNodeUVMap")
        if uv_map:
            coords.uv_map = uv_map
        mapping = nodes.new("ShaderNodeMapping")
        mapping.inputs["Scale"].default_value = (1.0 / metres, 1.0 / metres, 1.0)
        links.new(coords.outputs["UV"], mapping.inputs["Vector"])
        links.new(mapping.outputs["Vector"], node.inputs["Vector"])
        return node

    bsdf.inputs["Roughness"].default_value = spec.get("roughness", 0.5)
    bsdf.inputs["Metallic"].default_value = spec.get("metallic", 0.0)
    # Bevy's reflectance r gives F0 = 0.16 r²; Blender's level l gives F0 = 0.08 l.
    bsdf.inputs["Specular IOR Level"].default_value = 2.0 * spec.get("reflectance", 0.5) ** 2
    bsdf.inputs["Coat Weight"].default_value = spec.get("clearcoat", 0.0)
    bsdf.inputs["Coat Roughness"].default_value = spec.get("clearcoat_roughness", 0.03)
    linear_tint = [srgb_to_linear(c) for c in tint] + [1.0]
    if albedo_file:
        albedo = texture(albedo_file, True)
        mix = nodes.new("ShaderNodeMix")
        mix.data_type = "RGBA"
        mix.blend_type = "MULTIPLY"
        mix.inputs["Factor"].default_value = 1.0
        links.new(albedo.outputs["Color"], mix.inputs["A"])
        mix.inputs["B"].default_value = linear_tint
        links.new(mix.outputs["Result"], bsdf.inputs["Base Color"])
        if "cutout" in spec:
            cut = nodes.new("ShaderNodeMath")
            cut.operation = "GREATER_THAN"
            cut.inputs[1].default_value = spec["cutout"]
            links.new(albedo.outputs["Alpha"], cut.inputs[0])
            links.new(cut.outputs["Value"], bsdf.inputs["Alpha"])
    else:
        bsdf.inputs["Base Color"].default_value = linear_tint
    if normal_file:
        normal = texture(normal_file, False, "Detail" if detail else None)
        normal_map = nodes.new("ShaderNodeNormalMap")
        if detail:
            normal_map.uv_map = "Detail"
        links.new(normal.outputs["Color"], normal_map.inputs["Color"])
        links.new(normal_map.outputs["Normal"], bsdf.inputs["Normal"])
    if arm_file:
        arm = texture(arm_file, False)
        split = nodes.new("ShaderNodeSeparateColor")
        links.new(arm.outputs["Color"], split.inputs["Color"])
        rough = nodes.new("ShaderNodeMath")
        rough.operation = "MULTIPLY"
        rough.inputs[1].default_value = spec.get("roughness", 1.0)
        links.new(split.outputs["Green"], rough.inputs[0])
        links.new(rough.outputs["Value"], bsdf.inputs["Roughness"])
    if "opacity" in spec:
        bsdf.inputs["Alpha"].default_value = spec["opacity"]
    if "reflects" in spec:
        bsdf.inputs["Specular Tint"].default_value = [srgb_to_linear(c) for c in spec["reflects"]] + [1.0]
    if "translucency" in spec:
        bsdf.inputs["Subsurface Weight"].default_value = 0.0
        bsdf.inputs["Transmission Weight"].default_value = 0.0
        # Light through a leaf: a translucent lobe mixed in.
        through = nodes.new("ShaderNodeBsdfTranslucent")
        links.new(bsdf.inputs["Base Color"].links[0].from_socket, through.inputs["Color"]) if bsdf.inputs[
            "Base Color"
        ].links else None
        mix = nodes.new("ShaderNodeMixShader")
        mix.inputs["Fac"].default_value = spec["translucency"] * 0.6
        links.new(bsdf.outputs["BSDF"], mix.inputs[1])
        links.new(through.outputs["BSDF"], mix.inputs[2])
        if "cutout" in spec and bsdf.inputs["Alpha"].links:
            clear = nodes.new("ShaderNodeBsdfTransparent")
            cutmix = nodes.new("ShaderNodeMixShader")
            links.new(bsdf.inputs["Alpha"].links[0].from_socket, cutmix.inputs["Fac"])
            links.new(clear.outputs["BSDF"], cutmix.inputs[1])
            links.new(mix.outputs["Shader"], cutmix.inputs[2])
            links.new(cutmix.outputs["Shader"], out.inputs["Surface"])
        else:
            links.new(mix.outputs["Shader"], out.inputs["Surface"])
    if "glow" in spec:
        bsdf.inputs["Emission Color"].default_value = list(spec["glow"]) + [1.0]
        bsdf.inputs["Emission Strength"].default_value = 3.0
    return mat


def build_all(materials):
    """Every material the scene uses that the data describes; says which it does not."""
    missing = []
    for mat in bpy.data.materials:
        if mat.users == 0:
            continue
        if mat.name in materials:
            build(mat.name, materials[mat.name])
        else:
            missing.append(mat.name)
    return missing
