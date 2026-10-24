//! Player health: damage from lava, fire, explosions, arrows and mobs, armor, natural
//! regeneration and death with a respawn at the world's starting area.

use super::controller::{Player, find_spawn_column};
use crate::audio::{PlaySound, Sound};
use crate::inventory::{ARMOR_START, Inventory};
use crate::world::streaming::Generator;
use bevy::prelude::*;

pub const MAX_HEALTH: f32 = 20.0;
/// Seconds of immunity after a hit (explosions ignore it).
const INVULNERABLE_SECS: f32 = 0.5;
const REGEN_SECS: f32 = 4.0;
const REGEN_DELAY_SECS: f32 = 5.0;
const LAVA_DAMAGE: f32 = 4.0;
const LAVA_INTERVAL: f32 = 0.5;
const BURN_SECS: f32 = 6.0;
const DEATH_SCREEN_SECS: f32 = 2.5;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DamageKind {
    Lava,
    Fire,
    Explosion,
    Arrow,
    Melee,
}

impl DamageKind {
    /// Fire and lava burn straight through armor.
    pub fn ignores_armor(self) -> bool {
        matches!(self, DamageKind::Lava | DamageKind::Fire)
    }
}

/// Damage dealt to the player, with an optional shove.
#[derive(Message, Clone, Copy, Debug)]
pub struct Damage {
    pub amount: f32,
    pub kind: DamageKind,
    pub impulse: Vec3,
}

#[derive(Resource, Debug, Clone)]
pub struct Health {
    pub current: f32,
    pub invulnerable: f32,
    pub since_hurt: f32,
    regen: f32,
    lava_timer: f32,
    fire_timer: f32,
    /// Seconds left on fire.
    pub burning: f32,
    /// Red screen flash, 1 right after a hit.
    pub flash: f32,
    pub dead: bool,
    death_timer: f32,
}

impl Default for Health {
    fn default() -> Self {
        Self {
            current: MAX_HEALTH,
            invulnerable: 0.0,
            since_hurt: 99.0,
            regen: 0.0,
            lava_timer: 0.0,
            fire_timer: 0.0,
            burning: 0.0,
            flash: 0.0,
            dead: false,
            death_timer: 0.0,
        }
    }
}

impl Health {
    /// Applies a hit. Returns `false` if it was ignored (already dead / still invulnerable).
    pub fn hurt(&mut self, amount: f32, kind: DamageKind, armor: f32) -> bool {
        if self.dead || amount <= 0.0 {
            return false;
        }
        if self.invulnerable > 0.0 && kind != DamageKind::Explosion {
            return false;
        }
        let taken = if kind.ignores_armor() { amount } else { amount * (1.0 - armor_reduction(armor)) };
        self.current = (self.current - taken).max(0.0);
        self.invulnerable = INVULNERABLE_SECS;
        self.since_hurt = 0.0;
        self.flash = 1.0;
        if self.current <= 0.0 {
            self.dead = true;
            self.death_timer = 0.0;
        }
        true
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn death_timer(&self) -> f32 {
        self.death_timer
    }
}

/// Fraction of damage that armor absorbs (each point is worth 4 %, at most 80 %).
pub fn armor_reduction(points: f32) -> f32 {
    (points.clamp(0.0, 20.0)) / 25.0
}

/// Total armor points of the equipped pieces (iron: helmet 2, chestplate 6, leggings 5, boots 2).
pub fn armor_points(inventory: &Inventory) -> f32 {
    const POINTS: [f32; 4] = [2.0, 6.0, 5.0, 2.0];
    (0..4)
        .filter(|i| inventory.slots[ARMOR_START + i].is_some_and(|s| s.item.armor_slot() == Some(*i)))
        .map(|i| POINTS[i])
        .sum()
}

pub struct HealthPlugin;

impl Plugin for HealthPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Health>().add_message::<Damage>().add_systems(
            Update,
            (environmental_damage, apply_damage, regenerate, handle_death).chain(),
        );
    }
}

fn environmental_damage(
    time: Res<Time>,
    mut health: ResMut<Health>,
    player: Query<&Player>,
    mut damage: MessageWriter<Damage>,
) {
    let Ok(p) = player.single() else { return };
    let dt = time.delta_secs();
    if !p.active || health.dead || dt == 0.0 {
        return;
    }
    if p.in_lava {
        health.burning = BURN_SECS;
        health.lava_timer -= dt;
        if health.lava_timer <= 0.0 {
            health.lava_timer = LAVA_INTERVAL;
            damage.write(Damage { amount: LAVA_DAMAGE, kind: DamageKind::Lava, impulse: Vec3::ZERO });
        }
    } else {
        health.lava_timer = 0.0;
    }
    if p.in_water {
        health.burning = 0.0;
    }
    if health.burning > 0.0 {
        health.burning -= dt;
        health.fire_timer -= dt;
        if health.fire_timer <= 0.0 {
            health.fire_timer = 1.0;
            damage.write(Damage { amount: 1.0, kind: DamageKind::Fire, impulse: Vec3::ZERO });
        }
    }
}

