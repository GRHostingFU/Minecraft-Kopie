//! Audio: a procedural sound synthesiser ([`synth`]) and the glue that plays its output.
//!
//! The game talks to this module through [`PlaySound`] messages. The synthesis (and the mapping
//! from game events to clips) always compiles and is unit-tested; the actual playback through
//! Bevy's audio backend is behind the `audio` cargo feature (on by default, it needs the ALSA
//! development files on Linux).

pub mod synth;
pub mod triggers;
pub mod wav;

use crate::world::block::{BlockId, Kind};
use bevy::math::Vec3;
use bevy::prelude::*;
pub use synth::Material;

/// A sound the game wants to hear.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sound {
    Step(BlockId),
    Dig(BlockId),
    Break(BlockId),
    Place(BlockId),
    UiClick,
    Pop,
    LeverClick,
    ButtonClick,
    PlateClick,
    PistonOut,
    PistonIn,
    Explosion,
    Hiss,
    Shoot,
    ArrowHit,
    PlayerHurt,
    MobHurt,
    Death,
    Splash,
    Fizz,
    LavaPop,
    Fire,
    Bones,
}

/// Request to play a sound, at a world position (3D, fades with distance) or `None` (UI / own body).
#[derive(Message, Clone, Copy, Debug)]
pub struct PlaySound {
    pub sound: Sound,
    pub at: Option<Vec3>,
    pub volume: f32,
}

impl PlaySound {
    pub fn global(sound: Sound) -> Self {
        Self { sound, at: None, volume: 1.0 }
    }

    pub fn at(sound: Sound, pos: Vec3) -> Self {
        Self { sound, at: Some(pos), volume: 1.0 }
    }

    pub fn with_volume(mut self, volume: f32) -> Self {
        self.volume = volume;
        self
    }
}

/// Which surface sound a block makes.
pub fn material_of(block: BlockId) -> Material {
    match block.kind() {
        Kind::Solid => match block {
            BlockId::GRASS => Material::Grass,
            BlockId::DIRT => Material::Dirt,
            BlockId::SAND => Material::Sand,
            BlockId::LOG | BlockId::PLANKS => Material::Wood,
            BlockId::SNOW => Material::Snow,
            BlockId::LEAVES => Material::Leaves,
            _ => Material::Stone,
        },
        Kind::Lever { .. } | Kind::Torch { .. } | Kind::PistonHead { .. } => Material::Wood,
        Kind::Dust { .. } => Material::Sand,
        Kind::Piston { .. } => Material::Wood,
        _ => Material::Stone,
    }
}

/// A concrete pre-rendered clip.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Clip {
    Step(Material, u8),
    Dig(Material, u8),
    Break(Material, u8),
    Place(Material, u8),
    UiClick,
    Pop,
    Lever,
    Button,
    Plate,
    PistonOut,
    PistonIn,
    Explosion(u8),
    Hiss,
    Shoot,
    ArrowHit,
    PlayerHurt,
    MobHurt,
    Death,
    Splash,
    Fizz,
    LavaPop(u8),
    Fire,
    Bones,
}

/// Variants rendered per footstep / dig / break material, so repeated sounds do not machine-gun.
pub const VARIANTS: u8 = 4;

impl Clip {
    pub fn synthesize(self) -> Vec<f32> {
        match self {
            Clip::Step(m, v) => synth::footstep(m, v as u32),
            Clip::Dig(m, v) => synth::dig(m, v as u32),
            Clip::Break(m, v) => synth::break_block(m, v as u32),
            Clip::Place(m, v) => synth::place_block(m, v as u32),
            Clip::UiClick => synth::ui_click(),
            Clip::Pop => synth::pop(),
            Clip::Lever => synth::click(1500.0, 0.0),
            Clip::Button => synth::click(2100.0, 0.3),
            Clip::Plate => synth::click(900.0, 0.5),
            Clip::PistonOut => synth::piston(true),
            Clip::PistonIn => synth::piston(false),
            Clip::Explosion(v) => synth::explosion(v as u32),
            Clip::Hiss => synth::hiss(),
            Clip::Shoot => synth::shoot(),
            Clip::ArrowHit => synth::arrow_hit(),
            Clip::PlayerHurt => synth::hurt(true),
            Clip::MobHurt => synth::hurt(false),
            Clip::Death => synth::death(),
            Clip::Splash => synth::splash(),
            Clip::Fizz => synth::fizz(),
            Clip::LavaPop(v) => synth::lava_pop(v as u32),
            Clip::Fire => synth::fire(),
            Clip::Bones => synth::bones(),
        }
    }

