//! Full-screen feedback: the tint of being under water or lava, the red flash when hurt, the glow
//! of burning, and the death screen.

use super::screens::Screen;
use super::widgets::text_font;
use crate::physics::fluids::{FluidKind, surface_in_cell};
use crate::player::controller::PlayerCamera;
use crate::player::health::Health;
use crate::world::VoxelWorld;
use bevy::prelude::*;

#[derive(Component)]
struct FluidTint;

#[derive(Component)]
struct HurtTint;

#[derive(Component)]
struct DeathScreen;

pub struct OverlayPlugin;

impl Plugin for OverlayPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostStartup, spawn_overlays).add_systems(Update, update_overlays);
    }
}

fn full_screen(z: i32) -> (Node, GlobalZIndex) {
    (
        Node {
            position_type: PositionType::Absolute,
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_direction: FlexDirection::Column,
            ..default()
        },
        GlobalZIndex(z),
    )
}

fn spawn_overlays(mut commands: Commands) {
    commands.spawn((FluidTint, full_screen(-30), BackgroundColor(Color::NONE), Pickable::IGNORE));
    commands.spawn((HurtTint, full_screen(-20), BackgroundColor(Color::NONE), Pickable::IGNORE));
    commands
        .spawn((DeathScreen, full_screen(50), BackgroundColor(Color::NONE), Visibility::Hidden, Pickable::IGNORE))
        .with_children(|p| {
            p.spawn((
                Text::new("You died!"),
                text_font(56.0),
                TextColor(Color::srgb(1.0, 0.9, 0.9)),
                TextShadow { offset: Vec2::splat(3.0), color: Color::BLACK },
            ));
            p.spawn((Text::new("Respawning..."), text_font(22.0), TextColor(Color::srgba(1.0, 1.0, 1.0, 0.75))));
        });
}

/// The fluid the camera is inside of, if any.
pub fn camera_fluid(world: &VoxelWorld, eye: Vec3) -> Option<FluidKind> {
    let cell = eye.floor().as_ivec3();
    let (kind, surface) = surface_in_cell(world, cell)?;
    (eye.y < cell.y as f32 + surface).then_some(kind)
}

fn update_overlays(
    screen: Res<Screen>,
    health: Res<Health>,
    world: Res<VoxelWorld>,
    cam: Query<&GlobalTransform, With<PlayerCamera>>,
    mut fluid: Query<&mut BackgroundColor, (With<FluidTint>, Without<HurtTint>)>,
    mut hurt: Query<&mut BackgroundColor, (With<HurtTint>, Without<FluidTint>)>,
    mut death: Query<(&mut Visibility, &mut BackgroundColor), (With<DeathScreen>, Without<FluidTint>, Without<HurtTint>)>,
) {
    let playing = *screen == Screen::Playing || *screen == Screen::Inventory;
    if let Ok(mut bg) = fluid.single_mut() {
        let tint = match cam.single().ok().and_then(|g| camera_fluid(&world, g.translation())) {
            Some(FluidKind::Water) if playing => Color::srgba(0.04, 0.18, 0.5, 0.42),
            Some(FluidKind::Lava) if playing => Color::srgba(0.95, 0.3, 0.02, 0.88),
            _ => Color::NONE,
        };
        if bg.0 != tint {
            bg.0 = tint;
        }
    }
    if let Ok(mut bg) = hurt.single_mut() {
        let flash = health.flash * 0.4;
        let burn = if health.burning > 0.0 { 0.16 } else { 0.0 };
        let tint = if flash > 0.0 {
            Color::srgba(0.85, 0.05, 0.05, flash + burn)
        } else if burn > 0.0 {
            Color::srgba(1.0, 0.45, 0.05, burn)
        } else {
            Color::NONE
        };
        if bg.0 != tint {
            bg.0 = tint;
        }
    }
    if let Ok((mut vis, mut bg)) = death.single_mut() {
        let want = if health.dead { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        bg.0 = Color::srgba(0.35, 0.0, 0.0, (health.death_timer() * 1.5).min(0.65));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::block::BlockId;
    use crate::world::chunk::Chunk;

    #[test]
    fn the_camera_knows_which_fluid_it_is_in() {
        let mut world = VoxelWorld::default();
        world.insert_chunk(IVec2::ZERO, Chunk::empty());
        world.set_block(IVec3::new(2, 10, 2), BlockId::WATER);
        world.set_block(IVec3::new(4, 10, 2), BlockId::LAVA);
        assert_eq!(camera_fluid(&world, Vec3::new(2.5, 10.4, 2.5)), Some(FluidKind::Water));
        assert_eq!(camera_fluid(&world, Vec3::new(4.5, 10.4, 2.5)), Some(FluidKind::Lava));
        assert_eq!(camera_fluid(&world, Vec3::new(2.5, 10.95, 2.5)), None, "above the slightly lowered surface");
        assert_eq!(camera_fluid(&world, Vec3::new(8.5, 10.4, 8.5)), None);
    }
}
