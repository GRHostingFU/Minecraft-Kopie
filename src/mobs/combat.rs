//! The player hitting mobs: which mob is under the crosshair and the melee attack.

use super::{Mob, MobDamage};
use crate::physics::raycast::ray_box;
use crate::player::controller::{InputCapture, Player, PlayerCamera};
use bevy::prelude::*;

/// How far the player can hit a mob.
pub const MELEE_REACH: f32 = 4.0;
const ATTACK_COOLDOWN: f32 = 0.45;
pub const ATTACK_DAMAGE: f32 = 3.0;

/// The mob under the crosshair and its distance.
#[derive(Resource, Default, Clone, Copy, Debug)]
pub struct MobTarget(pub Option<(Entity, f32)>);

impl MobTarget {
    /// Is a mob closer than a block hit at `block_distance` (or any block at all)?
    pub fn in_front_of(&self, block_distance: Option<f32>) -> bool {
        self.0.is_some_and(|(_, d)| block_distance.is_none_or(|b| d < b))
    }
}

pub fn update_mob_target(
    cam: Query<&GlobalTransform, With<PlayerCamera>>,
    mobs: Query<(Entity, &Mob, &Transform)>,
    mut target: ResMut<MobTarget>,
) {
    target.0 = cam.single().ok().and_then(|g| {
        let (origin, dir) = (g.translation(), g.forward().as_vec3());
        mobs.iter()
            .filter_map(|(e, mob, t)| {
                let body = mob.body(t.translation);
                // A little slack makes thin skeletons easier to hit.
                let (lo, hi) = (body.min() - Vec3::splat(0.08), body.max() + Vec3::splat(0.08));
                ray_box(origin, dir, lo, hi).filter(|(d, _)| *d <= MELEE_REACH).map(|(d, _)| (e, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    });
}

#[allow(clippy::too_many_arguments)]
pub fn attack_mobs(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    capture: Res<InputCapture>,
    target: Res<MobTarget>,
    player: Query<&Player>,
    cam: Query<&GlobalTransform, With<PlayerCamera>>,
    mut cooldown: Local<f32>,
    mut damage: MessageWriter<MobDamage>,
) {
    *cooldown = (*cooldown - time.delta_secs()).max(0.0);
    let Ok(p) = player.single() else { return };
    if !capture.0 || !p.active || *cooldown > 0.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some((e, _)) = target.0 else { return };
    let forward = cam.single().map(|g| g.forward().as_vec3()).unwrap_or(Vec3::NEG_Z);
    let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or_zero();
    damage.write(MobDamage { target: e, amount: ATTACK_DAMAGE, knockback: flat * 5.5 + Vec3::Y * 3.5 });
    *cooldown = ATTACK_COOLDOWN;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mob_in_front_of_the_block_wins_only_if_closer() {
        let t = MobTarget(Some((Entity::PLACEHOLDER, 2.0)));
        assert!(t.in_front_of(None));
        assert!(t.in_front_of(Some(3.0)));
        assert!(!t.in_front_of(Some(1.5)));
        assert!(!MobTarget(None).in_front_of(None));
    }
}
