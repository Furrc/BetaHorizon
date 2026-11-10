use flate2::read::ZlibDecoder;
use glam::Vec3;
use oldmine_common::world::{Chunk, CHUNK_SIZE, CHUNK_VOLUME, WORLD_HEIGHT};
use std::io::{Cursor, Read};
use std::sync::mpsc::Sender;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use crate::state::NetworkEvent;

pub struct BetaConnection { runtime: tokio::runtime::Runtime, stream: TcpStream }

impl BetaConnection {
    pub fn connect(address: &str, username: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_io().build()?;
        let mut stream = runtime.block_on(TcpStream::connect(address))?;
        runtime.block_on(async {
            write_string(&mut stream, 0x02, username).await?;
            if stream.read_u8().await? != 0x02 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid handshake response")); }
            let _ = read_utf16(&mut stream, 32).await?;
            stream.write_u8(0x01).await?; stream.write_i32(14).await?;
            write_utf16(&mut stream, username).await?; stream.write_i64(0).await?; stream.write_i8(0).await?;
            Ok::<(), std::io::Error>(())
        })?;
        Ok(Self { runtime, stream })
    }

    pub fn run(self, tx: Sender<NetworkEvent>) -> NetworkEvent {
        let BetaConnection { runtime, mut stream } = self;
        let result: std::io::Result<NetworkEvent> = runtime.block_on(async move {
            loop {
                let id = stream.read_u8().await?;
                match id {
                    0 => {}
                    1 => { let _ = stream.read_i32().await?; let _ = read_utf16(&mut stream, 16).await?; let _ = stream.read_i64().await?; let _ = stream.read_i8().await?; }
                    3 => { let message = read_utf16(&mut stream, 119).await?; let _ = tx.send(NetworkEvent::Chat(message)); }
                    4 => { let time = stream.read_i64().await?; let _ = tx.send(NetworkEvent::TimeUpdate(time)); }
                    5 => { let _ = stream.read_i32().await?; let _ = stream.read_i16().await?; let _ = stream.read_i16().await?; let _ = stream.read_i16().await?; }
                    6 => { let x = stream.read_i32().await?; let y = stream.read_i32().await?; let z = stream.read_i32().await?; let _ = tx.send(NetworkEvent::Connected { spawn: (x, y, z) }); }
                    8 => { let _ = stream.read_i16().await?; }
                    9 => { let _ = stream.read_i8().await?; }
                    10 => { let _ = stream.read_u8().await?; }
                    11 => { let _ = stream.read_f64().await?; let _ = stream.read_f64().await?; let _ = stream.read_f64().await?; let _ = stream.read_f64().await?; let _ = stream.read_u8().await?; }
                    12 => { let _ = stream.read_f32().await?; let _ = stream.read_f32().await?; let _ = stream.read_u8().await?; }
                    13 => {
                        let x = stream.read_f64().await?; let y = stream.read_f64().await?; let _stance = stream.read_f64().await?; let z = stream.read_f64().await?; let yaw = stream.read_f32().await?; let pitch = stream.read_f32().await?; let on_ground = stream.read_u8().await? != 0;
                        stream.write_u8(0x0d).await?; stream.write_f64(x).await?; stream.write_f64(y).await?; stream.write_f64(y + 1.62).await?; stream.write_f64(z).await?; stream.write_f32(yaw).await?; stream.write_f32(pitch).await?; stream.write_u8(on_ground as u8).await?;
                        let _ = tx.send(NetworkEvent::ServerPosition { x, y, z, yaw, pitch });
                    }
                    16 => { let _ = stream.read_i16().await?; }
                    17 => { let _ = stream.read_i32().await?; let _ = stream.read_i8().await?; let _ = stream.read_i32().await?; let _ = stream.read_i8().await?; let _ = stream.read_i32().await?; }
                    18 => { let _ = stream.read_i32().await?; let _ = stream.read_i8().await?; }
                    20 => { let entity_id = stream.read_i32().await?; let name = read_utf16(&mut stream, 16).await?; let x = stream.read_i32().await? as f32 / 32.0; let y = stream.read_i32().await? as f32 / 32.0; let z = stream.read_i32().await? as f32 / 32.0; let yaw = stream.read_i8().await? as f32 * 360.0 / 256.0; let pitch = stream.read_i8().await? as f32 * 360.0 / 256.0; let _ = stream.read_i16().await?; let _ = tx.send(NetworkEvent::EntitySpawn { id: entity_id, kind: 1, x, y, z, yaw, pitch, name: Some(name) }); }
                    21 => { let entity_id = stream.read_i32().await?; let _item = stream.read_i16().await?; let _count = stream.read_i8().await?; let _damage = stream.read_i16().await?; let position = Vec3::new(stream.read_i32().await? as f32 / 32.0, stream.read_i32().await? as f32 / 32.0, stream.read_i32().await? as f32 / 32.0); let yaw = stream.read_i8().await? as f32 * 360.0 / 256.0; let pitch = stream.read_i8().await? as f32 * 360.0 / 256.0; let _roll = stream.read_i8().await?; let _ = tx.send(NetworkEvent::EntitySpawn { id: entity_id, kind: 2, x: position.x, y: position.y, z: position.z, yaw, pitch, name: None }); }
                    22 => { let entity_id = stream.read_i32().await?; let _collector = stream.read_i32().await?; let _ = tx.send(NetworkEvent::EntityDestroy(entity_id)); }
                    23 => { let entity_id = stream.read_i32().await?; let kind = stream.read_i8().await? as u8; let position = Vec3::new(stream.read_i32().await? as f32 / 32.0, stream.read_i32().await? as f32 / 32.0, stream.read_i32().await? as f32 / 32.0); let owner = stream.read_i32().await?; let velocity = if owner > 0 { Vec3::new(stream.read_i16().await? as f32 / 8000.0, stream.read_i16().await? as f32 / 8000.0, stream.read_i16().await? as f32 / 8000.0) } else { Vec3::ZERO }; let _ = tx.send(NetworkEvent::EntitySpawn { id: entity_id, kind: 32 + kind, x: position.x, y: position.y, z: position.z, yaw: 0.0, pitch: 0.0, name: None }); if velocity != Vec3::ZERO { let _ = tx.send(NetworkEvent::EntityVelocity { id: entity_id, velocity }); } }
                    24 => { let entity_id = stream.read_i32().await?; let kind = stream.read_i8().await? as u8; let x = stream.read_i32().await? as f32 / 32.0; let y = stream.read_i32().await? as f32 / 32.0; let z = stream.read_i32().await? as f32 / 32.0; let yaw = stream.read_i8().await? as f32 * 360.0 / 256.0; let pitch = stream.read_i8().await? as f32 * 360.0 / 256.0; skip_metadata(&mut stream).await?; let _ = tx.send(NetworkEvent::EntitySpawn { id: entity_id, kind, x, y, z, yaw, pitch, name: None }); }
                    25 => { let _ = stream.read_i32().await?; let _ = read_utf16(&mut stream, 64).await?; for _ in 0..4 { let _ = stream.read_i32().await?; } }
                    28 => { let entity_id = stream.read_i32().await?; let velocity = Vec3::new(stream.read_i16().await? as f32 / 8000.0, stream.read_i16().await? as f32 / 8000.0, stream.read_i16().await? as f32 / 8000.0); let _ = tx.send(NetworkEvent::EntityVelocity { id: entity_id, velocity }); }
                    29 => { let count = stream.read_u8().await? as usize; for _ in 0..count { let _ = tx.send(NetworkEvent::EntityDestroy(stream.read_i32().await?)); } }
                    30 => { let _entity_id = stream.read_i32().await?; }
                    31 => { let entity_id = stream.read_i32().await?; let delta = Vec3::new(stream.read_i8().await? as f32 / 32.0, stream.read_i8().await? as f32 / 32.0, stream.read_i8().await? as f32 / 32.0); let _ = tx.send(NetworkEvent::EntityRelativeMove { id: entity_id, delta, yaw: None, pitch: None }); }
                    32 => { let entity_id = stream.read_i32().await?; let yaw = stream.read_i8().await? as f32 * 360.0 / 256.0; let pitch = stream.read_i8().await? as f32 * 360.0 / 256.0; let _ = tx.send(NetworkEvent::EntityRelativeMove { id: entity_id, delta: Vec3::ZERO, yaw: Some(yaw), pitch: Some(pitch) }); }
                    33 => { let entity_id = stream.read_i32().await?; let delta = Vec3::new(stream.read_i8().await? as f32 / 32.0, stream.read_i8().await? as f32 / 32.0, stream.read_i8().await? as f32 / 32.0); let yaw = stream.read_i8().await? as f32 * 360.0 / 256.0; let pitch = stream.read_i8().await? as f32 * 360.0 / 256.0; let _ = tx.send(NetworkEvent::EntityRelativeMove { id: entity_id, delta, yaw: Some(yaw), pitch: Some(pitch) }); }
                    34 => { let entity_id = stream.read_i32().await?; let position = Vec3::new(stream.read_i32().await? as f32 / 32.0, stream.read_i32().await? as f32 / 32.0, stream.read_i32().await? as f32 / 32.0); let yaw = stream.read_i8().await? as f32 * 360.0 / 256.0; let pitch = stream.read_i8().await? as f32 * 360.0 / 256.0; let _ = tx.send(NetworkEvent::EntityTeleport { id: entity_id, position, yaw, pitch }); }
                    38 => { let _ = stream.read_i32().await?; let _ = stream.read_i8().await?; }
                    39 => { let _ = stream.read_i32().await?; let _ = stream.read_i32().await?; }
                    40 => { let _ = stream.read_i32().await?; skip_metadata(&mut stream).await?; }
                    50 => { let x = stream.read_i32().await?; let z = stream.read_i32().await?; let mode = stream.read_u8().await? != 0; if !mode { let _ = tx.send(NetworkEvent::ChunkUnloaded { x, z }); } }
                    51 => { let packet = read_map_chunk(&mut stream).await?; let _ = tx.send(NetworkEvent::ChunkLoaded(packet)); }
                    52 => { read_multi_block(&mut stream, &tx).await?; }
                    53 => { let x = stream.read_i32().await?; let y = stream.read_u8().await? as i32; let z = stream.read_i32().await?; let id = stream.read_u8().await?; let metadata = stream.read_u8().await?; let _ = tx.send(NetworkEvent::BlockChanged { x, y, z, id, metadata }); }
                    70 => { let _ = stream.read_i8().await?; }
                    71 => { let _ = stream.read_i32().await?; let _ = stream.read_i8().await?; let _ = stream.read_i32().await?; let _ = stream.read_i32().await?; let _ = stream.read_i32().await?; }
                    100 => { let _ = stream.read_i8().await?; let _ = stream.read_i8().await?; let _ = read_modified_utf(&mut stream).await?; let _ = stream.read_i8().await?; }
                    101 => { let _ = stream.read_i8().await?; }
                    103 => { let _ = stream.read_i8().await?; let _ = stream.read_i16().await?; skip_item_stack(&mut stream).await?; }
                    104 => { let _ = stream.read_i8().await?; let count = stream.read_i16().await?; if count < 0 || count > 4096 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid inventory slot count")); } for _ in 0..count { skip_item_stack(&mut stream).await?; } }
                    105 => { let _ = stream.read_i8().await?; let _ = stream.read_i16().await?; let _ = stream.read_i16().await?; }
                    106 => { let _ = stream.read_i8().await?; let _ = stream.read_i16().await?; let _ = stream.read_u8().await?; }
                    130 => { let _ = stream.read_i32().await?; let _ = stream.read_i16().await?; let _ = stream.read_i32().await?; for _ in 0..4 { let _ = read_utf16(&mut stream, 15).await?; } }
                    131 => { let _ = stream.read_i16().await?; let len = stream.read_i16().await?; if len < 0 || len > 32767 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid map data length")); } let mut data = vec![0; len as usize]; stream.read_exact(&mut data).await?; }
                    200 => { let _ = stream.read_i32().await?; let _ = stream.read_i8().await?; }
                    0xff => { let reason = read_utf16(&mut stream, 119).await?; return Ok(NetworkEvent::Failed { reason }); }
                    _ => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unsupported server packet 0x{id:02x}"))),
                }
            }
        });
        result.unwrap_or_else(|error| NetworkEvent::Failed { reason: error.to_string() })
    }
}

async fn read_map_chunk(stream: &mut TcpStream) -> std::io::Result<Chunk> {
        let x = stream.read_i32().await?; let y = stream.read_i16().await? as usize; let z = stream.read_i32().await?;
        let xs = stream.read_u8().await? as usize + 1; let ys = stream.read_u8().await? as usize + 1; let zs = stream.read_u8().await? as usize + 1;
        let compressed_len = stream.read_i32().await? as usize; let mut compressed = vec![0; compressed_len]; stream.read_exact(&mut compressed).await?;
        let mut decoded = Vec::new(); ZlibDecoder::new(Cursor::new(compressed)).read_to_end(&mut decoded)?;
        let block_bytes = xs.checked_mul(ys).and_then(|value| value.checked_mul(zs))
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "chunk dimensions overflow"))?;
        if decoded.len() < block_bytes {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "chunk block section is truncated"));
        }
        let nibble_len = block_bytes / 2;
        if decoded.len() < block_bytes + nibble_len * 3 {
            return Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "chunk light section is truncated"));
        }
        let mut blocks = vec![0u8; CHUNK_VOLUME];
        let mut metadata = vec![0u8; CHUNK_VOLUME];
        let mut block_light = vec![0u8; CHUNK_VOLUME];
        let mut sky_light = vec![0u8; CHUNK_VOLUME];
        for lx in 0..xs.min(CHUNK_SIZE) {
            for lz in 0..zs.min(CHUNK_SIZE) {
                for ly in 0..ys.min(WORLD_HEIGHT.saturating_sub(y)) {
                    let source = (lx * zs + lz) * ys + ly;
                    let target = (lx << 11) | (lz << 7) | (y + ly);
                    blocks[target] = decoded[source];
                    let nibble = (source / 2) as usize;
                    metadata[target] = if source & 1 == 0 { decoded[block_bytes + nibble] & 0x0f } else { decoded[block_bytes + nibble] >> 4 };
                    block_light[target] = if source & 1 == 0 { decoded[block_bytes + nibble_len + nibble] & 0x0f } else { decoded[block_bytes + nibble_len + nibble] >> 4 };
                    sky_light[target] = if source & 1 == 0 { decoded[block_bytes + nibble_len * 2 + nibble] & 0x0f } else { decoded[block_bytes + nibble_len * 2 + nibble] >> 4 };
                }
            }
        }
        Chunk::from_sections(x.div_euclid(16), z.div_euclid(16), blocks, metadata, block_light, sky_light).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk"))
    }

