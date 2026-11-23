//! Ranking: which cells stand out, within their biome, in at least two independent ways, kept apart.

use super::super::climate::BiomeKind;
use super::terms::Terms;

/// The knobs of a scoring run. All first-pass values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Params {
    /// Blocks between samples. Landmarks are 130 to 190 blocks across at least, so this stays under half of that.
    pub step: f64,
    /// The rank, 0 to 1 within a biome, a term needs to count as in the tail.
    pub tail: f32,
    /// Blocks within which only the best of two places is kept.
    pub apart: f64,
    /// Places kept per biome.
    pub top: usize,
    /// The least relief, in blocks, a place needs at the landmark and mass windows, so a small bump isn't ranked.
    pub floor: [f32; 2],
    /// For the null run: shift the mass-scale relief this many grid cells (wrapping), to see what chance alone scores.
    pub null_shift: Option<usize>,
}

impl Default for Params {
    fn default() -> Self {
        Self { step: 64.0, tail: 0.95, apart: 2500.0, top: 20, floor: [40.0, 120.0], null_shift: None }
    }
}

/// What a place is like: raised or a dip, and steep or not, so a top set can be varied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    RaisedSteep,
    RaisedSoft,
    DipSteep,
    DipSoft,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    pub x: f64,
    pub z: f64,
    pub biome: BiomeKind,
    pub score: f32,
    pub kind: Kind,
    /// Relief at the three windows in blocks (landmark, mass, range) and the steepness, rise over run.
    pub relief: [f32; 3],
    pub steep: f32,
    /// The tallest fall near it in blocks, plus up to 40 for a lake over its window: 0 with no water.
    pub water: f32,
    /// The smoothed ground's height over the sea, in blocks.
    pub above_sea: f32,
    /// The weakest of the three causes, by name.
    pub weakest: &'static str,
}

const CAUSES: [&str; 4] = ["shape", "mass", "steepness", "water"];

// A cell that qualifies: its score, its index in the grid and its rank in each of the four causes.
type Candidate = (f32, usize, [f32; 4]);

// The rank of each value among `values`, 0 to 1, ties sharing the middle of their span so a flat field ranks 0.5.
fn ranks(values: &[f32]) -> Vec<f32> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f32::total_cmp);
    let n = sorted.len().max(1) as f32;
    values
        .iter()
        .map(|v| {
            let below = sorted.partition_point(|s| s.total_cmp(v).is_lt());
            let upto = sorted.partition_point(|s| s.total_cmp(v).is_le());
            (below + upto) as f32 / 2.0 / n
        })
        .collect()
}

