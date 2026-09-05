//! Procedural pixel-art textures for Embersong.
//!
//! Every creature, the hero, and the tileset are drawn in code at startup —
//! no image files, nothing to download, MIT-clean. Chunky 16-bit JRPG style:
//! baked drop shadows, element-colored accents, glowing bits on star-spawn.
//! All sprites use nearest-neighbour sampling for crisp pixels.

use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use embersong_core::{Element, MonsterKind};
use std::collections::HashMap;

pub type Rgba = (u8, u8, u8, u8);

// --- palette ---------------------------------------------------------------

const INK: Rgba = (20, 16, 28, 255);
const GOBLIN: Rgba = (86, 168, 66, 255);
const GOBLIN_DK: Rgba = (58, 128, 48, 255);
const TROLL: Rgba = (112, 126, 104, 255);
const TROLL_DK: Rgba = (78, 90, 74, 255);
const MOSS: Rgba = (62, 122, 70, 255);
const ORC: Rgba = (172, 122, 62, 255);
const ORC_DK: Rgba = (130, 90, 46, 255);
const PALE: Rgba = (226, 210, 198, 255);
const CAPE: Rgba = (26, 20, 42, 255);
const CAPE_STAR: Rgba = (30, 40, 110, 255);
const BLOOD: Rgba = (202, 32, 44, 255);
const ROCK: Rgba = (142, 112, 90, 255);
const ROCK_DK: Rgba = (92, 72, 56, 255);
const WISP: Rgba = (255, 240, 182, 255);
const WING: Rgba = (255, 255, 255, 150);
const DRAKE: Rgba = (72, 152, 92, 255);
const DRAKE_DK: Rgba = (48, 110, 78, 255);
const BELLY: Rgba = (222, 200, 132, 255);
const CLOAK: Rgba = (72, 62, 142, 255);
const CLOAK_DK: Rgba = (48, 42, 100, 255);
const FACE: Rgba = (236, 202, 162, 255);
const LANTERN: Rgba = (255, 190, 92, 255);
const GOLD: Rgba = (242, 202, 92, 255);
const STEEL: Rgba = (182, 187, 198, 255);
const STEEL_DK: Rgba = (120, 126, 140, 255);
const BONE: Rgba = (242, 236, 222, 255);
const EYE_Y: Rgba = (250, 220, 80, 255);
const EYE_R: Rgba = (230, 40, 50, 255);
const EYE_V: Rgba = (170, 120, 250, 255);
const RUNE: Rgba = (120, 230, 255, 255);
const WOOD: Rgba = (150, 102, 62, 255);
const SHADOW: Rgba = (10, 8, 18, 110);
const HALO: Rgba = (255, 210, 120, 70);

// --- canvas -----------------------------------------------------------------

pub struct Canvas {
    w: i32,
    h: i32,
    buf: Vec<u8>,
}

impl Canvas {
    pub fn new(w: i32, h: i32) -> Self {
        Self {
            w,
            h,
            buf: vec![0; (w * h * 4) as usize],
        }
    }