fn apply_damage(
    time: Res<Time>,
    mut health: ResMut<Health>,
    mut hits: MessageReader<Damage>,
    inventory: Res<Inventory>,
    mut player: Query<&mut Player>,
    mut sounds: MessageWriter<PlaySound>,
) {
    let dt = time.delta_secs();
    health.invulnerable = (health.invulnerable - dt).max(0.0);
    health.since_hurt += dt;
    health.flash = (health.flash - dt * 2.5).max(0.0);
    let armor = armor_points(&inventory);
    for hit in hits.read() {
        let was_dead = health.dead;
        if health.hurt(hit.amount, hit.kind, armor) {
            if let Ok(mut p) = player.single_mut() {
                p.vel += hit.impulse;
            }
            let sound = if health.dead && !was_dead { Sound::Death } else { Sound::PlayerHurt };
            sounds.write(PlaySound::global(sound));
        }
    }
}

fn regenerate(time: Res<Time>, mut health: ResMut<Health>) {
    if health.dead || health.current >= MAX_HEALTH || health.since_hurt < REGEN_DELAY_SECS {
        health.regen = 0.0;
        return;
    }
    health.regen += time.delta_secs();
    if health.regen >= REGEN_SECS {
        health.regen = 0.0;
        health.current = (health.current + 1.0).min(MAX_HEALTH);
    }
}

/// Freezes the player while dead, then sends them back to the starting area.
fn handle_death(
    time: Res<Time>,
    mut health: ResMut<Health>,
    generator: Res<Generator>,
    mut player: Query<(&mut Player, &mut Transform)>,
) {
    let Ok((mut p, mut t)) = player.single_mut() else { return };
    if !health.dead {
        return;
    }
    p.active = false;
    p.vel = Vec3::ZERO;
    health.death_timer += time.delta_secs();
    if health.death_timer >= DEATH_SCREEN_SECS {
        let c = find_spawn_column(&generator.0);
        t.translation = Vec3::new(c.x as f32 + 0.5, 200.0, c.y as f32 + 0.5);
        p.needs_spawn_search = true;
        p.flying = false;
        health.reset();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inventory::{Item, ItemStack};

    #[test]
    fn damage_is_reduced_by_armor_but_not_fire() {
        let mut h = Health::default();
        assert!(h.hurt(10.0, DamageKind::Arrow, 15.0));
        assert!((h.current - (20.0 - 10.0 * (1.0 - 0.6))).abs() < 1e-4);
        let mut h = Health::default();
        h.hurt(4.0, DamageKind::Lava, 20.0);
        assert_eq!(h.current, 16.0);
    }

    #[test]
    fn invulnerability_blocks_repeated_hits_except_explosions() {
        let mut h = Health::default();
        assert!(h.hurt(2.0, DamageKind::Melee, 0.0));
        assert!(!h.hurt(2.0, DamageKind::Melee, 0.0));
        assert_eq!(h.current, 18.0);
        assert!(h.hurt(5.0, DamageKind::Explosion, 0.0));
        assert_eq!(h.current, 13.0);
    }

    #[test]
    fn health_never_goes_below_zero_and_flags_death() {
        let mut h = Health::default();
        h.hurt(100.0, DamageKind::Explosion, 0.0);
        assert_eq!(h.current, 0.0);
        assert!(h.dead);
        assert!(!h.hurt(1.0, DamageKind::Explosion, 0.0), "the dead take no damage");
        h.reset();
        assert!(!h.dead && h.current == MAX_HEALTH);
    }

    #[test]
    fn armor_points_count_only_matching_pieces() {
        let mut inv = Inventory::empty();
        assert_eq!(armor_points(&inv), 0.0);
        inv.slots[ARMOR_START] = Some(ItemStack::new(Item::IRON_HELMET, 1));
        inv.slots[ARMOR_START + 1] = Some(ItemStack::new(Item::IRON_CHESTPLATE, 1));
        assert_eq!(armor_points(&inv), 8.0);
        inv.slots[ARMOR_START + 2] = Some(ItemStack::new(Item::IRON_LEGGINGS, 1));
        inv.slots[ARMOR_START + 3] = Some(ItemStack::new(Item::IRON_BOOTS, 1));
        assert_eq!(armor_points(&inv), 15.0);
        assert!(armor_reduction(15.0) < armor_reduction(20.0));
        assert_eq!(armor_reduction(100.0), 0.8);
    }
}
