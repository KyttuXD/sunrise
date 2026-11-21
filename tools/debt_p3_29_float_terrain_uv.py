#!/usr/bin/env python3
"""Widen terrain arena slots for two float UV components (debt p3-29).

Run on the pre-migration tree. Compact records keep their six payload words;
only their arena stride changes. Existing producers keep their sampled UVs.
"""

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1] / "crates/sunrise-client-render/src"


def rewrite(name, edits):
    path = ROOT / name
    text = path.read_text()
    for old, new in edits:
        assert old in text, (name, old)
        text = text.replace(old, new)
    path.write_text(text)


rewrite("mesher.rs", [
    ("UV0, UV2) with the sprite's array layer beside the light, in 24 bytes\n"
     "/// rather than vanilla's 28: the UV is fixed point, which holds model UVs\n"
     "/// (multiples of 1/16 and their halves) exactly. The position stays a float:\n"
     "/// fixed point would move the edges of offset plants and rotated elements by\n"
     "/// a fraction of a pixel. Terrain has no normal: `ModelBlockRenderer` bakes",
     "UV0, UV2), with the sprite's array layer beside the light. Terrain has no\n"
     "/// normal: `ModelBlockRenderer` bakes"),
    ("    /// UV within the sprite, `UV_STEPS` per unit (`TerrainVertex::new`,\n"
     "    /// `terrain.wgsl`'s `terrain_uv`).\n"
     "    pub(crate) uv: [u16; 2],", "    /// UV in sprite units.\n    pub(crate) uv: [f32; 2],"),
    ("uv: uv.map(|v| round_to_u16(v * UV_STEPS)),", "uv: uv.map(|v| f32::from(round_to_u16(v * UV_STEPS)) / UV_STEPS),"),
    ("            uv,\n            layer,\n            light,\n            color,\n        }\n    }\n    /// A vertex of a merged", "            uv: uv.map(|v| f32::from(v) / UV_STEPS),\n            layer,\n            light,\n            color,\n        }\n    }\n    /// A vertex of a merged"),
    ("    /// face's corner. Drawn with an instance UV scale of `1 / TILED_UV_STEPS`.", "    /// face's corner."),
    ("uv: uv.map(|v| round_to_u16(v * TILED_UV_STEPS)),", "uv: uv.map(|v| f32::from(round_to_u16(v * TILED_UV_STEPS)) / TILED_UV_STEPS),"),
    ("self.uv.map(|v| f32::from(v) / UV_STEPS)", "self.uv"),
    ("the native 24-byte vertex format", "the native vertex format"),
    ("pub packed: Vec<[u32; 6]>", "pub packed: Vec<packed::ArenaSlot>"),
    ("size_of::<[u32; 6]>()", "size_of::<packed::ArenaSlot>()"),
])
rewrite("mesher/packed.rs", [
    ("one 24-byte record each instead of\n//! four 24-byte vertices", "one arena slot each instead of\n//! four vertices"),
    ("let raw = u32::from(v.uv[k]);\n            if raw & 1023 != 0 {", "let steps = v.uv[k] * 32.0;\n            let raw = steps as u32;\n            if raw > 63 || steps != raw as f32 {"),
    ("uv[k] = raw >> 10;", "uv[k] = raw;"),
    (".map(|k| (k << 10) as u16)", ".map(|k| k as f32 / 32.0)"),
    ("/// The compact layout of sorted solid quads", "/// An arena slot is one vertex or a six-word compact record plus padding.\npub type ArenaSlot = [u32; 7];\n\n/// The compact layout of sorted solid quads"),
    (" -> Vec<[u32; 6]>", " -> Vec<ArenaSlot>"),
    ("out: &mut Vec<[u32; 6]>", "out: &mut Vec<ArenaSlot>"),
    ("out.push(record.unwrap_or_else(|| {", "let [a, b, c, d, e, f] = record.unwrap_or_else(|| {"),
    ("[(at - i) as u32, 0, 0, 0, 0, FULL]\n        }));", "[(at - i) as u32, 0, 0, 0, 0, FULL]\n        });\n        out.push([a, b, c, d, e, f, 0]);"),
    ("out.resize(n + pad, [0; 6])", "out.resize(n + pad, [0; 7])"),
    ("bytemuck::cast::<TerrainVertex, [u32; 6]>", "bytemuck::cast::<TerrainVertex, ArenaSlot>"),
    ("packed_quads(layout: &[[u32; 6]]", "packed_quads(layout: &[ArenaSlot]"),
    ("let uv = [[0, 0], [16 << 10, 0], [16 << 10, 16 << 10], [0, 16 << 10]];", "let uv = [[0., 0.], [0.5, 0.], [0.5, 0.5], [0., 0.5]];"),
    ("uv[2].uv[0] += 1;", "uv[2].uv[0] += 1.0 / super::UV_STEPS;"),
    ("bytemuck::cast::<[u32; 6], TerrainVertex>", "bytemuck::cast::<ArenaSlot, TerrainVertex>"),
])
rewrite("lod/packed.rs", [
    ("one 24-byte\n//! record each instead of four 24-byte vertices", "one arena slot\n//! each instead of four vertices"),
    ("[[u32; 6]; 4]", "[packed::ArenaSlot; 4]"),
    (" -> Vec<[u32; 6]>", " -> Vec<packed::ArenaSlot>"),
    ("let face = Face::from_words(*record)", "let [a, b, c, d, e, f, _] = *record;\n            let face = Face::from_words([a, b, c, d, e, f])"),
])
rewrite("lod/mesh.rs", [
    ("v.uv.map(|u| f32::from(u) / crate::mesher::TILED_UV_STEPS)", "v.uv()"),
    ("[[u32; 6]; 4]", "[crate::mesher::packed::ArenaSlot; 4]"),
])
rewrite("gfx.rs", [("0=>Float32x3,1=>Uint16x2,2=>Uint16", "0=>Float32x3,1=>Float32x2,2=>Uint16")])
rewrite("gfx/tests.rs", [("size_of::<TerrainVertex>(), 24", "size_of::<TerrainVertex>(), 28")])
rewrite("gfx/pack_atlas/midpoint_test.rs", [
    ("let uv = [[0, 0], [16 << 10, 0], [16 << 10, 16 << 10], [0, 16 << 10]];", "let uv = [[0., 0.], [0.5, 0.], [0.5, 0.5], [0., 0.5]];"),
    ("Vec<[u32; 6]>", "Vec<packed::ArenaSlot>"),
    ("bytemuck::cast::<_, [u32; 6]>", "bytemuck::cast::<_, packed::ArenaSlot>"),
])
rewrite("gfx/displace.rs", [
    ("as `TerrainVertex` holds it (24 bytes)", "as `TerrainVertex` holds it"),
    ("quad: u32, i: u32, step: f32) -> SunriseDisplaceCorner", "quad: u32, i: u32) -> SunriseDisplaceCorner"),
    ("(quad * 4u + i) * 6u", "(quad * 4u + i) * 7u"),
    ("    let uv = sunrise_displace_vertices[b + 3u];\n"
     "    c.uv = vec2<f32>(vec2(uv & 0xffffu, uv >> 16u)) * step;\n"
     "    let w = sunrise_displace_vertices[b + 4u];", "    c.uv = vec2(bitcast<f32>(sunrise_displace_vertices[b + 3u]),\n"
     "        bitcast<f32>(sunrise_displace_vertices[b + 4u]));\n"
     "    let w = sunrise_displace_vertices[b + 5u];"),
    ("unpack4x8unorm(sunrise_displace_vertices[b + 5u])", "unpack4x8unorm(sunrise_displace_vertices[b + 6u])"),
    *[(f"sunrise_displace_corner(pick.x, {i}u, section.origin.w)", f"sunrise_displace_corner(pick.x, {i}u)") for i in range(4)],
    ("vertices[quad * 24u + 4u]", "vertices[quad * 28u + 5u]"),
    ("[((dx * 16) << 10) as u16, ((dz * 16) << 10) as u16]", "[dx as f32 * 0.5, dz as f32 * 0.5]"),
    ("uv: [0; 2]", "uv: [0.; 2]"),
    ("words: &[[u32; 6]]", "words: &[packed::ArenaSlot]"),
    ("bytemuck::cast::<TerrainVertex, [u32; 6]>", "bytemuck::cast::<TerrainVertex, packed::ArenaSlot>"),
])