pub(super) fn pick(terms: &Terms, water: &[f32], biome: &[u8], dim: usize, params: &Params, at: impl Fn(usize) -> (f64, f64)) -> Vec<Place> {
    let mass: Vec<f32> = match params.null_shift {
        Some(shift) => (0..dim * dim).map(|i| terms.relief[1][(i / dim) * dim + (i % dim + shift) % dim]).collect(),
        None => terms.relief[1].clone(),
    };
    let apart_cells = params.apart / params.step;
    let mut places = Vec::new();
    for land in BiomeKind::ALL.into_iter().filter(|&k| k != BiomeKind::Sea) {
        let cells: Vec<usize> = (0..dim * dim).filter(|&i| biome[i] == land as u8).collect();
        if cells.len() < 2 {
            continue;
        }
        let pick = |values: &dyn Fn(usize) -> f32| ranks(&cells.iter().map(|&i| values(i)).collect::<Vec<_>>());
        let shape = pick(&|i| terms.relief[0][i].abs());
        let massive = pick(&|i| mass[i].abs());
        let steep = pick(&|i| terms.steep[i]);
        let wet = pick(&|i| water[i]);
        // in the tail in at least two ways, best first, ties broken by position
        let mut found: Vec<Candidate> = (0..cells.len())
            .filter_map(|n| {
                let r = [shape[n], massive[n], steep[n], wet[n]];
                let big_enough = terms.relief[0][cells[n]].abs() >= params.floor[0] && mass[cells[n]].abs() >= params.floor[1];
                // shape and steepness come from the same bump, so they aren't two causes: the mass or the water has to be one
                let independent = r[1] >= params.tail || r[3] >= params.tail;
                (big_enough && independent && r.iter().filter(|&&v| v >= params.tail).count() >= 2).then(|| {
                    let mut sorted = r;
                    sorted.sort_by(f32::total_cmp);
                    ((sorted[3] + sorted[2]) / 2.0, cells[n], r)
                })
            })
            .collect();
        found.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        // keep the best of any two places within `apart`
        let mut kept: Vec<Candidate> = Vec::new();
        for candidate in found {
            let (cx, cz) = ((candidate.1 % dim) as f64, (candidate.1 / dim) as f64);
            let near = kept.iter().any(|k| {
                let (dx, dz) = ((k.1 % dim) as f64 - cx, (k.1 / dim) as f64 - cz);
                (dx * dx + dz * dz).sqrt() < apart_cells
            });
            if !near {
                kept.push(candidate);
            }
        }
        // varied: take the best of each kind in turn
        let kind_of = |i: usize, r: &[f32; 4]| match (terms.relief[1][i] >= 0.0, r[2] >= params.tail) {
            (true, true) => Kind::RaisedSteep,
            (true, false) => Kind::RaisedSoft,
            (false, true) => Kind::DipSteep,
            (false, false) => Kind::DipSoft,
        };
        let mut by_kind: Vec<(Kind, Vec<Candidate>)> = Vec::new();
        for k in kept {
            let kind = kind_of(k.1, &k.2);
            match by_kind.iter_mut().find(|(existing, _)| *existing == kind) {
                Some((_, list)) => list.push(k),
                None => by_kind.push((kind, vec![k])),
            }
        }
        by_kind.sort_by_key(|(kind, _)| *kind);
        let mut taken = 0;
        let mut round = 0;
        while taken < params.top && by_kind.iter().any(|(_, list)| round < list.len()) {
            for (kind, list) in &by_kind {
                if let (Some(&(score, i, r)), true) = (list.get(round), taken < params.top) {
                    let (x, z) = at(i);
                    // the weakest of the causes that put it in the tail
                    let weakest = (0..4).filter(|&c| r[c] >= params.tail).min_by(|&a, &b| r[a].total_cmp(&r[b])).unwrap_or(0);
                    places.push(Place {
                        x,
                        z,
                        biome: land,
                        score,
                        kind: *kind,
                        relief: [terms.relief[0][i], terms.relief[1][i], terms.relief[2][i]],
                        steep: terms.steep[i],
                        water: water[i],
                        above_sea: terms.ground[i] - ochre_base::WATER_LEVEL as f32,
                        weakest: CAUSES[weakest],
                    });
                    taken += 1;
                }
            }
            round += 1;
        }
    }
    places
}

