//! Box models of the mobs. Every model is a list of coloured cuboids (feet at the origin, the
//! face looks along -Z) plus legs and arms that swing around a pivot.

use super::MobKind;
use bevy::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct BoxSpec {
    pub size: Vec3,
    /// Centre of the box relative to the feet.
    pub center: Vec3,
    pub color: [f32; 3],
}

/// A limb hanging from `hip`: its box is `size` long, starting at the pivot and extending downward.
#[derive(Clone, Copy, Debug)]
pub struct LimbSpec {
    pub hip: Vec3,
    pub size: Vec3,
    pub color: [f32; 3],
    /// Swing phase offset: opposite legs use `PI`.
    pub phase: f32,
    /// Hold the limb straight forward (zombie arms) instead of swinging it.
    pub raised: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ModelSpec {
    pub boxes: Vec<BoxSpec>,
    pub limbs: Vec<LimbSpec>,
}

fn b(size: [f32; 3], center: [f32; 3], color: [f32; 3]) -> BoxSpec {
    BoxSpec { size: Vec3::from(size), center: Vec3::from(center), color }
}

fn limb(hip: [f32; 3], size: [f32; 3], color: [f32; 3], phase: f32) -> LimbSpec {
    LimbSpec { hip: Vec3::from(hip), size: Vec3::from(size), color, phase, raised: false }
}

const EYE: [f32; 3] = [0.05, 0.05, 0.07];

pub fn model(kind: MobKind) -> ModelSpec {
    use std::f32::consts::PI;
    match kind {
        MobKind::Pig => {
            let skin = [0.96, 0.67, 0.66];
            let dark = [0.88, 0.52, 0.52];
            ModelSpec {
                boxes: vec![
                    b([0.62, 0.56, 0.95], [0.0, 0.64, 0.0], skin),
                    b([0.50, 0.46, 0.46], [0.0, 0.74, -0.66], skin),
                    b([0.26, 0.18, 0.10], [0.0, 0.66, -0.93], dark),
                    b([0.07, 0.07, 0.03], [0.14, 0.80, -0.90], EYE),
                    b([0.07, 0.07, 0.03], [-0.14, 0.80, -0.90], EYE),
                ],
                limbs: vec![
                    limb([0.2, 0.36, -0.3], [0.2, 0.36, 0.2], dark, 0.0),
                    limb([-0.2, 0.36, -0.3], [0.2, 0.36, 0.2], dark, PI),
                    limb([0.2, 0.36, 0.3], [0.2, 0.36, 0.2], dark, PI),
                    limb([-0.2, 0.36, 0.3], [0.2, 0.36, 0.2], dark, 0.0),
                ],
            }
        }
        MobKind::Cow => {
            let hide = [0.36, 0.24, 0.17];
            let white = [0.93, 0.92, 0.90];
            ModelSpec {
                boxes: vec![
                    b([0.78, 0.72, 1.2], [0.0, 0.92, 0.0], hide),
                    b([0.40, 0.30, 0.04], [0.0, 1.0, -0.62], white),
                    b([0.30, 0.34, 0.04], [0.40, 0.98, 0.15], white),
                    b([0.52, 0.48, 0.45], [0.0, 1.1, -0.84], hide),
                    b([0.30, 0.22, 0.10], [0.0, 1.0, -1.1], [0.85, 0.72, 0.65]),
                    b([0.08, 0.14, 0.08], [0.22, 1.42, -0.80], [0.9, 0.88, 0.8]),
                    b([0.08, 0.14, 0.08], [-0.22, 1.42, -0.80], [0.9, 0.88, 0.8]),
                    b([0.07, 0.07, 0.03], [0.15, 1.16, -1.07], EYE),
                    b([0.07, 0.07, 0.03], [-0.15, 1.16, -1.07], EYE),
                ],
                limbs: vec![
                    limb([0.24, 0.56, -0.42], [0.22, 0.56, 0.22], hide, 0.0),
                    limb([-0.24, 0.56, -0.42], [0.22, 0.56, 0.22], hide, PI),
                    limb([0.24, 0.56, 0.42], [0.22, 0.56, 0.22], hide, PI),
                    limb([-0.24, 0.56, 0.42], [0.22, 0.56, 0.22], hide, 0.0),
                ],
            }
        }
        MobKind::Sheep => {
            let wool = [0.94, 0.94, 0.92];
            let skin = [0.80, 0.70, 0.62];
            ModelSpec {
                boxes: vec![
                    b([0.78, 0.72, 1.0], [0.0, 0.88, 0.0], wool),
                    b([0.46, 0.44, 0.40], [0.0, 1.08, -0.68], skin),
                    b([0.50, 0.22, 0.30], [0.0, 1.26, -0.62], wool),
                    b([0.07, 0.07, 0.03], [0.12, 1.12, -0.89], EYE),
                    b([0.07, 0.07, 0.03], [-0.12, 1.12, -0.89], EYE),
                ],
                limbs: vec![
                    limb([0.2, 0.52, -0.32], [0.18, 0.52, 0.18], skin, 0.0),
                    limb([-0.2, 0.52, -0.32], [0.18, 0.52, 0.18], skin, PI),
                    limb([0.2, 0.52, 0.32], [0.18, 0.52, 0.18], skin, PI),
                    limb([-0.2, 0.52, 0.32], [0.18, 0.52, 0.18], skin, 0.0),
                ],
            }
        }
        MobKind::Zombie => {
            let skin = [0.34, 0.56, 0.32];
            let shirt = [0.18, 0.46, 0.68];
            let pants = [0.22, 0.24, 0.48];
            let mut arm_l = limb([0.37, 1.55, 0.0], [0.2, 0.7, 0.2], skin, 0.0);
            let mut arm_r = limb([-0.37, 1.55, 0.0], [0.2, 0.7, 0.2], skin, 0.0);
            arm_l.raised = true;
            arm_r.raised = true;
            ModelSpec {
                boxes: vec![
                    b([0.5, 0.5, 0.5], [0.0, 1.82, 0.0], skin),
                    b([0.5, 0.78, 0.26], [0.0, 1.17, 0.0], shirt),
                    b([0.09, 0.09, 0.03], [0.12, 1.86, -0.26], EYE),
                    b([0.09, 0.09, 0.03], [-0.12, 1.86, -0.26], EYE),
                    b([0.22, 0.05, 0.03], [0.0, 1.70, -0.26], [0.2, 0.3, 0.2]),
                ],
                limbs: vec![
                    arm_l,
                    arm_r,
                    limb([0.13, 0.78, 0.0], [0.24, 0.78, 0.24], pants, 0.0),
                    limb([-0.13, 0.78, 0.0], [0.24, 0.78, 0.24], pants, PI),
                ],
            }
        }
        MobKind::Villager => {
            let robe = [0.46, 0.33, 0.22];
            let skin = [0.80, 0.60, 0.50];
            ModelSpec {
                boxes: vec![
                    b([0.55, 1.15, 0.38], [0.0, 0.7, 0.0], robe),
                    b([0.46, 0.54, 0.46], [0.0, 1.55, 0.0], skin),
                    b([0.12, 0.22, 0.14], [0.0, 1.5, -0.3], [0.72, 0.52, 0.42]),
                    b([0.52, 0.06, 0.48], [0.0, 1.86, 0.0], [0.30, 0.20, 0.12]),
                    b([0.62, 0.2, 0.22], [0.0, 1.05, -0.26], skin),
                    b([0.07, 0.07, 0.03], [0.11, 1.6, -0.24], [0.1, 0.35, 0.15]),
                    b([0.07, 0.07, 0.03], [-0.11, 1.6, -0.24], [0.1, 0.35, 0.15]),
                ],
                limbs: vec![
                    limb([0.12, 0.3, 0.0], [0.22, 0.3, 0.24], [0.3, 0.22, 0.14], 0.0),
                    limb([-0.12, 0.3, 0.0], [0.22, 0.3, 0.24], [0.3, 0.22, 0.14], PI),
                ],
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn models_stand_on_the_ground_and_match_the_hit_box() {
        for kind in MobKind::ALL {
            let spec = model(kind);
            let (w, h) = kind.size();
            let mut top = 0.0f32;
            let mut bottom = f32::MAX;
            let mut half = 0.0f32;
            for bx in &spec.boxes {
                top = top.max(bx.center.y + bx.size.y / 2.0);
                bottom = bottom.min(bx.center.y - bx.size.y / 2.0);
                half = half.max(bx.center.x.abs() + bx.size.x / 2.0);
            }
            for l in &spec.limbs {
                bottom = bottom.min(l.hip.y - l.size.y);
                top = top.max(l.hip.y);
                half = half.max(l.hip.x.abs() + l.size.x / 2.0);
            }
            assert!(bottom.abs() < 0.06, "{kind:?} floats or sinks: lowest point {bottom}");
            assert!((top - h).abs() < 0.35, "{kind:?}: model is {top} tall, hit box {h}");
            assert!(half <= w + 0.2 && half >= w * 0.5, "{kind:?}: half width {half} vs hit box {w}");
        }
    }

    #[test]
    fn only_zombies_hold_their_arms_out() {
        for kind in MobKind::ALL {
            let raised = model(kind).limbs.iter().filter(|l| l.raised).count();
            assert_eq!(raised, if kind == MobKind::Zombie { 2 } else { 0 });
        }
    }
}
