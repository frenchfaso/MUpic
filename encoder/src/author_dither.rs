//! The precomputed, multi-component "improved algorithm 1" described by Joel
//! Yliluoma: https://bisqwit.iki.fi/story/howto/dither/jy/ . This is separate from
//! the encoder's two-colour OKLab adaptation. The article publishes pseudocode
//! for this variant, not its complete candidate-list generator.
//!
//! Candidate multisets contain 1..=8 palette slots, including repeated colours.
//! Their encoded CCIR601 luma range is at most seven times the mean successive
//! palette-luma gap. Means use gamma 2.2; fixed Bayer8 ranks address the sorted
//! component list. Selection uses CIEDE2000, with a conservative branch bound
//! rather than treating that colour difference as an Euclidean metric.

use std::collections::HashMap;
use std::f64::consts::PI;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::atomic::{AtomicUsize, Ordering};

const GAMMA: f64 = 2.2;
const MAX_SLOTS: usize = 8;
const LEAF_SIZE: usize = 12;
const NONE: usize = usize::MAX;
const RGB_WORK_BATCH: usize = 128;
const NEUTRAL: [u8; 5] = [0, 1, 11, 12, 15];
const BAYER: [usize; 64] = [
    0, 48, 12, 60, 3, 51, 15, 63, 32, 16, 44, 28, 35, 19, 47, 31, 8, 56, 4, 52, 11, 59, 7, 55, 40,
    24, 36, 20, 43, 27, 39, 23, 2, 50, 14, 62, 1, 49, 13, 61, 34, 18, 46, 30, 33, 17, 45, 29, 10,
    58, 6, 54, 9, 57, 5, 53, 42, 26, 38, 22, 41, 25, 37, 21,
];

fn default_threads() -> usize {
    #[cfg(target_arch = "wasm32")]
    {
        1
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::available_parallelism().map_or(1, usize::from)
    }
}

#[derive(Clone)]
struct Candidate {
    slots: [u8; MAX_SLOTS],
    len: usize,
    lab: [f64; 3],
    chroma: f64,
    // Actual mean of the 64 displayed hardware cells, in linear-light sRGB.
    mean_linear: [f64; 3],
    neutral: bool,
}

impl Candidate {
    fn colour(&self, rank: usize) -> u8 {
        self.slots[rank * self.len / 64]
    }
}

struct Node {
    lo: [f64; 3],
    hi: [f64; 3],
    max_chroma: f64,
    children: [usize; 2],
    start: usize,
    end: usize,
}

struct SearchTree {
    nodes: Vec<Node>,
    indices: Vec<usize>,
    root: usize,
}

impl SearchTree {
    fn new(candidates: &[Candidate], neutral_only: bool) -> Self {
        let mut tree = Self {
            nodes: Vec::new(),
            indices: candidates
                .iter()
                .enumerate()
                .filter_map(|(i, c)| (!neutral_only || c.neutral).then_some(i))
                .collect(),
            root: NONE,
        };
        if !tree.indices.is_empty() {
            tree.root = tree.build(candidates, 0, tree.indices.len());
        }
        tree
    }

    fn build(&mut self, candidates: &[Candidate], start: usize, end: usize) -> usize {
        let mut lo = [f64::INFINITY; 3];
        let mut hi = [f64::NEG_INFINITY; 3];
        let mut max_chroma: f64 = 0.0;
        for &index in &self.indices[start..end] {
            let c = &candidates[index];
            for axis in 0..3 {
                lo[axis] = lo[axis].min(c.lab[axis]);
                hi[axis] = hi[axis].max(c.lab[axis]);
            }
            max_chroma = max_chroma.max(c.chroma);
        }
        let id = self.nodes.len();
        self.nodes.push(Node {
            lo,
            hi,
            max_chroma,
            children: [NONE; 2],
            start,
            end,
        });
        if end - start > LEAF_SIZE {
            let axis = (0..3)
                .max_by(|&a, &b| (hi[a] - lo[a]).total_cmp(&(hi[b] - lo[b])))
                .unwrap();
            let middle = start + (end - start) / 2;
            self.indices[start..end].select_nth_unstable_by(middle - start, |&a, &b| {
                candidates[a].lab[axis]
                    .total_cmp(&candidates[b].lab[axis])
                    .then(a.cmp(&b))
            });
            let left = self.build(candidates, start, middle);
            let right = self.build(candidates, middle, end);
            self.nodes[id].children = [left, right];
        }
        id
    }

    fn nearest(&self, candidates: &[Candidate], query: [f64; 3]) -> usize {
        self.nearest_seeded(candidates, query, None)
    }