    pub fn set(&mut self, x: i32, y: i32, c: Rgba) {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return;
        }
        let i = ((y * self.w + x) * 4) as usize;
        if c.3 == 255 || self.buf[i + 3] == 0 {
            self.buf[i..i + 4].copy_from_slice(&[c.0, c.1, c.2, c.3]);
        } else {
            // alpha-over blend
            let (sr, sg, sb, sa) = (
                c.0 as f32 / 255.0,
                c.1 as f32 / 255.0,
                c.2 as f32 / 255.0,
                c.3 as f32 / 255.0,
            );
            let da = self.buf[i + 3] as f32 / 255.0;
            let out_a = sa + da * (1.0 - sa);
            let ch = [sr, sg, sb];
            for (k, &chan) in ch.iter().enumerate() {
                let d = self.buf[i + k] as f32 / 255.0;
                self.buf[i + k] = (((chan * sa + d * da * (1.0 - sa)) / out_a) * 255.0) as u8;
            }
            self.buf[i + 3] = (out_a * 255.0) as u8;
        }
    }

    #[cfg(test)]
    pub fn get(&self, x: i32, y: i32) -> Rgba {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return (0, 0, 0, 0);
        }
        let i = ((y * self.w + x) * 4) as usize;
        (
            self.buf[i],
            self.buf[i + 1],
            self.buf[i + 2],
            self.buf[i + 3],
        )
    }

    pub fn rect(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.set(x, y, c);
            }
        }
    }

    pub fn hline(&mut self, x0: i32, x1: i32, y: i32, c: Rgba) {
        for x in x0.min(x1)..=x0.max(x1) {
            self.set(x, y, c);
        }
    }

    pub fn vline(&mut self, x: i32, y0: i32, y1: i32, c: Rgba) {
        for y in y0.min(y1)..=y0.max(y1) {
            self.set(x, y, c);
        }
    }

    pub fn disc(&mut self, cx: i32, cy: i32, r: i32, c: Rgba) {
        for y in (cy - r)..=(cy + r) {
            for x in (cx - r)..=(cx + r) {
                let (dx, dy) = (x - cx, y - cy);
                if dx * dx + dy * dy <= r * r {
                    self.set(x, y, c);
                }
            }
        }
    }

    pub fn ell(&mut self, cx: i32, cy: i32, rx: i32, ry: i32, c: Rgba) {
        if rx <= 0 || ry <= 0 {
            return;
        }
        for y in (cy - ry)..=(cy + ry) {
            for x in (cx - rx)..=(cx + rx) {
                let (dx, dy) = (x - cx, y - cy);
                if dx * dx * ry * ry + dy * dy * rx * rx <= rx * rx * ry * ry {
                    self.set(x, y, c);
                }
            }
        }
    }

    /// Upward triangle: apex at (cx, y_tip), base half-width `half` at `y_tip + h`.
    pub fn tri_up(&mut self, cx: i32, y_tip: i32, half: i32, h: i32, c: Rgba) {
        for r in 0..=h {
            let w = (half as f32 * r as f32 / h.max(1) as f32) as i32;
            self.hline(cx - w, cx + w, y_tip + r, c);
        }
    }

    /// Downward triangle: apex at (cx, y_tip), base at `y_tip - h`.
    pub fn tri_down(&mut self, cx: i32, y_tip: i32, half: i32, h: i32, c: Rgba) {
        for r in 0..=h {
            let w = (half as f32 * r as f32 / h.max(1) as f32) as i32;
            self.hline(cx - w, cx + w, y_tip - r, c);
        }
    }

    pub fn eyes(&mut self, y: i32, lx: i32, rx: i32, pupil: Rgba) {
        self.rect(lx, y, lx + 4, y + 5, (246, 246, 240, 255));
        self.rect(rx, y, rx + 4, y + 5, (246, 246, 240, 255));
        self.rect(lx + 1, y + 2, lx + 2, y + 4, pupil);
        self.rect(rx + 1, y + 2, rx + 2, y + 4, pupil);
    }

    pub fn shadow(&mut self, cx: i32) {
        self.ell(cx, self.h - 5, 14, 4, SHADOW);
    }

    /// Pixels with any opacity, as a fraction of the canvas.
    #[cfg(test)]
    pub fn coverage(&self) -> f32 {
        let n = self.buf.chunks_exact(4).filter(|p| p[3] > 8).count();
        n as f32 / (self.w * self.h) as f32
    }

    pub fn into_image(self) -> Image {
        let mut img = Image::new(
            Extent3d {
                width: self.w as u32,
                height: self.h as u32,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            self.buf,
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::RENDER_WORLD,
        );
        img.sampler = ImageSampler::nearest();
        img
    }
}

pub const CREATURE_W: i32 = 48;
pub const CREATURE_H: i32 = 56;

// --- creatures ---------------------------------------------------------------