for name in ["gfx/native_material.rs", "gfx/native_shadow.rs", "terrain.wgsl"]:
    path = ROOT / name
    source = path.read_text()
    source = source.replace("@location(1) uv: vec2<u32>", "@location(1) uv: vec2<f32>")
    source = re.sub(r"(?:sunrise_terrain_)?terrain_uv\(((?:c\.|i\.)?uv), origin\.w\)", r"\1", source)
    path.write_text(source)
rewrite("terrain.wgsl", [
    (
        "// tile's drew over it, two waters, opaque. A tile's UV step is 1/32, a\n// section's 1/32768 (`terrain_uv`).",
        "// tile's drew over it, two waters, opaque."),
    ("// mesher.rs's TerrainVertex holds the UV in fixed point; the instance's\n"
     "// origin.w is the step: 1/32768 per unit for sections (a power of two on an\n"
     "// integer below 2^24, so it decodes exactly), 1/32 per block for distant\n"
     "// terrain tiles (TerrainVertex::tiled). A fading tile negates it past its\n"
     "// dither code (gfx/lod.rs's `instance`): the step is the fraction either way.\n"
     "fn terrain_uv(packed: vec2<u32>, w: f32) -> vec2<f32> {\n"
     "    return vec2<f32>(packed) * instance_step(w);\n}\n", ""),
    ("(`TerrainVertex`, six words, as `TERRAIN_ATTRIBUTES` reads them)", "(`TerrainVertex`, as `TERRAIN_ATTRIBUTES` reads it)"),
    ("Either way they come out as the six words of the vertex, so the work after is the same.",
     "Both decode to the same vertex fields before shading."),
    ("arena_page: array<vec2<u32>>;", "arena_page: array<u32>;"),
    ("// `TerrainVertex::tiled`: a far tile's UVs are in blocks from the face's corner, this many\n"
     "// fixed-point steps each (`mesher::TILED_UV_STEPS`).\nconst TILED_UV_STEPS: f32 = 32.0;\n", ""),
    ("    r: vec3<u32>,\n}\nfn arena_vertex(v: u32) -> ArenaCorner {\n"
     "    let a = arena_page[v * 3u];\n    let b = arena_page[v * 3u + 1u];\n"
     "    let c = arena_page[v * 3u + 2u];\n"
     "    return ArenaCorner(vec3(a.x, a.y, b.x), vec3(b.y, c.x, c.y));",
     "    r: vec4<u32>,\n}\nfn arena_vertex(v: u32) -> ArenaCorner {\n"
     "    let b = v * 7u;\n"
     "    return ArenaCorner(vec3(arena_page[b], arena_page[b + 1u], arena_page[b + 2u]),\n"
     "        vec4(arena_page[b + 3u], arena_page[b + 4u], arena_page[b + 5u], arena_page[b + 6u]));"),
    ("vec3((u << 10u) | (v << 26u), (hi.x & 0xffffu) | (packed_light << 16u), color)",
     "vec4(bitcast<u32>(f32(u) / 32.0), bitcast<u32>(f32(v) / 32.0),\n"
     "            (hi.x & 0xffffu) | (packed_light << 16u), color)"),
    ("let uv = vec2<u32>(vec2(far_u - u0, v1 - far_v) * TILED_UV_STEPS);", "let uv = vec2(far_u - u0, v1 - far_v);"),
    ("vec3(uv.x | (uv.y << 16u), lo.y, lo.x)", "vec4(bitcast<vec2<u32>>(uv), lo.y, lo.x)"),
    ("        let a = arena_page[far_slot * 3u];\n"
     "        let b = arena_page[far_slot * 3u + 1u];\n"
     "        let c = arena_page[far_slot * 3u + 2u];", "        let r = far_slot * 7u;\n"
     "        let lo = vec3(arena_page[r], arena_page[r + 1u], arena_page[r + 2u]);\n"
     "        let hi = vec3(arena_page[r + 3u], arena_page[r + 4u], arena_page[r + 5u]);"),
    ("    let a = arena_page[slot * 3u];\n    let b = arena_page[slot * 3u + 1u];\n"
     "    let c = arena_page[slot * 3u + 2u];", "    let r = slot * 7u;\n"
     "    let lo = vec3(arena_page[r], arena_page[r + 1u], arena_page[r + 2u]);\n"
     "    let hi = vec3(arena_page[r + 3u], arena_page[r + 4u], arena_page[r + 5u]);"),
    ("(c.y & 0x80000000u)", "(hi.z & 0x80000000u)"),
    ("far_slot + a.x", "far_slot + lo.x"),
    ("slot + a.x", "slot + lo.x"),
    ("vec3(a.x, a.y, b.x), vec3(b.y, c.x, c.y)", "lo, hi"),
    ("return c.r.y & 0xffffu;", "return c.r.z & 0xffffu;"),
    ("    uv: vec2<u32>,", "    uv: vec2<f32>,"),
    ("vec2(c.r.x & 0xffffu, c.r.x >> 16u),\n        corner_layer(c), unorm8x4(c.r.z), vec2((c.r.y >> 16u) & 0xffu, c.r.y >> 24u)",
     "bitcast<vec2<f32>>(c.r.xy),\n        corner_layer(c), unorm8x4(c.r.w), vec2((c.r.z >> 16u) & 0xffu, c.r.z >> 24u)"),
])

