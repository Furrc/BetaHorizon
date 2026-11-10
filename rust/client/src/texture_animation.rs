/// CPU port of Beta 1.7.3 `TextureWaterFX.onTick` for the 16x16 moving-water atlas tile.
/// Vanilla uses `Math.random()`; a local deterministic generator makes debug captures repeatable.
pub struct WaterFx {
    field_g: [f32; 256],
    field_h: [f32; 256],
    field_i: [f32; 256],
    field_j: [f32; 256],
    image_data: [u8; 16 * 16 * 4],
    random_state: u64,
    pub ticks: u64,
}

/// CPU port of Beta 1.7.3 `TextureLavaFX` and `TextureLavaFlowFX`.
/// `flowing` matches TextureLavaFlowFX's tileSize=2 and vertical pixel offset.
pub struct LavaFx {
    field_g: [f32; 256],
    field_h: [f32; 256],
    field_i: [f32; 256],
    field_j: [f32; 256],
    image_data: [u8; 16 * 16 * 4],
    random_state: u64,
    flowing: bool,
    pub ticks: u64,
}

impl LavaFx {
    pub fn still() -> Self { Self::new(false, 0x1A_AA_73_01) }
    pub fn flowing() -> Self { Self::new(true, 0x1A_AA_73_02) }

    fn new(flowing: bool, random_state: u64) -> Self {
        Self {
            field_g: [0.0; 256], field_h: [0.0; 256],
            field_i: [0.0; 256], field_j: [0.0; 256],
            image_data: [0; 16 * 16 * 4], random_state, flowing, ticks: 0,
        }
    }

    pub fn tick(&mut self) {
        self.tick_with_anaglyph(false);
    }

    pub fn tick_with_anaglyph(&mut self, anaglyph_enabled: bool) {
        self.ticks += 1;
        for x in 0..16 {
            for y in 0..16 {
                let offset_y = ((y as f32 * std::f32::consts::TAU / 16.0).sin() * 1.2) as i32;
                let offset_x = ((x as f32 * std::f32::consts::TAU / 16.0).sin() * 1.2) as i32;
                let mut sum = 0.0_f32;
                for sx in -1_i32..=1 {
                    for sy in -1_i32..=1 {
                        let sample_x = (x as i32 + sx + offset_y).rem_euclid(16) as usize;
                        let sample_y = (y as i32 + sy + offset_x).rem_euclid(16) as usize;
                        sum += self.field_g[sample_x + sample_y * 16];
                    }
                }
                let index = x + y * 16;
                self.field_h[index] = sum / 10.0
                    + (self.field_i[index]
                        + self.field_i[(x + 1) % 16 + y * 16]
                        + self.field_i[(x + 1) % 16 + ((y + 1) % 16) * 16]
                        + self.field_i[x + ((y + 1) % 16) * 16]) / 4.0 * 0.8;
                self.field_i[index] = (self.field_i[index] + self.field_j[index] * 0.01).max(0.0);
                self.field_j[index] -= 0.06;
                if self.next_random() < 0.005 { self.field_j[index] = 1.5; }
            }
        }
        std::mem::swap(&mut self.field_h, &mut self.field_g);

        for index in 0_usize..256 {
            let source = if self.flowing { index.wrapping_sub((self.ticks / 3 * 16) as usize) & 255 } else { index };
            let value = (self.field_g[source] * 2.0).clamp(0.0, 1.0);
            let mut red = (value * 100.0 + 155.0) as u8;
            let mut green = (value * value * 255.0) as u8;
            let mut blue = (value.powi(4) * 128.0) as u8;
            if anaglyph_enabled {
                let (r, g, b) = (red as u32, green as u32, blue as u32);
                red = ((r * 30 + g * 59 + b * 11) / 100) as u8;
                green = ((r * 30 + g * 70) / 100) as u8;
                blue = ((r * 30 + b * 70) / 100) as u8;
            }
            self.image_data[index * 4] = red;
            self.image_data[index * 4 + 1] = green;
            self.image_data[index * 4 + 2] = blue;
            self.image_data[index * 4 + 3] = 255;
        }
    }

    pub fn pixels(&self) -> &[u8; 16 * 16 * 4] { &self.image_data }
    pub fn tile_size(&self) -> u32 { if self.flowing { 2 } else { 1 } }

    fn next_random(&mut self) -> f32 {
        self.random_state = self.random_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.random_state >> 40) as u32) as f32 / ((1_u32 << 24) as f32)
    }
}

impl Default for WaterFx {
    fn default() -> Self {
        Self {
            field_g: [0.0; 256],
            field_h: [0.0; 256],
            field_i: [0.0; 256],
            field_j: [0.0; 256],
            image_data: [0; 16 * 16 * 4],
            random_state: 0xB1_7A_73_03,
            ticks: 0,
        }
    }
}

impl WaterFx {
    pub fn tick(&mut self) {
        self.tick_with_anaglyph(false);
    }

    pub fn tick_with_anaglyph(&mut self, anaglyph_enabled: bool) {
        self.ticks += 1;
        for x in 0..16 {
            for y in 0..16 {
                let mut sum = 0.0_f32;
                for offset in -1_i32..=1 {
                    let sample_x = (x as i32 + offset).rem_euclid(16) as usize;
                    let index = sample_x + y * 16;
                    sum += self.field_g[index];
                }
                let index = x + y * 16;
                self.field_h[index] = sum / 3.3 + self.field_i[index] * 0.8;
            }
        }

        for x in 0..16 {
            for y in 0..16 {
                let index = x + y * 16;
                self.field_i[index] += self.field_j[index] * 0.05;
                if self.field_i[index] < 0.0 { self.field_i[index] = 0.0; }
                self.field_j[index] -= 0.1;
                if self.next_random() < 0.05 { self.field_j[index] = 0.5; }
            }
        }

        std::mem::swap(&mut self.field_h, &mut self.field_g);
        for index in 0..256 {
            let value = self.field_g[index].clamp(0.0, 1.0);
            let squared = value * value;
            let mut red = (32.0 + squared * 32.0) as u32;
            let mut green = (50.0 + squared * 64.0) as u32;
            let mut blue = 255_u32;
            if anaglyph_enabled {
                let r = (red * 30 + green * 59 + blue * 11) / 100;
                let g = (red * 30 + green * 70) / 100;
                let b = (red * 30 + blue * 70) / 100;
                red = r;
                green = g;
                blue = b;
            }
            self.image_data[index * 4] = red as u8;
            self.image_data[index * 4 + 1] = green as u8;
            self.image_data[index * 4 + 2] = blue as u8;
            self.image_data[index * 4 + 3] = (146.0 + squared * 50.0) as u8;
        }
    }

    pub fn pixels(&self) -> &[u8; 16 * 16 * 4] { &self.image_data }

    fn next_random(&mut self) -> f32 {
        self.random_state = self.random_state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.random_state >> 40) as u32) as f32 / ((1_u32 << 24) as f32)
    }
}
