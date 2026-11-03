"""Orthographic top-down rendering of world geometry on the GPU (moderngl, offscreen).

Geometry is added in world space (Unity: x east, y up, z north) with a material per
triangle (base color texture layer + tint + optional alpha clip). Rendering a region
yields the lit albedo color and the height (world y) of the topmost surface per pixel,
which map scripts composite with water, hillshade and so on.
"""
from __future__ import annotations

import numpy as np

TEX_SIZE = 128

VERTEX = """
#version 430
in vec3 in_pos;
in vec2 in_uv;
in float in_layer;
in vec4 in_tint;
in float in_clip;
uniform vec4 u_view;  // min east, min north, 1/width, 1/height
uniform vec2 u_height; // min, max y
out vec3 v_pos;
out vec2 v_uv;
flat out float v_layer;
flat out vec4 v_tint;
flat out float v_clip;
void main() {
    vec2 ndc = (in_pos.xz - u_view.xy) * u_view.zw * 2.0 - 1.0;
    float d = 1.0 - (in_pos.y - u_height.x) / (u_height.y - u_height.x) * 2.0;  // higher = nearer
    gl_Position = vec4(ndc.x, ndc.y, clamp(d, -1.0, 1.0), 1.0);
    v_pos = in_pos; v_uv = in_uv; v_layer = in_layer; v_tint = in_tint; v_clip = in_clip;
}
"""

FRAGMENT = """
#version 430
in vec3 v_pos;
in vec2 v_uv;
flat in float v_layer;
flat in vec4 v_tint;
flat in float v_clip;
#ifdef SINGLE_TEXTURE
uniform sampler2D u_tex;
#else
uniform sampler2DArray u_tex;
#endif
uniform vec3 u_sun;
layout(location = 0) out vec4 out_color;
layout(location = 1) out float out_height;
void main() {
    vec4 c = v_tint;
#ifdef SINGLE_TEXTURE
    c *= texture(u_tex, v_uv);
#else
    if (v_layer >= 0.0) c *= texture(u_tex, vec3(v_uv, v_layer));
#endif
    if (v_clip > 0.0 && c.a < v_clip) discard;
    vec3 n = normalize(cross(dFdx(v_pos), dFdy(v_pos)));
    if (n.y < 0.0) n = -n;  // seen from above; meshes may be mirrored or double-sided
    float light = 0.55 + 0.55 * max(dot(n, u_sun), 0.0);
    out_color = vec4(c.rgb * light, 1.0);
    out_height = v_pos.y;
}
"""


