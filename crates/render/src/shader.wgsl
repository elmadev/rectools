struct View {
    // Bottom left corner of the view in world meters, exact and snapped to pixels
    corner: vec2<f32>,
    level_corner: vec2<f32>,
    // The level's texture origin, snapped to the middle of a pixel at this zoom
    canvas_origin: vec2<f32>,
    // Size of the view and its top left corner in the render target, in pixels
    size: vec2<f32>,
    offset: vec2<f32>,
    pixels_per_meter: f32,
    // Screen pixels per texture pixel
    texture_scale: f32,
    // Horizontal sky offset in screen pixels
    sky_x: f32,
    // Rows of qgrass under a front picture that move in front of the bikes
    front_grass_rows: f32,
    // 1 to tile textures from the canvas origin like the game does at each zoom, 0 to keep
    // each tile where it is at zoom 1
    game_textures: u32,
    // In the background slot, which the present pass reads: 1 when the frame draws translucent
    // ghosts into their own layer, which is otherwise left out
    translucent_ghosts: u32,
}

@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var atlas: texture_2d_array<u32>;
@group(0) @binding(2) var timer_map: texture_2d<u32>;
@group(0) @binding(3) var palette: texture_2d<f32>;

const MODE_IMAGE: u32 = 0u;
const MODE_MASKED_TEXTURE: u32 = 1u;
const MODE_RECOLOR: u32 = 3u;
const MODE_TEXTURE: u32 = 4u;
const MODE_SKY: u32 = 5u;
const MODE_AFFINE: u32 = 6u;

struct InstanceIn {
    @location(0) origin: vec2<f32>,
    @location(1) axis_u: vec2<f32>,
    @location(2) axis_v: vec2<f32>,
    @location(3) depth: f32,
    @location(4) snap: u32,
    @location(5) image: vec4<u32>,
    @location(6) texture: vec4<u32>,
    @location(7) texture_size: vec2<f32>,
    @location(8) mode: u32,
    @location(9) transparency: u32,
    @location(10) color: u32,
    @location(11) layer: u32,
    // image | texture << 8
    @location(12) atlas_layers: u32,
}

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) st: vec2<f32>,
    @location(1) world: vec2<f32>,
    @location(2) @interpolate(flat) image: vec4<u32>,
    @location(3) @interpolate(flat) texture: vec4<u32>,
    @location(4) @interpolate(flat) texture_size: vec2<f32>,
    @location(5) @interpolate(flat) params: vec4<u32>,
    // The atlas layers of image and texture
    @location(6) @interpolate(flat) atlas_layers: vec2<u32>,
    // The quad's corner and sides in pixels from the view's corner, for affine pictures
    @location(7) @interpolate(flat) quad_origin: vec2<f32>,
    @location(8) @interpolate(flat) quad_u: vec2<f32>,
    @location(9) @interpolate(flat) quad_v: vec2<f32>,
}

fn to_clip(world: vec2<f32>, corner: vec2<f32>, depth: f32) -> vec4<f32> {
    let pixels = (world - corner) * view.pixels_per_meter;
    let clip = pixels / view.size * 2.0 - vec2<f32>(1.0, 1.0);
    return vec4<f32>(clip, depth, 1.0);
}

@vertex
fn vs_instance(@builtin(vertex_index) vertex: u32, instance: InstanceIn) -> VertexOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let st = corners[vertex];
    let world = instance.origin + instance.axis_u * st.x + instance.axis_v * st.y;
    var out: VertexOut;
    if (instance.snap == 1u) {
        // The level's pictures can have their edges on pixel centers, and where two meet,
        // such as grass pieces, each works its edge out a little differently. Rounded so they
        // meet exactly, and moved off the centers like the polygons, so that rounding doesn't
        // change from frame to frame which pixels they cover, or leave gaps between them.
        let pixels = round((world - view.level_corner) * view.pixels_per_meter * 64.0) / 64.0;
        let nudged = pixels + vec2<f32>(-0.01, 0.01);
        out.position = vec4<f32>(nudged / view.size * 2.0 - vec2<f32>(1.0, 1.0), instance.depth, 1.0);
    } else {
        // The game samples bikes at pixel corners rather than centers
        let corner = view.corner - vec2<f32>(0.5, 0.5) / view.pixels_per_meter;
        out.position = to_clip(world, corner, instance.depth);
    }
    out.st = st;
    out.world = world;
    out.image = instance.image;
    out.texture = instance.texture;
    out.texture_size = instance.texture_size;
    out.params = vec4<u32>(instance.mode, instance.transparency, instance.color, instance.layer);
    out.atlas_layers = vec2<u32>(instance.atlas_layers & 0xffu, instance.atlas_layers >> 8u);
    out.quad_origin = (instance.origin - view.corner) * view.pixels_per_meter;
    out.quad_u = instance.axis_u * view.pixels_per_meter;
    out.quad_v = instance.axis_v * view.pixels_per_meter;
    return out;
}