    /// Every clip the bank pre-renders.
    pub fn all() -> Vec<Clip> {
        let mut v = Vec::new();
        for m in Material::ALL {
            for i in 0..VARIANTS {
                v.push(Clip::Step(m, i));
                v.push(Clip::Dig(m, i));
            }
            v.push(Clip::Break(m, 0));
            v.push(Clip::Break(m, 1));
            v.push(Clip::Place(m, 0));
            v.push(Clip::Place(m, 1));
        }
        v.extend([Clip::UiClick, Clip::Pop, Clip::Lever, Clip::Button, Clip::Plate, Clip::PistonOut, Clip::PistonIn]);
        v.extend([Clip::Explosion(0), Clip::Explosion(1), Clip::Hiss, Clip::Shoot, Clip::ArrowHit]);
        v.extend([Clip::PlayerHurt, Clip::MobHurt, Clip::Death, Clip::Splash, Clip::Fizz, Clip::Fire, Clip::Bones]);
        v.extend((0..3).map(Clip::LavaPop));
        v
    }
}

/// The clip for a sound; `variant` selects between alternatives.
pub fn clip_for(sound: Sound, variant: u32) -> Clip {
    let v4 = (variant % VARIANTS as u32) as u8;
    let v2 = (variant % 2) as u8;
    match sound {
        Sound::Step(b) => Clip::Step(material_of(b), v4),
        Sound::Dig(b) => Clip::Dig(material_of(b), v4),
        Sound::Break(b) => Clip::Break(material_of(b), v2),
        Sound::Place(b) => Clip::Place(material_of(b), v2),
        Sound::UiClick => Clip::UiClick,
        Sound::Pop => Clip::Pop,
        Sound::LeverClick => Clip::Lever,
        Sound::ButtonClick => Clip::Button,
        Sound::PlateClick => Clip::Plate,
        Sound::PistonOut => Clip::PistonOut,
        Sound::PistonIn => Clip::PistonIn,
        Sound::Explosion => Clip::Explosion(v2),
        Sound::Hiss => Clip::Hiss,
        Sound::Shoot => Clip::Shoot,
        Sound::ArrowHit => Clip::ArrowHit,
        Sound::PlayerHurt => Clip::PlayerHurt,
        Sound::MobHurt => Clip::MobHurt,
        Sound::Death => Clip::Death,
        Sound::Splash => Clip::Splash,
        Sound::Fizz => Clip::Fizz,
        Sound::LavaPop => Clip::LavaPop((variant % 3) as u8),
        Sound::Fire => Clip::Fire,
        Sound::Bones => Clip::Bones,
    }
}

/// Sounds farther away than this are not played at all.
pub const HEARING_RANGE: f32 = 40.0;

/// Volume factor for a sound `distance` blocks from the listener.
pub fn attenuation(distance: f32) -> f32 {
    let x = (1.0 - distance / HEARING_RANGE).clamp(0.0, 1.0);
    x * x
}

pub struct GameAudioPlugin;

impl Plugin for GameAudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<PlaySound>().add_plugins(triggers::SoundTriggersPlugin);
        #[cfg(feature = "audio")]
        app.add_plugins(playback::PlaybackPlugin);
        #[cfg(not(feature = "audio"))]
        app.add_systems(Update, |mut sounds: MessageReader<PlaySound>| sounds.clear());
    }
}

