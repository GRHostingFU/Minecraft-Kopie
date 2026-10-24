//! Procedural sound synthesis. Every sound effect in the game is computed here from noise,
//! oscillators and filters; there are no recorded samples anywhere. All functions are
//! deterministic for a given seed, which keeps them testable.

use std::f32::consts::TAU;

pub const SAMPLE_RATE: u32 = 44_100;
const SR: f32 = SAMPLE_RATE as f32;

/// Small, fast xorshift generator (no external crates).
#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9) | 1)
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in `[0, 1)`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    /// Uniform in `[-1, 1)`.
    pub fn noise(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FilterKind {
    LowPass,
    HighPass,
    BandPass,
}

/// Second-order resonant filter (RBJ cookbook biquad).
#[derive(Clone, Debug)]
pub struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
    kind: FilterKind,
    q: f32,
}

impl Biquad {
    pub fn new(kind: FilterKind, freq: f32, q: f32) -> Self {
        let mut f = Self { b0: 0.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0, z1: 0.0, z2: 0.0, kind, q };
        f.set_freq(freq);
        f
    }

    /// Retunes the filter without resetting its state, so the cutoff can be swept.
    pub fn set_freq(&mut self, freq: f32) {
        let freq = freq.clamp(20.0, SR * 0.45);
        let w = TAU * freq / SR;
        let (sin, cos) = w.sin_cos();
        let alpha = sin / (2.0 * self.q.max(0.1));
        let a0 = 1.0 + alpha;
        let (b0, b1, b2) = match self.kind {
            FilterKind::LowPass => ((1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0),
            FilterKind::HighPass => ((1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0),
            FilterKind::BandPass => (alpha, 0.0, -alpha),
        };
        self.b0 = b0 / a0;
        self.b1 = b1 / a0;
        self.b2 = b2 / a0;
        self.a1 = -2.0 * cos / a0;
        self.a2 = (1.0 - alpha) / a0;
    }

    pub fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// `exp(-t / tau)`.
fn decay(t: f32, tau: f32) -> f32 {
    (-t / tau).exp()
}

/// Linear attack of `attack` seconds followed by an exponential decay.
fn ad(t: f32, attack: f32, tau: f32) -> f32 {
    if t < attack { t / attack } else { decay(t - attack, tau) }
}

/// Renders `secs` of audio by calling `f(t, sample_index)` for every sample.
fn render(secs: f32, mut f: impl FnMut(f32, usize) -> f32) -> Vec<f32> {
    let n = (secs * SR) as usize;
    (0..n).map(|i| f(i as f32 / SR, i)).collect()
}

/// Peak-normalises to `peak` and applies a short fade-out so there are no clicks at the end.
pub fn finish(mut s: Vec<f32>, peak: f32) -> Vec<f32> {
    let max = s.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    if max > 0.0 {
        let k = peak / max;
        s.iter_mut().for_each(|v| *v *= k);
    }
    let fade = (0.004 * SR) as usize;
    let n = s.len();
    for i in 0..fade.min(n) {
        s[n - 1 - i] *= i as f32 / fade as f32;
    }
    s
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Material {
    Grass,
    Dirt,
    Sand,
    Stone,
    Wood,
    Snow,
    Leaves,
}

impl Material {
    pub const ALL: [Material; 7] = [
        Material::Grass,
        Material::Dirt,
        Material::Sand,
        Material::Stone,
        Material::Wood,
        Material::Snow,
        Material::Leaves,
    ];

    /// (noise filter kind, centre frequency, resonance, body decay seconds, thump frequency, noise level)
    fn voice(self) -> (FilterKind, f32, f32, f32, f32, f32) {
        match self {
            Material::Grass => (FilterKind::LowPass, 1400.0, 0.8, 0.050, 0.0, 0.9),
            Material::Dirt => (FilterKind::LowPass, 700.0, 0.9, 0.060, 110.0, 0.8),
            Material::Sand => (FilterKind::HighPass, 2200.0, 0.7, 0.055, 0.0, 0.7),
            Material::Stone => (FilterKind::BandPass, 1900.0, 2.2, 0.040, 190.0, 0.55),
            Material::Wood => (FilterKind::BandPass, 480.0, 4.5, 0.075, 150.0, 0.5),
            Material::Snow => (FilterKind::HighPass, 3200.0, 0.6, 0.090, 0.0, 0.5),
            Material::Leaves => (FilterKind::BandPass, 2600.0, 0.9, 0.060, 0.0, 0.6),
        }
    }
}

/// One impact of `material` lasting `secs`, with `pitch` scaling the filter (0.8..1.25) and
/// `hardness` scaling the click at the start (1.0 is a footstep).
fn impact(material: Material, secs: f32, pitch: f32, gain: f32, rng: &mut Rng) -> Vec<f32> {
    let (kind, freq, q, tau, thump, noise_level) = material.voice();
    let mut filter = Biquad::new(kind, freq * pitch, q);
    let mut crunch = Rng::new(rng.next_u32());
    let granular = matches!(material, Material::Sand | Material::Snow | Material::Leaves);
    let out = render(secs, |t, _| {
        let mut n = rng.noise();
        if granular {
            // Crunch: the noise is chopped into little grains.
            n *= if crunch.unit() < 0.35 { 1.0 } else { 0.1 };
        }
        let body = filter.process(n) * noise_level * ad(t, 0.004, tau);
        let thump_part = if thump > 0.0 { (TAU * thump * pitch * t).sin() * decay(t, 0.035) * 0.6 } else { 0.0 };
        let click = rng.noise() * decay(t, 0.0025) * 0.5;
        body * 2.0 + thump_part + click
    });
    finish(out, 0.6 * gain)
}

pub fn footstep(material: Material, variant: u32) -> Vec<f32> {
    let mut rng = Rng::new(0x57E9 + variant * 7919 + material as u32 * 104_729);
    let pitch = 0.9 + 0.1 * (variant % 4) as f32;
    impact(material, 0.18, pitch, 0.7, &mut rng)
}

/// The short tap while a block is being mined.
pub fn dig(material: Material, variant: u32) -> Vec<f32> {
    let mut rng = Rng::new(0xD16 + variant * 6151 + material as u32 * 3571);
    impact(material, 0.14, 1.1 + 0.05 * (variant % 3) as f32, 0.85, &mut rng)
}

/// The crunch when a block is broken: a few overlapping impacts.
pub fn break_block(material: Material, variant: u32) -> Vec<f32> {
    let mut rng = Rng::new(0xB4E4 + variant * 1237 + material as u32 * 9973);
    let mut out = vec![0.0; (0.34 * SR) as usize];
    for (i, (delay, gain, pitch)) in
        [(0.0, 1.0, 0.85), (0.045, 0.8, 1.0), (0.10, 0.6, 1.15), (0.17, 0.4, 0.95)].into_iter().enumerate()
    {
        let hit = impact(material, 0.16, pitch + 0.03 * i as f32, gain, &mut rng);
        let start = (delay * SR) as usize;
        for (j, v) in hit.iter().enumerate() {
            if let Some(o) = out.get_mut(start + j) {
                *o += v;
            }
        }
    }
    finish(out, 0.8)
}

pub fn place_block(material: Material, variant: u32) -> Vec<f32> {
    let mut rng = Rng::new(0x91A + variant * 2999 + material as u32 * 4421);
    impact(material, 0.12, 0.8, 0.8, &mut rng)
}

/// A soft UI blip: two quick sine steps.
pub fn ui_click() -> Vec<f32> {
    let out = render(0.07, |t, _| {
        let f = if t < 0.025 { 1250.0 } else { 880.0 };
        (TAU * f * t).sin() * ad(t, 0.002, 0.02) * 0.8
    });
    finish(out, 0.35)
}

/// Picking an item up: a quick upward glide.
pub fn pop() -> Vec<f32> {
    let mut phase = 0.0f32;
    let out = render(0.10, |t, _| {
        let f = 500.0 + 1800.0 * (t / 0.1);
        phase += TAU * f / SR;
        phase.sin() * ad(t, 0.003, 0.03)
    });
    finish(out, 0.4)
}

/// Lever / button / plate click: noise tick plus a short resonant tone.
pub fn click(freq: f32, softness: f32) -> Vec<f32> {
    let mut rng = Rng::new(0xC11C + freq as u32);
    let mut hp = Biquad::new(FilterKind::HighPass, 1500.0, 0.8);
    let out = render(0.09, |t, _| {
        let tick = hp.process(rng.noise()) * decay(t, 0.0018) * 1.2;
        let ring = (TAU * freq * t).sin() * decay(t, 0.012) * 0.7;
        (tick + ring) * (1.0 - softness * 0.4)
    });
    finish(out, 0.55 - softness * 0.2)
}

/// Piston: a dull thump with a sliding noise sweep. `out` extends (rising), otherwise falls.
pub fn piston(extend: bool) -> Vec<f32> {
    let mut rng = Rng::new(if extend { 0x9157 } else { 0x9158 });
    let mut lp = Biquad::new(FilterKind::BandPass, 400.0, 1.2);
    let secs = 0.32;
    let out = render(secs, |t, _| {
        let p = t / secs;
        let sweep = if extend { 300.0 + 1500.0 * p } else { 1800.0 - 1500.0 * p };
        lp.set_freq(sweep);
        let slide = lp.process(rng.noise()) * (p * (1.0 - p) * 4.0).min(1.0) * 1.5;
        let impact_at = if extend { 0.75 } else { 0.0 };
        let dt = t - impact_at * secs;
        let thump = if dt >= 0.0 { (TAU * 70.0 * dt).sin() * decay(dt, 0.06) * 1.6 } else { 0.0 };
        slide * 0.6 + thump
    });
    finish(out, 0.75)
}

/// Explosion: a crack, a sweeping low-passed roar and a sub-bass drop, soft-clipped.
pub fn explosion(seed: u32) -> Vec<f32> {
    let mut rng = Rng::new(0xE7 + seed);
    let mut roar = Biquad::new(FilterKind::LowPass, 3000.0, 0.7);
    let mut rumble = Biquad::new(FilterKind::LowPass, 180.0, 0.9);
    let secs = 2.1;
    let mut phase = 0.0f32;
    let out = render(secs, |t, _| {
        roar.set_freq(120.0 + 3600.0 * decay(t, 0.22));
        let n = rng.noise();
        let crack = n * decay(t, 0.012) * 1.4;
        let body = roar.process(n) * ad(t, 0.004, 0.42) * 2.4;
        let sub_f = 28.0 + 70.0 * decay(t, 0.25);
        phase += TAU * sub_f / SR;
        let sub = phase.sin() * ad(t, 0.01, 0.55) * 1.6;
        let crumble = rumble.process(rng.noise()) * (0.5 + 0.5 * (TAU * 9.0 * t).sin()).powi(2) * decay(t, 0.9) * 1.8;
        ((crack + body + sub + crumble) * 0.55).tanh()
    });
    finish(out, 0.95)
}

/// The creeper's fuse: filtered noise whose brightness climbs while the level swells.
pub fn hiss() -> Vec<f32> {
    let mut rng = Rng::new(0x4155);
    let mut bp = Biquad::new(FilterKind::BandPass, 4200.0, 1.4);
    let mut hp = Biquad::new(FilterKind::HighPass, 2500.0, 0.7);
    let secs = 1.5;
    let out = render(secs, |t, _| {
        let p = t / secs;
        bp.set_freq(4200.0 + 2500.0 * p);
        let flutter = 0.8 + 0.2 * (TAU * 38.0 * t).sin();
        let n = rng.noise();
        (bp.process(n) + 0.6 * hp.process(n)) * (0.25 + 0.75 * p.powf(0.8)) * flutter
    });
    finish(out, 0.55)
}

/// Bow shot: a plucked-string twang (Karplus-Strong) over a short whoosh.
pub fn shoot() -> Vec<f32> {
    let mut rng = Rng::new(0xB0F);
    let delay_len = (SR / 196.0) as usize;
    let mut line: Vec<f32> = (0..delay_len).map(|_| rng.noise()).collect();
    let mut idx = 0;
    let mut whoosh = Biquad::new(FilterKind::BandPass, 1800.0, 0.9);
    let out = render(0.45, |t, _| {
        let next = (idx + 1) % delay_len;
        let y = line[idx];
        line[idx] = 0.5 * (line[idx] + line[next]) * 0.996;
        idx = next;
        whoosh.set_freq(1200.0 + 3000.0 * (t / 0.2).min(1.0));
        let air = whoosh.process(rng.noise()) * ad(t, 0.01, 0.06) * 0.7;
        y * ad(t, 0.0, 0.5) * 0.9 + air
    });
    finish(out, 0.6)
}

/// An arrow sticking into a block: a short wooden knock.
pub fn arrow_hit() -> Vec<f32> {
    let mut rng = Rng::new(0xA77);
    impact(Material::Wood, 0.14, 1.3, 0.8, &mut rng)
}

/// Ouch: a falling pitch with some roughness. `low` for the player, higher for mobs.
pub fn hurt(low: bool) -> Vec<f32> {
    let mut rng = Rng::new(if low { 0x4172 } else { 0x4173 });
    let (f0, f1) = if low { (210.0, 120.0) } else { (330.0, 190.0) };
    let mut phase = 0.0f32;
    let mut formant = Biquad::new(FilterKind::BandPass, 700.0, 2.0);
    let out = render(0.28, |t, _| {
        let p = t / 0.28;
        phase += TAU * (f0 + (f1 - f0) * p) / SR;
        // A band-limited sawtooth with some grit.
        let saw = (phase / TAU).fract() * 2.0 - 1.0;
        let rough = saw + 0.15 * rng.noise();
        (formant.process(rough) * 2.0 + rough * 0.3) * ad(t, 0.006, 0.12)
    });
    finish(out, 0.6)
}

pub fn death() -> Vec<f32> {
    let mut v = hurt(false);
    let mut tail = hurt(true);
    tail.iter_mut().for_each(|s| *s *= 0.8);
    v.extend(tail);
    finish(v, 0.65)
}

/// Water splash: a burst of mid-high noise with a few bubbly chirps.
pub fn splash() -> Vec<f32> {
    let mut rng = Rng::new(0x5B1A5);
    let mut bp = Biquad::new(FilterKind::BandPass, 1800.0, 0.7);
    let bubbles: Vec<(f32, f32)> = (0..6).map(|_| (rng.range(0.03, 0.35), rng.range(500.0, 1500.0))).collect();
    let out = render(0.55, |t, _| {
        let burst = bp.process(rng.noise()) * ad(t, 0.006, 0.09) * 1.6;
        let mut b = 0.0;
        for &(start, f) in &bubbles {
            let dt = t - start;
            if dt > 0.0 {
                b += (TAU * (f + 4000.0 * dt) * dt).sin() * decay(dt, 0.025) * 0.35;
            }
        }
        burst + b
    });
    finish(out, 0.6)
}

/// Lava meeting water: steam hiss and a pop.
pub fn fizz() -> Vec<f32> {
    let mut rng = Rng::new(0xF122);
    let mut hp = Biquad::new(FilterKind::HighPass, 3500.0, 0.7);
    let out = render(0.8, |t, _| {
        let hiss = hp.process(rng.noise()) * ad(t, 0.01, 0.25);
        let pop = (TAU * 220.0 * t).sin() * decay(t, 0.03) * 0.7;
        hiss + pop
    });
    finish(out, 0.55)
}

/// A bubble bursting on a lava surface.
pub fn lava_pop(seed: u32) -> Vec<f32> {
    let mut rng = Rng::new(0x1A7A + seed);
    let f0 = rng.range(120.0, 260.0);
    let mut phase = 0.0f32;
    let out = render(0.2, |t, _| {
        phase += TAU * (f0 + 600.0 * t) / SR;
        phase.sin() * ad(t, 0.004, 0.05) + rng.noise() * decay(t, 0.006) * 0.3
    });
    finish(out, 0.5)
}

/// Sparse crackling of fire.
pub fn fire() -> Vec<f32> {
    let mut rng = Rng::new(0xF12E);
    let mut hp = Biquad::new(FilterKind::HighPass, 1800.0, 0.8);
    let mut env = 0.0f32;
    let out = render(0.6, |_, _| {
        if rng.unit() < 0.0016 {
            env = rng.range(0.4, 1.0);
        }
        env *= 0.992;
        hp.process(rng.noise()) * env
    });
    finish(out, 0.4)
}

/// Dry bones knocking together.
pub fn bones() -> Vec<f32> {
    let mut rng = Rng::new(0xB07E5);
    let hits: Vec<(f32, f32, f32)> = (0..5)
        .map(|i| (i as f32 * 0.05 + rng.range(0.0, 0.03), rng.range(1800.0, 3200.0), rng.range(0.4, 1.0)))
        .collect();
    let out = render(0.4, |t, _| {
        let mut v = 0.0;
        for &(start, f, g) in &hits {
            let dt = t - start;
            if dt > 0.0 {
                v += (TAU * f * dt).sin() * decay(dt, 0.008) * g;
            }
        }
        v + rng.noise() * 0.02
    });
    finish(out, 0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(s: &[f32]) -> f32 {
        (s.iter().map(|v| v * v).sum::<f32>() / s.len().max(1) as f32).sqrt()
    }

    fn zero_crossings_per_sample(s: &[f32]) -> f32 {
        s.windows(2).filter(|w| w[0].signum() != w[1].signum()).count() as f32 / s.len() as f32
    }

    fn assert_sane(name: &str, s: &[f32]) {
        assert!(!s.is_empty(), "{name} is empty");
        assert!(s.iter().all(|v| v.is_finite() && v.abs() <= 1.0), "{name} has invalid samples");
        assert!(rms(s) > 0.01, "{name} is silent (rms {})", rms(s));
        assert!(s.last().unwrap().abs() < 0.05, "{name} does not fade out");
    }

    #[test]
    fn every_effect_is_finite_audible_and_bounded() {
        for m in Material::ALL {
            for v in 0..4 {
                assert_sane("step", &footstep(m, v));
            }
            assert_sane("dig", &dig(m, 0));
            assert_sane("break", &break_block(m, 0));
            assert_sane("place", &place_block(m, 0));
        }
        assert_sane("ui_click", &ui_click());
        assert_sane("pop", &pop());
        assert_sane("lever", &click(1800.0, 0.0));
        assert_sane("piston out", &piston(true));
        assert_sane("piston in", &piston(false));
        assert_sane("explosion", &explosion(0));
        assert_sane("hiss", &hiss());
        assert_sane("shoot", &shoot());
        assert_sane("arrow hit", &arrow_hit());
        assert_sane("hurt", &hurt(true));
        assert_sane("mob hurt", &hurt(false));
        assert_sane("death", &death());
        assert_sane("splash", &splash());
        assert_sane("fizz", &fizz());
        assert_sane("lava pop", &lava_pop(1));
        assert_sane("fire", &fire());
        assert_sane("bones", &bones());
    }

    #[test]
    fn synthesis_is_deterministic_and_variants_differ() {
        assert_eq!(footstep(Material::Stone, 1), footstep(Material::Stone, 1));
        assert_ne!(footstep(Material::Stone, 1), footstep(Material::Stone, 2));
        assert_ne!(footstep(Material::Stone, 1), footstep(Material::Grass, 1));
        assert_eq!(explosion(3), explosion(3));
    }

    #[test]
    fn materials_have_a_different_character() {
        // Sand and snow crunch are bright, dirt and grass are dull.
        let bright = zero_crossings_per_sample(&footstep(Material::Sand, 0));
        let dull = zero_crossings_per_sample(&footstep(Material::Dirt, 0));
        assert!(bright > dull * 1.5, "sand {bright} vs dirt {dull}");
    }

    #[test]
    fn the_explosion_is_long_loud_and_bass_heavy() {
        let e = explosion(0);
        let step = footstep(Material::Stone, 0);
        assert!(e.len() as f32 / SR > 1.5);
        assert!(e.len() > step.len() * 5);
        // Mostly low frequencies: few zero crossings compared to a hiss.
        assert!(zero_crossings_per_sample(&e) < zero_crossings_per_sample(&hiss()) / 3.0);
        assert!(rms(&e) > rms(&step) * 0.9);
    }

    #[test]
    fn the_fuse_gets_louder_towards_the_end() {
        let h = hiss();
        let third = h.len() / 3;
        assert!(rms(&h[2 * third..]) > rms(&h[..third]) * 1.5);
    }

    #[test]
    fn filters_pass_and_block_the_right_frequencies() {
        let tone = |f: f32| -> Vec<f32> { (0..4410).map(|i| (TAU * f * i as f32 / SR).sin()).collect() };
        let run = |kind, cutoff, f: f32| {
            let mut b = Biquad::new(kind, cutoff, 0.707);
            let out: Vec<f32> = tone(f).into_iter().map(|x| b.process(x)).collect();
            rms(&out[1000..])
        };
        assert!(run(FilterKind::LowPass, 500.0, 100.0) > 0.6);
        assert!(run(FilterKind::LowPass, 500.0, 5000.0) < 0.05);
        assert!(run(FilterKind::HighPass, 2000.0, 8000.0) > 0.6);
        assert!(run(FilterKind::HighPass, 2000.0, 100.0) < 0.05);
    }

    #[test]
    fn rng_is_uniform_enough() {
        let mut r = Rng::new(5);
        let v: Vec<f32> = (0..10_000).map(|_| r.unit()).collect();
        let mean = v.iter().sum::<f32>() / v.len() as f32;
        assert!((mean - 0.5).abs() < 0.02, "mean {mean}");
        assert!(v.iter().all(|x| (0.0..1.0).contains(x)));
    }
}
