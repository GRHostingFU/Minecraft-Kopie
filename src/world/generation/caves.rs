//! Underground voids: "cheese" caverns (big rooms where a 3D noise field is high), "spaghetti"
//! tunnels (the zero crossings of two noise fields coincide), thin "noodle" tunnels and surface
//! entrances where a 2D mask lets tunnels break through the ground.

use noise::{Fbm, MultiFractal, NoiseFn, OpenSimplex};

pub struct Caves {
    tunnel_a: OpenSimplex,
    tunnel_b: OpenSimplex,
    thickness: OpenSimplex,
    noodle_a: OpenSimplex,
    noodle_b: OpenSimplex,
    cheese: Fbm<OpenSimplex>,
    entrance: OpenSimplex,
}

impl Caves {
    pub fn new(seed: u32) -> Self {
        let s = |n: u32| seed.wrapping_add(100 + n);
        Self {
            tunnel_a: OpenSimplex::new(s(0)),
            tunnel_b: OpenSimplex::new(s(1)),
            thickness: OpenSimplex::new(s(2)),
            noodle_a: OpenSimplex::new(s(3)),
            noodle_b: OpenSimplex::new(s(4)),
            cheese: Fbm::<OpenSimplex>::new(s(5)).set_octaves(3).set_frequency(1.0),
            entrance: OpenSimplex::new(s(6)),
        }
    }

    /// How far below the surface (in blocks) caves must stay. `None` lets them open to the sky.
    pub fn ceiling_margin(&self, x: i32, z: i32) -> i32 {
        // Wide, rare patches where tunnels may break through the surface.
        if self.entrance.get([x as f64 / 34.0, z as f64 / 34.0]) > 0.27 { -1 } else { 3 }
    }

    /// Whether `(x, y, z)` is carved out (the caller decides how close to the surface that may be).
    pub fn is_cave(&self, x: i32, y: i32, z: i32) -> bool {
        let (fx, fy, fz) = (x as f64, y as f64, z as f64);

        // Spaghetti: thickness varies slowly along the tunnel.
        let a = self.tunnel_a.get([fx / 38.0, fy / 26.0, fz / 38.0]);
        let b = self.tunnel_b.get([fx / 38.0, fy / 26.0, fz / 38.0]);
        let thick = 0.0024 + 0.0024 * (0.5 + 0.5 * self.thickness.get([fx / 55.0, fy / 40.0, fz / 55.0]));
        if a * a + b * b < thick {
            return true;
        }

        // Cheese: large irregular rooms, bigger and more common deep down.
        if y < 72 {
            let depth = ((72.0 - fy) / 72.0).clamp(0.0, 1.0);
            let threshold = 0.60 - 0.17 * depth;
            if self.cheese.get([fx / 64.0, fy / 36.0, fz / 64.0]) > threshold {
                return true;
            }
        }

        // Noodles: very thin, winding tubes between y = 8 and y = 100.
        if (8..100).contains(&y) {
            let c = self.noodle_a.get([fx / 22.0, fy / 16.0, fz / 22.0]);
            let d = self.noodle_b.get([fx / 22.0, fy / 16.0, fz / 22.0]);
            if c * c + d * d < 0.0011 {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn carved_fraction(c: &Caves, y0: i32, y1: i32) -> f64 {
        let (mut carved, mut total) = (0u32, 0u32);
        for x in (-200..200).step_by(5) {
            for z in (-200..200).step_by(5) {
                for y in (y0..y1).step_by(3) {
                    carved += c.is_cave(x, y, z) as u32;
                    total += 1;
                }
            }
        }
        carved as f64 / total as f64
    }

    #[test]
    fn caves_exist_but_rock_dominates() {
        let c = Caves::new(5);
        let f = carved_fraction(&c, 10, 60);
        assert!((0.01..0.30).contains(&f), "carved fraction {f}");
    }

    #[test]
    fn deep_layers_are_more_hollow_than_shallow_ones() {
        let c = Caves::new(42);
        let deep = carved_fraction(&c, 5, 30);
        let shallow = carved_fraction(&c, 90, 120);
        assert!(deep > shallow, "deep {deep} shallow {shallow}");
    }

    #[test]
    fn different_seeds_give_different_caves() {
        let (a, b) = (Caves::new(1), Caves::new(2));
        let differs = (0..2000).any(|i| a.is_cave(i, 30, i * 3) != b.is_cave(i, 30, i * 3));
        assert!(differs);
    }

    #[test]
    fn entrances_exist_in_some_places_only() {
        let c = Caves::new(9);
        let open = (-300..300).step_by(7).flat_map(|x| (-300..300).step_by(7).map(move |z| (x, z))).filter(|&(x, z)| c.ceiling_margin(x, z) < 0).count();
        let total = (600 / 7 + 1) * (600 / 7 + 1);
        assert!(open > 0 && open < total / 3, "{open}/{total}");
    }
}