    fn nearest_seeded(
        &self,
        candidates: &[Candidate],
        query: [f64; 3],
        hint: Option<(usize, usize)>,
    ) -> usize {
        assert_ne!(self.root, NONE);
        // Start with a close leaf instead of an arbitrary point in the root.
        // The following exact search still visits every admissible branch.
        let query_chroma = query[1].hypot(query[2]);
        let exact_hint_leaf = hint.and_then(|(candidate, leaf)| {
            self.nodes
                .get(leaf)
                .filter(|n| {
                    n.children[0] == NONE && self.indices[n.start..n.end].contains(&candidate)
                })
                .map(|_| leaf)
        });
        let (mut best, visited_leaf) =
            if let Some((seed, _)) = hint.filter(|_| exact_hint_leaf.is_none()) {
                (
                    (
                        ciede2000_precomputed(
                            query,
                            candidates[seed].lab,
                            query_chroma,
                            candidates[seed].chroma,
                        ),
                        seed,
                    ),
                    NONE,
                )
            } else {
                let mut id = exact_hint_leaf.unwrap_or(self.root);
                while self.nodes[id].children[0] != NONE {
                    let [a, b] = self.nodes[id].children;
                    id = if self.nodes[a].lower_bound(query, query_chroma)
                        <= self.nodes[b].lower_bound(query, query_chroma)
                    {
                        a
                    } else {
                        b
                    };
                }
                let mut best = (f64::INFINITY, NONE);
                for &index in &self.indices[self.nodes[id].start..self.nodes[id].end] {
                    let c = &candidates[index];
                    let cost = ciede2000_precomputed(query, c.lab, query_chroma, c.chroma);
                    if cost < best.0 || (cost == best.0 && index < best.1) {
                        best = (cost, index);
                    }
                }
                (best, id)
            };
        self.search(
            candidates,
            query,
            query_chroma,
            self.root,
            visited_leaf,
            &mut best,
        );
        best.1
    }

    fn search(
        &self,
        candidates: &[Candidate],
        query: [f64; 3],
        query_chroma: f64,
        id: usize,
        visited_leaf: usize,
        best: &mut (f64, usize),
    ) {
        // The initial CPU leaf was already checked using the same exact
        // distances. Avoid evaluating those palette plans a second time.
        if id == visited_leaf {
            return;
        }
        let node = &self.nodes[id];
        // The small tolerance also admits deterministic equal-distance ties.
        if node.lower_bound(query, query_chroma) > best.0 * (1.0 + 1e-12) + 1e-12 {
            return;
        }
        if node.children[0] == NONE {
            for &index in &self.indices[node.start..node.end] {
                let c = &candidates[index];
                let penalty = ciede2000_precomputed(query, c.lab, query_chroma, c.chroma);
                if penalty < best.0 || (penalty == best.0 && index < best.1) {
                    *best = (penalty, index);
                }
            }
        } else {
            let [a, b] = node.children;
            let ba = self.nodes[a].lower_bound(query, query_chroma);
            let bb = self.nodes[b].lower_bound(query, query_chroma);
            let children = if ba <= bb { [a, b] } else { [b, a] };
            for child in children {
                self.search(candidates, query, query_chroma, child, visited_leaf, best);
            }
        }
    }
}

impl Node {
    fn lower_bound(&self, query: [f64; 3], query_chroma: f64) -> f64 {
        let delta = std::array::from_fn::<_, 3, _>(|axis| {
            if query[axis] < self.lo[axis] {
                self.lo[axis] - query[axis]
            } else if query[axis] > self.hi[axis] {
                query[axis] - self.hi[axis]
            } else {
                0.0
            }
        });
        let l50 = ((query[0] + self.lo[0]) * 0.5 - 50.0)
            .abs()
            .max(((query[0] + self.hi[0]) * 0.5 - 50.0).abs());
        let max_sl = 1.0 + 0.015 * l50 * l50 / (20.0 + l50 * l50).sqrt();
        // G is in [0,0.5], hence C' <= 1.5 C and the common a scaling
        // never shrinks the distance between the two original a/b points.
        let max_cp = 0.75 * (query_chroma + self.max_chroma);
        let max_sc = 1.0 + 0.045 * max_cp;
        // |R_T| <= sqrt(3). The smallest eigenvalue of the normalised
        // chroma/hue quadratic is therefore >= 1-sqrt(3)/2. S_C bounds
        // S_H too, because T <= 1.93 and 0.015*1.93 < 0.045.
        let chromatic_factor = 1.0 - 3.0_f64.sqrt() * 0.5;
        (delta[0] / max_sl).powi(2)
            + chromatic_factor * (delta[1].powi(2) + delta[2].powi(2)) / max_sc.powi(2)
    }
}

pub struct AuthorDither {
    palette: [[u8; 3]; 16],
    candidates: Vec<Candidate>,
    tree: SearchTree,
    neutral_tree: SearchTree,
    cache: HashMap<u32, usize>,
    neutral_guard: bool,
    luma_limit: f64,
    luma_range: f64,
    threads: usize,
    max_workers_used: usize,
    planning_batches: usize,
    gpu_seeded_rgb_values: usize,
}

impl AuthorDither {
    pub fn new(palette: &[[u8; 3]; 16], neutral_guard: bool) -> Result<Self, String> {
        let lumas = palette.map(encoded_luma);
        let mut order: Vec<usize> = (0..16).collect();
        order.sort_by(|&a, &b| lumas[a].total_cmp(&lumas[b]).then(a.cmp(&b)));
        let luma_range = lumas[order[15]] - lumas[order[0]];
        // Telescoping sum of the 15 successive gaps, divided by 15.
        let luma_limit = luma_range * 7.0 / 15.0;
        let gamma_palette = palette.map(|p| p.map(|v| (f64::from(v) / 255.0).powf(GAMMA)));
        let linear_palette = palette.map(|p| p.map(srgb_linear));
        let mut candidates = Vec::new();
        // Pure plans retain the hardware colour index as their plan id.
        for colour in 0..16 {
            let mut slots = [0; MAX_SLOTS];
            slots[0] = colour as u8;
            candidates.push(make_candidate(slots, 1, &gamma_palette, &linear_palette));
        }
        for len in 2..=MAX_SLOTS {
            let mut slots = [0; MAX_SLOTS];
            enumerate(
                &order,
                &lumas,
                luma_limit,
                len,
                0,
                0,
                &mut slots,
                &mut candidates,
                &gamma_palette,
                &linear_palette,
            );
        }
        if candidates.is_empty()
            || candidates
                .iter()
                .any(|c| c.lab.iter().any(|x| !x.is_finite()))
        {
            return Err("Author dither generated invalid candidate coordinates".into());
        }
        let tree = SearchTree::new(&candidates, false);
        let neutral_tree = SearchTree::new(&candidates, true);
        Ok(Self {
            palette: *palette,
            candidates,
            tree,
            neutral_tree,
            cache: HashMap::new(),
            neutral_guard,
            luma_limit,
            luma_range,
            threads: default_threads(),
            max_workers_used: 0,
            planning_batches: 0,
            gpu_seeded_rgb_values: 0,
        })
    }

