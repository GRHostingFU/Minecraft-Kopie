//! On-disk persistence. Each modified chunk is stored as a run-length encoded
//! file `c.<x>.<z>.bin`; global player/world state lives in `level.json`.

use super::block::BlockId;
use super::chunk::{CHUNK_VOLUME, Chunk, ChunkPos};
use crate::inventory::ItemStack;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 4] = b"MKC1";

/// Bumped whenever `level.json` gains fields that older builds cannot interpret.
pub const LEVEL_VERSION: u32 = 2;

/// Everything about a world that is not stored in chunk files. New fields must carry
/// `#[serde(default)]` so saves written by older versions keep loading.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LevelData {
    #[serde(default)]
    pub version: u32,
    pub seed: u32,
    /// False for a world that has been created but whose spawn point is not chosen yet.
    #[serde(default = "yes")]
    pub spawned: bool,
    pub player_pos: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub time_of_day: f32,
    pub selected_slot: usize,
    pub flying: bool,
    /// Inventory slots, armor slots, then the cursor stack. `None` = never saved (use the starter kit).
    #[serde(default)]
    pub inventory: Option<Vec<Option<ItemStack>>>,
    #[serde(default)]
    pub options: OptionsData,
}

fn yes() -> bool {
    true
}

/// Options persisted with the world; `None` means "keep the default / command-line value".
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Default)]
pub struct OptionsData {
    pub render_distance: Option<i32>,
    pub fov: Option<f32>,
    pub mouse_sensitivity: Option<f32>,
    pub fullscreen: Option<bool>,
}

impl LevelData {
    /// A brand-new world: only the seed is known. Written immediately so chunks saved before the
    /// first autosave can never be paired with a different seed.
    pub fn fresh(seed: u32) -> Self {
        Self {
            version: LEVEL_VERSION,
            seed,
            spawned: false,
            player_pos: [0.0, 200.0, 0.0],
            yaw: 0.0,
            pitch: 0.0,
            time_of_day: 0.32,
            selected_slot: 0,
            flying: false,
            inventory: None,
            options: OptionsData::default(),
        }
    }
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
    write_atomic(&chunk_path(dir, pos), &encode_chunk(chunk))
}

/// Write-fsync-rename: a crash or power loss leaves either the old or the new file, never half of one.
fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)?;
    // Persist the rename itself; not supported on every platform, so best effort.
    if let Some(parent) = path.parent()
        && let Ok(d) = File::open(parent)
    {
        let _ = d.sync_all();
    }
    Ok(())
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
    // Keep the previous good copy so a corrupted file can fall back to it.
    if path.exists() {
        let _ = fs::copy(&path, dir.join("level.json.bak"));
    }
    write_atomic(&path, &json)
}

/// Reads `level.json`, falling back to the backup if the main file is unreadable.
pub fn load_level(dir: &Path) -> Option<LevelData> {
    for name in ["level.json", "level.json.bak"] {
        match fs::read(dir.join(name)) {
            Ok(bytes) => match serde_json::from_slice::<LevelData>(&bytes) {
                Ok(level) => return Some(level),
                Err(e) => bevy::log::error!("{name} is corrupt ({e})"),
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => bevy::log::error!("cannot read {name}: {e}"),
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::generation::TerrainGenerator;
    use bevy::math::IVec2;
    use bevy::prelude::default;

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
            spawned: true,
            player_pos: [1.0, 2.0, 3.0],
            yaw: 0.5,
            pitch: -0.2,
            time_of_day: 0.3,
            selected_slot: 2,
            inventory: Some(vec![Some(ItemStack::new(crate::inventory::Item::block(BlockId::DIRT), 12)), None]),
            options: OptionsData { render_distance: Some(14), fov: Some(95.0), ..default() },
            ..LevelData::fresh(9)
        };
        save_level(&dir, &level).unwrap();
        assert_eq!(load_level(&dir), Some(level));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn levels_from_older_versions_still_load() {
        let dir = temp_dir("legacy");
        fs::create_dir_all(&dir).unwrap();
        let old = r#"{"seed":77,"player_pos":[1.5,70.0,-3.0],"yaw":1.0,"pitch":0.1,"time_of_day":0.6,
            "selected_slot":3,"flying":true}"#;
        fs::write(dir.join("level.json"), old).unwrap();
        let level = load_level(&dir).expect("legacy level.json must parse");
        assert_eq!((level.seed, level.selected_slot, level.flying), (77, 3, true));
        assert!(level.spawned, "old worlds already have a spawn point");
        assert_eq!(level.inventory, None);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_level_falls_back_to_backup() {
        let dir = temp_dir("backup");
        save_level(&dir, &LevelData::fresh(5)).unwrap();
        save_level(&dir, &LevelData { time_of_day: 0.9, ..LevelData::fresh(5) }).unwrap();
        fs::write(dir.join("level.json"), b"{ not json").unwrap();
        let level = load_level(&dir).expect("backup should be used");
        assert_eq!(level.seed, 5);
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
