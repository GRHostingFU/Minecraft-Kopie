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
An existing world keeps the seed and the last used render distance stored in its `level.json`.

## Controls

| Input | Action |
|---|---|
| `W A S D` / mouse | move / look |
| `Space` | jump (swim up in water, ascend while flying) |
| `Ctrl` or double-tap `W` | sprint (wider FOV, sprint hand pose) |
| `Shift` | sneak: slow, lower camera, cannot fall off ledges (descends while flying) |
| `F` | toggle fly mode |
| `LMB` / `RMB` | break (item goes into the inventory) / place the selected block (hold to repeat) |
| `1`–`9` or mouse wheel | select hotbar slot |
| `E` | open / close the inventory |
| `Esc` | pause menu (saves the world); back / close inside menus |
| `F11` | toggle fullscreen |
| `T` | cycle day/night time-lapse speed |
| `F3` | toggle debug overlay |

**Inventory** (`E`): 27 main slots, the 9-slot hotbar and 4 armor slots with a mannequin that shows what is
equipped. Left click picks up / places / merges / swaps a stack, right click splits a stack or places one item,
`Shift`+click quick-moves between hotbar and main inventory and equips armor, and dragging with a stack on the
cursor spreads it over the slots you pass (left button: evenly, right button: one each). Stone drops cobblestone,
grass drops dirt, placing a block uses one item from the selected stack.

**Pause menu** (`Esc`): *Back to Game*, *Options*, *Save World*, *Save and Quit*. Options has a render distance
slider (2–32 chunks in single steps), mouse sensitivity (10–200 %), FOV (30–110) and a fullscreen toggle. While
the menu is open the world is frozen. Losing window focus also opens the menu.

## Architecture

```
src/
  world/       block registry, chunk storage, terrain generator, async streaming, save file formats
  rendering/   greedy mesher, procedural atlas (+ mipmaps), voxel material + WESL shader, chunk mesh tasks, day/night sky
  physics/     swept-AABB collision and voxel raycasting (pure functions, no Bevy dependency on the hot path)
  player/      first-person controller (sprint, sneak), block interaction, first-person hand (viewmodel)
  inventory.rs items, stacks and the click rules of the inventory (pure logic, unit-tested)
  settings.rs  FOV, mouse sensitivity, fullscreen (render distance lives in `WorldSettings`)
  save.rs      save requests, autosave, save on window close / exit, `level.json` snapshot
  ui/          HUD, inventory screen, pause/options menu, screen state machine, widgets
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
* **Atlas** (`rendering/atlas.rs`): 32×32 px tiles painted from tileable value noise, Worley cells and simple
  shapes (pebbles in dirt, veins in stone, bark ridges, rings in log ends, leaf clusters, planks with joints and
  nails, a grass overlay on the side tiles, armor icons). A box-filtered mip chain (linear-light, alpha weighted) is
  built at startup; the sampler is nearest when magnified and trilinear when minified.
* **Shader** (`rendering/voxel.wesl`, embedded in the binary) tiles the atlas over merged quads with `fract`, takes
  the mip level from the *continuous* uv gradient (so tile borders do not break mip selection) and keeps bilinear
  taps inside the tile. It applies AO, face shading, sky light, day/night brightness, a warm twilight tint and
  exponential-squared distance fog that is fully opaque at the render distance. The sky colour follows a smooth
  sun-elevation curve with a horizon glow at sunrise and sunset.
* **Hand** (`player/viewmodel.rs`): a second camera with its own render layer, fixed 70° FOV and its own depth buffer
  renders the arm and the selected hotbar item (a textured cube mesh for blocks, a flat icon for armor). Swing
  (left/right click), walking bob, sprint pose, sneak dip and an equip animation are composed from quaternions.
* **Physics** moves the player one axis at a time (Y, X, Z) and clips the motion against every solid block in the
  swept volume, so large frame times cannot tunnel through walls. Unloaded chunks count as solid.
* **Saving** (`save.rs`, `world/storage.rs`): edited chunks are run-length encoded to `c.<x>.<z>.bin`; `level.json`
  holds the seed, player position and look direction, fly state, time of day, the whole inventory (hotbar, main,
  armor and the stack on the cursor) and the options. Triggers: pause menu / `Esc`, *Save World*, every 30 s, the
  window close button and any exit. Files are written to a temp file, fsynced and renamed, `level.json` keeps a
  `.bak` of the previous version, a chunk whose save fails stays loaded instead of being dropped, and the seed is
  written the moment a world is created so chunks can never be paired with another seed. Old `level.json` files
  without the newer fields still load.

## Tests

```sh
cargo test          # unit tests (mesher, generator, physics, raycast, storage, ...) + headless integration tests
cargo clippy --all-targets
```

## Known limitations

* Sky light is a per-column approximation, not a flood fill; there are no torches or block light.
* No water physics beyond simple swimming, no mobs, crafting or dropped item entities (a full inventory simply
  loses the block you break). The armor pieces are inventory items only; they have no gameplay effect yet.
* There is no title screen; the pause menu is the game menu.
