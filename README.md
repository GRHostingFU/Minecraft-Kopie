# Minecraft Kopie

A clean-room, Minecraft-style voxel sandbox written in Rust on [Bevy](https://bevy.org) 0.20.

**Everything is generated at runtime.** There are no textures, models, sounds or decompiled sources in this
repository: terrain comes from noise functions and the 16×16 block textures are painted from hash noise
(`src/rendering/atlas.rs`). The code is an independent implementation of well-known, public voxel-engine
techniques (greedy meshing, 0fps-style vertex AO, DDA ray traversal, swept-AABB collision).

## Run

Requires Rust **1.97.1+**. On Linux you also need the usual Bevy system libraries (plus ALSA for sound):

```sh
sudo apt install libudev-dev libwayland-dev libxkbcommon-dev libasound2-dev   # Debian/Ubuntu
cargo run --release
```

Sound is the optional cargo feature `audio` (enabled by default). Without the ALSA headers build with
`cargo run --release --no-default-features`; the game then runs silently (the synthesiser itself is always
compiled and tested).

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
| `LMB` / `RMB` | break (item goes into the inventory) / place the selected block (hold to repeat). `LMB` on a mob hits it, `RMB` on a lever or button uses it (`Shift`+`RMB` places against it instead) |
| `F6` / `F7` | spawn a creeper / a skeleton a few blocks ahead (testing aid) |
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

## Fluids, redstone, mobs and sound

**Water and lava** (`physics/fluids`). Sources, flowing levels `1..=7` (water; lava reaches 3), falling columns. A
scheduled-tick cellular simulation (water every 0.25 s, lava every 1.5 s) lets a source spread up to seven blocks,
fall down cliffs, run towards nearby drops, and dry up when the source is removed. Two water sources next to each
other create an infinite source. Water that reaches lava (source *or* flow) turns it into **obsidian**; lava that
flows into water leaves **cobblestone**. Surfaces are meshed with sloped corner heights, lava glows. Swimming has
buoyancy, flowing water pushes the player along (falling water pulls down), lava is thick and burns.

**Redstone** (`redstone`). Dust carries power 15 → 1 (−1 per block, climbs and descends steps), fed by levers,
buttons (1 s pulse), pressure plates (pressed by the player or any mob standing on them) and redstone torches
(inverters with a 0.1 s delay and burnout). Sources power adjacent parts weakly and the block they are attached to
strongly, which in turn powers its neighbours - dust never powers other dust through a block. Redstone lamps light
up, and **pistons / sticky pistons** push up to 12 blocks (obsidian, bedrock and extended pistons are immovable;
dust, torches and fluids in the way are destroyed) and sticky ones pull one block back.
Parts break when their support disappears. Everything is stored in the block id, so it saves with the chunks.

**Mobs** (`mobs`). *Creepers* wander, chase the player along A* routes, hiss and swell once within 3 blocks and
explode after 1.5 s (ray-cast crater, blast resistance per block, water and obsidian shelter, damage with knockback);
they give up if you get 7 blocks away. *Skeletons* keep their distance, strafe and shoot arrows with ballistic aim
and scatter; **they burn in daylight** under open sky (water extinguishes). Hostile mobs spawn 22-40 blocks away in
**darkness only** (night under the open sky, or always in caves; lava, torches and lit lamps keep an area safe),
despawn far away, and are limited to 10 at once. The player has health (hearts, armor, regeneration, lava / fire /
explosion / arrow damage, death and respawn).

**Sound** (`audio`). No samples: every effect is synthesised at start-up from noise, oscillators and resonant
filters (footsteps and digging per material, block break/place, UI clicks, lever / button / plate clicks, piston
slide and thump, creeper hiss, explosion, bow twang, hurt / death, splashes, lava bubbles, fire, bones). Played
through Bevy's audio backend with positional panning and distance falloff.

## Architecture

```
src/
  world/       block registry, chunk storage, terrain generator, async streaming, save file formats
  rendering/   greedy mesher, procedural atlas (+ mipmaps), voxel material + WESL shader, chunk mesh tasks, day/night sky
  physics/     swept-AABB collision and voxel raycasting (pure functions, no Bevy dependency on the hot path);
               fluids/ holds the cellular water/lava simulation and the forces on bodies
  redstone/    wire solver, power rules, pistons, tick simulation (pure, tested) + the Bevy glue
  mobs/        creeper and skeleton AI, A* path finding, explosions, spawning rules, procedural models
  audio/       DSP synthesiser (noise, oscillators, biquad filters), WAV encoder, sound triggers
  effects.rs   debris, smoke / spark bursts, explosion flash
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

* Sky light is a per-column approximation, not a flood fill; torches, lava and lamps glow but do not light their
  surroundings (they only keep mobs from spawning nearby).
* No crafting or dropped item entities (a full inventory simply loses the block you break; mobs drop nothing).
  Water and lava are placed from the starter-kit stacks and cannot be picked up again.
* Mobs are not saved, redstone has no repeaters / comparators, and sticky pistons pull a single block only.
* There is no title screen; the pause menu is the game menu.
