//! Survival rules: health, hunger, saturation, experience, fall and drowning damage, armor and
//! death. The numbers follow the familiar sandbox rules (20 health points, 20 food points,
//! exhaustion drains saturation first, regeneration needs a well-fed player). Creative mode
//! keeps every value full.

pub mod drops;
pub mod mining;

use crate::inventory::{ARMOR_SLOTS, ARMOR_START, Inventory};
use crate::items::ItemKind;
use crate::player::controller::{InputCapture, Player};
use crate::session::{AppState, GameMode};
use crate::world::block::BlockId;
use crate::world::{BlockQuery, VoxelWorld};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const MAX_HEALTH: f32 = 20.0;
pub const MAX_FOOD: f32 = 20.0;
pub const MAX_AIR: f32 = 15.0;
/// Hit points regained per second-ish: one point every this many seconds while well fed.
const REGEN_INTERVAL: f32 = 4.0;
const STARVE_INTERVAL: f32 = 4.0;
/// Food level above which the player regenerates.
pub const REGEN_FOOD: f32 = 18.0;
/// Food level at or below which sprinting is not possible.
pub const SPRINT_FOOD: f32 = 6.0;
/// Seconds of immunity after a hit.
const HURT_COOLDOWN: f32 = 0.5;
/// Blocks the player may fall without harm.
pub const SAFE_FALL: f32 = 3.0;

/// Health, hunger and breath of the player.
#[derive(Resource, Clone, Debug, PartialEq)]
pub struct Vitals {
    pub health: f32,
    pub food: f32,
    pub saturation: f32,
    pub exhaustion: f32,
    regen_timer: f32,
    starve_timer: f32,
    /// Seconds of breath left under water.
    pub air: f32,
    drown_timer: f32,
    /// Seconds during which further hits are ignored.
    pub invulnerable: f32,
    /// Seconds of the red screen flash left.
    pub hurt_flash: f32,
}

impl Default for Vitals {
    fn default() -> Self {
        Self::full()
    }
}

impl Vitals {
    pub fn full() -> Self {
        Self {
            health: MAX_HEALTH,
            food: MAX_FOOD,
            saturation: 5.0,
            exhaustion: 0.0,
            regen_timer: 0.0,
            starve_timer: 0.0,
            air: MAX_AIR,
            drown_timer: 0.0,
            invulnerable: 0.0,
            hurt_flash: 0.0,
        }
    }

    pub fn from_save(health: f32, food: f32, saturation: f32) -> Self {
        let fix = |v: f32, hi: f32, d: f32| if v.is_finite() { v.clamp(0.0, hi) } else { d };
        let mut v = Self::full();
        v.health = fix(health, MAX_HEALTH, MAX_HEALTH);
        // A saved corpse is revived: nobody should load into a dead character.
        if v.health <= 0.0 {
            v.health = MAX_HEALTH;
        }
        v.food = fix(food, MAX_FOOD, MAX_FOOD);
        v.saturation = fix(saturation, v.food, 5.0);
        v
    }

    pub fn is_dead(&self) -> bool {
        self.health <= 0.0
    }

    pub fn add_exhaustion(&mut self, amount: f32) {
        self.exhaustion += amount;
    }

    /// Takes damage, reduced by `armor_points`. Returns what was actually lost.
    pub fn damage(&mut self, amount: f32, armor_points: u32) -> f32 {
        if self.invulnerable > 0.0 || amount <= 0.0 || self.is_dead() {
            return 0.0;
        }
        let taken = amount * armor_factor(armor_points);
        self.health = (self.health - taken).max(0.0);
        self.invulnerable = HURT_COOLDOWN;
        self.hurt_flash = 0.45;
        self.exhaustion += 0.1;
        taken
    }

    /// Damage that ignores armor and the hurt cooldown (starvation, drowning).
    fn hurt_directly(&mut self, amount: f32, floor: f32) {
        if self.health > floor {
            self.health = (self.health - amount).max(floor);
            self.hurt_flash = 0.45;
        }
    }