// An image's place in the atlas is its rect, x, y, width and height, and its layer
fn atlas_texel(rect: vec4<u32>, layer: u32, x: u32, y: u32) -> u32 {
    return textureLoad(atlas, vec2<u32>(rect.x + x, rect.y + y), layer, 0).r;
}

// The texel of `rect` at (s, t) in 0..1
fn image_texel(rect: vec4<u32>, layer: u32, st: vec2<f32>) -> u32 {
    let size = rect.zw;
    let x = min(u32(max(floor(st.x * f32(size.x)), 0.0)), size.x - 1u);
    let y = min(u32(max(floor(st.y * f32(size.y)), 0.0)), size.y - 1u);
    return atlas_texel(rect, layer, x, y);
}

fn positive_mod(a: f32, n: f32) -> f32 {
    return a - n * floor(a / n);
}

// A texture pixel at game pixel (gx, gy), counted from its bottom left corner. Textures are
// stored bottom row first.
fn texture_texel(rect: vec4<u32>, layer: u32, game_size: vec2<f32>, gx: f32, gy: f32) -> u32 {
    let size = rect.zw;
    let x = positive_mod(gx, game_size.x);
    let y = positive_mod(gy, game_size.y);
    let sx = min(u32(x * f32(size.x) / game_size.x), size.x - 1u);
    let sy = min(u32(y * f32(size.y) / game_size.y), size.y - 1u);
    return atlas_texel(rect, layer, sx, sy);
}

fn positive_mod_i(a: i32, n: i32) -> i32 {
    return ((a % n) + n) % n;
}

// The game resizes textures for the zoom (before flipping them) and tiles the result. Maps a
// pixel of the zoomed, tiled texture back to the texture. In integers: the GPU's division
// isn't exact, and 972.0 / 486.0 coming out just under 2 makes a 1008 wide texture wrap at
// 2015 instead of 2016.
fn zoomed_texel(rect: vec4<u32>, layer: u32, game_size: vec2<f32>, zx: f32, zy: f32) -> u32 {
    if (view.texture_scale == 1.0) {
        return texture_texel(rect, layer, game_size, zx, zy);
    }
    let w = i32(game_size.x);
    let h = i32(game_size.y);
    let height = max(i32(floor(game_size.y * view.texture_scale)), 1);
    let width = max(w * height / h, 1);
    let x = positive_mod_i(i32(zx), width);
    let y = positive_mod_i(i32(zy), height);
    let gx = min(x * h / height, w - 1);
    let gy = h - 1 - min((height - 1 - y) * h / height, h - 1);
    return texture_texel(rect, layer, game_size, f32(gx), f32(gy));
}

// Where tile `k` of a texture `size` pixels long starts, in pixels at this zoom
fn tile_start(k: i32, size: i32) -> i32 {
    return i32(floor(f32(k * size) * view.texture_scale));
}

// The tile of a texture `size` pixels long that pixel `p` is in, as its start and length
fn locked_tile(p: i32, size: i32) -> vec2<i32> {
    let k = i32(floor(f32(p) / (f32(size) * view.texture_scale)));
    var start = tile_start(k, size);
    var end = tile_start(k + 1, size);
    // The division can be a tile off from where tile_start puts the tiles
    if (start > p) {
        end = start;
        start = tile_start(k - 1, size);
    } else if (end <= p) {
        start = end;
        end = tile_start(k + 2, size);
    }
    return vec2<i32>(start, max(end - start, 1));
}

// Like zoomed_texel, but each tile is where the texture is at zoom 1, to the nearest pixel, so
// that textures stay on the level as the zoom changes. Each is resized to its own whole number
// of pixels, which is the game's size and place for every tile when the zoom resizes the
// texture to whole pixels.
fn locked_texel(rect: vec4<u32>, layer: u32, game_size: vec2<f32>, zx: f32, zy: f32) -> u32 {
    if (view.texture_scale == 1.0) {
        return texture_texel(rect, layer, game_size, zx, zy);
    }
    let w = i32(game_size.x);
    let h = i32(game_size.y);
    let column = locked_tile(i32(zx), w);
    let row = locked_tile(i32(zy), h);
    let x = i32(zx) - column.x;
    let y = i32(zy) - row.x;
    let gx = min(x * w / column.y, w - 1);
    let gy = h - 1 - min((row.y - 1 - y) * h / row.y, h - 1);
    return texture_texel(rect, layer, game_size, f32(gx), f32(gy));
}

