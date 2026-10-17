//! Decorative, game-themed animations painted behind the main menu.
//!
//! Every effect is a pure function of the elapsed time and a per-launch seed:
//! there is no per-frame state and no texture uploads. Only the Julia fractal
//! memoizes its two current keyframes. One effect is picked at random for each
//! application start; clicking the menu logo advances to the next one.

use std::{
    f32::consts::TAU,
    sync::{Arc, LazyLock},
};

use egui::{Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuEffect {
    Starfield,
    SynthGrid,
    HexRain,
    Invaders,
    Blocks,
    Julia,
    Sierpinski,
}

impl MenuEffect {
    pub const ALL: [Self; 7] = [
        Self::Starfield,
        Self::SynthGrid,
        Self::HexRain,
        Self::Invaders,
        Self::Blocks,
        Self::Julia,
        Self::Sierpinski,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuBackground {
    pub effect: MenuEffect,
    seed: u64,
}

impl Default for MenuBackground {
    fn default() -> Self {
        Self::random()
    }
}

impl MenuBackground {
    pub fn new(effect: MenuEffect, seed: u64) -> Self {
        Self { effect, seed }
    }

    pub fn from_seed(seed: u64) -> Self {
        let index = (mix(seed, u64::MAX) % MenuEffect::ALL.len() as u64) as usize;
        Self::new(MenuEffect::ALL[index], seed)
    }

    /// The next effect in the cycle, keeping the launch seed (logo Easter egg).
    pub fn next(&self) -> Self {
        let current = MenuEffect::ALL.iter().position(|effect| *effect == self.effect).unwrap_or(0);
        Self::new(MenuEffect::ALL[(current + 1) % MenuEffect::ALL.len()], self.seed)
    }

    /// A background for a second screen: same launch seed, never the same effect.
    pub fn alternate(&self) -> Self {
        let current = MenuEffect::ALL.iter().position(|effect| *effect == self.effect).unwrap_or(0);
        let others = MenuEffect::ALL.len() - 1;
        let offset = 1 + (mix(self.seed, 0xAB0) % others as u64) as usize;
        Self::new(MenuEffect::ALL[(current + offset) % MenuEffect::ALL.len()], mix(self.seed, 0xAB1))
    }

    pub fn random() -> Self {
        use std::hash::BuildHasher;
        let seed = std::collections::hash_map::RandomState::new().hash_one(std::time::SystemTime::now());
        Self::from_seed(seed)
    }

    /// Paint one frame of the effect into `rect` at `time` seconds.
    pub fn paint(&self, painter: &Painter, rect: Rect, time: f64) {
        // Wrap to keep f32 precision; the jump once per hour is invisible.
        let t = (time.rem_euclid(3600.0)) as f32;
        match self.effect {
            MenuEffect::Starfield => starfield(painter, rect, t, self.seed),
            MenuEffect::SynthGrid => synth_grid(painter, rect, t, self.seed),
            MenuEffect::HexRain => hex_rain(painter, rect, t, self.seed),
            MenuEffect::Invaders => invaders(painter, rect, t, self.seed),
            MenuEffect::Blocks => blocks(painter, rect, t, self.seed),
            MenuEffect::Julia => julia(painter, rect, t, self.seed),
            MenuEffect::Sierpinski => sierpinski(painter, rect, t, self.seed),
        }
    }
}

/// `SplitMix64` finalizer over a seed and an index: cheap, stateless noise.
fn mix(seed: u64, index: u64) -> u64 {
    let mut z = seed ^ index.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(0x6A09_E667_F3BC_C909);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

fn unit(seed: u64, index: u64) -> f32 {
    (mix(seed, index) >> 40) as f32 / 16_777_216.0
}

/// Triangle wave in `0..=1` with a period of 2.
fn tri(u: f32) -> f32 {
    let f = u.rem_euclid(2.0);
    if f < 1.0 { f } else { 2.0 - f }
}

fn fade(color: Color32, alpha: f32) -> Color32 {
    color.gamma_multiply(alpha.clamp(0.0, 1.0))
}

fn starfield(painter: &Painter, rect: Rect, t: f32, seed: u64) {
    let count = (rect.area() / 3600.0).clamp(90.0, 360.0) as u64;
    for i in 0..count {
        let depth = unit(seed, i * 6 + 2);
        let speed = 0.4 + depth * 1.8;
        let x = (unit(seed, i * 6) * rect.width() + t * speed).rem_euclid(rect.width());
        let y = (unit(seed, i * 6 + 1) * rect.height() + t * speed * 0.3).rem_euclid(rect.height());
        let center = rect.min + vec2(x, y);
        // Fade at the edges so wrapping never produces a visible jump.
        let edge = (x.min(rect.width() - x).min(y.min(rect.height() - y)) / 20.0).clamp(0.0, 1.0);
        let phase = unit(seed, i * 6 + 3) * TAU;
        let twinkle = 0.8 + 0.2 * (t * (0.6 + unit(seed, i * 6 + 4) * 0.7) + phase).sin();
        let brightness = (0.2 + depth * 0.55) * twinkle * edge;
        if brightness < 0.01 {
            continue;
        }
        let radius = 0.6 + depth * depth * 1.2;
        let tint = unit(seed, i * 6 + 5);
        let color = if tint < 0.2 {
            Color32::from_rgb(150, 205, 255)
        } else if tint > 0.9 {
            Color32::from_rgb(255, 224, 175)
        } else {
            Color32::from_rgb(225, 236, 255)
        };
        if depth > 0.7 {
            for (scale, opacity) in [(3.5, 0.035), (2.0, 0.09)] {
                let halo = fade(color, brightness * opacity);
                if halo.a() > 0 {
                    painter.circle_filled(center, radius * scale, halo);
                }
            }
        }
        if depth > 0.94 {
            // A few compact, tapered glints among the round stars.
            let ray = radius * (2.3 + twinkle * 0.5);
            let waist = radius * 0.4;
            for (axis, cross) in [(vec2(ray, 0.0), vec2(0.0, waist)), (vec2(0.0, ray), vec2(waist, 0.0))] {
                painter.add(Shape::convex_polygon(
                    vec![center - axis, center + cross, center + axis, center - cross],
                    fade(color, brightness * 0.4),
                    Stroke::NONE,
                ));
            }
        }
        painter.circle_filled(center, radius, fade(color, brightness));
    }
}

fn synth_grid(painter: &Painter, rect: Rect, t: f32, seed: u64) {
    const MAGENTA: Color32 = Color32::from_rgb(255, 70, 190);
    const CYAN: Color32 = Color32::from_rgb(70, 210, 255);
    let horizon = rect.top() + rect.height() * 0.6;
    let cx = rect.center().x;

    for i in 0..70 {
        let x = rect.left() + unit(seed, i * 2) * rect.width();
        let y = rect.top() + unit(seed, i * 2 + 1) * (horizon - rect.top()) * 0.9;
        let twinkle = 0.5 + 0.5 * (t * 1.7 + i as f32 * 2.3).sin();
        painter.circle_filled(pos2(x, y), 1.2, fade(Color32::WHITE, 0.1 + 0.35 * twinkle));
    }

    // Retro sun with scrolling cut-out stripes on its lower half.
    let radius = (rect.width() * 0.34).min(rect.height() * 0.46).max(280.0);
    let slices = 48;
    let mut sun = Mesh::default();
    for k in 0..slices {
        let s = k as f32 / slices as f32;
        let y0 = horizon - radius + radius * s;
        let y1 = y0 + radius / slices as f32;
        let dy = horizon - (y0 + y1) * 0.5;
        let half = (radius * radius - dy * dy).max(0.0).sqrt();
        if s > 0.45 && (s * 9.0 - t * 0.6).rem_euclid(1.0) < (s - 0.45) * 0.9 {
            continue;
        }
        let color = Color32::from_rgb(255, 214, 90).lerp_to_gamma(MAGENTA, s);
        sun.add_colored_rect(Rect::from_min_max(pos2(cx - half, y0), pos2(cx + half, y1)), fade(color, 0.5));
    }
    painter.add(Shape::mesh(sun));

    painter.rect_filled(
        Rect::from_min_max(pos2(rect.left(), horizon), rect.max),
        0.0,
        Color32::from_rgba_unmultiplied(22, 6, 36, 190),
    );
    painter.line_segment([pos2(rect.left(), horizon), pos2(rect.right(), horizon)], Stroke::new(1.5, fade(MAGENTA, 0.7)));

    let depth = rect.bottom() - horizon;
    let rows = 14;
    let scroll = (t * 0.5).fract();
    for k in 0..rows {
        let s = (k as f32 + scroll) / rows as f32;
        let y = horizon + depth * s.powf(2.2);
        painter.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(1.0, fade(MAGENTA, 0.12 + 0.45 * s)));
    }
    let columns = 12;
    for j in -columns..=columns {
        let f = j as f32 / columns as f32;
        painter.line_segment(
            [pos2(cx + f * rect.width() * 0.06, horizon), pos2(cx + f * rect.width() * 1.6, rect.bottom())],
            Stroke::new(1.0, fade(CYAN, 0.3)),
        );
    }
}

static HEX_BYTES: LazyLock<Vec<String>> = LazyLock::new(|| (0..=255_u8).map(|byte| format!("{byte:02X}")).collect());

fn hex_rain(painter: &Painter, rect: Rect, t: f32, seed: u64) {
    const COLUMN_WIDTH: f32 = 26.0;
    const ROW_HEIGHT: f32 = 18.0;
    let head_color = Color32::from_rgb(200, 255, 230);
    let trail_color = Color32::from_rgb(40, 210, 150);
    let font = FontId::monospace(13.0);
    let columns = (rect.width() / COLUMN_WIDTH).ceil() as u64;
    for column in 0..columns {
        let length = 6 + mix(seed, column * 4) % 12;
        let speed = 45.0 + unit(seed, column * 4 + 1) * 95.0;
        let span = rect.height() + length as f32 * ROW_HEIGHT;
        let head = (unit(seed, column * 4 + 2) * span + t * speed).rem_euclid(span);
        // Snap to rows so the trail steps like a terminal instead of sliding.
        let head_row = (head / ROW_HEIGHT).floor() as i64;
        let x = rect.left() + (column as f32 + 0.5) * COLUMN_WIDTH;
        for glyph in 0..length {
            let row = head_row - glyph as i64;
            if row < 0 {
                break;
            }
            let y = rect.top() + row as f32 * ROW_HEIGHT;
            if y > rect.bottom() + ROW_HEIGHT {
                continue;
            }
            // The head flickers through values; the trail keeps what it "read".
            let flicker = if glyph == 0 { (t * 12.0) as u64 } else { 0 };
            let byte = mix(seed ^ column.rotate_left(32), row as u64 ^ flicker.rotate_left(16)) as u8;
            let color = if glyph == 0 {
                fade(head_color, 0.75)
            } else {
                fade(trail_color, 0.55 * (1.0 - glyph as f32 / length as f32))
            };
            painter.text(pos2(x, y), Align2::CENTER_CENTER, &HEX_BYTES[byte as usize], font.clone(), color);
        }
    }
}

type Sprite = [&'static [&'static str]; 2];

const JELLY: Sprite = [
    &["...###...", ".#######.", "##.###.##", "#########", ".#.#.#.#.", "#.#.#.#.#"],
    &["...###...", ".#######.", "##.###.##", "#########", "#.#.#.#.#", ".#.#.#.#."],
];
const SAUCER: Sprite = [
    &["....###....", "..#######..", ".##.#.#.##.", "###########", "..##...##..", ".#.......#."],
    &["....###....", "..#######..", ".##.#.#.##.", "###########", "..##...##..", "..#.....#.."],
];
const BOT: Sprite = [
    &[".######.", "########", "##.##.##", "########", ".#.##.#.", "#......#"],
    &[".######.", "########", "##.##.##", "########", ".#.##.#.", ".#....#."],
];
const SHIP: &[&str] = &["....#....", "...###...", ".#######.", "#########", "#.#...#.#"];

fn add_sprite(mesh: &mut Mesh, rows: &[&str], center: Pos2, pixel: f32, color: Color32) {
    let width = rows.first().map_or(0, |row| row.len()) as f32 * pixel;
    let origin = center - vec2(width, rows.len() as f32 * pixel) * 0.5;
    for (y, row) in rows.iter().enumerate() {
        for (x, cell) in row.bytes().enumerate() {
            if cell == b'#' {
                let min = origin + vec2(x as f32, y as f32) * pixel;
                mesh.add_colored_rect(Rect::from_min_size(min, vec2(pixel, pixel)), color);
            }
        }
    }
}

fn invaders(painter: &Painter, rect: Rect, t: f32, seed: u64) {
    const PIXEL: f32 = 4.0;
    const SPACING: egui::Vec2 = egui::vec2(78.0, 62.0);
    const DESCENT: f32 = 9.0;
    const SPRITES: [(Sprite, Color32); 3] = [
        (JELLY, Color32::from_rgb(110, 255, 160)),
        (SAUCER, Color32::from_rgb(90, 200, 255)),
        (BOT, Color32::from_rgb(255, 110, 200)),
    ];
    let mut mesh = Mesh::default();

    // Classic stepped march: sideways in beats, slowly descending forever.
    let step = (t * 2.0).floor();
    let frame = (step as i64).rem_euclid(2) as usize;
    let sway = (tri(step / 10.0) - 0.5) * 52.0;
    let descent = t * DESCENT;
    let scrolled_rows = (descent / SPACING.y).floor() as i64;
    let offset = descent.rem_euclid(SPACING.y);
    let rows = (rect.height() / SPACING.y).ceil() as i64 + 1;
    let columns = (rect.width() / SPACING.x).ceil() as i64 + 1;
    for row in -1..rows {
        let global_row = row - scrolled_rows;
        let (sprite, color) = SPRITES[global_row.rem_euclid(3) as usize];
        let y = rect.top() + row as f32 * SPACING.y + offset;
        for column in 0..columns {
            if unit(seed, (global_row as u64).wrapping_mul(1009) ^ column as u64) < 0.3 {
                continue;
            }
            let x = rect.left() + (column as f32 + 0.5) * SPACING.x + sway;
            add_sprite(&mut mesh, sprite[frame], pos2(x, y), PIXEL, fade(color, 0.3));
        }
    }

    let phase = unit(seed, 7) * TAU;
    let ship_x = |time: f32| rect.center().x + (time * 0.9 + phase).sin() * rect.width() * 0.4 + (time * 2.3).sin() * rect.width() * 0.04;
    let ship_y = rect.bottom() - 28.0;
    const SHOT_PERIOD: f32 = 0.7;
    const SHOT_SPEED: f32 = 320.0;
    let lifetime = rect.height() / SHOT_SPEED;
    let first = ((t - lifetime) / SHOT_PERIOD).floor() as i64;
    let last = (t / SHOT_PERIOD).floor() as i64;
    for shot in first..=last {
        let fired = shot as f32 * SHOT_PERIOD;
        let age = t - fired;
        if age < 0.0 {
            continue;
        }
        let y = ship_y - 14.0 - age * SHOT_SPEED;
        let shot_rect = Rect::from_center_size(pos2(ship_x(fired), y), vec2(3.0, 12.0));
        mesh.add_colored_rect(shot_rect, fade(Color32::from_rgb(255, 225, 120), 0.6));
    }
    add_sprite(&mut mesh, SHIP, pos2(ship_x(t), ship_y), PIXEL, fade(Color32::from_rgb(120, 255, 170), 0.55));
    painter.add(Shape::mesh(mesh));
}

/// Tetromino cells in a 4x4 box, one entry per piece.
const PIECES: [([(u8, u8); 4], Color32); 7] = [
    ([(0, 1), (1, 1), (2, 1), (3, 1)], Color32::from_rgb(80, 220, 240)),
    ([(1, 1), (2, 1), (1, 2), (2, 2)], Color32::from_rgb(250, 215, 80)),
    ([(0, 1), (1, 1), (2, 1), (1, 2)], Color32::from_rgb(190, 110, 245)),
    ([(1, 1), (2, 1), (0, 2), (1, 2)], Color32::from_rgb(110, 230, 120)),
    ([(0, 1), (1, 1), (1, 2), (2, 2)], Color32::from_rgb(245, 95, 110)),
    ([(0, 1), (1, 1), (2, 1), (0, 2)], Color32::from_rgb(250, 160, 70)),
    ([(0, 1), (1, 1), (2, 1), (2, 2)], Color32::from_rgb(90, 140, 250)),
];

fn blocks(painter: &Painter, rect: Rect, t: f32, seed: u64) {
    const CELL: f32 = 18.0;
    let mut mesh = Mesh::default();
    let count = ((rect.width() * rect.height()) / 16_000.0).clamp(12.0, 140.0) as u64;
    let span = rect.height() + CELL * 8.0;
    for piece in 0..count {
        let (cells, color) = PIECES[(mix(seed, piece * 5) % PIECES.len() as u64) as usize];
        let speed = 28.0 + unit(seed, piece * 5 + 1) * 46.0;
        let fallen = unit(seed, piece * 5 + 2) * span + t * speed;
        let cycle = (fallen / span).floor() as u64;
        // Snap to the cell grid so pieces drop in steps like the real game.
        let y = rect.top() - CELL * 4.0 + (fallen.rem_euclid(span) / CELL).floor() * CELL;
        // Every pass starts in a new column so the screen fills evenly.
        let columns = (rect.width() / CELL).floor().max(4.0) as u64 - 3;
        let column = mix(seed, piece * 5 + 3 + cycle * 7919) % columns;
        let x = rect.left() + column as f32 * CELL;
        let turns = ((t * (0.4 + unit(seed, piece * 5 + 4) * 0.5)).floor() as u64 + piece) % 4;
        for (cx, cy) in cells {
            let (mut cx, mut cy) = (f32::from(cx), f32::from(cy));
            for _ in 0..turns {
                (cx, cy) = (3.0 - cy, cx);
            }
            let cell = Rect::from_min_size(pos2(x + cx * CELL, y + cy * CELL), vec2(CELL, CELL)).shrink(1.0);
            mesh.add_colored_rect(cell, fade(color, 0.28));
            let bevel = Rect::from_min_size(cell.min, vec2(cell.width(), 3.0));
            mesh.add_colored_rect(bevel, fade(Color32::WHITE, 0.12));
        }
    }
    painter.add(Shape::mesh(mesh));
}

/// Upper bound for the Julia grid, keeping the per-frame cost independent of
/// the window size; large windows simply get coarser (still smoothly shaded) cells.
const JULIA_MAX_CELLS: f32 = 22_000.0;
const JULIA_ITERATIONS: u32 = 56;
/// Constants on or near the Mandelbrot boundary, giving connected, filigree
/// sets (dendrites, spirals, the Douady rabbit) instead of sparse dust.
const JULIA_CONSTANTS: [(f32, f32); 8] = [
    (-0.8, 0.156),
    (-0.7269, 0.1889),
    (-0.123, 0.745),
    (-0.391, -0.587),
    (0.285, 0.01),
    (-0.70176, -0.3842),
    (-0.835, -0.2321),
    (-0.4, 0.6),
];
const JULIA_SEGMENT_SECONDS: f32 = 12.0;

/// Smooth-shaded Julia set gliding between well-known constants.
///
/// The escape-time grid is the most expensive effect, so it is only evaluated
/// at [`JULIA_KEYFRAME_RATE`]; frames in between blend the vertex colors of the
/// two surrounding keyframes. The keyframes are memoized in egui's temporary
/// memory, but the painted result is still a pure function of `t`.
fn julia(painter: &Painter, rect: Rect, t: f32, seed: u64) {
    let position = t * JULIA_KEYFRAME_RATE;
    let index = position.floor();
    let blend = position - index;
    let index = index as i64;

    let ctx = painter.ctx();
    let id = egui::Id::new("menu_background_julia_keyframes");
    let mut cache = ctx.data(|data| data.get_temp::<JuliaKeyframes>(id)).unwrap_or_default();
    let from = cache.get_or_compute(rect, seed, index);
    let to = cache.get_or_compute(rect, seed, index + 1);
    cache.0.retain(|entry| entry.key == (seed, rect, index) || entry.key == (seed, rect, index + 1));
    ctx.data_mut(|data| data.insert_temp(id, cache));

    if blend <= 0.0 {
        painter.add(Shape::mesh(from));
        return;
    }
    let mut mesh = (*from).clone();
    for (vertex, target) in mesh.vertices.iter_mut().zip(&to.vertices) {
        vertex.color = vertex.color.lerp_to_gamma(target.color, blend);
    }
    painter.add(Shape::mesh(mesh));
}

/// Julia keyframes per second; frames in between blend vertex colors.
pub(crate) const JULIA_KEYFRAME_RATE: f32 = 10.0;

#[derive(Clone)]
struct JuliaKeyframe {
    key: (u64, Rect, i64),
    mesh: Arc<Mesh>,
}

/// At most the two keyframes surrounding the current time.
#[derive(Clone, Default)]
struct JuliaKeyframes(Vec<JuliaKeyframe>);

impl JuliaKeyframes {
    fn get_or_compute(&mut self, rect: Rect, seed: u64, index: i64) -> Arc<Mesh> {
        let key = (seed, rect, index);
        if let Some(keyframe) = self.0.iter().find(|keyframe| keyframe.key == key) {
            return keyframe.mesh.clone();
        }
        let mesh = Arc::new(julia_mesh(rect, index as f32 / JULIA_KEYFRAME_RATE, seed));
        self.0.push(JuliaKeyframe { key, mesh: mesh.clone() });
        mesh
    }
}

#[cfg(test)]
thread_local! {
    static JULIA_MESHES_COMPUTED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn julia_mesh(rect: Rect, t: f32, seed: u64) -> Mesh {
    #[cfg(test)]
    JULIA_MESHES_COMPUTED.with(|count| count.set(count.get() + 1));

    let cell = (rect.area() / JULIA_MAX_CELLS).sqrt().max(6.0);
    let columns = (rect.width() / cell).ceil().max(1.0) as usize;
    let rows = (rect.height() / cell).ceil().max(1.0) as usize;
    let step = vec2(rect.width() / columns as f32, rect.height() / rows as f32);

    let progress = t / JULIA_SEGMENT_SECONDS + (mix(seed, 1) % JULIA_CONSTANTS.len() as u64) as f32;
    let from = JULIA_CONSTANTS[progress as usize % JULIA_CONSTANTS.len()];
    let to = JULIA_CONSTANTS[(progress as usize + 1) % JULIA_CONSTANTS.len()];
    // Linger on each constant, then ease to the next one.
    let blend = ((progress.fract() - 0.55) / 0.45).clamp(0.0, 1.0);
    let blend = blend * blend * (3.0 - 2.0 * blend);
    let (cx, cy) = (egui::lerp(from.0..=to.0, blend), egui::lerp(from.1..=to.1, blend));
    let (spin_sin, spin_cos) = (t * 0.012 + unit(seed, 2) * TAU).sin_cos();
    let scale = 1.6 / rect.height().max(rect.width() * 0.6) * (1.0 + 0.1 * (t * 0.11).sin());
    let hue = unit(seed, 3) + t * 0.02;
    let palette = |v: f32| {
        let channel = |shift: f32| 0.5 + 0.5 * (TAU * (v + shift)).cos();
        let [r, g, b] = [channel(0.0), channel(0.12), channel(0.25)].map(|c| (c * 255.0) as u8);
        Color32::from_rgb(r, g, b)
    };
    let interior = fade(palette(hue + 0.5), 0.1);

    let mut mesh = Mesh::default();
    mesh.reserve_vertices((columns + 1) * (rows + 1));
    mesh.reserve_triangles(columns * rows * 2);
    for row in 0..=rows {
        for column in 0..=columns {
            let pos = rect.min + vec2(column as f32 * step.x, row as f32 * step.y);
            let offset = (pos - rect.center()) * scale;
            let (mut x, mut y) = (offset.x * spin_cos - offset.y * spin_sin, offset.x * spin_sin + offset.y * spin_cos);
            let mut escaped = None;
            for iteration in 0..JULIA_ITERATIONS {
                let (xx, yy) = (x * x, y * y);
                if xx + yy > 64.0 {
                    // Continuous escape count, avoiding visible iteration bands.
                    let smooth = iteration as f32 + 1.0 - (0.5 * (xx + yy).ln()).ln() / std::f32::consts::LN_2;
                    escaped = Some(smooth.max(0.0));
                    break;
                }
                (x, y) = (xx - yy + cx, 2.0 * x * y + cy);
            }
            let color = match escaped {
                Some(smooth) => {
                    // The far field stays dark; the filaments near the set glow.
                    let glow = ((smooth - 2.0) / 16.0).clamp(0.0, 1.0).powf(1.3);
                    fade(palette(smooth / 20.0 + hue), 0.02 + 0.45 * glow)
                }
                None => interior,
            };
            mesh.colored_vertex(pos, color);
        }
    }
    let stride = (columns + 1) as u32;
    for row in 0..rows as u32 {
        for column in 0..columns as u32 {
            let top_left = row * stride + column;
            mesh.add_triangle(top_left, top_left + 1, top_left + stride);
            mesh.add_triangle(top_left + 1, top_left + stride + 1, top_left + stride);
        }
    }
    mesh
}

/// Base-3 address digits per Sierpinski point; 2^-14 is far below a pixel.
const SIERPINSKI_DEPTH: u32 = 14;
/// Circumradius relative to the window, keeping most of the triangle visible.
const SIERPINSKI_RADIUS: f32 = 0.68;

/// Sparkling Sierpinski triangle. Each point is a fixed barycentric
/// combination of the corners (a "chaos game" result), so the whole fractal
/// can sway and breathe without any per-frame state.
fn sierpinski(painter: &Painter, rect: Rect, t: f32, seed: u64) {
    const COLORS: [Color32; 3] = [
        Color32::from_rgb(90, 215, 255),
        Color32::from_rgb(255, 95, 200),
        Color32::from_rgb(255, 205, 90),
    ];
    let count = (rect.area() / 60.0).clamp(5_000.0, 20_000.0) as u64;
    // The empty middle triangle surrounds the centroid, right behind the menu card.
    let center = rect.center();
    let radius = rect.height().max(rect.width() * 0.56) * SIERPINSKI_RADIUS * (1.0 + 0.04 * (t * 0.35).sin());
    let angle = -std::f32::consts::FRAC_PI_2 + 0.3 * (t * 0.07 + unit(seed, 1) * TAU).sin();
    let corners: [Pos2; 3] = std::array::from_fn(|k| {
        let a = angle + k as f32 * TAU / 3.0;
        center + vec2(a.cos(), a.sin()) * radius
    });

    // One of the nine second-level sub-triangles lights up at a time.
    let beat = t * 0.45;
    let active = mix(seed, beat.floor() as u64) % 9;
    let glow = (beat.fract() * std::f32::consts::PI).sin();

    let mut mesh = Mesh::default();
    for i in 0..count {
        let mut address = mix(seed, i);
        let mut weights = [0.0_f32; 3];
        let mut share = 0.5;
        let mut prefix = [0_u64; 2];
        let mut digit = 0;
        for level in 0..SIERPINSKI_DEPTH {
            digit = address % 3;
            address /= 3;
            if level < 2 {
                prefix[level as usize] = digit;
            }
            weights[digit as usize] += share;
            share *= 0.5;
        }
        weights[digit as usize] += share * 2.0;
        let pos = Pos2::ZERO + corners[0].to_vec2() * weights[0] + corners[1].to_vec2() * weights[1] + corners[2].to_vec2() * weights[2];
        if !rect.contains(pos) {
            continue;
        }

        let twinkle = 0.5 + 0.5 * (t * (1.0 + unit(seed ^ 0x51E7, i) * 2.0) + unit(seed ^ 0x7A11, i) * TAU).sin();
        let lit = if prefix[0] * 3 + prefix[1] == active { glow } else { 0.0 };
        let color = COLORS[prefix[0] as usize].lerp_to_gamma(Color32::WHITE, 0.15 * prefix[1] as f32 + 0.5 * lit);
        let size = 1.4 + 0.8 * twinkle + 0.8 * lit;
        mesh.add_colored_rect(Rect::from_center_size(pos, vec2(size, size)), fade(color, 0.18 + 0.27 * twinkle + 0.4 * lit));
    }
    if !mesh.is_empty() {
        painter.add(Shape::mesh(mesh));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(background: MenuBackground, size: egui::Vec2, time: f64) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, size);
        let mut shapes = Vec::new();
        // Two passes: fonts are only guaranteed to be ready from the second one.
        for _ in 0..2 {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ui| {
                    let painter = ui.painter().with_clip_rect(screen);
                    background.paint(&painter, screen, time);
                },
            );
            output.textures_delta.clear();
            shapes = output.shapes;
        }
        shapes
    }

    fn flatten(shape: &Shape, out: &mut Vec<Shape>) {
        if let Shape::Vec(children) = shape {
            children.iter().for_each(|child| flatten(child, out));
        } else {
            out.push(shape.clone());
        }
    }

    #[test]
    fn random_seeds_cover_every_effect() {
        for effect in MenuEffect::ALL {
            assert!(
                (0..256).any(|seed| MenuBackground::from_seed(seed).effect == effect),
                "{effect:?} is never selected"
            );
        }
        assert_eq!(MenuBackground::from_seed(42), MenuBackground::from_seed(42));
    }

    #[test]
    fn alternate_background_is_stable_and_always_a_different_effect() {
        for effect in MenuEffect::ALL {
            let mut seen = Vec::new();
            for seed in 0..512 {
                let menu = MenuBackground::new(effect, seed);
                let about = menu.alternate();
                assert_ne!(about.effect, effect, "About must never repeat the main menu effect");
                assert_eq!(about, menu.alternate(), "About must not change between frames");
                if !seen.contains(&about.effect) {
                    seen.push(about.effect);
                }
            }
            assert_eq!(seen.len(), MenuEffect::ALL.len() - 1, "Every other effect must be possible after {effect:?}");
        }
    }

    #[test]
    fn every_effect_animates_cheaply_within_the_menu_area_without_textures() {
        for effect in MenuEffect::ALL {
            for size in [egui::vec2(640.0, 420.0), egui::vec2(1100.0, 720.0), egui::vec2(2560.0, 1440.0)] {
                let background = MenuBackground::new(effect, 0x5EED);
                let screen = Rect::from_min_size(Pos2::ZERO, size);
                let first = render(background, size, 12.0);
                let mut shapes = Vec::new();
                for clipped in &first {
                    assert_eq!(clipped.clip_rect, screen, "{effect:?} must stay clipped to the menu area");
                    flatten(&clipped.shape, &mut shapes);
                }
                assert!(!shapes.is_empty(), "{effect:?} painted nothing at {size:?}");
                assert!(shapes.len() < 2_500, "{effect:?} painted {} shapes at {size:?}", shapes.len());
                for shape in &shapes {
                    assert_eq!(shape.texture_id(), egui::TextureId::default(), "{effect:?} must not upload textures");
                    let bounds = shape.visual_bounding_rect();
                    assert!(bounds.is_finite(), "{effect:?} produced a non-finite shape");
                    assert!(bounds.intersects(screen.expand(64.0)), "{effect:?} painted far outside the menu area");
                }
                assert_eq!(first, render(background, size, 12.0), "{effect:?} must be deterministic");
                assert_ne!(first, render(background, size, 12.5), "{effect:?} must animate");
            }
        }
    }

    #[test]
    fn next_cycles_through_every_effect_and_keeps_the_seed() {
        let start = MenuBackground::new(MenuEffect::Starfield, 77);
        let mut background = start;
        let mut seen = Vec::new();
        for _ in 0..MenuEffect::ALL.len() {
            seen.push(background.effect);
            background = background.next();
            assert_ne!(background.alternate().effect, background.effect);
        }
        assert_eq!(background, start, "Cycling must return to the first effect");
        assert_eq!(seen, MenuEffect::ALL);
    }

    fn julia_vertices(ctx: &egui::Context, size: egui::Vec2, time: f64) -> Vec<egui::epaint::Vertex> {
        let screen = Rect::from_min_size(Pos2::ZERO, size);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| MenuBackground::new(MenuEffect::Julia, 3).paint(&ui.painter().with_clip_rect(screen), screen, time),
        );
        output.textures_delta.clear();
        let mut shapes = Vec::new();
        output.shapes.iter().for_each(|clipped| flatten(&clipped.shape, &mut shapes));
        let [Shape::Mesh(mesh)] = shapes.as_slice() else {
            panic!("Julia must paint a single mesh");
        };
        mesh.vertices.clone()
    }

    #[test]
    fn julia_evaluates_keyframes_at_a_reduced_rate_and_blends_smoothly() {
        let size = vec2(1100.0, 720.0);
        let ctx = egui::Context::default();
        JULIA_MESHES_COMPUTED.with(|count| count.set(0));
        let frames: Vec<_> = (0..30).map(|frame| julia_vertices(&ctx, size, 20.0 + f64::from(frame) / 30.0)).collect();
        let computed = JULIA_MESHES_COMPUTED.with(std::cell::Cell::get);
        assert!(
            computed <= JULIA_KEYFRAME_RATE as usize + 1,
            "A second at 30 Hz must reuse keyframes, computed {computed}"
        );

        // Memoization must not change the picture.
        for (frame, time) in [(7, 20.0 + 7.0 / 30.0), (29, 20.0 + 29.0 / 30.0)] {
            assert_eq!(frames[frame], julia_vertices(&egui::Context::default(), size, time));
        }

        // A frame between keyframes lies between their colors, so motion stays smooth.
        let screen = Rect::from_min_size(Pos2::ZERO, size);
        let before = julia_mesh(screen, 20.0, 3);
        let after = julia_mesh(screen, 20.1, 3);
        let middle = julia_vertices(&ctx, size, 20.05);
        let mut changed = 0;
        for ((a, b), m) in before.vertices.iter().zip(&after.vertices).zip(&middle) {
            for channel in 0..4 {
                let (a, b, m) = (a.color[channel], b.color[channel], m.color[channel]);
                assert!(m >= a.min(b).saturating_sub(1) && m <= a.max(b).saturating_add(1));
            }
            changed += usize::from(a.color != b.color);
        }
        assert!(changed > 100, "Consecutive keyframes must differ");

        let cache = ctx.data(|data| data.get_temp::<JuliaKeyframes>(egui::Id::new("menu_background_julia_keyframes")));
        assert!(cache.is_some_and(|cache| cache.0.len() <= 2), "Only the surrounding keyframes may stay cached");
        julia_vertices(&ctx, vec2(800.0, 600.0), 20.05);
        let cache = ctx.data(|data| data.get_temp::<JuliaKeyframes>(egui::Id::new("menu_background_julia_keyframes")));
        assert!(
            cache.is_some_and(|cache| cache.0.len() == 2 && cache.0.iter().all(|keyframe| keyframe.key.1.width() == 800.0)),
            "Resizing must replace stale keyframes"
        );
    }

    #[test]
    fn starfield_uses_small_stars_instead_of_streaks_and_drifts_gently() {
        let background = MenuBackground::new(MenuEffect::Starfield, 0x5EED);
        let size = vec2(1100.0, 720.0);
        let frames: Vec<Vec<Shape>> = [12.0, 12.5]
            .into_iter()
            .map(|time| {
                let mut shapes = Vec::new();
                for clipped in render(background, size, time) {
                    flatten(&clipped.shape, &mut shapes);
                }
                shapes
            })
            .collect();
        let mut radii = Vec::new();
        let mut glints = 0;
        for before in &frames[0] {
            match before {
                Shape::Circle(a) => {
                    assert!(a.radius <= 6.3, "Stars and halos must stay compact");
                    if a.radius <= 1.8 && Rect::from_min_size(Pos2::ZERO, size).shrink(20.0).contains(a.center) {
                        let b = frames[1]
                            .iter()
                            .find_map(|shape| match shape {
                                Shape::Circle(b) if b.radius == a.radius => Some(b),
                                _ => None,
                            })
                            .expect("A visible star must persist between frames");
                        assert!(a.center.distance(b.center) < 1.2, "Stars must drift slowly, not warp");
                    }
                    radii.push(a.radius);
                }
                Shape::Path(path) => {
                    assert!(path.closed && path.points.len() == 4, "Glints must be filled diamonds");
                    assert!(path.visual_bounding_rect().size().max_elem() < 11.0);
                    glints += 1;
                }
                _ => panic!("Starfield must use light points and compact glints, not streaks"),
            }
        }
        assert!(radii.len() >= 220);
        assert!(radii.iter().any(|radius| *radius < 0.8));
        assert!(radii.iter().any(|radius| *radius > 3.0), "Bright stars need soft halos");
        assert!(glints > 0 && glints < 40, "Only a few stars should have glints");
    }

    #[test]
    fn julia_is_one_smoothly_shaded_mesh_with_a_bounded_grid() {
        for size in [vec2(640.0, 420.0), vec2(1100.0, 720.0), vec2(3840.0, 2160.0)] {
            let screen = Rect::from_min_size(Pos2::ZERO, size);
            let meshes = |time: f64| -> Vec<Mesh> {
                let mut shapes = Vec::new();
                for clipped in render(MenuBackground::new(MenuEffect::Julia, 3), size, time) {
                    flatten(&clipped.shape, &mut shapes);
                }
                shapes
                    .into_iter()
                    .map(|shape| match shape {
                        Shape::Mesh(mesh) => (*mesh).clone(),
                        other => panic!("Julia must be a single mesh, got {other:?}"),
                    })
                    .collect()
            };
            let first = meshes(20.0);
            assert_eq!(first.len(), 1);
            let mesh = &first[0];
            assert!(mesh.vertices.len() as f32 <= JULIA_MAX_CELLS * 1.1 + 400.0, "Grid too fine at {size:?}");
            assert!(mesh.vertices.len() > 5_000, "Grid too coarse at {size:?}");
            assert!(mesh.calc_bounds().expand(0.5).contains_rect(screen) && screen.expand(0.5).contains_rect(mesh.calc_bounds()));
            let mut distinct: Vec<_> = mesh.vertices.iter().map(|v| v.color).collect();
            distinct.sort_by_key(|color| color.to_array());
            distinct.dedup();
            assert!(distinct.len() > 100, "Julia must use smooth gradients, not a few flat colors");
            assert!(mesh.vertices.iter().all(|v| v.color.a() <= 160), "Julia must stay subdued behind the menu");
            assert_ne!(meshes(30.0)[0].vertices, mesh.vertices, "The Julia constant must keep moving");
        }
    }

    #[test]
    fn sierpinski_points_leave_the_middle_triangle_empty() {
        for size in [vec2(640.0, 420.0), vec2(1100.0, 720.0), vec2(2560.0, 1440.0)] {
            for time in [0.0, 7.3, 41.0] {
                let mut shapes = Vec::new();
                for clipped in render(MenuBackground::new(MenuEffect::Sierpinski, 9), size, time) {
                    flatten(&clipped.shape, &mut shapes);
                }
                let [Shape::Mesh(mesh)] = shapes.as_slice() else {
                    panic!("Sierpinski must be a single mesh");
                };
                let points: Vec<Pos2> = mesh.vertices.as_chunks::<4>().0.iter().map(|quad| quad[0].pos.lerp(quad[2].pos, 0.5)).collect();
                assert!(points.len() >= 1_000, "Too few visible points at {size:?}");
                // The middle triangle's inradius is a quarter of the circumradius.
                let circumradius = size.y.max(size.x * 0.56) * SIERPINSKI_RADIUS * 0.96;
                let hole = circumradius * 0.25 - 3.0;
                let center = Rect::from_min_size(Pos2::ZERO, size).center();
                assert!(
                    points.iter().all(|point| point.distance(center) > hole),
                    "Points must not fill the central hole at {size:?}, t={time}"
                );
                assert!(points.iter().any(|point| point.distance(center) < hole * 2.2), "The hole must be outlined");
            }
        }
    }

    #[test]
    fn effects_stay_finite_after_long_uptime() {
        for effect in MenuEffect::ALL {
            let size = egui::vec2(1100.0, 720.0);
            for time in [0.0, 3599.99, 3600.0, 86_400.0 * 30.0] {
                let mut shapes = Vec::new();
                for clipped in render(MenuBackground::new(effect, 1), size, time) {
                    flatten(&clipped.shape, &mut shapes);
                }
                assert!(shapes.iter().all(|shape| shape.visual_bounding_rect().is_finite()), "{effect:?} at {time}");
            }
        }
    }
}