fn crown(c: &mut Canvas, x0: i32, y_base: i32) {
    // Three-point crown band.
    c.rect(x0, y_base - 2, x0 + 20, y_base, GOLD);
    c.tri_up(x0 + 3, y_base - 9, 3, 7, GOLD);
    c.tri_up(x0 + 10, y_base - 11, 3, 9, GOLD);
    c.tri_up(x0 + 17, y_base - 9, 3, 7, GOLD);
    c.set(x0 + 10, y_base - 4, BLOOD);
}

fn draw_goblinish(c: &mut Canvas, king: bool) {
    c.shadow(24);
    let skin = GOBLIN;
    // Ears jut sideways.
    c.rect(2, 20, 12, 26, GOBLIN_DK);
    c.rect(34, 20, 44, 26, GOBLIN_DK);
    c.rect(4, 22, 10, 24, skin);
    c.rect(36, 22, 42, 24, skin);
    // Head + jaw.
    c.rect(12, 12, 34, 30, skin);
    c.rect(12, 12, 34, 15, GOBLIN_DK);
    c.rect(16, 28, 30, 33, skin);
    c.eyes(17, 16, 26, if king { EYE_R } else { EYE_Y });
    c.set(23, 25, INK);
    c.hline(18, 28, 30, INK); // mouth
    if king {
        crown(c, 13, 12);
        c.rect(18, 29, 20, 31, BONE); // foam
        c.rect(26, 29, 28, 31, BONE);
    }
    // Tunic body, arms, dagger, feet.
    c.rect(15, 34, 31, 46, WOOD);
    c.rect(15, 34, 31, 37, ORC_DK);
    c.rect(9, 34, 13, 44, skin);
    c.rect(33, 34, 37, 44, skin);
    c.rect(35, 32, 38, 45, STEEL); // dagger blade
    c.rect(34, 45, 39, 47, WOOD); // grip
    c.rect(16, 47, 21, 51, INK);
    c.rect(25, 47, 30, 51, INK);
}

fn draw_trollish(c: &mut Canvas, elder: bool) {
    c.shadow(24);
    // Thick arms first (behind torso).
    c.rect(2, 22, 10, 44, TROLL);
    c.rect(36, 22, 44, 44, TROLL);
    c.rect(2, 40, 10, 44, TROLL_DK);
    c.rect(36, 40, 44, 44, TROLL_DK);
    // Torso + belly.
    c.rect(8, 20, 38, 48, TROLL);
    c.rect(14, 30, 32, 46, (128, 142, 118, 255));
    // Moss patches.
    for (x, y) in [(12, 24), (32, 34), (20, 42), (36, 26)] {
        c.disc(x, y, 2, MOSS);
    }
    // Head with heavy brow shelf.
    c.rect(12, 6, 34, 22, TROLL);
    c.rect(12, 13, 34, 18, TROLL_DK);
    c.rect(15, 19, 19, 22, EYE_Y);
    c.rect(27, 19, 31, 22, EYE_Y);
    c.set(17, 20, INK);
    c.set(29, 20, INK);
    c.rect(20, 23, 26, 26, TROLL_DK); // snout
    c.set(22, 24, INK);
    c.set(25, 24, INK);
    if elder {
        // Flowing beard + glowing runes.
        c.rect(13, 26, 33, 44, (200, 200, 205, 255));
        c.vline(19, 28, 42, STEEL_DK);
        c.vline(27, 28, 42, STEEL_DK);
        c.set(23, 10, RUNE);
        c.set(6, 30, RUNE);
        c.set(40, 30, RUNE);
    }
    // Stubby legs.
    c.rect(12, 48, 20, 52, TROLL_DK);
    c.rect(26, 48, 34, 52, TROLL_DK);
}