    pub fn set_threads(&mut self, threads: usize) {
        assert!(threads > 0);
        // Browser parallelism is supplied by separate Web Workers. Spawning a
        // Rust OS thread from wasm32-unknown-unknown would panic, even with N=1.
        self.threads = if cfg!(target_arch = "wasm32") {
            1
        } else {
            threads
        };
    }

    pub fn cached_rgb_count(&self) -> usize {
        self.cache.len()
    }

    /// Discard only memoized lookups; fixed candidates, palette and positional
    /// pattern remain identical. Streaming callers can bound cache memory.
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    // Scalar API is retained for callers/tests; the CLI preplans every distinct
    // source RGB once before converting frames.
    #[allow(dead_code)]
    pub fn plan(&mut self, rgb: [u8; 3]) -> usize {
        // This also prevents a floating-point inverse-gamma round trip from
        // replacing an exact hardware colour with a near-equivalent mixture.
        if let Some(index) = self.palette.iter().position(|&p| p == rgb) {
            return index;
        }
        let key = rgb_key(rgb);
        if let Some(&id) = self.cache.get(&key) {
            return id;
        }
        let id = self.uncached_plan(rgb);
        self.cache.insert(key, id);
        id
    }

    fn uncached_plan(&self, rgb: [u8; 3]) -> usize {
        self.uncached_plan_seeded(rgb, None)
    }

    fn uncached_plan_seeded(&self, rgb: [u8; 3], hint: Option<u32>) -> usize {
        let target = comparison_lab(rgb.map(|v| f64::from(v) / 255.0));
        let neutral =
            self.neutral_guard && rgb.iter().max().unwrap() - rgb.iter().min().unwrap() <= 8;
        let tree = if neutral {
            &self.neutral_tree
        } else {
            &self.tree
        };
        // Packed GPU hints contain a 17-bit candidate and 15-bit global leaf.
        // The CPU verifies membership and evaluates that complete leaf before
        // excluding already checked plans from the remaining exact search.
        let hint = hint
            .map(|h| {
                let candidate = (h & 0x1ffff) as usize;
                let global_leaf = (h >> 17) as usize;
                let leaf = if neutral {
                    global_leaf
                        .checked_sub(self.tree.nodes.len())
                        .unwrap_or(NONE)
                } else {
                    global_leaf
                };
                (candidate, leaf)
            })
            .filter(|&(candidate, _)| {
                candidate < self.candidates.len()
                    && (!neutral || self.candidates[candidate].neutral)
            });
        tree.nearest_seeded(&self.candidates, target, hint)
    }

    pub fn missing_rgb_keys(&self, pixels: &[u8]) -> Vec<u32> {
        // Distinct full 8-bit RGB values are cached; the article's optional
        // six-bit shortcut is deliberately not used here. The 2 MiB bitmap
        // bounds unique collection memory independently of clip duration.
        assert!(pixels.len().is_multiple_of(3));
        let mut seen = vec![0u64; 1 << 18];
        let mut missing = Vec::new();
        for p in pixels.chunks_exact(3) {
            let key = rgb_key([p[0], p[1], p[2]]);
            let word = key as usize / 64;
            let bit = 1u64 << (key & 63);
            if seen[word] & bit == 0 {
                seen[word] |= bit;
                if !self.cache.contains_key(&key) {
                    missing.push(key);
                }
            }
        }
        missing.sort_unstable();
        missing
    }

    pub fn preplan_rgb(&mut self, pixels: &[u8]) {
        let missing = self.missing_rgb_keys(pixels);
        self.preplan_keys(&missing, None);
    }

    /// GPU results are candidate hints, never final colour decisions. An exact
    /// f64 CPU search refines every uncached RGB, retaining deterministic ties.
    pub fn preplan_rgb_seeded(&mut self, pixels: &[u8], seeds: &[u32]) -> Result<(), String> {
        if pixels.len() % 3 != 0 || seeds.len() != pixels.len() / 3 {
            return Err("GPU seed count must match RGB pixel count".into());
        }
        let mut hints = HashMap::new();
        for (p, &seed) in pixels.chunks_exact(3).zip(seeds) {
            let key = rgb_key([p[0], p[1], p[2]]);
            if !self.cache.contains_key(&key) {
                hints.entry(key).or_insert(seed);
            }
        }
        let mut keys: Vec<_> = hints.keys().copied().collect();
        keys.sort_unstable();
        let seeds: Vec<_> = keys.iter().map(|k| hints[k]).collect();
        self.preplan_keys(&keys, Some(&seeds));
        Ok(())
    }

    pub fn preplan_keys_seeded(&mut self, keys: &[u32], seeds: &[u32]) -> Result<(), String> {
        if keys.len() != seeds.len() || keys.iter().any(|&k| k > 0xffffff) {
            return Err("Invalid GPU RGB key or seed count".into());
        }
        self.preplan_keys(keys, Some(seeds));
        Ok(())
    }