    pub fn heal(&mut self, amount: f32) {
        self.health = (self.health + amount).min(MAX_HEALTH);
    }

    /// Eating: raises the food level and the saturation (which never exceeds the food level).
    pub fn eat(&mut self, hunger: u8, saturation: f32) {
        self.food = (self.food + hunger as f32).min(MAX_FOOD);
        self.saturation = (self.saturation + saturation).min(self.food);
    }

    /// Advances hunger, regeneration and the damage timers.
    pub fn tick(&mut self, dt: f32) {
        self.invulnerable = (self.invulnerable - dt).max(0.0);
        self.hurt_flash = (self.hurt_flash - dt).max(0.0);
        while self.exhaustion >= 4.0 {
            self.exhaustion -= 4.0;
            if self.saturation > 0.0 {
                self.saturation = (self.saturation - 1.0).max(0.0);
            } else {
                self.food = (self.food - 1.0).max(0.0);
            }
        }
        if self.food >= REGEN_FOOD && self.health < MAX_HEALTH {
            self.regen_timer += dt;
            if self.regen_timer >= REGEN_INTERVAL {
                self.regen_timer = 0.0;
                self.heal(1.0);
                self.exhaustion += 6.0;
            }
        } else {
            self.regen_timer = 0.0;
        }
        if self.food <= 0.0 {
            self.starve_timer += dt;
            if self.starve_timer >= STARVE_INTERVAL {
                self.starve_timer = 0.0;
                // Starvation leaves half a heart alive, like the normal difficulty.
                self.hurt_directly(1.0, 1.0);
            }
        } else {
            self.starve_timer = 0.0;
        }
    }

    /// Breath: `submerged` drains it, fresh air refills it quickly. Returns whether the player is drowning.
    pub fn breathe(&mut self, submerged: bool, dt: f32) -> bool {
        if !submerged {
            self.air = (self.air + dt * 6.0).min(MAX_AIR);
            self.drown_timer = 0.0;
            return false;
        }
        self.air = (self.air - dt).max(0.0);
        if self.air > 0.0 {
            return false;
        }
        self.drown_timer += dt;
        if self.drown_timer >= 1.0 {
            self.drown_timer = 0.0;
            self.hurt_directly(2.0, 0.0);
        }
        true
    }
}

/// Share of damage that gets through `armor_points` of armor (each point blocks 4 %, at most 80 %).
pub fn armor_factor(armor_points: u32) -> f32 {
    1.0 - armor_points.min(20) as f32 * 0.04
}

/// Defense points of everything the player wears.
pub fn armor_points(inventory: &Inventory) -> u32 {
    (ARMOR_START..ARMOR_START + ARMOR_SLOTS)
        .filter_map(|i| inventory.slots[i])
        .map(|s| match s.item.kind() {
            Some(ItemKind::Armor { defense, .. }) => defense as u32,
            _ => 0,
        })
        .sum()
}

/// Damage to apply to the player; the survival systems turn it into lost health and a knockback.
#[derive(Message, Clone, Copy, Debug)]
pub struct DamagePlayer {
    pub amount: f32,
    /// Horizontal push away from the attacker (blocks per second), zero for fall damage etc.
    pub knockback: Vec3,
}

/// Sent once when the player's health reaches zero.
#[derive(Message, Clone, Copy, Debug)]
pub struct PlayerDied;

/// Experience points; the level and bar are derived from the total.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Experience {
    pub total: u32,
}

/// Total experience needed to reach `level`.
pub fn xp_for_level(level: u32) -> u32 {
    let l = level as f64;
    (match level {
        0..=16 => l * l + 6.0 * l,
        17..=31 => 2.5 * l * l - 40.5 * l + 360.0,
        _ => 4.5 * l * l - 162.5 * l + 2220.0,
    }) as u32
}