class TopDownRenderer:
    def __init__(self, height_range: tuple[float, float] = (-500.0, 500.0)):
        import moderngl

        self.ctx = moderngl.create_standalone_context(require=430)
        self.prog = self.ctx.program(vertex_shader=VERTEX, fragment_shader=FRAGMENT)
        single = FRAGMENT.replace("#version 430", "#version 430\n#define SINGLE_TEXTURE", 1)
        self.single_prog = self.ctx.program(vertex_shader=VERTEX, fragment_shader=single)
        self.height_range = height_range
        self.layers: list[np.ndarray] = []
        self.layer_index: dict[object, int] = {}
        self.chunks: list[np.ndarray] = []  # structured vertex arrays
        self.index_chunks: list[np.ndarray] = []
        self.vertex_total = 0
        self.textured: list[tuple[object, object]] = []  # (vao, texture) with their own image
        self.vao = None

    DTYPE = np.dtype([("pos", "f4", 3), ("uv", "f4", 2), ("layer", "f4"), ("tint", "f4", 4), ("clip", "f4")])

    def texture_layer(self, key, rgba: np.ndarray | None) -> int:
        """Registers a texture (TEX_SIZE square RGBA uint8) once per key; -1 if none."""
        if key in self.layer_index:
            return self.layer_index[key]
        if rgba is None:
            self.layer_index[key] = -1
            return -1
        self.layer_index[key] = len(self.layers)
        self.layers.append(rgba)
        return self.layer_index[key]

    def add(self, positions: np.ndarray, uv: np.ndarray | None, triangles: np.ndarray, layer: int, tint, clip: float | None):
        """Adds triangles (indices into `positions`, world space) with one material."""
        if len(triangles) == 0:
            return
        used, remapped = np.unique(triangles.reshape(-1), return_inverse=True)
        v = np.empty(len(used), dtype=self.DTYPE)
        v["pos"] = positions[used]
        v["uv"] = uv[used] if uv is not None else 0.0
        v["layer"] = layer
        v["tint"] = tint
        v["clip"] = clip or 0.0
        self.chunks.append(v)
        self.index_chunks.append(remapped.astype(np.uint32) + np.uint32(self.vertex_total))
        self.vertex_total += len(used)

    def add_textured(self, positions: np.ndarray, uv: np.ndarray, triangles: np.ndarray, rgba: np.ndarray):
        """Adds a mesh with its own (large) RGBA texture, e.g. a terrain's color map.
        Row 0 of `rgba` is v = 0."""
        idx = triangles.reshape(-1)
        v = np.empty(len(idx), dtype=self.DTYPE)
        v["pos"], v["uv"], v["layer"], v["tint"], v["clip"] = positions[idx], uv[idx], 0.0, 1.0, 0.0
        vbo = self.ctx.buffer(v.tobytes())
        # `in_layer` is unused (compiled out) with a single texture; skip its bytes.
        vao = self.ctx.vertex_array(self.single_prog, [(vbo, "3f 2f 4x 4f 1f", "in_pos", "in_uv", "in_tint", "in_clip")])
        h, w = rgba.shape[:2]
        tex = self.ctx.texture((w, h), 4, np.ascontiguousarray(rgba).tobytes())
        tex.build_mipmaps()
        self.textured.append((vao, tex))

    def build(self):
        verts = np.concatenate(self.chunks) if self.chunks else np.zeros(1, self.DTYPE)
        indices = np.concatenate(self.index_chunks) if self.index_chunks else np.zeros(3, np.uint32)
        self.chunks, self.index_chunks = [], []
        self.vertex_count, self.triangle_count = len(verts), len(indices) // 3
        vbo = self.ctx.buffer(verts.tobytes())
        ibo = self.ctx.buffer(indices.tobytes())
        del verts, indices
        self.vao = self.ctx.vertex_array(
            self.prog, [(vbo, "3f 2f 1f 4f 1f", "in_pos", "in_uv", "in_layer", "in_tint", "in_clip")], ibo, index_element_size=4
        )
        n = max(1, len(self.layers))
        tex = self.ctx.texture_array((TEX_SIZE, TEX_SIZE, n), 4, np.stack(self.layers).tobytes() if self.layers else bytes(TEX_SIZE * TEX_SIZE * 4))
        tex.build_mipmaps()
        tex.repeat_x = tex.repeat_y = True
        self.tex = tex
        sun = np.array([-0.45, 0.8, 0.4])
        for prog in (self.prog, self.single_prog):
            prog["u_sun"].value = tuple(sun / np.linalg.norm(sun))
            prog["u_height"].value = self.height_range

    def render(self, min_east: float, min_north: float, width: float, height: float, px: int, py: int):
        """Renders the world rectangle into (color RGB uint8 [py, px, 3] with row 0 at
        max north, height float32 [py, px] with NaN where nothing was drawn)."""
        ctx = self.ctx
        color = ctx.texture((px, py), 4)
        heights = ctx.texture((px, py), 1, dtype="f4")
        depth = ctx.depth_texture((px, py))
        fbo = ctx.framebuffer([color, heights], depth)
        fbo.use()
        fbo.clear(0.0, 0.0, 0.0, 0.0, depth=1.0)
        ctx.enable(ctx.DEPTH_TEST)
        ctx.disable(ctx.CULL_FACE)
        view = (min_east, min_north, 1.0 / width, 1.0 / height)
        self.single_prog["u_view"].value = view
        self.single_prog["u_tex"].value = 0
        for vao, tex in self.textured:
            tex.use(0)
            vao.render()
        self.tex.use(0)
        self.prog["u_tex"].value = 0
        self.prog["u_view"].value = view
        self.vao.render()
        rgba = np.frombuffer(color.read(), dtype=np.uint8).reshape(py, px, 4)[::-1]
        h = np.frombuffer(heights.read(), dtype=np.float32).reshape(py, px)[::-1].copy()
        h[rgba[..., 3] == 0] = np.nan
        for r in (fbo, color, heights, depth):
            r.release()
        return rgba[..., :3].copy(), h
