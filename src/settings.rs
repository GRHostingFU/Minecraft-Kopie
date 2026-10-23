//! Player-adjustable options that live outside the world itself (the render
//! distance is part of [`WorldSettings`](crate::world::streaming::WorldSettings)).

use bevy::prelude::*;

pub const MIN_RENDER_DISTANCE: i32 = 2;
pub const MAX_RENDER_DISTANCE: i32 = 32;
pub const DEFAULT_RENDER_DISTANCE: i32 = 10;
pub const MIN_FOV: f32 = 30.0;
pub const MAX_FOV: f32 = 110.0;
pub const MIN_SENSITIVITY: f32 = 0.1;
pub const MAX_SENSITIVITY: f32 = 2.0;

#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct GameSettings {
    /// Field of view in degrees (before the sprint widening).
    pub fov: f32,
    /// Multiplier on the base mouse speed; `1.0` is 100 %.
    pub mouse_sensitivity: f32,
    pub fullscreen: bool,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self { fov: 80.0, mouse_sensitivity: 1.0, fullscreen: false }
    }
}

impl GameSettings {
    /// Pulls every value into its valid range (guards against hand-edited save files).
    pub fn sanitized(mut self) -> Self {
        self.fov = if self.fov.is_finite() { self.fov.clamp(MIN_FOV, MAX_FOV) } else { 80.0 };
        self.mouse_sensitivity = if self.mouse_sensitivity.is_finite() {
            self.mouse_sensitivity.clamp(MIN_SENSITIVITY, MAX_SENSITIVITY)
        } else {
            1.0
        };
        self
    }
}

pub fn clamp_render_distance(d: i32) -> i32 {
    d.clamp(MIN_RENDER_DISTANCE, MAX_RENDER_DISTANCE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizing_clamps_and_rejects_nan() {
        let s = GameSettings { fov: 500.0, mouse_sensitivity: f32::NAN, fullscreen: true }.sanitized();
        assert_eq!((s.fov, s.mouse_sensitivity, s.fullscreen), (MAX_FOV, 1.0, true));
        assert_eq!(clamp_render_distance(99), 32);
        assert_eq!(clamp_render_distance(-3), 2);
    }
}