fn resized_texel(rect: vec4<u32>, layer: u32, game_size: vec2<f32>, zx: f32, zy: f32) -> u32 {
    if (view.game_textures != 0u) {
        return zoomed_texel(rect, layer, game_size, zx, zy);
    }
    return locked_texel(rect, layer, game_size, zx, zy);
}

fn world_texture(in: VertexOut) -> u32 {
    let p = floor((in.world - view.canvas_origin) * view.pixels_per_meter);
    return resized_texel(in.texture, in.atlas_layers.y, in.texture_size, p.x, p.y);
}

fn sky_texture(in: VertexOut) -> u32 {
    let x = floor(in.position.x - view.offset.x);
    let y = floor(view.offset.y + view.size.y - in.position.y);
    // Resized and tiled like the ground, with the screen's rows rather than the level's
    return resized_texel(in.texture, in.atlas_layers.y, in.texture_size, x + view.sky_x, y);
}

// Where in its quad an affine picture is at this pixel's corner, worked out from the pixel
// rather than interpolated: the GPU snaps the quad's corners to a fraction of a pixel, and
// shrunk 5 to 1, as the bike's frame is, that picks a neighboring texel here and there.
fn affine_st(in: VertexOut) -> vec2<f32> {
    let p = vec2<f32>(
        floor(in.position.x - view.offset.x),
        floor(view.offset.y + view.size.y - in.position.y),
    );
    let d = p - in.quad_origin;
    let u = in.quad_u;
    let v = in.quad_v;
    let det = u.x * v.y - v.x * u.y;
    return vec2<f32>(d.x * v.y - v.x * d.y, u.x * d.y - d.x * u.y) / det;
}

fn shade(in: VertexOut) -> u32 {
    let mode = in.params.x;
    // Pictures, masks and recolored pictures skip their transparent pixels
    if (mode == MODE_IMAGE || mode == MODE_MASKED_TEXTURE || mode == MODE_RECOLOR) {
        let c = image_texel(in.image, in.atlas_layers.x, in.st);
        if (c == in.params.y) {
            discard;
        }
        if (mode == MODE_IMAGE) {
            return c;
        }
    }
    switch mode {
        case MODE_MASKED_TEXTURE, MODE_TEXTURE: {
            return world_texture(in);
        }
        case MODE_SKY: {
            return sky_texture(in);
        }
        default: {}
    }
    // Solid and recolored pictures are a single color
    return in.params.z;
}

// Only the bikes have affine pictures. Their shading stays out of `shade`: the level's pass
// covers every pixel, and with it there that pass took a sixth longer, though never using it.
fn shade_dynamic(in: VertexOut) -> u32 {
    if (in.params.x != MODE_AFFINE) {
        return shade(in);
    }
    // The game draws the pixels whose corner is within u and v, which reach from the middle
    // of the first texel to the middle of the last, so not the outer half texel
    let st = affine_st(in);
    let half = vec2<f32>(0.5, 0.5) / vec2<f32>(in.image.zw);
    if (any(st < half) || any(st > vec2<f32>(1.0, 1.0) - half)) {
        discard;
    }
    let c = image_texel(in.image, in.atlas_layers.x, st);
    if (c == in.params.y) {
        discard;
    }
    return c;
}

struct LevelOut {
    @location(0) index: u32,
    @location(1) layer: u32,
}

@fragment
fn fs_level(in: VertexOut) -> LevelOut {
    var out: LevelOut;
    out.index = shade(in);
    out.layer = in.params.w;
    return out;
}

struct DynamicOut {
    @location(0) index: u32,
    // Clears any translucent ghost under the bike
    @location(1) ghost: u32,
}

@fragment
fn fs_dynamic(in: VertexOut) -> DynamicOut {
    return DynamicOut(shade_dynamic(in), 0u);
}

// A translucent ghost goes into its own layer as palette index + 1
@fragment
fn fs_ghost(in: VertexOut) -> DynamicOut {
    return DynamicOut(0u, shade_dynamic(in) + 1u);
}

// Solid polygons, drawn into the stencil buffer only