fn draw_orcish(c: &mut Canvas, warlord: bool) {
    c.shadow(24);
    // Shoulder plates.
    c.rect(5, 20, 13, 27, STEEL);
    c.rect(33, 20, 41, 27, STEEL);
    c.rect(5, 20, 13, 22, STEEL_DK);
    c.rect(33, 20, 41, 22, STEEL_DK);
    // Torso + arms.
    c.rect(12, 24, 34, 48, ORC);
    c.rect(6, 26, 11, 44, ORC_DK);
    c.rect(35, 26, 40, 44, ORC_DK);
    c.rect(16, 40, 30, 48, ORC_DK); // loincloth
                                    // Head, angry brow, tusks.
    c.rect(14, 8, 32, 26, ORC);
    c.rect(14, 8, 32, 12, ORC_DK);
    c.rect(15, 13, 21, 15, INK); // brow L
    c.rect(25, 13, 31, 15, INK); // brow R
    c.rect(17, 16, 21, 20, (246, 246, 240, 255));
    c.rect(25, 16, 29, 20, (246, 246, 240, 255));
    c.rect(18, 17, 19, 19, EYE_R);
    c.rect(26, 17, 27, 19, EYE_R);
    c.rect(17, 22, 20, 29, BONE); // tusks jut up
    c.rect(26, 22, 29, 29, BONE);
    c.hline(21, 25, 24, INK);
    if warlord {
        // Horned helm + war paint.
        c.rect(13, 2, 33, 9, STEEL_DK);
        c.tri_up(8, 2, 5, 8, BONE);
        c.tri_up(38, 2, 5, 8, BONE);
        c.hline(14, 32, 20, BLOOD);
        c.hline(16, 34, 38, BLOOD);
        c.rect(26, 22, 29, 31, BONE); // bigger tusks
        c.rect(17, 22, 20, 31, BONE);
    }
    c.rect(16, 48, 22, 52, INK);
    c.rect(25, 48, 31, 52, INK);
}

fn draw_vampiric(c: &mut Canvas, star: bool) {
    c.shadow(24);
    let cape = if star { CAPE_STAR } else { CAPE };
    // High cape wings behind.
    c.tri_up(8, 12, 7, 12, cape);
    c.tri_up(38, 12, 7, 12, cape);
    c.rect(8, 22, 38, 50, cape);
    if star {
        for (x, y) in [(12, 30), (34, 36), (20, 44), (28, 26), (14, 40)] {
            c.set(x, y, (255, 255, 255, 255));
        }
        c.rect(8, 22, 38, 24, GOLD); // trim
    }
    // Pale body + collar.
    c.rect(17, 26, 29, 48, (30, 26, 44, 255));
    c.rect(19, 28, 27, 46, PALE);
    c.set(23, 34, BLOOD); // medallion
    c.tri_down(14, 28, 5, 7, INK);
    c.tri_down(32, 28, 5, 7, INK);
    // Head: widow's peak, red eyes, fangs.
    c.rect(16, 6, 30, 25, PALE);
    c.rect(16, 6, 30, 12, INK);
    c.tri_down(23, 13, 4, 4, INK);
    c.rect(18, 15, 22, 19, (246, 246, 240, 255));
    c.rect(25, 15, 29, 19, (246, 246, 240, 255));
    let pupil = if star { EYE_V } else { EYE_R };
    c.rect(19, 16, 20, 18, pupil);
    c.rect(26, 16, 27, 18, pupil);
    c.hline(20, 27, 22, INK);
    c.set(21, 23, BONE);
    c.set(26, 23, BONE);
}

fn draw_giantish(c: &mut Canvas, crowned: bool) {
    c.shadow(24);
    // Legs + torso of living rock.
    c.rect(10, 36, 20, 52, ROCK_DK);
    c.rect(26, 36, 36, 52, ROCK_DK);
    c.rect(8, 16, 38, 38, ROCK);
    c.rect(8, 16, 38, 20, ROCK_DK);
    // Cracks.
    c.vline(18, 22, 34, ROCK_DK);
    c.hline(18, 30, 27, ROCK_DK);
    // Boulder shoulders + arms.
    c.disc(7, 16, 6, STEEL_DK);
    c.disc(39, 16, 6, STEEL_DK);
    c.rect(2, 20, 8, 40, ROCK);
    c.rect(38, 20, 44, 40, ROCK);
    // Small fierce head.
    c.rect(17, 2, 29, 15, ROCK);
    c.rect(17, 2, 29, 5, ROCK_DK);
    c.rect(19, 8, 22, 10, EYE_Y);
    c.rect(25, 8, 28, 10, EYE_Y);
    c.hline(20, 27, 13, INK);
    if crowned {
        crown(c, 14, 4);
        c.vline(24, 22, 36, RUNE);
        c.set(5, 28, RUNE);
        c.set(41, 28, RUNE);
    }
}

