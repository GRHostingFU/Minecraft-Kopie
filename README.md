# Minecraft Kopie

A clean-room, Minecraft-style voxel sandbox written in Rust on [Bevy](https://bevy.org) 0.20.

**Everything is generated at runtime.** There are no textures, models, fonts, sounds or decompiled sources in this
repository: terrain comes from noise functions, the 16×16 block/item textures are painted from hash noise
(`src/rendering/atlas/`), and all text is drawn with a hand-written 5×7 pixel font (`src/ui/font.rs`).

## Run

Requires a recent stable Rust. On Linux you also need the usual Bevy system libraries:

```sh
sudo apt install libudev-dev libwayland-dev libxkbcommon-dev   # Debian/Ubuntu
cargo run --release
```

The game starts on the **title screen** with an animated voxel panorama.
Command line (all optional): `--save-dir PATH` (jump straight into that world), `--seed N`, `--creative`,
`--render-distance N` (2–32). Worlds live in `saves/<name>/`, options in `saves/options.json`.

## Features

* **Main menu** – Singleplayer (world list, create world with name / seed / game mode, delete), Options, Quit.
* **Options** (applied live, saved): render distance 2–32, FOV 60–110, fullscreen, VSync, particles; mouse
  sensitivity, sprint and sneak as *toggle* or *hold*, invert mouse; master / music / weather volume
  (placeholders – there is no audio engine).
* **Inventory** (`E`): 27 main slots, 9 hotbar slots, 4 armor slots, offhand, 2×2 crafting grid. Click, right-click
  split, shift-click quick move, drag distribution, tooltips. Creative mode shows a tabbed block directory.
* **HUD**: 9-slot hotbar with selection frame, XP bar, hearts, hunger, armor and air bubbles (survival).
* **World**: biomes, 3D spaghetti and cheese caves, ores in height layers, villages on flat ground, trees.
* **Blocks**: 43 block types incl. ores, bricks, stone variants, glass, fences, torches (emit block light),
  crafting table, furnace.
* **Survival**: health, hunger / saturation, fall and drowning damage, death screen and respawn, XP,
  mining time depends on tool tier, tools wear out, item drops with pickup magnet, crack overlay while mining.
* **Crafting & smelting**: shaped and shapeless recipes, placeable 3×3 crafting table, furnace with fuel.
* **Mobs**: pigs, cows, sheep, zombies, villagers with a small AI state machine (idle / wander / flee / chase).

## Controls

| Input | Action |
|---|---|
| `W A S D` / mouse | move / look |
| `Space` | jump (swim up, ascend while flying) |
| `Ctrl` / double-tap `W` | sprint (depends on the sprint option) |
| `Shift` | sneak (depends on the sneak option) |
| `F` | toggle fly mode (creative) |
| `LMB` / `RMB` | break or attack / place, use, eat, open crafting table or furnace |
| `1`–`9`, mouse wheel | hotbar slot |
| `E` | inventory |
| `Q` / `X` | drop item / swap with offhand |
| `F3` / `F4` / `F11` | debug overlay / switch creative⇄survival / fullscreen |
| `T` | cycle day/night time-lapse |
| `Esc` | pause menu (saves the world), back |

## Architecture

```
src/
  world/       block registry, chunks, generation/{mod,caves,ores,villages}, async streaming, save formats
  rendering/   greedy mesher with AO + block light, procedural atlas (blocks, icons, UI sprites), sky, shader
  physics/     swept-AABB collision and voxel raycasting
  player/      controller, controls (sprint/sneak modes), interaction (mining, placing, using), viewmodel
  items.rs     items, tools, tiers, mining formula, loot, fuel
  inventory/   slot logic, crafting & smelting recipes, creative directory
  survival/    health, hunger, XP, damage, drops, particles, mining progress
  mobs/        mob kinds, models, AI, spawning
  ui/          pixel font, widgets, screen state machine, menu/{title,worlds,options}, pause, HUD, inventory screen
  session.rs   AppState (MainMenu / InGame), world list, start / end of a game session
  settings.rs  user options and their file
tests/         headless end-to-end tests for chunk streaming and persistence
```

* Chunks are 16×256×16 arrays shared as `Arc<Chunk>`; generation and meshing run on Bevy's async task pool.
* Terrain, caves, ores and villages are pure functions of `(seed, x, z)`, so chunks generated independently
  line up (ore veins and village plans are replayed from neighbouring chunks).
* Light: sky light per column plus flood-filled block light from torches, baked into vertex colours.

## Development

CI runs `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings` and `cargo test`.

## Known limitations

No sound engine (volume sliders are placeholders), fences use full-block collision, mobs are not persisted,
the inventory is kept on death, no multiplayer, no Nether/End (only decorative nether-style blocks).
