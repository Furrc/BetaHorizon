use glam::Vec3;
use oldmine_common::world::Chunk;
use crate::world_client::WorldClient;
use crate::player::LocalPlayer;
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClientPhase { Menu, Connecting, Playing, Disconnected }

#[derive(Debug, Clone)]
pub enum NetworkEvent {
    Connected { spawn: (i32, i32, i32) },
    TimeUpdate(i64),
    Failed { reason: String },
    Chat(String),
    ServerPosition { x: f64, y: f64, z: f64, yaw: f32, pitch: f32 },
    ChunkLoaded(Chunk),
    ChunkUnloaded { x: i32, z: i32 },
    BlockChanged { x: i32, y: i32, z: i32, id: u8, metadata: u8 },
    EntitySpawn { id: i32, kind: u8, x: f32, y: f32, z: f32, yaw: f32, pitch: f32, name: Option<String> },
    EntityRelativeMove { id: i32, delta: Vec3, yaw: Option<f32>, pitch: Option<f32> },
    EntityTeleport { id: i32, position: Vec3, yaw: f32, pitch: f32 },
    EntityVelocity { id: i32, velocity: Vec3 },
    EntityDestroy(i32),
}

#[derive(Debug, Clone)]
pub struct RemoteEntity {
    pub kind: u8,
    pub name: Option<String>,
    pub position: Vec3,
    pub target: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub velocity: Vec3,
}

#[derive(Debug)]
pub struct ClientState {
    pub phase: ClientPhase,
    pub camera_position: Vec3,
    pub server_spawn: Option<(i32, i32, i32)>,
    pub network_events: Vec<NetworkEvent>,
    pub tick: u64,
    pub world_time: i64,
    pub world: WorldClient,
    pub player: LocalPlayer,
    pub entities: HashMap<i32, RemoteEntity>,
}

impl Default for ClientState {
    fn default() -> Self {
        Self { phase: ClientPhase::Menu, camera_position: Vec3::new(8.0, 72.0, 28.0), server_spawn: None, network_events: Vec::new(), tick: 0, world_time: 0, world: WorldClient::default(), player: LocalPlayer::default(), entities: HashMap::new() }
    }
}

impl ClientState {
    pub fn tick(&mut self, dt: f32) {
        self.tick = self.tick.wrapping_add(1);
        self.world_time = self.world_time.wrapping_add((dt * 20.0) as i64);
        let interpolation = 1.0 - (-12.0 * dt).exp();
        for entity in self.entities.values_mut() { entity.position = entity.position.lerp(entity.target, interpolation); }
        self.network_events.clear();
    }
    pub fn apply_network_event(&mut self, event: NetworkEvent) {
        match &event {
            NetworkEvent::Connected { spawn } => {
                self.phase = ClientPhase::Playing;
                self.server_spawn = Some(*spawn);
                self.camera_position = Vec3::new(spawn.0 as f32 / 32.0, spawn.1 as f32 / 32.0, spawn.2 as f32 / 32.0);
                self.player.set_server_spawn(*spawn);
            }
            NetworkEvent::TimeUpdate(time) => self.world_time = *time,
            NetworkEvent::Failed { .. } => self.phase = ClientPhase::Disconnected,
            NetworkEvent::Chat(message) => eprintln!("server chat: {message}"),
            NetworkEvent::ServerPosition { x, y, z, yaw, pitch } => {
                self.player.position = Vec3::new(*x as f32, *y as f32, *z as f32);
                self.player.yaw = yaw.to_radians();
                self.player.pitch = pitch.to_radians();
            }
            NetworkEvent::ChunkLoaded(chunk) => self.world.load_chunk(chunk.clone()),
            NetworkEvent::ChunkUnloaded { x, z } => self.world.unload_chunk(*x, *z),
            NetworkEvent::BlockChanged { x, y, z, id, metadata } => self.world.set_block_state(*x, *y, *z, *id, *metadata),
            NetworkEvent::EntitySpawn { id, kind, x, y, z, yaw, pitch, name } => {
                let position = Vec3::new(*x, *y, *z);
                self.entities.insert(*id, RemoteEntity { kind: *kind, name: name.clone(), position, target: position, yaw: *yaw, pitch: *pitch, velocity: Vec3::ZERO });
            }
            NetworkEvent::EntityRelativeMove { id, delta, yaw, pitch } => if let Some(entity) = self.entities.get_mut(id) {
                entity.target += *delta; if let Some(yaw) = yaw { entity.yaw = *yaw; } if let Some(pitch) = pitch { entity.pitch = *pitch; }
            },
            NetworkEvent::EntityTeleport { id, position, yaw, pitch } => if let Some(entity) = self.entities.get_mut(id) { entity.target = *position; entity.yaw = *yaw; entity.pitch = *pitch; },
            NetworkEvent::EntityVelocity { id, velocity } => if let Some(entity) = self.entities.get_mut(id) { entity.velocity = *velocity; },
            NetworkEvent::EntityDestroy(id) => { self.entities.remove(id); }
        }
        self.network_events.push(event);
    }
}