fn draw_mothwisp(c: &mut Canvas) {
    // Halo + wings behind body.
    c.disc(24, 24, 13, HALO);
    c.ell(10, 26, 8, 12, WING);
    c.ell(38, 26, 8, 12, WING);
    c.ell(10, 26, 5, 8, (255, 255, 255, 200));
    c.ell(38, 26, 5, 8, (255, 255, 255, 200));
    c.shadow(24);
    // Round glowing body.
    c.disc(24, 28, 9, WISP);
    c.disc(24, 28, 6, (255, 252, 230, 255));
    // Antennae.
    c.vline(20, 12, 19, INK);
    c.vline(28, 12, 19, INK);
    c.set(20, 11, GOLD);
    c.set(28, 11, GOLD);
    // Big soulful eyes with sparkle.
    c.disc(20, 28, 3, INK);
    c.disc(28, 28, 3, INK);
    c.set(19, 27, (255, 255, 255, 255));
    c.set(27, 27, (255, 255, 255, 255));
    c.ell(24, 35, 3, 2, (200, 150, 100, 255)); // smile
}

fn draw_drake(c: &mut Canvas) {
    c.shadow(24);
    // Tail curls left.
    c.disc(8, 44, 4, DRAKE_DK);
    c.disc(12, 40, 5, DRAKE);
    // Wing membranes.
    c.tri_up(6, 20, 8, 14, DRAKE_DK);
    c.tri_up(42, 20, 8, 14, DRAKE_DK);
    // Body + belly scales.
    c.ell(25, 36, 12, 12, DRAKE);
    c.ell(25, 38, 7, 9, BELLY);
    c.hline(19, 31, 38, DRAKE_DK);
    c.hline(20, 32, 42, DRAKE_DK);
    // Back spikes.
    for x in [16, 22, 28, 34] {
        c.tri_up(x, 24, 2, 5, DRAKE_DK);
    }
    // Head, snout, horns, slit eye.
    c.rect(17, 10, 33, 24, DRAKE);
    c.rect(17, 20, 37, 26, DRAKE);
    c.rect(33, 21, 37, 24, DRAKE_DK); // nostril
    c.tri_up(18, 10, 4, 7, BONE);
    c.tri_up(30, 10, 4, 7, BONE);
    c.rect(21, 14, 27, 18, EYE_Y);
    c.vline(24, 14, 18, INK);
    // Clawed feet.
    c.rect(16, 46, 22, 51, DRAKE_DK);
    c.rect(28, 46, 34, 51, DRAKE_DK);
    c.set(16, 51, BONE);
    c.set(28, 51, BONE);
}

fn draw_hero(c: &mut Canvas) {
    c.shadow(20);
    // Staff + lantern (right side).
    c.vline(33, 14, 44, WOOD);
    c.rect(29, 22, 37, 31, INK);
    c.rect(30, 23, 36, 30, LANTERN);
    c.rect(32, 25, 34, 28, (255, 246, 220, 255));
    c.disc(33, 26, 9, HALO);
    // Cloak body.
    c.rect(10, 22, 28, 50, CLOAK);
    c.rect(10, 22, 28, 26, CLOAK_DK);
    c.rect(10, 22, 13, 50, CLOAK_DK);
    // Lute across the chest.
    c.ell(17, 36, 5, 7, WOOD);
    c.disc(17, 36, 2, INK);
    c.vline(22, 24, 34, (110, 74, 44, 255));
    // Hood + face.
    c.rect(12, 4, 28, 22, CLOAK_DK);
    c.rect(15, 9, 25, 21, FACE);
    c.rect(17, 13, 20, 16, INK);
    c.rect(22, 13, 25, 16, INK);
    c.hline(19, 23, 19, (150, 90, 80, 255)); // smile
                                             // Boots.
    c.rect(12, 50, 17, 53, INK);
    c.rect(21, 50, 26, 53, INK);
}