    fn preplan_keys(&mut self, missing: &[u32], seeds: Option<&[u32]>) {
        if missing.is_empty() {
            return;
        }
        self.cache.reserve(missing.len());
        if seeds.is_some() {
            self.gpu_seeded_rgb_values += missing.len();
        }
        let workers = self.threads.min(missing.len().div_ceil(RGB_WORK_BATCH));
        self.max_workers_used = self.max_workers_used.max(workers);
        self.planning_batches += 1;
        #[cfg(not(target_arch = "wasm32"))]
        let next = AtomicUsize::new(0);
        // A single scoped pool drains the whole clip. Dynamic batches balance
        // unequal exact CIEDE2000 search costs without changing tie-breaking.
        let calculate = |begin: usize, end: usize| {
            missing[begin..end]
                .iter()
                .enumerate()
                .map(|(i, &key)| {
                    let rgb = key_rgb(key);
                    let id = self
                        .palette
                        .iter()
                        .position(|&p| p == rgb)
                        .unwrap_or_else(|| {
                            self.uncached_plan_seeded(rgb, seeds.map(|s| s[begin + i]))
                        });
                    (key, id)
                })
                .collect::<Vec<_>>()
        };
        // This path is essential for portable WASM and also avoids thread
        // setup on short native batches. Browser workers own separate caches.
        if workers == 1 {
            let calculated = calculate(0, missing.len());
            self.cache.extend(calculated);
            return;
        }
        #[cfg(not(target_arch = "wasm32"))]
        let calculated: Vec<Vec<(u32, usize)>> = std::thread::scope(|scope| {
            let this = &*self;
            let tasks: Vec<_> = (0..workers)
                .map(|_| {
                    let missing = &missing;
                    let next = &next;
                    scope.spawn(move || {
                        let mut result = Vec::new();
                        loop {
                            let begin = next.fetch_add(RGB_WORK_BATCH, Ordering::Relaxed);
                            if begin >= missing.len() {
                                break;
                            }
                            let end = (begin + RGB_WORK_BATCH).min(missing.len());
                            for (offset, &key) in missing[begin..end].iter().enumerate() {
                                let rgb = key_rgb(key);
                                let id =
                                    this.palette.iter().position(|&p| p == rgb).unwrap_or_else(
                                        || {
                                            this.uncached_plan_seeded(
                                                rgb,
                                                seeds.map(|s| s[begin + offset]),
                                            )
                                        },
                                    );
                                result.push((key, id));
                            }
                        }
                        result
                    })
                })
                .collect();
            tasks.into_iter().map(|t| t.join().unwrap()).collect()
        });
        #[cfg(target_arch = "wasm32")]
        let calculated = vec![calculate(0, missing.len())];
        for batch in calculated {
            self.cache.extend(batch);
        }
    }

    /// Shared native/browser WGSL ABI: header[8], nodes[12], candidate Lab[4],
    /// then leaf indices. Float values are IEEE754 f32 bits in a u32 buffer.
    pub fn gpu_search_data(&self) -> Vec<u32> {
        let node_count = self.tree.nodes.len() + self.neutral_tree.nodes.len();
        // The current C64 palette fits this compact hint ABI. Other palettes
        // may admit more multisets; let GPU callers retain their CPU path.
        if node_count >= 32768 || self.candidates.len() >= 131072 {
            return Vec::new();
        }
        let candidate_offset = 8 + node_count * 12;
        let index_offset = candidate_offset + self.candidates.len() * 4;
        let mut data = vec![
            node_count as u32,
            self.candidates.len() as u32,
            self.tree.root as u32,
            (self.tree.nodes.len() + self.neutral_tree.root) as u32,
            index_offset as u32,
            candidate_offset as u32,
            8,
            u32::from(self.neutral_guard),
        ];
        for (tree, node_base, index_base) in [
            (&self.tree, 0, 0),
            (
                &self.neutral_tree,
                self.tree.nodes.len(),
                self.tree.indices.len(),
            ),
        ] {
            for node in &tree.nodes {
                data.extend(node.lo.map(|v| (v as f32).to_bits()));
                data.push((index_base + node.start) as u32);
                data.extend(node.hi.map(|v| (v as f32).to_bits()));
                data.push((index_base + node.end) as u32);
                data.extend(node.children.map(|c| {
                    if c == NONE {
                        u32::MAX
                    } else {
                        (node_base + c) as u32
                    }
                }));
                data.extend([(node.max_chroma as f32).to_bits(), 0]);
            }
        }
        for c in &self.candidates {
            data.extend(c.lab.map(|v| (v as f32).to_bits()));
            data.push(u32::from(c.neutral));
        }
        data.extend(
            self.tree
                .indices
                .iter()
                .chain(&self.neutral_tree.indices)
                .map(|&v| v as u32),
        );
        data
    }

    pub fn frame_plans(&mut self, frame: &[u8]) -> Vec<usize> {
        let cached: Option<Vec<usize>> = frame
            .chunks_exact(3)
            .map(|p| self.cache.get(&rgb_key([p[0], p[1], p[2]])).copied())
            .collect();
        if let Some(plans) = cached {
            return plans;
        }
        self.preplan_rgb(frame);
        frame
            .chunks_exact(3)
            .map(|p| self.cache[&rgb_key([p[0], p[1], p[2]])])
            .collect()
    }

    pub fn colour(&self, plan: usize, x: usize, y: usize) -> u8 {
        self.candidates[plan].colour(BAYER[(x & 7) + ((y & 7) << 3)])
    }