rewrite("visbuffer.wgsl", [
    ("/ 24 bytes =\n// 11 184 810 slots", "/ 28 bytes"),
    ("let uv = vec2(s.c.r.x & 0xffffu, s.c.r.x >> 16u);", "let uv = bitcast<vec2<f32>>(s.c.r.xy);"),
    ("vec2((s.c.r.y >> 16u) & 0xffu, s.c.r.y >> 24u)", "vec2((s.c.r.z >> 16u) & 0xffu, s.c.r.z >> 24u)"),
    ("out.uv = terrain_uv(uv, origin.w);", "out.uv = uv;"),
    ("TerrainVertex, 24 bytes: position, UV as two u16,\n// layer and light as u16 each, RGBA8 colour), a quad's 96 bytes as six vec4s; at bindings 0 and",
     "TerrainVertex: position and UV as floats,\n// layer and light as u16 each, RGBA8 colour), a quad as seven vec4s; at bindings 0 and"),
    ("// A quad's 96 bytes, six 16-byte words", "// A quad's seven vec4s"),
    ("q: array<vec4<u32>, 6>", "q: array<vec4<u32>, 7>"),
    *[(f"vis_page_{i}[i + 5u]));", f"vis_page_{i}[i + 5u], vis_page_{i}[i + 6u]));") for i in range(16)],
    ("// A corner's `terrain_vertex` from its six words: position, UV, layer and light, colour (the\n"
     "// vertex attributes `TERRAIN_ATTRIBUTES` in gfx.rs give: Float32x3, Uint16x2, Uint16, Uint8x2\n"
     "// with sky in the low byte, Unorm8x4).\n"
     "fn vis_corner(position: vec3<u32>, rest: vec3<u32>, origin: vec4<f32>) -> TerrainOut {\n"
     "    let uv = vec2(rest.x & 0xffffu, rest.x >> 16u);\n"
     "    let light = vec2((rest.y >> 16u) & 0xffu, rest.y >> 24u);\n"
     "    return terrain_vertex(bitcast<vec3<f32>>(position), terrain_uv(uv, origin.w),\n"
     "        rest.y & 0xffffu, unpack4x8unorm(rest.z), light, origin.xyz, origin.w);\n}",
     "fn vis_word(words: VisQuad, offset: u32) -> u32 {\n"
     "    return words.q[offset >> 2u][offset & 3u];\n}\n\n"
     "fn vis_corner(position: vec3<u32>, rest: vec4<u32>, origin: vec4<f32>) -> TerrainOut {\n"
     "    let uv = bitcast<vec2<f32>>(rest.xy);\n"
     "    let light = vec2((rest.z >> 16u) & 0xffu, rest.z >> 24u);\n"
     "    return terrain_vertex(bitcast<vec3<f32>>(position), uv,\n"
     "        rest.z & 0xffffu, unpack4x8unorm(rest.w), light, origin.xyz, origin.w);\n}"),
    ("        // The record's six words from word 6 * index: two 16-byte words from the one it\n"
     "        // starts in, at its start or its middle.\n"
     "        let w = vis_quad(slot, (6u * index) >> 2u).q;\n"
     "        let middle = ((6u * index) & 3u) != 0u;\n"
     "        lo = select(w[0].xyz, vec3(w[0].zw, w[1].x), middle);\n"
     "        hi = select(vec3(w[0].w, w[1].xy), w[1].yzw, middle);",
     "        // Six record words at the start of a seven-word arena slot.\n"
     "        let word = 7u * index;\n"
     "        let w = vis_quad(slot, word >> 2u);\n"
     "        let at = word & 3u;\n"
     "        lo = vec3(vis_word(w, at), vis_word(w, at + 1u), vis_word(w, at + 2u));\n"
     "        hi = vec3(vis_word(w, at + 3u), vis_word(w, at + 4u), vis_word(w, at + 5u));"),
    *[(f"var r{i}: vec3<u32>;", f"var r{i}: vec4<u32>;") for i in range(4)],
    ("        // Corner k is words 6k to 6k + 5 of the quad's 24.\n"
     "        let words = vis_quad(slot, 6u * (first >> 2u)).q;\n"
     "        let q0 = words[0];\n        let q1 = words[1];\n        let q2 = words[2];\n"
     "        let q3 = words[3];\n        let q4 = words[4];\n        let q5 = words[5];\n"
     "        p0 = q0.xyz;\n        r0 = vec3(q0.w, q1.x, q1.y);\n"
     "        p1 = vec3(q1.z, q1.w, q2.x);\n        r1 = q2.yzw;\n"
     "        p2 = q3.xyz;\n        r2 = vec3(q3.w, q4.x, q4.y);\n"
     "        p3 = vec3(q4.z, q4.w, q5.x);\n        r3 = q5.yzw;",
     "        // Corner k occupies seven words; the quad starts at a vec4 boundary.\n"
     "        let q = vis_quad(slot, 7u * (first >> 2u)).q;\n"
     "        p0 = q[0].xyz;\n        r0 = vec4(q[0].w, q[1].xyz);\n"
     "        p1 = vec3(q[1].w, q[2].xy);\n        r1 = vec4(q[2].zw, q[3].xy);\n"
     "        p2 = vec3(q[3].zw, q[4].x);\n        r2 = vec4(q[4].yzw, q[5].x);\n"
     "        p3 = q[5].yzw;\n        r3 = q[6];"),
])