pub fn draw_creature(kind: MonsterKind) -> Canvas {
    let mut c = Canvas::new(CREATURE_W, CREATURE_H);
    match kind {
        MonsterKind::Goblin => draw_goblinish(&mut c, false),
        MonsterKind::RabidGoblinKing => draw_goblinish(&mut c, true),
        MonsterKind::Troll => draw_trollish(&mut c, false),
        MonsterKind::ElderTroll => draw_trollish(&mut c, true),
        MonsterKind::Orc => draw_orcish(&mut c, false),
        MonsterKind::WarlordOrc => draw_orcish(&mut c, true),
        MonsterKind::Vampire => draw_vampiric(&mut c, false),
        MonsterKind::StarTouchedVampire => draw_vampiric(&mut c, true),
        MonsterKind::HillGiant => draw_giantish(&mut c, false),
        MonsterKind::CrownedGiant => draw_giantish(&mut c, true),
        MonsterKind::MothWisp => draw_mothwisp(&mut c),
        MonsterKind::ThornDrake => draw_drake(&mut c),
    }
    c
}

pub fn draw_hero_sprite() -> Canvas {
    let mut c = Canvas::new(40, 56);
    draw_hero(&mut c);
    c
}

// --- tiles ------------------------------------------------------------------

fn speckle(c: &mut Canvas, seed: u64, n: u32, color: Rgba) {
    let mut s = seed;
    for _ in 0..n {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let x = (s % c.w as u64) as i32;
        s ^= s << 13;
        let y = (s % c.h as u64) as i32;
        c.set(x, y, color);
    }
}

pub fn draw_floor(variant: u64) -> Canvas {
    let mut c = Canvas::new(24, 12);
    let base = if variant == 0 {
        (26, 44, 56, 255)
    } else {
        (30, 50, 60, 255)
    };
    c.rect(0, 0, 23, 11, base);
    speckle(&mut c, 1234 + variant * 77, 26, (20, 34, 44, 255));
    speckle(&mut c, 987 + variant * 131, 12, (52, 84, 92, 255));
    c.hline(0, 23, 11, (16, 26, 34, 255));
    c
}

pub fn draw_wall() -> Canvas {
    let mut c = Canvas::new(24, 20);
    c.rect(0, 0, 23, 19, (44, 30, 72, 255));
    // Brick courses.
    for y in [5, 11, 17] {
        c.hline(0, 23, y, (26, 18, 48, 255));
    }
    for (x, y0, y1) in [
        (6, 0, 5),
        (15, 0, 5),
        (3, 6, 11),
        (12, 6, 11),
        (20, 6, 11),
        (8, 12, 17),
        (18, 12, 17),
    ] {
        c.vline(x, y0, y1, (26, 18, 48, 255));
    }
    speckle(&mut c, 555, 20, (64, 46, 100, 255));
    c
}

pub fn draw_stairs() -> Canvas {
    let mut c = Canvas::new(24, 28);
    // Golden arch portal.
    c.disc(12, 14, 10, (60, 40, 16, 255));
    c.disc(12, 14, 8, GOLD);
    c.disc(12, 14, 5, (255, 236, 170, 255));
    c.disc(12, 14, 2, (255, 252, 235, 255));
    // Steps.
    c.rect(4, 24, 19, 27, (70, 60, 90, 255));
    c.hline(4, 19, 24, (50, 42, 66, 255));
    c
}

/// Soft radial lantern glow (replaces flat translucent quads).
pub fn draw_glow() -> Canvas {
    let mut c = Canvas::new(32, 32);
    for y in 0..32 {
        for x in 0..32 {
            let (dx, dy) = (x - 16, y - 16);
            let d = ((dx * dx + dy * dy) as f32).sqrt() / 16.0;
            if d < 1.0 {
                let a = ((1.0 - d).powi(2) * 150.0) as u8;
                c.set(x, y, (255, 180, 100, a));
            }
        }
    }
    c
}