    pub fn mean_linear(&self, plan: usize) -> [f64; 3] {
        self.candidates[plan].mean_linear
    }

    pub fn is_mixed(&self, plan: usize) -> bool {
        let c = &self.candidates[plan];
        c.slots[..c.len].iter().any(|&slot| slot != c.slots[0])
    }

    pub fn metadata(&self) -> String {
        let mut sizes = [0usize; MAX_SLOTS];
        for candidate in &self.candidates {
            sizes[candidate.len - 1] += 1;
        }
        format!(
            concat!(
                "{{\"algorithm\":\"Reconstruction of Yliluoma improved algorithm 1 from published pseudocode\",",
                "\"source\":\"https://bisqwit.iki.fi/story/howto/dither/jy/\",",
                "\"gamma\":2.2,\"matrix\":\"fixed Bayer8x8 at physical canvas coordinates\",",
                "\"max_component_slots\":8,\"candidate_generation\":\"all nondecreasing palette multisets of 1..8 slots, repeated colours allowed\",",
                "\"plans\":{},\"plans_by_slot_count\":{:?},\"cached_rgb_values\":{},",
                "\"source_rgb_bits_per_channel\":8,\"neutral_guard\":{},",
                "\"neutral_rule\":\"source max(RGB)-min(RGB)<=8; all components restricted to hardware indices0,1,11,12,15\",",
                "\"requested_adaptations\":\"neutral guard added at user request; exact palette input retains its pure hardware colour\",",
                "\"mixing\":\"gamma2.2 arithmetic mean then inverse gamma\",",
                "\"metric\":\"CIEDE2000 squared; kL=kC=kH=1\",",
                "\"lab_conversion\":\"article C-illuminant RGB matrix and encoded-RGB input; corrected C=sqrt(a*a+b*b)\",",
                "\"luma\":\"encoded CCIR601, 299R+587G+114B\",",
                "\"luma_range_limit\":{},\"palette_luma_range\":{},",
                "\"luma_rule\":\"7*(max_luma-min_luma)/15, seven times the mean successive gap;46.6667% for16colours rather than treating article rounded45% as a second filter\",",
                "\"search\":\"exact CIEDE2000 with conservative lightness/chroma rectangle bounds; full8-bit RGB cache; global unique RGB preplanning with a scoped dynamic worker pool\",",
                "\"threads_requested\":{},\"max_workers_used\":{},\"work_batch_rgb_values\":{},\"planning_batches\":{},",
                "\"gpu_seeded_rgb_values\":{},\"gpu_role\":\"optional approximate CIEDE2000-bound tree walk hints; exact f64 CPU refinement remains authoritative\",",
                "\"statistics_mean\":\"actual64-cell output mean in linear-light sRGB; differs from ideal slot proportions for lengths3,5,6,7\",",
                "\"stability\":\"same RGB and canvas position gives same hardware index; no frame index or neighbour error propagation\"}}"
            ),
            self.candidates.len(),
            sizes,
            self.cache.len(),
            self.neutral_guard,
            self.luma_limit,
            self.luma_range,
            self.threads,
            self.max_workers_used,
            RGB_WORK_BATCH,
            self.planning_batches,
            self.gpu_seeded_rgb_values,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn enumerate(
    order: &[usize],
    lumas: &[f64; 16],
    limit: f64,
    len: usize,
    depth: usize,
    begin: usize,
    slots: &mut [u8; MAX_SLOTS],
    candidates: &mut Vec<Candidate>,
    gamma_palette: &[[f64; 3]; 16],
    linear_palette: &[[f64; 3]; 16],
) {
    if depth == len {
        candidates.push(make_candidate(*slots, len, gamma_palette, linear_palette));
        return;
    }
    for at in begin..order.len() {
        let colour = order[at];
        if depth > 0 && lumas[colour] - lumas[usize::from(slots[0])] > limit {
            break;
        }
        slots[depth] = colour as u8;
        enumerate(
            order,
            lumas,
            limit,
            len,
            depth + 1,
            at,
            slots,
            candidates,
            gamma_palette,
            linear_palette,
        );
    }
}

fn make_candidate(
    slots: [u8; MAX_SLOTS],
    len: usize,
    gamma_palette: &[[f64; 3]; 16],
    linear_palette: &[[f64; 3]; 16],
) -> Candidate {
    let gamma_mean: [f64; 3] = std::array::from_fn(|axis| {
        slots[..len]
            .iter()
            .map(|&slot| gamma_palette[usize::from(slot)][axis])
            .sum::<f64>()
            / len as f64
    });
    let mixed_rgb = gamma_mean.map(|v| v.powf(1.0 / GAMMA));
    let lab = comparison_lab(mixed_rgb);
    let mean_linear = std::array::from_fn(|axis| {
        (0..64)
            .map(|rank| linear_palette[usize::from(slots[rank * len / 64])][axis])
            .sum::<f64>()
            / 64.0
    });
    Candidate {
        slots,
        len,
        lab,
        chroma: lab[1].hypot(lab[2]),
        mean_linear,
        neutral: slots[..len].iter().all(|slot| NEUTRAL.contains(slot)),
    }
}

fn rgb_key([r, g, b]: [u8; 3]) -> u32 {
    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

fn key_rgb(key: u32) -> [u8; 3] {
    [(key >> 16) as u8, (key >> 8) as u8, key as u8]
}

fn encoded_luma([r, g, b]: [u8; 3]) -> f64 {
    299.0 * f64::from(r) + 587.0 * f64::from(g) + 114.0 * f64::from(b)
}

fn srgb_linear(v: u8) -> f64 {
    let v = f64::from(v) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

// Keep the article's comparison-space conversion independently of the
// physically linear sRGB mean used by the encoder's existing quality statistics.
// This is not a claim that this historic matrix is a modern D65 sRGB transform.
fn comparison_lab([r, g, b]: [f64; 3]) -> [f64; 3] {
    let xyz = [
        (0.488718 * r + 0.310680 * g + 0.200602 * b) / (0.488718 + 0.176204),
        (0.176204 * r + 0.812985 * g + 0.0108109 * b) / (0.310680 + 0.812985 + 0.0102048),
        (0.0102048 * g + 0.989795 * b) / (0.200602 + 0.0108109 + 0.989795),
    ];
    let f = xyz.map(|v| {
        if v > (6.0_f64 / 29.0).powi(3) {
            v.cbrt()
        } else {
            (29.0_f64 / 6.0).powi(2) * v / 3.0 + 4.0 / 29.0
        }
    });
    [
        116.0 * f[1] - 16.0,
        500.0 * (f[0] - f[1]),
        200.0 * (f[1] - f[2]),
    ]
}

/// Correct CIEDE2000 squared, verified against Sharma/Wu/Dalal's supplementary
/// Lab pairs: https://hajim.rochester.edu/ece/sites/gsharma/ciede2000/ .
fn ciede2000_squared(first: [f64; 3], second: [f64; 3]) -> f64 {
    ciede2000_precomputed(
        first,
        second,
        first[1].hypot(first[2]),
        second[1].hypot(second[2]),
    )
}

// Reusing the already computed query/candidate chroma removes two hypot calls
// per examined plan while keeping every f64 operation and tie exactly intact.
fn ciede2000_precomputed(
    first: [f64; 3],
    second: [f64; 3],
    first_chroma: f64,
    second_chroma: f64,
) -> f64 {
    let [l1, a1, b1] = first;
    let [l2, a2, b2] = second;
    let cmean = (first_chroma + second_chroma) * 0.5;
    let c7 = cmean.powi(7);
    let g = 0.5 * (1.0 - (c7 / (c7 + 25.0_f64.powi(7))).sqrt());
    let ap1 = a1 * (1.0 + g);
    let ap2 = a2 * (1.0 + g);
    let c1 = ap1.hypot(b1);
    let c2 = ap2.hypot(b2);
    let angle = |a: f64, b: f64| b.atan2(a).rem_euclid(2.0 * PI);
    let h1 = if c1 == 0.0 { 0.0 } else { angle(ap1, b1) };
    let h2 = if c2 == 0.0 { 0.0 } else { angle(ap2, b2) };
    let dh = if c1 * c2 == 0.0 {
        0.0
    } else if h2 - h1 > PI {
        h2 - h1 - 2.0 * PI
    } else if h2 - h1 < -PI {
        h2 - h1 + 2.0 * PI
    } else {
        h2 - h1
    };
    let hmean = if c1 * c2 == 0.0 {
        h1 + h2
    } else if (h1 - h2).abs() <= PI {
        (h1 + h2) * 0.5
    } else if h1 + h2 < 2.0 * PI {
        (h1 + h2 + 2.0 * PI) * 0.5
    } else {
        (h1 + h2 - 2.0 * PI) * 0.5
    };
    let cpmean = (c1 + c2) * 0.5;
    let t = 1.0 - 0.17 * (hmean - PI / 6.0).cos()
        + 0.24 * (2.0 * hmean).cos()
        + 0.32 * (3.0 * hmean + PI / 30.0).cos()
        - 0.20 * (4.0 * hmean - 63.0_f64.to_radians()).cos();
    let l50sq = ((l1 + l2) * 0.5 - 50.0).powi(2);
    let sl = 1.0 + 0.015 * l50sq / (20.0 + l50sq).sqrt();
    let sc = 1.0 + 0.045 * cpmean;
    let sh = 1.0 + 0.015 * cpmean * t;
    let rc7 = cpmean.powi(7);
    let rc = 2.0 * (rc7 / (rc7 + 25.0_f64.powi(7))).sqrt();
    let theta = 30.0_f64.to_radians() * (-((hmean.to_degrees() - 275.0) / 25.0).powi(2)).exp();
    let rt = -(2.0 * theta).sin() * rc;
    let dl = (l2 - l1) / sl;
    let dc = (c2 - c1) / sc;
    let dh = 2.0 * (c1 * c2).sqrt() * (dh * 0.5).sin() / sh;
    (dl * dl + dc * dc + dh * dh + rt * dc * dh).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PALETTE: [[u8; 3]; 16] = [
        [0, 0, 0],
        [247, 247, 247],
        [141, 47, 52],
        [106, 212, 205],
        [152, 53, 164],
        [76, 180, 66],
        [44, 41, 177],
        [239, 239, 93],
        [152, 78, 32],
        [91, 56, 0],
        [209, 103, 109],
        [74, 74, 74],
        [123, 123, 123],
        [159, 239, 147],
        [109, 106, 239],
        [178, 178, 178],
    ];

    #[test]
    fn ciede2000_sharma_supplementary_pairs() {
        // Published numerical test data, including hue discontinuities and
        // zero-chroma cases. Expected Delta-E is rounded to four decimals.
        let rows: [[f64; 7]; 34] = [
            [50., 2.6772, -79.7751, 50., 0., -82.7485, 2.0425],
            [50., 3.1571, -77.2803, 50., 0., -82.7485, 2.8615],
            [50., 2.8361, -74.0200, 50., 0., -82.7485, 3.4412],
            [50., -1.3802, -84.2814, 50., 0., -82.7485, 1.],
            [50., -1.1848, -84.8006, 50., 0., -82.7485, 1.],
            [50., -0.9009, -85.5211, 50., 0., -82.7485, 1.],
            [50., 0., 0., 50., -1., 2., 2.3669],
            [50., -1., 2., 50., 0., 0., 2.3669],
            [50., 2.49, -0.001, 50., -2.49, 0.0009, 7.1792],
            [50., 2.49, -0.001, 50., -2.49, 0.001, 7.1792],
            [50., 2.49, -0.001, 50., -2.49, 0.0011, 7.2195],
            [50., 2.49, -0.001, 50., -2.49, 0.0012, 7.2195],
            [50., -0.001, 2.49, 50., 0.0009, -2.49, 4.8045],
            [50., -0.001, 2.49, 50., 0.001, -2.49, 4.8045],
            [50., -0.001, 2.49, 50., 0.0011, -2.49, 4.7461],
            [50., 2.5, 0., 50., 0., -2.5, 4.3065],
            [50., 2.5, 0., 73., 25., -18., 27.1492],
            [50., 2.5, 0., 61., -5., 29., 22.8977],
            [50., 2.5, 0., 56., -27., -3., 31.903],
            [50., 2.5, 0., 58., 24., 15., 19.4535],
            [50., 2.5, 0., 50., 3.1736, 0.5854, 1.],
            [50., 2.5, 0., 50., 3.2972, 0., 1.],
            [50., 2.5, 0., 50., 1.8634, 0.5757, 1.],
            [50., 2.5, 0., 50., 3.2592, 0.335, 1.],
            [
                60.2574, -34.0099, 36.2677, 60.4626, -34.1751, 39.4387, 1.2644,
            ],
            [
                63.0109, -31.0961, -5.8663, 62.8187, -29.7946, -4.0864, 1.263,
            ],
            [61.2901, 3.7196, -5.3901, 61.4292, 2.248, -4.962, 1.8731],
            [35.0831, -44.1164, 3.7933, 35.0232, -40.0716, 1.5901, 1.8645],
            [22.7233, 20.0904, -46.694, 23.0331, 14.973, -42.5619, 2.0373],
            [36.4612, 47.858, 18.3852, 36.2715, 50.5065, 21.2231, 1.4146],
            [90.8027, -2.0831, 1.441, 91.1528, -1.6435, 0.0447, 1.4441],
            [90.9257, -0.5406, -0.9208, 88.6381, -0.8985, -0.7239, 1.5381],
            [6.7747, -0.2908, -2.4247, 5.8714, -0.0985, -2.2286, 0.6377],
            [2.0776, 0.0795, -1.135, 0.9033, -0.0636, -0.5514, 0.9082],
        ];
        for (i, row) in rows.into_iter().enumerate() {
            let a = [row[0], row[1], row[2]];
            let b = [row[3], row[4], row[5]];
            let got = ciede2000_squared(a, b).sqrt();
            assert!(
                (got - row[6]).abs() < 5e-5,
                "pair{}: {} != {}",
                i + 1,
                got,
                row[6]
            );
            assert!((ciede2000_squared(a, b) - ciede2000_squared(b, a)).abs() < 1e-10);
        }
    }

    #[test]
    fn gamma_mean_and_bayer_slot_counts_are_explicit() {
        let mut palette = PALETTE;
        palette[1] = [255; 3];
        let gp = palette.map(|p| p.map(|v| (f64::from(v) / 255.).powf(GAMMA)));
        let lp = palette.map(|p| p.map(srgb_linear));
        let c = make_candidate([0, 1, 1, 0, 0, 0, 0, 0], 3, &gp, &lp);
        let rgb = (2.0_f64 / 3.0).powf(1. / GAMMA);
        assert_eq!(c.lab, comparison_lab([rgb; 3]));
        let count = (0..64).filter(|&rank| c.colour(rank) == 1).count();
        assert_eq!(count, 42);
        assert_eq!(c.mean_linear, [42. / 64.; 3]);
        let mut ranks = BAYER;
        ranks.sort_unstable();
        assert_eq!(ranks, std::array::from_fn(|i| i));
    }

    #[test]
    fn palette_identity_guard_and_pixel_locality() {
        let mut d = AuthorDither::new(&PALETTE, true).unwrap();
        assert_eq!(d.candidates.len(), 99384);
        for (index, &rgb) in PALETTE.iter().enumerate() {
            let plan = d.plan(rgb);
            assert!(!d.is_mixed(plan));
            for y in 0..8 {
                for x in 0..8 {
                    assert_eq!(d.colour(plan, x, y), index as u8);
                }
            }
        }
        for rgb in [[80, 80, 80], [124, 129, 124], [40, 47, 43], [180, 180, 180]] {
            let p = d.plan(rgb);
            assert!(d.candidates[p].neutral);
            for y in 0..8 {
                for x in 0..8 {
                    assert!(NEUTRAL.contains(&d.colour(p, x, y)));
                }
            }
        }
        let rgb = [131, 79, 36];
        let p = d.plan(rgb);
        let before: Vec<_> = (0..64).map(|i| d.colour(p, i % 8, i / 8)).collect();
        d.plan([132, 79, 36]);
        d.plan([44, 143, 123]);
        assert_eq!(d.plan(rgb), p);
        assert_eq!(
            before,
            (0..64)
                .map(|i| d.colour(p, i % 8, i / 8))
                .collect::<Vec<_>>()
        );
        assert_eq!(d.colour(p, 1, 2), d.colour(p, 9, 10));
    }

    #[test]
    fn candidate_pruning_and_exact_search_match_full_scan() {
        let d = AuthorDither::new(&PALETTE, false).unwrap();
        for c in &d.candidates {
            let lo = encoded_luma(PALETTE[usize::from(c.slots[0])]);
            let hi = encoded_luma(PALETTE[usize::from(c.slots[c.len - 1])]);
            assert!(hi - lo <= d.luma_limit);
        }
        let mut seed = 123456789u32;
        for _ in 0..50 {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let rgb = key_rgb(seed & 0xffffff);
            let query = comparison_lab(rgb.map(|v| f64::from(v) / 255.));
            let got = d.tree.nearest(&d.candidates, query);
            let expected = (0..d.candidates.len())
                .min_by(|&a, &b| {
                    ciede2000_squared(query, d.candidates[a].lab)
                        .total_cmp(&ciede2000_squared(query, d.candidates[b].lab))
                        .then(a.cmp(&b))
                })
                .unwrap();
            assert_eq!(got, expected, "RGB{rgb:?}");
        }
    }

    #[test]
    fn rectangle_bound_never_exceeds_actual_colour_difference() {
        let mut seed = 777u32;
        for _ in 0..4000 {
            let mut next = || {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                f64::from(seed >> 8) / 16777215.
            };
            let query = [next() * 100., next() * 200. - 100., next() * 200. - 100.];
            let point = [next() * 100., next() * 200. - 100., next() * 200. - 100.];
            let node = Node {
                lo: point,
                hi: point,
                max_chroma: point[1].hypot(point[2]),
                children: [NONE; 2],
                start: 0,
                end: 0,
            };
            let bound = node.lower_bound(query, query[1].hypot(query[2]));
            assert!(
                bound <= ciede2000_squared(query, point) + 1e-9,
                "{query:?},{point:?}"
            );
        }
    }

    #[test]
    fn frame_cache_is_full_rgb_and_order_independent() {
        let mut d = AuthorDither::new(&PALETTE, true).unwrap();
        let source = [131, 79, 36, 132, 79, 36, 80, 80, 80, 131, 79, 36];
        let first = d.frame_plans(&source);
        let rev = [80, 80, 80, 132, 79, 36, 131, 79, 36];
        let second = d.frame_plans(&rev);
        assert_eq!([first[2], first[1], first[0]], second[..]);
        assert_eq!(first[0], first[3]);
        assert_eq!(d.cache.len(), 3);
        assert!(d.metadata().contains("\"source_rgb_bits_per_channel\":8"));
    }

    #[test]
    fn global_dynamic_workers_preserve_exact_plans_and_patterns() {
        let mut source = Vec::new();
        for i in 0..513u32 {
            source.extend(key_rgb(i.wrapping_mul(7919) & 0xffffff));
        }
        source.extend([80, 80, 80, 132, 79, 36, 131, 79, 36]);
        source.extend(source.clone());
        let mut reference = AuthorDither::new(&PALETTE, true).unwrap();
        reference.set_threads(1);
        reference.preplan_rgb(&source);
        let expected = reference.frame_plans(&source);
        for workers in [4, 8] {
            let mut parallel = AuthorDither::new(&PALETTE, true).unwrap();
            parallel.set_threads(workers);
            parallel.preplan_rgb(&source);
            let got = parallel.frame_plans(&source);
            assert_eq!(got, expected);
            assert_eq!(parallel.cache.len(), reference.cache.len());
            assert_eq!(parallel.planning_batches, 1);
            assert!(parallel.max_workers_used > 1);
            for (i, &plan) in got.iter().enumerate() {
                assert_eq!(
                    parallel.colour(plan, i % 320, i / 320),
                    reference.colour(expected[i], i % 320, i / 320)
                );
                assert_eq!(
                    parallel.mean_linear(plan),
                    reference.mean_linear(expected[i])
                );
            }
            let rev: Vec<_> = source.chunks_exact(3).rev().flatten().copied().collect();
            parallel.frame_plans(&rev);
            assert_eq!(parallel.planning_batches, 1);
        }
    }

    #[test]
    fn arbitrary_gpu_hints_cannot_change_exact_plans() {
        let mut source = Vec::new();
        for i in 0..257u32 {
            source.extend(key_rgb(i.wrapping_mul(7919) & 0xffffff));
        }
        source.extend([80, 80, 80, 124, 129, 124]);
        let mut cpu = AuthorDither::new(&PALETTE, true).unwrap();
        cpu.set_threads(1);
        let expected = cpu.frame_plans(&source);
        for bad_hint in [0u32, 42, u32::MAX] {
            let mut gpu = AuthorDither::new(&PALETTE, true).unwrap();
            gpu.set_threads(1);
            gpu.preplan_rgb_seeded(&source, &vec![bad_hint; source.len() / 3])
                .unwrap();
            assert_eq!(gpu.frame_plans(&source), expected);
        }
        assert!(cpu.preplan_rgb_seeded(&source, &[]).is_err());
        let tree = cpu.gpu_search_data();
        assert_eq!(tree[1] as usize, cpu.candidates.len());
        assert!(tree[2] < tree[0] && tree[3] < tree[0]);
        assert_eq!(tree[5], 8 + tree[0] * 12);
        assert_eq!(tree[4], tree[5] + tree[1] * 4);
        assert!(tree[tree[4] as usize..].iter().all(|&i| i < tree[1]));
    }
}
