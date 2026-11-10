use std::collections::{HashMap, HashSet};
use oldmine_common::world::{Chunk, CHUNK_SIZE, WORLD_HEIGHT};

#[derive(Debug, Default)]
pub struct WorldClient {
    chunks: HashMap<(i32, i32), Chunk>,
    dirty: HashSet<(i32, i32)>,
}

impl WorldClient {
    pub fn load_chunk(&mut self, chunk: Chunk) {
        let key = (chunk.x, chunk.z);
        self.chunks.insert(key, chunk);
        self.dirty.insert(key);
    }

    pub fn unload_chunk(&mut self, x: i32, z: i32) {
        self.chunks.remove(&(x, z));
        self.dirty.remove(&(x, z));
    }

    pub fn set_block(&mut self, x: i32, y: i32, z: i32, id: u8) { self.set_block_state(x, y, z, id, 0); }

    pub fn set_light(&mut self, x: i32, y: i32, z: i32, block_light: u8, sky_light: u8) {
        if !(0..WORLD_HEIGHT as i32).contains(&y) { return; }
        let chunk_x = x.div_euclid(CHUNK_SIZE as i32);
        let chunk_z = z.div_euclid(CHUNK_SIZE as i32);
        let local_x = x.rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_z = z.rem_euclid(CHUNK_SIZE as i32) as usize;
        if let Some(chunk) = self.chunks.get_mut(&(chunk_x, chunk_z)) {
            chunk.set_light(local_x, y as usize, local_z, block_light, sky_light);
        }
    }

    pub fn set_block_state(&mut self, x: i32, y: i32, z: i32, id: u8, metadata: u8) {
        if !(0..WORLD_HEIGHT as i32).contains(&y) { return; }
        let chunk_x = x.div_euclid(CHUNK_SIZE as i32);
        let chunk_z = z.div_euclid(CHUNK_SIZE as i32);
        let local_x = x.rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_z = z.rem_euclid(CHUNK_SIZE as i32) as usize;
        if let Some(chunk) = self.chunks.get_mut(&(chunk_x, chunk_z)) {
            chunk.set_block_state(local_x, y as usize, local_z, id, metadata);
            self.dirty.insert((chunk_x, chunk_z));
            if local_x == 0 { self.dirty.insert((chunk_x - 1, chunk_z)); }
            if local_x + 1 == CHUNK_SIZE { self.dirty.insert((chunk_x + 1, chunk_z)); }
            if local_z == 0 { self.dirty.insert((chunk_x, chunk_z - 1)); }
            if local_z + 1 == CHUNK_SIZE { self.dirty.insert((chunk_x, chunk_z + 1)); }
        }
    }

    pub fn take_dirty(&mut self) -> Vec<(i32, i32)> { self.dirty.drain().collect() }
    pub fn mark_dirty(&mut self, x: i32, z: i32) { self.dirty.insert((x, z)); }
    pub fn chunk(&self, x: i32, z: i32) -> Option<&Chunk> { self.chunks.get(&(x, z)) }
    pub fn loaded_count(&self) -> usize { self.chunks.len() }
    pub fn chunks(&self) -> impl Iterator<Item = &Chunk> { self.chunks.values() }

    pub fn block_at(&self, x: i32, y: i32, z: i32) -> u8 {
        if !(0..WORLD_HEIGHT as i32).contains(&y) { return 0; }
        let chunk_x = x.div_euclid(CHUNK_SIZE as i32);
        let chunk_z = z.div_euclid(CHUNK_SIZE as i32);
        let local_x = x.rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_z = z.rem_euclid(CHUNK_SIZE as i32) as usize;
        self.chunks.get(&(chunk_x, chunk_z))
            .and_then(|chunk| chunk.block(local_x, y as usize, local_z))
            .unwrap_or(0)
    }

    pub fn is_solid_at(&self, x: i32, y: i32, z: i32) -> bool {
        let (id, metadata, _) = self.state_at(x, y, z);
        let bounds = oldmine_common::world::block_bounds(id, metadata);
        oldmine_common::world::block_definition(id).solid && bounds[3] > bounds[0] && bounds[4] > bounds[1] && bounds[5] > bounds[2]
    }

    pub fn collision_boxes_at(&self, x: i32, y: i32, z: i32) -> Vec<[f32; 6]> {
        let (id, metadata, _) = self.state_at(x, y, z);
        if !oldmine_common::world::block_definition(id).solid { return Vec::new(); }
        if id == 85 {
            return oldmine_common::world::fence_boxes(metadata, self.block_at(x, y, z - 1) == 85, self.block_at(x, y, z + 1) == 85, self.block_at(x - 1, y, z) == 85, self.block_at(x + 1, y, z) == 85);
        }
        oldmine_common::world::block_boxes(id, metadata)
    }

    pub fn is_face_occluded(&self, x: i32, y: i32, z: i32, face: usize, bounds: [f32; 6]) -> bool {
        // A face inside a compound shape (for example the riser inside stairs)
        // is not on the block boundary and must never be removed by a neighbor.
        let reaches_boundary = match face {
            0 => bounds[0] == 0.0, 1 => bounds[3] == 1.0,
            2 => bounds[1] == 0.0, 3 => bounds[4] == 1.0,
            4 => bounds[2] == 0.0, _ => bounds[5] == 1.0,
        };
        if !reaches_boundary { return false; }
        let (nx, ny, nz) = match face {
            0 => (x - 1, y, z), 1 => (x + 1, y, z),
            2 => (x, y - 1, z), 3 => (x, y + 1, z),
            4 => (x, y, z - 1), _ => (x, y, z + 1),
        };
        let (neighbor, metadata, _) = self.state_at(nx, ny, nz);
        let definition = oldmine_common::world::block_definition(neighbor);
        let full_cube = oldmine_common::world::block_boxes(neighbor, metadata)
            .iter().any(|b| *b == [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
        (definition.opaque && full_cube) || (definition.translucent && neighbor == self.block_at(x, y, z))
    }

    pub fn is_opaque_at(&self, x: i32, y: i32, z: i32) -> bool {
        oldmine_common::world::block_definition(self.block_at(x, y, z)).opaque
    }

    pub fn state_at(&self, x: i32, y: i32, z: i32) -> (u8, u8, u8) {
        if !(0..WORLD_HEIGHT as i32).contains(&y) { return (0, 0, 0); }
        let chunk_x = x.div_euclid(CHUNK_SIZE as i32);
        let chunk_z = z.div_euclid(CHUNK_SIZE as i32);
        let local_x = x.rem_euclid(CHUNK_SIZE as i32) as usize;
        let local_z = z.rem_euclid(CHUNK_SIZE as i32) as usize;
        self.chunks.get(&(chunk_x, chunk_z)).map(|chunk| {
            let id = chunk.block(local_x, y as usize, local_z).unwrap_or(0);
            let metadata = chunk.metadata(local_x, y as usize, local_z).unwrap_or(0);
            let (block, sky) = chunk.light(local_x, y as usize, local_z).unwrap_or((0, 0));
            (id, metadata, block.max(sky))
        }).unwrap_or((0, 0, 0))
    }
}