/// Floating bard note: bright gold + cyan glow for bonuses,
/// deep indigo + blood shadow for penalties. 24x28 eighth-note.
pub fn draw_note(bright: bool) -> Canvas {
    let mut c = Canvas::new(24, 28);
    let (body, glow, detail) = if bright {
        (
            (255, 220, 120, 255),
            (120, 230, 255, 90),
            (255, 252, 235, 255),
        )
    } else {
        ((52, 40, 96, 255), (202, 32, 44, 80), (20, 14, 36, 255))
    };
    // Glow halo.
    c.disc(12, 14, 10, glow);
    // Stem.
    c.vline(17, 4, 20, body);
    // Flag.
    c.tri_up(17, 4, 5, 5, body);
    // Head.
    c.ell(10, 21, 7, 5, body);
    c.ell(10, 21, 4, 3, detail);
    // Sparkle for bright, crack for dark.
    if bright {
        c.set(7, 8, (255, 255, 255, 255));
        c.set(16, 12, (255, 255, 255, 255));
    } else {
        c.hline(6, 14, 21, detail);
    }
    c
}

/// Floor tint per element so dens read as territory, not just tokens.
pub fn element_floor(e: Element) -> Color {
    let (r, g, b, a) = match e {
        Element::Ember => (96, 44, 30, 255),
        Element::Frost => (40, 78, 104, 255),
        Element::Thorn => (38, 84, 52, 255),
        Element::Gloom => (64, 44, 104, 255),
        Element::Star => (96, 84, 52, 255),
    };
    Color::srgba_u8(r, g, b, a)
}

// --- set ---------------------------------------------------------------------

pub const ALL_KINDS: [MonsterKind; 12] = [
    MonsterKind::Goblin,
    MonsterKind::Troll,
    MonsterKind::Orc,
    MonsterKind::Vampire,
    MonsterKind::HillGiant,
    MonsterKind::RabidGoblinKing,
    MonsterKind::ElderTroll,
    MonsterKind::WarlordOrc,
    MonsterKind::StarTouchedVampire,
    MonsterKind::CrownedGiant,
    MonsterKind::MothWisp,
    MonsterKind::ThornDrake,
];

#[derive(Resource)]
pub struct SpriteSet {
    pub creatures: HashMap<MonsterKind, Handle<Image>>,
    pub hero: Handle<Image>,
    pub glow: Handle<Image>,
    pub note_high: Handle<Image>,
    pub note_low: Handle<Image>,
    pub floor_a: Handle<Image>,
    pub floor_b: Handle<Image>,
    pub wall: Handle<Image>,
    pub stairs: Handle<Image>,
}

impl SpriteSet {
    pub fn creature(&self, kind: MonsterKind) -> Handle<Image> {
        self.creatures
            .get(&kind)
            .cloned()
            .unwrap_or_else(|| self.hero.clone())
    }

    pub fn bard_note(&self, bright: bool) -> Handle<Image> {
        if bright {
            self.note_high.clone()
        } else {
            self.note_low.clone()
        }
    }
}

