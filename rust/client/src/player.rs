use glam::Vec3;
use crate::world_client::WorldClient;

#[derive(Debug, Clone)]
pub struct LocalPlayer {
    pub position: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub on_ground: bool,
}

impl Default for LocalPlayer {
    fn default() -> Self { Self { position: Vec3::new(8.0, 65.0, 8.0), velocity: Vec3::ZERO, yaw: 0.0, pitch: 0.0, on_ground: false } }
}

impl LocalPlayer {
    pub fn set_server_spawn(&mut self, spawn: (i32, i32, i32)) {
        self.position = Vec3::new(spawn.0 as f32 / 32.0, spawn.1 as f32 / 32.0, spawn.2 as f32 / 32.0);
        self.velocity = Vec3::ZERO;
    }

    pub fn tick(&mut self, world: &WorldClient, forward: f32, strafe: f32, jump: bool, dt: f32) {
        let speed = if self.on_ground { 4.3 } else { 3.0 };
        let yaw = self.yaw;
        // Matches Beta's Entity.moveFlying(strafe, forward): left is positive strafe.
        let direction = Vec3::new(yaw.cos() * strafe - yaw.sin() * forward, 0.0, yaw.cos() * forward + yaw.sin() * strafe);
        self.velocity.x = direction.x * speed;
        self.velocity.z = direction.z * speed;
        if jump && self.on_ground { self.velocity.y = 8.0; self.on_ground = false; }
        self.velocity.y -= 24.0 * dt;
        let next = self.position + self.velocity * dt;
        self.position.x = if collides(world, next.x, self.position.y, self.position.z) { self.position.x } else { next.x };
        self.position.z = if collides(world, self.position.x, self.position.y, next.z) { self.position.z } else { next.z };
        if collides(world, self.position.x, next.y, self.position.z) {
            if self.velocity.y < 0.0 { self.on_ground = true; }
            self.velocity.y = 0.0;
        } else { self.position.y = next.y; self.on_ground = false; }
    }
}

fn collides(world: &WorldClient, x: f32, y: f32, z: f32) -> bool {
    let min_x = (x - 0.3).floor() as i32; let max_x = (x + 0.3).floor() as i32;
    let min_y = (y - 0.1).floor() as i32; let max_y = (y + 1.7).floor() as i32;
    let min_z = (z - 0.3).floor() as i32; let max_z = (z + 0.3).floor() as i32;
    for bx in min_x..=max_x { for by in min_y..=max_y { for bz in min_z..=max_z {
        for bounds in world.collision_boxes_at(bx, by, bz) {
        if x + 0.3 > bx as f32 + bounds[0] && x - 0.3 < bx as f32 + bounds[3]
            && y + 1.7 > by as f32 + bounds[1] && y - 0.1 < by as f32 + bounds[4]
            && z + 0.3 > bz as f32 + bounds[2] && z - 0.3 < bz as f32 + bounds[5] { return true; }
        }
    }}}
    false
}