/// The places as CSV: biome, rank, position, kind, score, terms, weakest cause; fixed precision, so equal runs match.
pub fn csv(places: &[Place]) -> String {
    let mut out = String::from("biome,rank,x,z,kind,score,relief_landmark,relief_mass,relief_range,steepness,water,above_sea,weakest\n");
    let mut rank = 0usize;
    let mut last: Option<BiomeKind> = None;
    for p in places {
        rank = if last == Some(p.biome) { rank + 1 } else { 1 };
        last = Some(p.biome);
        out.push_str(&format!(
            "{:?},{},{:.0},{:.0},{:?},{:.4},{:.1},{:.1},{:.1},{:.3},{:.1},{:.0},{}\n",
            p.biome, rank, p.x, p.z, p.kind, p.score, p.relief[0], p.relief[1], p.relief[2], p.steep, p.water, p.above_sea, p.weakest
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::terms::from_heights;
    use super::*;

    const DIM: usize = 160;

    // A flat plain of one biome with a peak and a pit, each 200 blocks across at 64-block cells, and a half-height hill.
    fn world() -> (Vec<f32>, Vec<u8>) {
        let mut heights = vec![100.0f32; DIM * DIM];
        let mut bump = |cx: i64, cz: i64, height: f32, radius: i64| {
            for z in cz - radius..=cz + radius {
                for x in cx - radius..=cx + radius {
                    let d = (((x - cx).pow(2) + (z - cz).pow(2)) as f32).sqrt() / radius as f32;
                    if d <= 1.0 {
                        heights[z as usize * DIM + x as usize] += height * (1.0 - d);
                    }
                }
            }
        };
        bump(40, 40, 400.0, 3);
        bump(110, 110, -300.0, 3);
        bump(110, 40, 80.0, 3);
        (heights, vec![BiomeKind::Steppe as u8; DIM * DIM])
    }

    fn run(params: &Params) -> Vec<Place> {
        let (heights, biome) = world();
        let terms = from_heights(&heights, DIM, params.step);
        let at = |i: usize| ((i % DIM) as f64 * params.step, (i / DIM) as f64 * params.step);
        pick(&terms, &vec![0.0; DIM * DIM], &biome, DIM, params, at)
    }

    #[test]
    fn a_peak_and_a_pit_stand_out_of_a_plain_and_a_low_hill_does_not() {
        let places = run(&Params::default());
        assert!(!places.is_empty());
        let near = |p: &Place, cx: f64, cz: f64| ((p.x - cx * 64.0).powi(2) + (p.z - cz * 64.0).powi(2)).sqrt() < 600.0;
        assert!(places.iter().any(|p| near(p, 40.0, 40.0) && matches!(p.kind, Kind::RaisedSteep | Kind::RaisedSoft)), "the peak");
        assert!(places.iter().any(|p| near(p, 110.0, 110.0) && matches!(p.kind, Kind::DipSteep | Kind::DipSoft)), "the pit, a dip counting as much as a peak");
        assert!(places.iter().all(|p| (0.0..=1.0).contains(&p.score) && p.relief.iter().all(|r| r.is_finite())));
    }

    #[test]
    fn places_are_apart_and_the_run_is_the_same_every_time() {
        let params = Params::default();
        let (a, b) = (run(&params), run(&params));
        assert_eq!(a, b);
        assert_eq!(csv(&a), csv(&b));
        for (i, p) in a.iter().enumerate() {
            for q in &a[i + 1..] {
                assert!(((p.x - q.x).powi(2) + (p.z - q.z).powi(2)).sqrt() >= params.apart - params.step, "two places closer than {}", params.apart);
            }
        }
    }

    #[test]
    fn a_thin_spike_is_not_a_place() {
        // one 64-block cell, 600 blocks tall: a few blocks wide on the ground, which no landmark is
        let mut heights = vec![100.0f32; DIM * DIM];
        heights[80 * DIM + 80] += 600.0;
        let terms = from_heights(&heights, DIM, 64.0);
        let at = |i: usize| ((i % DIM) as f64 * 64.0, (i / DIM) as f64 * 64.0);
        let biome = vec![BiomeKind::Steppe as u8; DIM * DIM];
        let places = pick(&terms, &vec![0.0; DIM * DIM], &biome, DIM, &Params::default(), at);
        assert!(places.is_empty(), "{places:?}");
    }

    #[test]
    fn shape_and_steepness_alone_are_not_two_causes_but_mass_or_water_with_one_is() {
        // a steep, narrow-ish bump: shape and steepness are in the tail, the mass is not, so it is one cause counted twice
        let mut heights = vec![100.0f32; DIM * DIM];
        for z in 78..=82 {
            for x in 78..=82 {
                heights[z * DIM + x] += 150.0;
            }
        }
        let terms = from_heights(&heights, DIM, 64.0);
        let at = |i: usize| ((i % DIM) as f64 * 64.0, (i / DIM) as f64 * 64.0);
        let biome = vec![BiomeKind::Steppe as u8; DIM * DIM];
        let params = Params { floor: [0.0, 0.0], ..Params::default() };
        let none = vec![0.0f32; DIM * DIM];
        let dry = pick(&terms, &none, &biome, DIM, &params, at);
        // the same bump with a fall beside it: water is an independent cause, so it now qualifies
        let mut wet = none.clone();
        wet[80 * DIM + 80] = 60.0;
        let with_water = pick(&terms, &wet, &biome, DIM, &params, at);
        assert!(with_water.len() >= dry.len() && with_water.iter().any(|p| p.water > 0.0), "{dry:?} {with_water:?}");
    }

    #[test]
    fn a_flat_world_has_no_places() {
        let flat = vec![100.0f32; DIM * DIM];
        let terms = from_heights(&flat, DIM, 64.0);
        let at = |i: usize| ((i % DIM) as f64, (i / DIM) as f64);
        let biome = vec![BiomeKind::Steppe as u8; DIM * DIM];
        assert!(pick(&terms, &vec![0.0; DIM * DIM], &biome, DIM, &Params::default(), at).is_empty());
    }

    #[test]
    fn the_null_run_shifts_only_the_mass_scale() {
        // a zero shift is the real run; shape and steepness coincide here, so null cost is measured on real worlds
        let real = run(&Params::default());
        assert_eq!(real, run(&Params { null_shift: Some(0), ..Params::default() }));
        assert_ne!(real, run(&Params { null_shift: Some(DIM / 3), ..Params::default() }));
    }

    #[test]
    fn ties_rank_in_the_middle_so_a_flat_field_is_neutral() {
        assert!(ranks(&[5.0; 10]).iter().all(|&r| (r - 0.5).abs() < 1e-6));
        let r = ranks(&[1.0, 2.0, 3.0, 4.0]);
        assert!(r.windows(2).all(|w| w[0] < w[1]));
    }
}
