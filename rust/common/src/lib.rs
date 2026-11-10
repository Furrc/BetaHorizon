use serde::{Deserialize, Serialize};
use noise::{NoiseFn, OpenSimplex};
use std::collections::HashMap;

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Packet {
    Handshake { protocol: u16, username: String },
    KeepAlive,
    Disconnect { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WirePacket {
    pub packet: Packet,
}

pub mod world {
    use super::*;

    pub const WORLD_HEIGHT: usize = 128;
    pub const CHUNK_SIZE: usize = 16;
    pub const CHUNK_VOLUME: usize = CHUNK_SIZE * CHUNK_SIZE * WORLD_HEIGHT;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
    #[repr(u8)]
    pub enum BlockId {
        Air = 0,
        Stone = 1,
        Grass = 2,
        Dirt = 3,
        Bedrock = 7,
        Water = 9,
        Sand = 12,
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct BlockDefinition {
        pub id: u8,
        pub name: &'static str,
        pub solid: bool,
        pub opaque: bool,
        pub translucent: bool,
        pub side_tiles: [u16; 6],
        pub bounds: [f32; 6],
    }

    pub fn block_definition(id: u8) -> BlockDefinition {
        let (name, solid, opaque, translucent, tiles) = match id {
            0 => ("air", false, false, false, [0; 6]),
            1 => ("stone", true, true, false, [1; 6]),
            2 => ("grass", true, true, false, [2, 0, 3, 3, 3, 3]),
            3 => ("dirt", true, true, false, [2; 6]),
            4 => ("cobblestone", true, true, false, [16; 6]),
            5 => ("planks", true, true, false, [4; 6]),
            6 => ("sapling", false, false, false, [15; 6]),
            7 => ("bedrock", true, true, false, [17; 6]),
            8 | 9 => ("water", false, false, true, [205; 6]),
            10 | 11 => ("lava", false, false, true, [237; 6]),
            12 => ("sand", true, true, false, [18; 6]),
            13 => ("gravel", true, true, false, [19; 6]),
            14 => ("gold_ore", true, true, false, [32; 6]),
            15 => ("iron_ore", true, true, false, [33; 6]),
            16 => ("coal_ore", true, true, false, [34; 6]),
            17 => ("log", true, true, false, [20, 20, 21, 21, 20, 20]),
            18 => ("leaves", true, false, true, [52; 6]),
            20 => ("glass", true, false, true, [49; 6]),
            21 => ("lapis_ore", true, true, false, [160; 6]),
            22 => ("lapis_block", true, true, false, [144; 6]),
            23 => ("dispenser", true, true, false, [45, 45, 46, 46, 44, 44]),
            24 => ("sandstone", true, true, false, [176, 176, 192, 192, 176, 176]),
            26 => ("bed", false, false, false, [142; 6]),
            27 => ("powered_rail", false, false, false, [179; 6]),
            28 => ("detector_rail", false, false, false, [195; 6]),
            29 => ("sticky_piston", true, true, false, [106; 6]),
            30 => ("web", false, false, true, [11; 6]),
            31 => ("tall_grass", false, false, false, [39; 6]),
            32 => ("dead_bush", false, false, false, [55; 6]),
            33 | 34 | 36 => ("piston", true, true, false, [107; 6]),
            35 => ("wool", true, true, false, [64; 6]),
            50 => ("torch", false, false, true, [80; 6]),
            51 => ("fire", false, false, true, [31; 6]),
            52 => ("mob_spawner", true, true, false, [65; 6]),
            53 | 67 => ("stairs", true, false, false, [4; 6]),
            55 => ("redstone_wire", false, false, false, [164; 6]),
            41 => ("gold_block", true, true, false, [23; 6]),
            42 => ("iron_block", true, true, false, [22; 6]),
            37 => ("yellow_flower", false, false, false, [13; 6]),
            38 => ("red_flower", false, false, false, [12; 6]),
            39 => ("brown_mushroom", false, false, false, [29; 6]),
            40 => ("red_mushroom", false, false, false, [28; 6]),
            43 => ("double_slab", true, true, false, [6; 6]),
            44 => ("slab", true, false, false, [6; 6]),
            45 => ("bricks", true, true, false, [7; 6]),
            46 => ("tnt", true, true, false, [8, 8, 9, 10, 8, 8]),
            47 => ("bookshelf", true, true, false, [35, 4, 35, 35, 35, 35]),
            48 => ("mossy_cobblestone", true, true, false, [36; 6]),
            49 => ("obsidian", true, true, false, [37; 6]),
            54 => ("chest", true, true, false, [25; 6]),
            56 => ("diamond_ore", true, true, false, [50; 6]),
            57 => ("diamond_block", true, true, false, [24; 6]),
            58 => ("crafting_table", true, true, false, [43, 43, 59, 4, 43, 43]),
            59 => ("crops", false, false, false, [88; 6]),
            60 => ("farmland", true, false, false, [87; 6]),
            61 | 62 => ("furnace", true, true, false, [45, 45, 62, 45, 45, 45]),
            63 | 68 => ("sign", false, false, true, [4; 6]),
            64 | 71 => ("door", true, false, true, [97; 6]),
            65 => ("ladder", false, false, true, [83; 6]),
            66 => ("rail", false, false, false, [128; 6]),
            69 => ("lever", false, false, false, [96; 6]),
            70 | 72 => ("pressure_plate", false, false, false, [1; 6]),
            73 | 74 => ("redstone_ore", true, true, false, [51; 6]),
            75 | 76 => ("redstone_torch", false, false, true, [115; 6]),
            77 => ("button", false, false, false, [1; 6]),
            78 => ("snow_layer", false, false, false, [66; 6]),
            79 => ("ice", true, false, true, [67; 6]),
            80 => ("snow_block", true, true, false, [66; 6]),
            83 => ("reeds", false, false, false, [73; 6]),
            84 => ("jukebox", true, true, false, [74; 6]),
            85 => ("fence", true, false, false, [4; 6]),
            86 | 91 => ("pumpkin", true, true, false, [102; 6]),
            81 => ("cactus", true, true, false, [70, 70, 69, 69, 70, 70]),
            82 => ("clay", true, true, false, [72; 6]),
            87 => ("netherrack", true, true, false, [103; 6]),
            88 => ("soul_sand", true, true, false, [104; 6]),
            89 => ("glowstone", true, true, false, [105; 6]),
            90 => ("portal", false, false, true, [14; 6]),
            92 => ("cake", false, false, false, [121; 6]),
            93 | 94 => ("repeater", false, false, false, [131; 6]),
            95 => ("locked_chest", true, true, false, [25; 6]),
            96 => ("trapdoor", false, false, false, [84; 6]),
            _ => ("unknown", true, true, false, [1; 6]),
        };
        let bounds = match id {
            44 => [0.0, 0.0, 0.0, 1.0, 0.5, 1.0],
            0 | 6 | 26..=28 | 31 | 32 | 37..=40 | 50 | 51 | 53 | 55 | 59 | 63 | 65..=70 | 72..=77 | 83 | 90 | 92..=96 => [0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            60 => [0.0, 0.0, 0.0, 1.0, 15.0 / 16.0, 1.0],
            81 => [1.0 / 16.0, 0.0, 1.0 / 16.0, 15.0 / 16.0, 15.0 / 16.0, 15.0 / 16.0],
            88 => [0.0, 0.0, 0.0, 1.0, 14.0 / 16.0, 1.0],
            _ => [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        };
        BlockDefinition { id, name, solid, opaque, translucent, side_tiles: tiles, bounds }
    }

    pub fn block_bounds(id: u8, metadata: u8) -> [f32; 6] {
        match id {
            44 if metadata & 8 != 0 => [0.0, 0.5, 0.0, 1.0, 1.0, 1.0],
            43 => [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            78 => {
                let height = (2 * (1 + (metadata & 7))) as f32 / 16.0;
                [0.0, 0.0, 0.0, 1.0, height, 1.0]
            }
            53 | 67 | 81 | 88 => block_definition(id).bounds,
            27 | 28 | 66 => [0.0, 0.0, 0.0, 1.0, if metadata & 7 >= 2 && metadata & 7 <= 5 { 10.0 / 16.0 } else { 2.0 / 16.0 }, 1.0],
            55 => [0.0, 0.0, 0.0, 1.0, 1.0 / 16.0, 1.0],
            64 | 71 => door_bounds(metadata),
            85 => [6.0 / 16.0, 0.0, 6.0 / 16.0, 10.0 / 16.0, 1.0, 10.0 / 16.0],
            70 | 72 => [1.0 / 16.0, 0.0, 1.0 / 16.0, 15.0 / 16.0, if metadata == 1 { 2.0 / 16.0 } else { 1.0 / 16.0 }, 15.0 / 16.0],
            96 => trapdoor_bounds(metadata),
            id => block_definition(id).bounds,
        }
    }

    pub fn block_boxes(id: u8, metadata: u8) -> Vec<[f32; 6]> {
        match id {
            27 | 28 | 66 | 55 | 70 | 72 | 96 => rail_or_thin_boxes(id, metadata),
            64 | 71 => vec![block_bounds(id, metadata)],
            85 => vec![block_bounds(id, metadata)],
            53 | 67 => match metadata & 3 {
                0 => vec![[0.0,0.0,0.0,0.5,0.5,1.0], [0.5,0.0,0.0,1.0,1.0,1.0]],
                1 => vec![[0.0,0.0,0.0,0.5,1.0,1.0], [0.5,0.0,0.0,1.0,0.5,1.0]],
                2 => vec![[0.0,0.0,0.0,1.0,0.5,0.5], [0.0,0.0,0.5,1.0,1.0,1.0]],
                _ => vec![[0.0,0.0,0.0,1.0,1.0,0.5], [0.0,0.0,0.5,1.0,0.5,1.0]],
            },
            81 => vec![[1.0/16.0,0.0,1.0/16.0,15.0/16.0,15.0/16.0,15.0/16.0]],
            88 => vec![[0.0,0.0,0.0,1.0,14.0/16.0,1.0]],
            id => {
                let bounds = block_bounds(id, metadata);
                if bounds[3] <= bounds[0] || bounds[4] <= bounds[1] || bounds[5] <= bounds[2] { Vec::new() } else { vec![bounds] }
            }
        }
    }

    fn rail_or_thin_boxes(id: u8, metadata: u8) -> Vec<[f32; 6]> {
        if matches!(id, 27 | 28 | 66) {
            let axis_x = metadata & 1 != 0;
            return if axis_x {
                vec![[0.0, 0.0, 7.0 / 16.0, 1.0, 2.0 / 16.0, 9.0 / 16.0]]
            } else {
                vec![[7.0 / 16.0, 0.0, 0.0, 9.0 / 16.0, 2.0 / 16.0, 1.0]]
            };
        }
        vec![block_bounds(id, metadata)]
    }

    fn door_bounds(metadata: u8) -> [f32; 6] {
        let thickness = 3.0 / 16.0;
        let open = metadata & 4 != 0;
        match (metadata & 3, open) {
            (0, false) => [0.0, 0.0, 0.0, thickness, 1.0, 1.0],
            (1, false) => [1.0 - thickness, 0.0, 0.0, 1.0, 1.0, 1.0],
            (2, false) => [0.0, 0.0, 0.0, 1.0, 1.0, thickness],
            (3, false) => [0.0, 0.0, 1.0 - thickness, 1.0, 1.0, 1.0],
            (0, true) => [0.0, 0.0, 0.0, 1.0, 1.0, thickness],
            (1, true) => [0.0, 0.0, 1.0 - thickness, 1.0, 1.0, 1.0],
            (2, true) => [0.0, 0.0, 1.0 - thickness, 1.0, 1.0, 1.0],
            _ => [0.0, 0.0, 0.0, 1.0, 1.0, thickness],
        }
    }

    pub fn fence_boxes(metadata: u8, north: bool, south: bool, west: bool, east: bool) -> Vec<[f32; 6]> {
        let mut boxes = vec![block_bounds(85, metadata)];
        if north { boxes.push([7.0 / 16.0, 6.0 / 16.0, 0.0, 9.0 / 16.0, 15.0 / 16.0, 6.0 / 16.0]); }
        if south { boxes.push([7.0 / 16.0, 6.0 / 16.0, 10.0 / 16.0, 9.0 / 16.0, 15.0 / 16.0, 1.0]); }
        if west { boxes.push([0.0, 6.0 / 16.0, 7.0 / 16.0, 6.0 / 16.0, 15.0 / 16.0, 9.0 / 16.0]); }
        if east { boxes.push([10.0 / 16.0, 6.0 / 16.0, 7.0 / 16.0, 1.0, 15.0 / 16.0, 9.0 / 16.0]); }
        boxes
    }

    fn trapdoor_bounds(metadata: u8) -> [f32; 6] {
        let thickness = 3.0 / 16.0;
        if metadata & 4 == 0 { return [0.0, 0.0, 0.0, 1.0, thickness, 1.0]; }
        match metadata & 3 {
            0 => [0.0, 0.0, 1.0 - thickness, 1.0, 1.0, 1.0],
            1 => [0.0, 0.0, 0.0, 1.0, 1.0, thickness],
            2 => [1.0 - thickness, 0.0, 0.0, 1.0, 1.0, 1.0],
            _ => [0.0, 0.0, 0.0, thickness, 1.0, 1.0],
        }
    }

    pub fn block_face_tile(id: u8, metadata: u8, face: usize) -> u16 {
        if matches!(id, 8..=11) {
            let base = if matches!(id, 8 | 9) { 205 } else { 237 };
            let side = [4, 5, 0, 1, 2, 3][face];
            return if side <= 1 { base } else { base + 1 };
        }
        if id == 17 {
            let side = [4, 5, 0, 1, 2, 3][face];
            return if side <= 1 { 21 } else if metadata & 3 == 1 { 116 } else if metadata & 3 == 2 { 117 } else { 20 };
        }
        if id == 43 || id == 44 {
            let slab_type = metadata & 7;
            let side = [4, 5, 0, 1, 2, 3][face];
            return match slab_type {
                0 => if side <= 1 { 6 } else { 5 },
                1 => if side == 0 { 208 } else if side == 1 { 176 } else { 192 },
                2 => if side <= 1 { 4 } else { 5 },
                3 => if side <= 1 { 16 } else { 5 },
                _ => 6,
            };
        }
        block_definition(id).side_tiles[[4, 5, 0, 1, 2, 3][face]]
    }

    pub fn block_is_crossed(id: u8) -> bool {
        matches!(id, 6 | 31 | 32 | 37..=40 | 59 | 83)
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct Chunk {
        pub x: i32,
        pub z: i32,
        blocks: Vec<u8>,
        metadata: Vec<u8>,
        block_light: Vec<u8>,
        sky_light: Vec<u8>,
    }

    impl Chunk {
        pub fn new_empty(x: i32, z: i32) -> Self {
            Self { x, z, blocks: vec![BlockId::Air as u8; CHUNK_VOLUME], metadata: vec![0; CHUNK_VOLUME], block_light: vec![0; CHUNK_VOLUME], sky_light: vec![0; CHUNK_VOLUME] }
        }

        pub fn block(&self, x: usize, y: usize, z: usize) -> Option<u8> {
            if x >= CHUNK_SIZE || y >= WORLD_HEIGHT || z >= CHUNK_SIZE {
                return None;
            }
            Some(self.blocks[(x << 11) | (z << 7) | y])
        }

        pub fn set_block(&mut self, x: usize, y: usize, z: usize, id: u8) -> bool {
            if x >= CHUNK_SIZE || y >= WORLD_HEIGHT || z >= CHUNK_SIZE {
                return false;
            }
            self.blocks[(x << 11) | (z << 7) | y] = id;
            true
        }

        pub fn metadata(&self, x: usize, y: usize, z: usize) -> Option<u8> {
            if x >= CHUNK_SIZE || y >= WORLD_HEIGHT || z >= CHUNK_SIZE { return None; }
            Some(self.metadata[(x << 11) | (z << 7) | y])
        }
        pub fn light(&self, x: usize, y: usize, z: usize) -> Option<(u8, u8)> {
            if x >= CHUNK_SIZE || y >= WORLD_HEIGHT || z >= CHUNK_SIZE { return None; }
            let index = (x << 11) | (z << 7) | y;
            Some((self.block_light[index], self.sky_light[index]))
        }

        pub fn set_light(&mut self, x: usize, y: usize, z: usize, block_light: u8, sky_light: u8) -> bool {
            if x >= CHUNK_SIZE || y >= WORLD_HEIGHT || z >= CHUNK_SIZE { return false; }
            let index = (x << 11) | (z << 7) | y;
            self.block_light[index] = block_light.min(15);
            self.sky_light[index] = sky_light.min(15);
            true
        }

        pub fn set_block_state(&mut self, x: usize, y: usize, z: usize, id: u8, metadata: u8) -> bool {
            if !self.set_block(x, y, z, id) { return false; }
            self.metadata[(x << 11) | (z << 7) | y] = metadata & 0x0f;
            true
        }

        pub fn blocks(&self) -> &[u8] { &self.blocks }

        pub fn from_blocks(x: i32, z: i32, blocks: Vec<u8>) -> Option<Self> {
            if blocks.len() != CHUNK_VOLUME { return None; }
            Some(Self { x, z, metadata: vec![0; CHUNK_VOLUME], block_light: vec![0; CHUNK_VOLUME], sky_light: vec![0; CHUNK_VOLUME], blocks })
        }

        pub fn from_sections(x: i32, z: i32, blocks: Vec<u8>, metadata: Vec<u8>, block_light: Vec<u8>, sky_light: Vec<u8>) -> Option<Self> {
            if blocks.len() != CHUNK_VOLUME || metadata.len() != CHUNK_VOLUME || block_light.len() != CHUNK_VOLUME || sky_light.len() != CHUNK_VOLUME { return None; }
            Some(Self { x, z, blocks, metadata, block_light, sky_light })
        }
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct World {
        pub seed: u32,
        pub time: u64,
        chunks: HashMap<(i32, i32), Chunk>,
    }

    impl World {
        pub fn new(seed: u32) -> Self {
            Self { seed, time: 0, chunks: HashMap::new() }
        }

        pub fn generate_chunk(&mut self, chunk_x: i32, chunk_z: i32) -> &Chunk {
            let seed = self.seed;
            self.chunks.entry((chunk_x, chunk_z)).or_insert_with(|| {
                generate_chunk(seed, chunk_x, chunk_z)
            })
        }

        pub fn chunk(&self, x: i32, z: i32) -> Option<&Chunk> {
            self.chunks.get(&(x, z))
        }

        pub fn tick(&mut self) { self.time = self.time.wrapping_add(1); }
    }

    pub fn generate_chunk(seed: u32, chunk_x: i32, chunk_z: i32) -> Chunk {
        let noise = OpenSimplex::new(seed);
        let mut chunk = Chunk::new_empty(chunk_x, chunk_z);
        for x in 0..CHUNK_SIZE {
            for z in 0..CHUNK_SIZE {
                let world_x = chunk_x * CHUNK_SIZE as i32 + x as i32;
                let world_z = chunk_z * CHUNK_SIZE as i32 + z as i32;
                let n = noise.get([world_x as f64 / 48.0, world_z as f64 / 48.0]);
                let surface = (62.0 + n * 9.0).round().clamp(8.0, 110.0) as usize;
                for y in 0..=surface {
                    let block = if y == 0 {
                        BlockId::Bedrock as u8
                    } else if y < surface.saturating_sub(3) {
                        BlockId::Stone as u8
                    } else if y < surface {
                        BlockId::Dirt as u8
                    } else {
                        BlockId::Grass as u8
                    };
                    chunk.set_block(x, y, z, block);
                }
                if surface < 62 {
                    for y in surface + 1..=62 {
                        chunk.set_block(x, y, z, BlockId::Water as u8);
                    }
                }
            }
        }
        chunk
    }
}