impl Experience {
    pub fn add(&mut self, points: u32) {
        self.total = self.total.saturating_add(points);
    }

    pub fn level(&self) -> u32 {
        let mut level = 0;
        while xp_for_level(level + 1) <= self.total && level < 10_000 {
            level += 1;
        }
        level
    }

    /// Progress to the next level, `0..1` (what the experience bar shows).
    pub fn progress(&self) -> f32 {
        let l = self.level();
        let (lo, hi) = (xp_for_level(l), xp_for_level(l + 1));
        ((self.total - lo) as f32 / (hi - lo).max(1) as f32).clamp(0.0, 1.0)
    }
}

/// Remembers how far the player has been falling.
#[derive(Resource, Default, Debug)]
struct FallTracker {
    peak: f32,
    airborne: bool,
    was_on_ground: bool,
    last_pos: Vec3,
}

/// Blocks fallen once the player lands, minus the harmless distance, rounded up.
pub fn fall_damage(distance: f32) -> f32 {
    if distance > SAFE_FALL { (distance - SAFE_FALL).ceil() } else { 0.0 }
}

pub struct SurvivalPlugin;

impl Plugin for SurvivalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Vitals>()
            .init_resource::<Experience>()
            .init_resource::<FallTracker>()
            .insert_resource(GameMode::Survival)
            .add_message::<DamagePlayer>()
            .add_message::<PlayerDied>()
            .add_plugins((mining::MiningPlugin, drops::DropsPlugin))
            .add_systems(
                Update,
                (creative_keeps_full, movement_exhaustion, track_falling, drowning, apply_damage, tick_vitals, detect_death)
                    .chain()
                    .run_if(in_state(AppState::InGame)),
            );
    }
}

fn creative_keeps_full(mode: Res<GameMode>, mut vitals: ResMut<Vitals>) {
    if *mode == GameMode::Creative && (vitals.health < MAX_HEALTH || vitals.food < MAX_FOOD) {
        let mut v = Vitals::full();
        v.saturation = MAX_FOOD;
        *vitals = v;
    }
}

/// Walking is free, sprinting and jumping make the player hungrier.
fn movement_exhaustion(
    mode: Res<GameMode>,
    capture: Res<InputCapture>,
    mut vitals: ResMut<Vitals>,
    mut tracker: ResMut<FallTracker>,
    player: Query<(&Player, &Transform)>,
) {
    let Ok((p, t)) = player.single() else { return };
    if *mode == GameMode::Creative || !p.active {
        tracker.last_pos = t.translation;
        return;
    }
    let moved = Vec2::new(t.translation.x - tracker.last_pos.x, t.translation.z - tracker.last_pos.z).length();
    tracker.last_pos = t.translation;
    if moved > 0.0 && moved < 5.0 {
        if p.sprinting && p.on_ground {
            vitals.add_exhaustion(0.1 * moved);
        } else if p.in_water {
            vitals.add_exhaustion(0.01 * moved);
        }
    }
    // Leaving the ground with upward speed is a jump.
    if capture.0 && tracker.was_on_ground && !p.on_ground && p.vel.y > 1.0 && !p.flying {
        vitals.add_exhaustion(if p.sprinting { 0.2 } else { 0.05 });
    }
    tracker.was_on_ground = p.on_ground;
}

fn track_falling(
    mode: Res<GameMode>,
    mut tracker: ResMut<FallTracker>,
    player: Query<(&Player, &Transform)>,
    mut damage: MessageWriter<DamagePlayer>,
) {
    let Ok((p, t)) = player.single() else { return };
    let y = t.translation.y;
    if !p.active || p.flying || p.in_water || *mode == GameMode::Creative {
        tracker.peak = y;
        tracker.airborne = false;
        return;
    }
    if p.on_ground {
        if tracker.airborne {
            let dmg = fall_damage(tracker.peak - y);
            if dmg > 0.0 {
                damage.write(DamagePlayer { amount: dmg, knockback: Vec3::ZERO });
            }
        }
        tracker.airborne = false;
        tracker.peak = y;
    } else {
        if !tracker.airborne {
            tracker.airborne = true;
            tracker.peak = y;
        }
        tracker.peak = tracker.peak.max(y);
    }
}