@vertex
fn vs_polygon(@location(0) world: vec2<f32>) -> @builtin(position) vec4<f32> {
    // A pixel center exactly on an edge takes the state right of it, and a row exactly at
    // the lower end of an edge is skipped, like the game's scanline fill
    let nudge = vec2<f32>(-0.01, 0.01) / view.pixels_per_meter;
    return to_clip(world + nudge, view.level_corner, 0.0);
}

@fragment
fn fs_polygon() -> LevelOut {
    return LevelOut(0u, 0u);
}

// Fullscreen triangle

struct FullscreenOut {
    @builtin(position) position: vec4<f32>,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) vertex: u32) -> FullscreenOut {
    let uv = vec2<f32>(f32((vertex << 1u) & 2u), f32(vertex & 2u));
    var out: FullscreenOut;
    out.position = vec4<f32>(uv * 2.0 - vec2<f32>(1.0, 1.0), 0.0, 1.0);
    return out;
}

// Qgrass right under a front picture moves in front of the bikes

@group(1) @binding(0) var layers: texture_2d<u32>;

const LAYER_FRONT: u32 = 1u;
const LAYER_QGRASS: u32 = 2u;
const FRONT_GRASS_DEPTH: f32 = 223.0 / 2048.0;

@fragment
fn fs_front_grass(in: FullscreenOut) -> @builtin(frag_depth) f32 {
    let p = vec2<i32>(floor(in.position.xy));
    if (textureLoad(layers, p, 0).r != LAYER_QGRASS) {
        discard;
    }
    let top = i32(view.offset.y);
    let rows = i32(view.front_grass_rows);
    for (var k = 1; k <= rows; k++) {
        let above = p - vec2<i32>(0, k);
        if (above.y < top) {
            break;
        }
        let l = textureLoad(layers, above, 0).r;
        if (l == LAYER_FRONT) {
            return FRONT_GRASS_DEPTH;
        }
        if (l != LAYER_QGRASS) {
            break;
        }
    }
    discard;
    return 1.0;
}

// Timer and text: a timer pixel is mapped to the brightest or darkest color, text is drawn
// in its own color

@group(1) @binding(0) var scene_copy: texture_2d<u32>;

// HudRect's color for the timer
const TIMER_COLOR: u32 = 0xffffffffu;

struct HudOut {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) color: u32,
}

@vertex
fn vs_timer(
    @builtin(vertex_index) vertex: u32,
    @location(0) rect: vec4<f32>,
    @location(1) color: u32,
) -> HudOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    // rect is x, y, width, height in view pixels from the bottom left
    let pixels = rect.xy + corners[vertex] * rect.zw;
    var out: HudOut;
    out.position = vec4<f32>(pixels / view.size * 2.0 - vec2<f32>(1.0, 1.0), 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_timer(in: HudOut) -> @location(0) u32 {
    if in.color != TIMER_COLOR {
        return in.color;
    }
    let c = textureLoad(scene_copy, vec2<i32>(floor(in.position.xy)), 0).r;
    return textureLoad(timer_map, vec2<i32>(i32(c), 0), 0).r;
}

// Palette lookup into the output

@group(1) @binding(0) var indices: texture_2d<u32>;
@group(1) @binding(1) var ghosts: texture_2d<u32>;

// Set for sRGB outputs, which expect linear colors
override linearize: bool = false;
// Color and opacity of translucent ghosts
const GHOST_TINT = vec4<f32>(0.35, 0.6, 1.0, 0.55);

fn srgb_to_linear(c: vec3<f32>) -> vec3<f32> {
    let low = c / 12.92;
    let high = pow((c + 0.055) / 1.055, vec3<f32>(2.4));
    return select(high, low, c <= vec3<f32>(0.04045));
}

@fragment
fn fs_present(in: FullscreenOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(floor(in.position.xy));
    let c = textureLoad(indices, p, 0).r;
    var color = textureLoad(palette, vec2<i32>(i32(c), 0), 0).rgb;
    // Only translucent ghosts leave anything in this layer, which isn't kept without them
    var g = 0u;
    if (view.translucent_ghosts != 0u) {
        g = textureLoad(ghosts, p, 0).r;
    }
    if (g != 0u) {
        let ghost = textureLoad(palette, vec2<i32>(i32(g - 1u), 0), 0).rgb;
        // Keep the ghost's shading, in the tint's color
        let shade = dot(ghost, vec3<f32>(0.299, 0.587, 0.114));
        let tinted = GHOST_TINT.rgb * (0.35 + 0.9 * shade);
        color = mix(color, tinted, GHOST_TINT.a);
    }
    if (linearize) {
        color = srgb_to_linear(color);
    }
    return vec4<f32>(color, 1.0);
}