#[cfg(feature = "audio")]
mod playback {
    use super::*;
    use crate::player::controller::PlayerCamera;
    use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, SpatialListener, Volume};
    use bevy::platform::collections::HashMap;

    /// Rendered clips, ready to play.
    #[derive(Resource, Default)]
    struct SoundBank(HashMap<Clip, Handle<AudioSource>>);

    #[derive(Component)]
    struct SoundInstance;

    /// Most sounds that may overlap before new ones are dropped.
    const MAX_VOICES: usize = 32;

    pub struct PlaybackPlugin;

    impl Plugin for PlaybackPlugin {
        fn build(&self, app: &mut App) {
            app.init_resource::<SoundBank>()
                .add_systems(Startup, build_bank)
                .add_systems(Update, (attach_listener, play_sounds));
        }
    }

    fn build_bank(mut bank: ResMut<SoundBank>, mut sources: ResMut<Assets<AudioSource>>) {
        for clip in Clip::all() {
            let bytes = wav::encode_mono16(&clip.synthesize(), synth::SAMPLE_RATE);
            bank.0.insert(clip, sources.add(AudioSource { bytes: bytes.into() }));
        }
    }

    fn attach_listener(mut commands: Commands, cam: Query<Entity, (With<PlayerCamera>, Without<SpatialListener>)>) {
        for e in &cam {
            commands.entity(e).insert(SpatialListener::new(0.35));
        }
    }

    fn play_sounds(
        mut commands: Commands,
        mut requests: MessageReader<PlaySound>,
        bank: Res<SoundBank>,
        listener: Query<&GlobalTransform, With<PlayerCamera>>,
        voices: Query<(), With<SoundInstance>>,
        mut counter: Local<u32>,
    ) {
        let mut active = voices.iter().count();
        let ear = listener.single().map(|g| g.translation()).unwrap_or(Vec3::ZERO);
        for req in requests.read() {
            if active >= MAX_VOICES {
                break;
            }
            *counter = counter.wrapping_add(1);
            // Mix the counter so consecutive requests of the same sound pick different variants.
            let variant = counter.wrapping_mul(2_654_435_761) >> 28;
            let Some(handle) = bank.0.get(&clip_for(req.sound, variant)) else { continue };
            let mut volume = req.volume;
            let mut settings = PlaybackSettings::DESPAWN;
            let mut transform = Transform::IDENTITY;
            if let Some(pos) = req.at {
                volume *= attenuation(pos.distance(ear));
                settings.spatial = true;
                transform.translation = pos;
            }
            if volume < 0.01 {
                continue;
            }
            settings.volume = Volume::Linear(volume.min(1.5));
            commands.spawn((SoundInstance, AudioPlayer(handle.clone()), settings, transform));
            active += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_map_to_plausible_materials() {
        assert_eq!(material_of(BlockId::GRASS), Material::Grass);
        assert_eq!(material_of(BlockId::PLANKS), Material::Wood);
        assert_eq!(material_of(BlockId::COBBLE), Material::Stone);
        assert_eq!(material_of(BlockId::OBSIDIAN), Material::Stone);
        assert_eq!(material_of(BlockId::SAND), Material::Sand);
        assert_eq!(material_of(BlockId::piston(false, crate::world::Facing::Up, false)), Material::Wood);
    }

    #[test]
    fn every_requested_clip_exists_in_the_bank() {
        let all = Clip::all();
        let mut sounds = vec![
            Sound::UiClick,
            Sound::Pop,
            Sound::LeverClick,
            Sound::ButtonClick,
            Sound::PlateClick,
            Sound::PistonOut,
            Sound::PistonIn,
            Sound::Explosion,
            Sound::Hiss,
            Sound::Shoot,
            Sound::ArrowHit,
            Sound::PlayerHurt,
            Sound::MobHurt,
            Sound::Death,
            Sound::Splash,
            Sound::Fizz,
            Sound::LavaPop,
            Sound::Fire,
            Sound::Bones,
        ];
        for id in [BlockId::GRASS, BlockId::STONE, BlockId::LOG, BlockId::SNOW, BlockId::LEAVES, BlockId::SAND, BlockId::DIRT] {
            sounds.extend([Sound::Step(id), Sound::Dig(id), Sound::Break(id), Sound::Place(id)]);
        }
        for s in sounds {
            for variant in 0..16 {
                assert!(all.contains(&clip_for(s, variant)), "{s:?} variant {variant} is not pre-rendered");
            }
        }
    }

    #[test]
    fn the_bank_renders_without_duplicates_and_in_a_sensible_total_size() {
        let all = Clip::all();
        let unique: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len());
        let samples: usize = all.iter().map(|c| c.synthesize().len()).sum();
        let seconds = samples as f32 / synth::SAMPLE_RATE as f32;
        assert!((10.0..120.0).contains(&seconds), "{seconds}s of audio");
    }

    #[test]
    fn attenuation_falls_off_smoothly() {
        assert_eq!(attenuation(0.0), 1.0);
        assert!(attenuation(10.0) > attenuation(20.0));
        assert_eq!(attenuation(HEARING_RANGE), 0.0);
        assert_eq!(attenuation(500.0), 0.0);
    }
}