fn drowning(
    time: Res<Time>,
    mode: Res<GameMode>,
    world: Res<VoxelWorld>,
    mut vitals: ResMut<Vitals>,
    player: Query<(&Player, &Transform)>,
) {
    let Ok((p, t)) = player.single() else { return };
    if !p.active || *mode == GameMode::Creative {
        return;
    }
    let eye = (t.translation + Vec3::Y * p.eye_height).floor().as_ivec3();
    let submerged = world.block(eye) == Some(BlockId::WATER);
    let dt = time.delta_secs();
    vitals.breathe(submerged, dt);
}

fn apply_damage(
    mut messages: MessageReader<DamagePlayer>,
    mode: Res<GameMode>,
    inventory: Res<Inventory>,
    mut vitals: ResMut<Vitals>,
    mut player: Query<&mut Player>,
) {
    let armor = armor_points(&inventory);
    for m in messages.read() {
        if *mode == GameMode::Creative {
            continue;
        }
        if vitals.damage(m.amount, armor) > 0.0
            && m.knockback != Vec3::ZERO
            && let Ok(mut p) = player.single_mut()
        {
            p.vel += m.knockback + Vec3::Y * 4.0;
        }
    }
}

fn tick_vitals(time: Res<Time>, mode: Res<GameMode>, mut vitals: ResMut<Vitals>) {
    if *mode == GameMode::Survival {
        vitals.tick(time.delta_secs());
    }
}