/// Build every texture once at startup.
pub fn build_sprites(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let mut creatures = HashMap::new();
    for kind in ALL_KINDS {
        creatures.insert(kind, images.add(draw_creature(kind).into_image()));
    }
    commands.insert_resource(SpriteSet {
        creatures,
        hero: images.add(draw_hero_sprite().into_image()),
        glow: images.add(draw_glow().into_image()),
        note_high: images.add(draw_note(true).into_image()),
        note_low: images.add(draw_note(false).into_image()),
        floor_a: images.add(draw_floor(0).into_image()),
        floor_b: images.add(draw_floor(1).into_image()),
        wall: images.add(draw_wall().into_image()),
        stairs: images.add(draw_stairs().into_image()),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_clips_and_blends() {
        let mut c = Canvas::new(4, 4);
        c.set(-1, -1, (255, 0, 0, 255));
        c.set(99, 99, (255, 0, 0, 255));
        c.set(1, 1, (255, 0, 0, 255));
        assert_eq!(c.get(1, 1), (255, 0, 0, 255));
        assert_eq!(c.get(0, 0), (0, 0, 0, 0));
        // Translucent over opaque blends.
        c.set(1, 1, (0, 0, 255, 128));
        let (pr, _pg, pb, pa) = c.get(1, 1);
        assert!(pb > pr && pa == 255);
    }

    #[test]
    fn every_creature_reads_as_a_sprite() {
        for kind in ALL_KINDS {
            let c = draw_creature(kind);
            let cov = c.coverage();
            assert!(cov > 0.08, "{kind:?} nearly empty ({cov:.2})");
            // Eyes or glow: every face has vivid highlight pixels.
            let bright = c
                .buf
                .chunks_exact(4)
                .any(|p| p[3] > 200 && (p[0] > 230 || p[1] > 230 || p[2] > 230));
            assert!(bright, "{kind:?} has no bright detail");
        }
        assert!(draw_hero_sprite().coverage() > 0.08);
        assert!(draw_floor(0).coverage() > 0.9);
        assert!(draw_wall().coverage() > 0.9);
        assert!(draw_stairs().coverage() > 0.3);
    }

    #[test]
    fn bard_notes_read_as_sprites() {
        let high = draw_note(true);
        let low = draw_note(false);
        assert!(high.coverage() > 0.05, "bright note empty");
        assert!(low.coverage() > 0.05, "dark note empty");
        // Bright note must carry vivid highlight pixels; dark must stay dim.
        let high_bright = high
            .buf
            .chunks_exact(4)
            .any(|p| p[3] > 200 && p[0] > 230 && p[1] > 200);
        assert!(high_bright, "bright note has no gold highlight");
        let low_luma: u32 = low
            .buf
            .chunks_exact(4)
            .map(|p| p[0] as u32 + p[1] as u32 + p[2] as u32)
            .sum();
        let high_luma: u32 = high
            .buf
            .chunks_exact(4)
            .map(|p| p[0] as u32 + p[1] as u32 + p[2] as u32)
            .sum();
        assert!(
            high_luma > low_luma,
            "bright note should outshine dark note"
        );
    }

    #[test]
    fn triangles_stay_in_bounds() {
        let mut c = Canvas::new(48, 56);
        c.tri_up(24, 0, 20, 30, GOLD);
        c.tri_down(24, 55, 20, 30, GOLD);
        c.ell(24, 28, 40, 40, GOLD);
        c.disc(-5, -5, 10, GOLD);
        // No panic = pass; spot-check nothing wrote out of range (clip guards).
        assert!(c.coverage() > 0.0);
    }

    /// Visual review helper: `SPRITE_DUMP=/tmp/shot cargo test -p
    /// embersong-host dump_sprites` writes every texture as PNG (4x nearest).
    #[test]
    fn dump_sprites() {
        let dir = std::env::var("SPRITE_DUMP").unwrap_or_else(|_| "/tmp/embersong-sprites".into());
        std::fs::create_dir_all(&dir).unwrap();
        let mut shots: Vec<(String, Canvas)> = ALL_KINDS
            .iter()
            .map(|k| (format!("{k:?}"), draw_creature(*k)))
            .collect();
        shots.push(("Hero".into(), draw_hero_sprite()));
        shots.push(("FloorA".into(), draw_floor(0)));
        shots.push(("Wall".into(), draw_wall()));
        shots.push(("Stairs".into(), draw_stairs()));
        for (name, c) in shots {
            // 4x nearest upscale for eyeballing.
            let (sw, sh) = (c.w * 4, c.h * 4);
            let mut big = vec![0u8; (sw * sh * 4) as usize];
            for y in 0..sh {
                for x in 0..sw {
                    let src = (((y / 4) * c.w + (x / 4)) * 4) as usize;
                    let dst = ((y * sw + x) * 4) as usize;
                    big[dst..dst + 4].copy_from_slice(&c.buf[src..src + 4]);
                }
            }
            let img = image::RgbaImage::from_raw(sw as u32, sh as u32, big).unwrap();
            img.save(format!("{dir}/{name}.png")).unwrap();
        }
    }
}
