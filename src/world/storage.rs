//! On-disk persistence. Each modified chunk is stored as a run-length encoded
//! file `c.<x>.<z>.bin`; global player/world state lives in `level.json`.

use super::block::BlockId;
use super::chunk::{CHUNK_VOLUME, Chunk, ChunkPos};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"MKC1";

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LevelData {
    pub seed: u32,
    pub player_pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub time_of_day: f32,
    pub selected_slot: usize,
    pub flying: bool,
}

pub fn chunk_path(dir: &Path, pos: ChunkPos) -> PathBuf {
    dir.join(format!("c.{}.{}.bin", pos.x, pos.y))
}

/// RLE: repeated `(block: u8, run_length: u16 LE)` records after the magic.
pub fn encode_chunk(chunk: &Chunk) -> Vec<u8> {
    let mut out = Vec::with_capacity(4096);
    out.extend_from_slice(MAGIC);
    let raw = chunk.raw();
    let mut i = 0;
    while i < raw.len() {
        let id = raw[i];
        let mut run = 1usize;
        while i + run < raw.len() && raw[i + run] == id && run < u16::MAX as usize {
            run += 1;
        }
        out.push(id.0);
        out.extend_from_slice(&(run as u16).to_le_bytes());
        i += run;
    }
    out
}

pub fn decode_chunk(bytes: &[u8]) -> io::Result<Chunk> {
    let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
    if bytes.len() < 4 || &bytes[..4] != MAGIC {
        return Err(bad("bad chunk magic"));
    }
    let mut chunk = Chunk::empty();
    let raw = chunk.raw_mut();
    let mut at = 0usize;
    for rec in bytes[4..].chunks(3) {
        if rec.len() != 3 {
            return Err(bad("truncated record"));
        }
        let run = u16::from_le_bytes([rec[1], rec[2]]) as usize;
        if at + run > CHUNK_VOLUME {
            return Err(bad("run overflows chunk"));
        }
        raw[at..at + run].fill(BlockId(rec[0]));
        at += run;
    }
    if at != CHUNK_VOLUME {
        return Err(bad("chunk too short"));
    }
    Ok(chunk)
}

pub fn save_chunk(dir: &Path, pos: ChunkPos, chunk: &Chunk) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let path = chunk_path(dir, pos);
    // Write-then-rename so a crash never leaves a half-written chunk behind.
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, encode_chunk(chunk))?;
    fs::rename(tmp, path)
}

/// `Ok(None)` if the chunk was never saved. A corrupt file is reported as an
/// error so the caller can fall back to regenerating the terrain.
pub fn load_chunk(dir: &Path, pos: ChunkPos) -> io::Result<Option<Chunk>> {
    match fs::read(chunk_path(dir, pos)) {
        Ok(bytes) => decode_chunk(&bytes).map(Some),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

pub fn save_level(dir: &Path, level: &LevelData) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let json = serde_json::to_vec_pretty(level).map_err(io::Error::other)?;
    let path = dir.join("level.json");
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, json)?;
    fs::rename(tmp, path)
}

pub fn load_level(dir: &Path) -> Option<LevelData> {
    serde_json::from_slice(&fs::read(dir.join("level.json")).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generation::TerrainGenerator;
    use bevy::math::IVec2;

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mk_test_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn rle_roundtrip_on_real_terrain() {
        let chunk = TerrainGenerator::new(11).generate(IVec2::new(2, 3));
        let bytes = encode_chunk(&chunk);
        assert!(bytes.len() < CHUNK_VOLUME / 4, "RLE should compress terrain, got {}", bytes.len());
        let back = decode_chunk(&bytes).unwrap();
        assert!(back.raw() == chunk.raw());
    }

    #[test]
    fn save_and_load_chunk_and_level() {
        let dir = temp_dir("save");
        let mut chunk = Chunk::empty();
        chunk.set(1, 2, 3, BlockId::PLANKS);
        let pos = IVec2::new(-4, 9);
        assert!(load_chunk(&dir, pos).unwrap().is_none());
        save_chunk(&dir, pos, &chunk).unwrap();
        let back = load_chunk(&dir, pos).unwrap().unwrap();
        assert_eq!(back.get(1, 2, 3), BlockId::PLANKS);

        let level = LevelData {
            seed: 9,
            player_pos: [1.0, 2.0, 3.0],
            yaw: 0.5,
            pitch: -0.2,
            time_of_day: 0.3,
            selected_slot: 2,
            flying: false,
        };
        save_level(&dir, &level).unwrap();
        assert_eq!(load_level(&dir), Some(level));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_data_is_rejected() {
        assert!(decode_chunk(b"nope").is_err());
        let mut bytes = encode_chunk(&Chunk::empty());
        bytes.truncate(bytes.len() - 1);
        assert!(decode_chunk(&bytes).is_err());
    }
}