fn detect_death(vitals: Res<Vitals>, mut died: MessageWriter<PlayerDied>, mut was_dead: Local<bool>) {
    if vitals.is_dead() && !*was_dead {
        died.write(PlayerDied);
    }
    *was_dead = vitals.is_dead();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::{Item, ItemStack};

    #[test]
    fn exhaustion_drains_saturation_before_food() {
        let mut v = Vitals::full();
        v.saturation = 2.0;
        v.add_exhaustion(8.5);
        v.tick(0.0);
        assert_eq!((v.saturation, v.food), (0.0, MAX_FOOD), "two units of saturation absorb the first two 4-point steps");
        v.add_exhaustion(4.0);
        v.tick(0.0);
        assert_eq!(v.food, MAX_FOOD - 1.0);
        assert!((v.exhaustion - 0.5).abs() < 1e-5, "{}", v.exhaustion);
    }

    #[test]
    fn well_fed_players_regenerate_and_pay_for_it() {
        let mut v = Vitals::full();
        v.health = 10.0;
        v.saturation = 0.0;
        for _ in 0..41 {
            v.tick(0.1);
        }
        assert_eq!(v.health, 11.0, "one point per {REGEN_INTERVAL} s");
        assert!(v.food < MAX_FOOD || v.exhaustion > 5.0, "regeneration costs hunger");
        // A hungry player does not regenerate.
        let mut hungry = Vitals { food: 10.0, health: 10.0, ..Vitals::full() };
        for _ in 0..200 {
            hungry.tick(0.1);
        }
        assert_eq!(hungry.health, 10.0);
    }

    #[test]
    fn starvation_stops_at_half_a_heart() {
        let mut v = Vitals { food: 0.0, saturation: 0.0, ..Vitals::full() };
        for _ in 0..2000 {
            v.tick(0.1);
        }
        assert_eq!(v.health, 1.0);
        assert!(!v.is_dead());
    }

    #[test]
    fn armor_reduces_damage_and_cooldown_blocks_repeat_hits() {
        let mut bare = Vitals::full();
        assert_eq!(bare.damage(10.0, 0), 10.0);
        assert_eq!(bare.health, 10.0);
        assert_eq!(bare.damage(10.0, 0), 0.0, "invulnerable right after a hit");
        bare.tick(0.6);
        assert!((bare.damage(4.0, 15) - 1.6).abs() < 1e-5);
        assert!((armor_factor(15) - 0.4).abs() < 1e-6);
        assert!((armor_factor(100) - 0.2).abs() < 1e-5, "armor tops out at 80 %");
        let mut dead = Vitals::full();
        dead.damage(100.0, 0);
        assert!(dead.is_dead());
    }

    #[test]
    fn eating_fills_food_and_saturation_up_to_the_cap() {
        let mut v = Vitals { food: 10.0, saturation: 0.0, ..Vitals::full() };
        v.eat(8, 12.8);
        assert_eq!(v.food, 18.0);
        assert_eq!(v.saturation, 12.8_f32.min(18.0));
        v.eat(8, 12.8);
        assert_eq!((v.food, v.saturation), (20.0, 20.0));
    }

    #[test]
    fn drowning_starts_after_the_air_runs_out() {
        let mut v = Vitals::full();
        for _ in 0..140 {
            v.breathe(true, 0.1);
        }
        assert_eq!(v.health, MAX_HEALTH, "14 s under water are survivable");
        for _ in 0..30 {
            v.breathe(true, 0.1);
        }
        assert!(v.health < MAX_HEALTH, "drowning hurts: {}", v.health);
        // Fresh air restores the breath.
        for _ in 0..40 {
            v.breathe(false, 0.1);
        }
        assert_eq!(v.air, MAX_AIR);
    }

    #[test]
    fn fall_damage_starts_beyond_three_blocks() {
        assert_eq!(fall_damage(2.9), 0.0);
        assert_eq!(fall_damage(3.0), 0.0);
        assert_eq!(fall_damage(4.2), 2.0);
        assert_eq!(fall_damage(23.0), 20.0);
    }

    #[test]
    fn experience_levels_follow_the_curve() {
        assert_eq!(xp_for_level(1), 7);
        assert_eq!(xp_for_level(16), 352);
        assert_eq!(xp_for_level(17), 394);
        assert_eq!(xp_for_level(31), 1507);
        assert_eq!(xp_for_level(32), 1628);
        let mut xp = Experience::default();
        assert_eq!((xp.level(), xp.progress()), (0, 0.0));
        xp.add(7);
        assert_eq!(xp.level(), 1);
        xp.add(5);
        assert_eq!(xp.level(), 1);
        assert!(xp.progress() > 0.3 && xp.progress() < 0.6, "{}", xp.progress());
        xp.add(10_000);
        assert!(xp.level() > 30);
        assert!(xp.progress() < 1.0);
    }

    #[test]
    fn worn_armor_adds_up() {
        let mut inv = Inventory::empty();
        assert_eq!(armor_points(&inv), 0);
        inv.slots[ARMOR_START] = Some(ItemStack::new(Item::IRON_HELMET, 1));
        inv.slots[ARMOR_START + 1] = Some(ItemStack::new(Item::IRON_CHESTPLATE, 1));
        assert_eq!(armor_points(&inv), 8);
        inv.slots[ARMOR_START + 2] = Some(ItemStack::new(Item::IRON_LEGGINGS, 1));
        inv.slots[ARMOR_START + 3] = Some(ItemStack::new(Item::IRON_BOOTS, 1));
        assert_eq!(armor_points(&inv), 15);
    }

    #[test]
    fn a_dead_save_is_revived_and_garbage_is_clamped() {
        assert_eq!(Vitals::from_save(0.0, 5.0, 3.0).health, MAX_HEALTH);
        let v = Vitals::from_save(f32::NAN, 500.0, 90.0);
        assert_eq!((v.health, v.food, v.saturation), (MAX_HEALTH, MAX_FOOD, MAX_FOOD));
    }
}