rewrite("gfx/native_material.rs", [
    ("c.uv.x, c.uv.y, c.layer", "bitcast<u32>(c.uv.x), bitcast<u32>(c.uv.y), c.layer"),
    ("v.uv.map(u32::from)", "v.uv.map(f32::to_bits)"),
])

rewrite("gfx/pack_atlas.rs", [
    ("out uint hz_uv)", "out vec2 hz_uv)"),
    ("hz_av * 6u", "hz_av * 7u"),
    ("hz_uv = hz_vertices[hz_w + 3u];", "hz_uv = uintBitsToFloat(uvec2(hz_vertices[hz_w + 3u], hz_vertices[hz_w + 4u]));"),
    ("hz_uv = (hz_u << 10u) | (hz_v << 26u);", "hz_uv = vec2(hz_u, hz_v) / 32.0;"),
    ("hz_slot * 6u", "hz_slot * 7u"),
    ("uint hz_uv0; uint hz_uv1; uint hz_uv2; uint hz_uv3;", "vec2 hz_uv0; vec2 hz_uv1; vec2 hz_uv2; vec2 hz_uv3;"),
    ("(hz_av + hz_own) * 6u", "(hz_av + hz_own) * 7u"),
    ("uvec2(hz_vertices[hz_ow + 4u], hz_vertices[hz_ow + 5u])", "uvec2(hz_vertices[hz_ow + 5u], hz_vertices[hz_ow + 6u])"),
    ("uvec3 hz_vr = uvec3(hz_own == 0u ? hz_uv0 : (hz_own == 1u ? hz_uv1 : (hz_own == 2u ? hz_uv2 : hz_uv3)), hz_rest);",
     "hz_v_uv = hz_own == 0u ? hz_uv0 : (hz_own == 1u ? hz_uv1 : (hz_own == 2u ? hz_uv2 : hz_uv3));"),
    ("    hz_v_uv = uvec2(hz_vr.x & 65535u, hz_vr.x >> 16u);\\n    \\\n", ""),
    ("hz_vr.y", "hz_rest.x"), ("hz_vr.z", "hz_rest.y"),
    ("    uvec3 hz_w = uvec3(hz_uv0, hz_uv1, hz_uv2);\\n    \\\n"
     "    vec2 hz_t0 = vec2(hz_w.x & 65535u, hz_w.x >> 16u);\\n    \\\n"
     "    vec2 hz_d1 = vec2(hz_w.y & 65535u, hz_w.y >> 16u) - hz_t0;\\n    \\\n"
     "    vec2 hz_d2 = vec2(hz_w.z & 65535u, hz_w.z >> 16u) - hz_t0;\\n    \\\n"
     "    uint hz_w3 = hz_uv3;\\n    \\\n"
     "    vec2 hz_t3 = vec2(hz_w3 & 65535u, hz_w3 >> 16u);\\n    \\\n"
     "    hz_mid = (clamp(hz_t0 * hz_uv_scale, 0.0, 1.0) + clamp((hz_t0 + hz_d1) * hz_uv_scale, 0.0, 1.0)\\n        \\\n"
     "        + clamp((hz_t0 + hz_d2) * hz_uv_scale, 0.0, 1.0) + clamp(hz_t3 * hz_uv_scale, 0.0, 1.0)) * 0.25;",
     "    vec2 hz_d1 = (hz_uv1 - hz_uv0) / hz_uv_scale;\\n    \\\n"
     "    vec2 hz_d2 = (hz_uv2 - hz_uv0) / hz_uv_scale;\\n    \\\n"
     "    hz_mid = (clamp(hz_uv0, 0.0, 1.0) + clamp(hz_uv1, 0.0, 1.0)\\n        \\\n"
     "        + clamp(hz_uv2, 0.0, 1.0) + clamp(hz_uv3, 0.0, 1.0)) * 0.25;"),
    ("uvec2 hz_v_uv", "vec2 hz_v_uv"),
    ("clamp(vec2(hz_v_uv) * hz_uv_scale, 0.0, 1.0)", "clamp(hz_v_uv, 0.0, 1.0)"),
])
rewrite("gfx/pack_atlas/source_ownership_test.rs", [("uvec2 hz_v_uv=uvec2(0)", "vec2 hz_v_uv=vec2(0)")])
rewrite("gfx/arena/source_tests.rs", [
    ("    let mut arena = Arena::new(&device, 24, GRAIN, true);",
     "    let stride = size_of::<TerrainVertex>() as u64;\n    let mut arena = Arena::new(&device, stride, GRAIN, true);"),
    ("Arena::translucent(&device, 24, GRAIN)", "Arena::translucent(&device, stride, GRAIN)"),
    (".1 * 24", ".1 * stride"),
    (", 96),", ", 4 * stride),"),
])
