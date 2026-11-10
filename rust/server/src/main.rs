use oldmine_common::world::World;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const BETA_1_7_3_PROTOCOL: i32 = 14;

async fn read_string(stream: &mut TcpStream, max_chars: usize) -> std::io::Result<String> {
    let length = stream.read_u16().await? as usize;
    if length > max_chars {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "protocol string exceeds its maximum length",
        ));
    }

    let mut bytes = vec![0; length * 2];
    stream.read_exact(&mut bytes).await?;
    let units = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]));
    String::from_utf16(&units.collect::<Vec<_>>()).map_err(|error| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, error)
    })
}

async fn write_string(stream: &mut TcpStream, value: &str) -> std::io::Result<()> {
    let units: Vec<u16> = value.encode_utf16().collect();
    let length = u16::try_from(units.len()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "protocol string is too long")
    })?;
    stream.write_u16(length).await?;
    for unit in units {
        stream.write_u16(unit).await?;
    }
    Ok(())
}

async fn handle_client(mut stream: TcpStream) -> std::io::Result<()> {
    let packet_id = stream.read_u8().await?;
    if packet_id != 0x02 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("expected handshake packet 0x02, got 0x{packet_id:02x}"),
        ));
    }

    let username = read_string(&mut stream, 32).await?;
    println!("handshake from {username}");
    stream.write_u8(0x02).await?;
    write_string(&mut stream, "-").await?;

    let packet_id = stream.read_u8().await?;
    if packet_id != 0x01 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("expected login packet 0x01, got 0x{packet_id:02x}"),
        ));
    }

    let protocol = stream.read_i32().await?;
    let login_name = read_string(&mut stream, 16).await?;
    let _map_seed = stream.read_i64().await?;
    let _dimension = stream.read_i8().await?;
    if protocol != BETA_1_7_3_PROTOCOL {
        stream.write_u8(0xff).await?;
        write_string(&mut stream, "Outdated client!").await?;
        return Ok(());
    }

    println!("login packet from {login_name} (protocol {protocol})");
    stream.write_u8(0x01).await?;
    stream.write_i32(1).await?;
    write_string(&mut stream, "").await?;
    stream.write_i64(0).await?;
    stream.write_i8(0).await?;
    stream.write_u8(0x06).await?;
    stream.write_i32(0).await?;
    stream.write_i32(64).await?;
    stream.write_i32(0).await?;
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let address = std::env::var("OLDMINE_BIND").unwrap_or_else(|_| "127.0.0.1:25565".into());
    let listener = TcpListener::bind(&address).await?;
    println!("Oldmine Rust server listening on {address}");

    let seed = std::env::var("OLDMINE_SEED")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0x0B17_0003);
    let mut world = World::new(seed);
    let spawn_chunk = world.generate_chunk(0, 0);
    println!("generated spawn chunk ({}, {}) with seed {seed}", spawn_chunk.x, spawn_chunk.z);
    let mut tick = tokio::time::interval(std::time::Duration::from_millis(50));

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                println!("connection from {peer}");
                tokio::spawn(async move {
                    if let Err(error) = handle_client(stream).await {
                        eprintln!("connection {peer} failed: {error}");
                    }
                });
            }
            _ = tick.tick() => world.tick(),
        }
    }
}
