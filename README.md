# Minecraft Kopie

A clean-room, Minecraft-style voxel sandbox written in Rust on [Bevy](https://bevy.org) 0.20.

**Everything is generated at runtime.** There are no textures, models, sounds or decompiled sources in this
repository: terrain comes from noise functions and the 16×16 block textures are painted from hash noise
(`src/rendering/atlas.rs`). The code is an independent implementation of well-known, public voxel-engine
techniques (greedy meshing, 0fps-style vertex AO, DDA ray traversal, swept-AABB collision).

## Run

Requires Rust **1.97.1+**. On Linux you also need the usual Bevy system libraries:

```sh
sudo apt install libudev-dev libwayland-dev libxkbcommon-dev   # Debian/Ubuntu
cargo run --release
```

Options: `--seed N`, `--render-distance N` (2–32, default 10), `--save-dir PATH` (default `saves/world1`).
An existing world keeps the seed stored in its `level.json`.

## Controls

| Input | Action |
|---|---|
| `W A S D` / mouse | move / look |
| `Space` | jump (swim up in water, ascend while flying) |
| `Ctrl` | sprint |
| `F` | toggle fly mode (`Shift` descends) |
| `LMB` / `RMB` | break / place block (hold to repeat) |
| `1`–`5` or mouse wheel | select hotbar slot |
| `T` | cycle day/night time-lapse speed |
| `F3` | toggle debug overlay |
| `Esc` / click | release / recapture the mouse |

## Architecture

```
src/
  world/       block registry, chunk storage, terrain generator, async streaming, save files
  rendering/   greedy mesher, procedural atlas, voxel material + WESL shader, chunk mesh tasks, day/night sky
  physics/     swept-AABB collision and voxel raycasting (pure functions, no Bevy dependency on the hot path)
  player/      first-person controller, block interaction, hotbar, level persistence
  ui/          crosshair, hotbar, debug overlay
tests/         headless end-to-end tests for chunk streaming and persistence
```

* **Chunks** are 16×256×16 flat `u8` arrays (64 KiB), shared as `Arc<Chunk>` so mesher threads snapshot
  neighbours without copying; edits are copy-on-write.
* **Terrain** (`world/generation.rs`) is a pure function of `(seed, x, z)`: continent/hill/ridged-mountain
  noise, temperature/humidity biomes (plains, forest, desert, snow, mountains, ocean, beach), 3D "spaghetti"
  tunnels plus deep caverns, sea level water and hash-placed trees that line up across chunk borders.
* **Generation and meshing run on Bevy's `AsyncComputeTaskPool`.** The main thread only polls finished tasks and
  uploads buffers (a few per frame), so chunk work never stalls rendering. Chunks are meshed only once all eight
  neighbours exist, nearest first; stale mesh jobs are cancelled when a chunk is edited again.
* **Greedy meshing** (`rendering/mesher.rs`) culls hidden faces and merges coplanar quads per block type. Vertex
  ambient occlusion and a column-based sky light are baked into the vertex colour; quads are only merged along
  axes where AO and light are constant, so shading is never stretched. Water is a separate translucent mesh.
* **Shader** (`rendering/voxel.wesl`, embedded in the binary) tiles the atlas over merged quads with `fract`,
  applies AO, face shading, sky light, day/night brightness and distance fog.
* **Physics** moves the player one axis at a time (Y, X, Z) and clips the motion against every solid block in the
  swept volume, so large frame times cannot tunnel through walls. Unloaded chunks count as solid.
* **Saving**: edited chunks are run-length encoded to `c.<x>.<z>.bin` on unload, every 30 s and on exit;
  player position, time of day and hotbar go to `level.json`.

## Tests

```sh
cargo test          # unit tests (mesher, generator, physics, raycast, storage, ...) + headless integration tests
cargo clippy --all-targets
```

## Known limitations

* The atlas uses nearest filtering without mipmaps, so distant textures shimmer.
* Sky light is a per-column approximation, not a flood fill; there are no torches or block light.
* No water physics beyond simple swimming, no inventory, mobs, or crafting.