async fn read_multi_block(stream: &mut TcpStream, tx: &Sender<NetworkEvent>) -> std::io::Result<()> {
        let chunk_x = stream.read_i32().await?; let chunk_z = stream.read_i32().await?; let count = stream.read_u16().await? as usize;
        let mut coords = vec![0u16; count]; for c in &mut coords { *c = stream.read_u16().await?; }
        let mut ids = vec![0u8; count]; stream.read_exact(&mut ids).await?; let mut metadata = vec![0u8; count]; stream.read_exact(&mut metadata).await?;
        for i in 0..count { let c = coords[i]; let x = chunk_x * 16 + ((c >> 12) & 15) as i32; let z = chunk_z * 16 + ((c >> 8) & 15) as i32; let y = (c & 255) as i32; let _ = tx.send(NetworkEvent::BlockChanged { x, y, z, id: ids[i], metadata: metadata[i] }); }
        Ok(())
    }

async fn write_string(stream: &mut TcpStream, packet_id: u8, value: &str) -> std::io::Result<()> { stream.write_u8(packet_id).await?; write_utf16(stream, value).await }
async fn skip_item_stack(stream: &mut TcpStream) -> std::io::Result<()> {
    let item_id = stream.read_i16().await?;
    if item_id >= 0 { let _ = stream.read_i8().await?; let _ = stream.read_i16().await?; }
    Ok(())
}
async fn read_modified_utf(stream: &mut TcpStream) -> std::io::Result<String> {
    let length = stream.read_u16().await? as usize;
    if length > 32767 { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "modified UTF string is too long")); }
    let mut bytes = vec![0; length]; stream.read_exact(&mut bytes).await?;
    let mut cursor = Cursor::new(bytes);
    let mut output = String::new();
    let mut index = 0;
    while index < length {
        let first = cursor.get_ref()[index]; index += 1;
        let code = if first & 0x80 == 0 { first as u16 }
        else if first & 0xe0 == 0xc0 { let second = cursor.get_ref()[index]; index += 1; (((first & 0x1f) as u16) << 6) | (second & 0x3f) as u16 }
        else { let second = cursor.get_ref()[index]; let third = cursor.get_ref()[index + 1]; index += 2; (((first & 0x0f) as u16) << 12) | (((second & 0x3f) as u16) << 6) | (third & 0x3f) as u16 };
        output.push(char::from_u32(code as u32).unwrap_or(char::REPLACEMENT_CHARACTER));
    }
    Ok(output)
}
async fn write_utf16(stream: &mut TcpStream, value: &str) -> std::io::Result<()> { let units: Vec<u16> = value.encode_utf16().collect(); stream.write_u16(u16::try_from(units.len()).map_err(|_| std::io::ErrorKind::InvalidInput)?).await?; for unit in units { stream.write_u16(unit).await?; } Ok(()) }
async fn read_utf16(stream: &mut TcpStream, max: usize) -> std::io::Result<String> { let len = stream.read_u16().await? as usize; if len > max { return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "string too long")); } let mut units = Vec::with_capacity(len); for _ in 0..len { units.push(stream.read_u16().await?); } String::from_utf16(&units).map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error)) }
async fn skip_metadata(stream: &mut TcpStream) -> std::io::Result<()> {
    loop {
        let header = stream.read_u8().await?;
        if header == 127 { return Ok(()); }
        match (header & 0xe0) >> 5 {
            0 => { let _ = stream.read_i8().await?; }
            1 => { let _ = stream.read_i16().await?; }
            2 => { let _ = stream.read_i32().await?; }
            3 => { let _ = stream.read_f32().await?; }
            4 => { let _ = read_utf16(stream, 64).await?; }
            5 => { let _ = stream.read_i16().await?; let _ = stream.read_i8().await?; let _ = stream.read_i16().await?; }
            6 => { for _ in 0..3 { let _ = stream.read_i32().await?; } }
            kind => return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("invalid entity metadata type {kind} (header 0x{header:02x})"))),
        }
    }
}
