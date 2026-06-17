use crate::engine::header::Move;
use crate::engine::state::GameState;
use crate::engine::movegen::generate_moves;
use crate::rl::features::{Features, Weights};
use crate::rl::meta_agent::MetaPolicyNetwork;
use rand::Rng;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub struct ZobristKeys {
    pub cell_keys: [[u64; 16]; 40],
    pub current_keys: [u64; 7],
    pub hold_keys: [u64; 8], // 0..6 for Pieces, 7 for None
    pub hold_used_keys: [u64; 2],
    pub b2b_keys: [u64; 2],
    pub combo_keys: [u64; 32],
    pub queue_keys: [[u64; 7]; 5],
}

impl ZobristKeys {
    pub fn new() -> Self {
        // Deterministic PRNG seed (SplitMix64)
        let mut seed = 0x9E37_79B9_7F4A_7C15u64;
        let mut next_u64 = || {
            seed = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
            let mut z = seed;
            z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
            z ^ (z >> 31)
        };

        let mut cell_keys = [[0u64; 16]; 40];
        for y in 0..40 {
            for x in 0..16 {
                cell_keys[y][x] = next_u64();
            }
        }

        let mut current_keys = [0u64; 7];
        for k in current_keys.iter_mut() {
            *k = next_u64();
        }

        let mut hold_keys = [0u64; 8];
        for k in hold_keys.iter_mut() {
            *k = next_u64();
        }

        let mut hold_used_keys = [0u64; 2];
        for k in hold_used_keys.iter_mut() {
            *k = next_u64();
        }

        let mut b2b_keys = [0u64; 2];
        for k in b2b_keys.iter_mut() {
            *k = next_u64();
        }

        let mut combo_keys = [0u64; 32];
        for k in combo_keys.iter_mut() {
            *k = next_u64();
        }

        let mut queue_keys = [[0u64; 7]; 5];
        for i in 0..5 {
            for j in 0..7 {
                queue_keys[i][j] = next_u64();
            }
        }

        Self {
            cell_keys,
            current_keys,
            hold_keys,
            hold_used_keys,
            b2b_keys,
            combo_keys,
            queue_keys,
        }
    }

    pub fn hash_state(&self, state: &GameState) -> u64 {
        let mut hash = 0u64;

        // Hash occupied board cells
        for y in 0..40 {
            let row = state.board.rows[y];
            if row != 0 {
                for x in 0..state.board.width {
                    if (row & (1 << x)) != 0 {
                        hash ^= self.cell_keys[y][x];
                    }
                }
            }
        }

        // Hash current piece
        let current_idx = state.current as usize;
        if current_idx < 7 {
            hash ^= self.current_keys[current_idx];
        }

        // Hash hold piece
        let hold_idx = match state.hold {
            Some(p) => p as usize,
            None => 7,
        };
        if hold_idx < 8 {
            hash ^= self.hold_keys[hold_idx];
        }

        // Hash hold used flag
        let hold_used_idx = if state.hold_used { 1 } else { 0 };
        hash ^= self.hold_used_keys[hold_used_idx];

        // Hash B2B flag
        let b2b_idx = if state.b2b { 1 } else { 0 };
        hash ^= self.b2b_keys[b2b_idx];

        // Hash combo counter
        let combo_idx = (state.combo as usize).min(31);
        hash ^= self.combo_keys[combo_idx];

        // Hash top 5 queue pieces
        for (i, &p) in state.queue.iter().take(5).enumerate() {
            let p_idx = p as usize;
            if p_idx < 7 {
                hash ^= self.queue_keys[i][p_idx];
            }
        }

        hash
    }
}

pub fn get_zobrist_keys() -> &'static ZobristKeys {
    static KEYS: OnceLock<ZobristKeys> = OnceLock::new();
    KEYS.get_or_init(ZobristKeys::new)
}

#[derive(Clone, Copy)]
pub struct TTEntry {
    pub hash: u64,
    pub depth: u8,
    pub score: f32,
}

impl Default for TTEntry {
    fn default() -> Self {
        Self {
            hash: 0,
            depth: 0,
            score: 0.0,
        }
    }
}

pub struct TranspositionTable {
    entries: Vec<TTEntry>,
}

impl TranspositionTable {
    pub fn new(size: usize) -> Self {
        let size = size.max(1).next_power_of_two();
        Self {
            entries: vec![TTEntry::default(); size],
        }
    }

    #[inline]
    fn index(&self, hash: u64) -> usize {
        (hash as usize) & (self.entries.len() - 1)
    }

    pub fn probe(&self, hash: u64, depth: u8) -> Option<f32> {
        let entry = self.entries[self.index(hash)];
        if entry.hash == hash && entry.depth >= depth {
            Some(entry.score)
        } else {
            None
        }
    }

    pub fn store(&mut self, hash: u64, depth: u8, score: f32) {
        let idx = self.index(hash);
        let entry = &mut self.entries[idx];
        if depth >= entry.depth || entry.hash != hash {
            *entry = TTEntry { hash, depth, score };
        }
    }
}

/// Find all possible states immediately reachable, including hold options.
/// Returns a vector of tuples: (resulting_game_state, chosen_move, hold_was_used)
pub fn get_all_next_states(state: &GameState) -> Vec<(GameState, Move, bool)> {
    let mut next_states = Vec::new();

    // 1. Check moves without hold
    let moves = generate_moves(&state.board, state.current);
    for m in moves {
        let mut sim_state = state.clone();
        sim_state.do_move(m);
        next_states.push((sim_state, m, false));
    }

    // 2. Check moves with hold (if allowed)
    if !state.hold_used {
        let mut hold_sim = state.clone();
        if hold_sim.hold() {
            let moves_hold = generate_moves(&hold_sim.board, hold_sim.current);
            for m in moves_hold {
                let mut sim_state = hold_sim.clone();
                sim_state.do_move(m);
                next_states.push((sim_state, m, true));
            }
        }
    }

    next_states
}

/// Recursively evaluate the board state to a specified lookahead depth using Beam Search pruning.
pub fn evaluate_state_recursive(
    state: &GameState,
    opponent_state: Option<&GameState>,
    weights: &Weights,
    current_depth: usize,
    max_depth: usize,
    tt: &mut TranspositionTable,
) -> f32 {
    if state.game_over {
        return -100000.0;
    }
    if state.board.highest_row() == 0 {
        let feat = Features::evaluate_state(state, opponent_state);
        return feat.dot_product(weights);
    }

    let is_loud = state.combo > 0;
    let quiescence_max_extensions = 2;

    if current_depth >= max_depth {
        if is_loud && current_depth < max_depth + quiescence_max_extensions {
            // Extend search since combo is active
        } else {
            let feat = Features::evaluate_state(state, opponent_state);
            return feat.dot_product(weights);
        }
    }

    let hash = get_zobrist_keys().hash_state(state);
    let remaining_depth = (max_depth as isize - current_depth as isize).max(0) as u8;
    if remaining_depth > 0 {
        if let Some(cached_score) = tt.probe(hash, remaining_depth) {
            return cached_score;
        }
    }

    let mut next_branches = get_all_next_states(state);
    if next_branches.is_empty() {
        return -50000.0; // Trapped
    }

    // Apply Beam Search pruning to prevent exponential branching growth
    // Sort moves by 1-ply heuristic score and keep candidates
    let beam_width = if current_depth >= max_depth {
        2 // Narrow beam for quiescence extensions
    } else if state.board.highest_row() <= 5 {
        if current_depth <= 2 { 6 } else { 4 }
    } else {
        4
    };
    if next_branches.len() > beam_width {
        next_branches.sort_by(|a, b| {
            let score_a = Features::evaluate_state(&a.0, opponent_state).dot_product(weights);
            let score_b = Features::evaluate_state(&b.0, opponent_state).dot_product(weights);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        next_branches.truncate(beam_width);
    }

    let mut best_score = f32::NEG_INFINITY;
    for (sim_state, _, _) in next_branches {
        let child_score = evaluate_state_recursive(&sim_state, opponent_state, weights, current_depth + 1, max_depth, tt);
        let reward = if sim_state.combo > 0 {
            10000.0 * (sim_state.combo as f32)
        } else {
            0.0
        };
        let score = child_score + reward;
        if score > best_score {
            best_score = score;
        }
    }

    if remaining_depth > 0 {
        tt.store(hash, remaining_depth, best_score);
    }
    best_score
}

/// Find the best move for a given GameState using recursive arbitrary lookahead depth.
/// Returns Some((best_move, use_hold))
pub fn find_best_move(
    state: &GameState,
    opponent_state: Option<&GameState>,
    weights: &Weights,
    depth: usize,
) -> Option<(Move, bool)> {
    let next_branches = get_all_next_states(state);
    if next_branches.is_empty() {
        return None;
    }

    // Pre-evaluate 1-ply scores for futility pruning at the root
    let mut evaluated_branches: Vec<(GameState, Move, bool, f32)> = next_branches
        .into_iter()
        .map(|(sim_state, m, used_hold)| {
            let heuristic = Features::evaluate_state(&sim_state, opponent_state).dot_product(weights);
            let reward = if sim_state.combo > 0 {
                10000.0 * (sim_state.combo as f32)
            } else {
                0.0
            };
            let score = heuristic + reward;
            (sim_state, m, used_hold, score)
        })
        .collect();

    // Find the maximum 1-ply score
    let max_1ply = evaluated_branches
        .iter()
        .map(|x| x.3)
        .fold(f32::NEG_INFINITY, f32::max);

    // Prune branches that are way worse than the best 1-ply move
    let futility_delta = 40.0;
    let cutoff = max_1ply - futility_delta;
    evaluated_branches.retain(|x| x.3 >= cutoff);

    // Sort the remaining branches by 1-ply score (descending)
    evaluated_branches.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));

    // Limit maximum root branches to 12 to bound worst-case search complexity
    if evaluated_branches.len() > 12 {
        evaluated_branches.truncate(12);
    }

    let mut best_score = f32::NEG_INFINITY;
    let mut best_choice = None;

    let mut tt = TranspositionTable::new(16384);

    for (sim_state1, m, used_hold, _) in evaluated_branches {
        let child_score = evaluate_state_recursive(&sim_state1, opponent_state, weights, 1, depth, &mut tt);
        let reward = if sim_state1.combo > 0 {
            10000.0 * (sim_state1.combo as f32)
        } else {
            0.0
        };
        let score = child_score + reward;
        if score > best_score {
            best_score = score;
            best_choice = Some((m, used_hold));
        }
    }

    best_choice
}

/// Perform online TD-learning update on the weights.
pub fn update_weights_td(
    weights: &mut Weights,
    s_feat: &Features,
    s_next_feat: &Features,
    reward: f32,
    alpha: f32,
    gamma: f32,
) {
    let v_s = s_feat.dot_product(weights);
    let v_s_next = s_next_feat.dot_product(weights);
    let td_error = reward + gamma * v_s_next - v_s;

    let mut arr = weights.to_array();
    let feat_arr = s_feat.to_array();
    for i in 0..arr.len() {
        arr[i] += alpha * td_error * feat_arr[i];
    }
    // Clip weights to prevent divergence
    for val in arr.iter_mut() {
        *val = val.clamp(-200.0, 200.0);
    }
    *weights = Weights::from_array(arr);
}

/// Noisy Cross-Entropy Method Optimizer for Heuristic Weights
pub struct GeneticOptimizer {
    pub mean: [f32; 15],
    pub std_dev: [f32; 15],
    pub generation: u32,
    pub best_fitness: f32,
    pub best_weights: Weights,
}

impl GeneticOptimizer {
    pub fn new(_pop_size: usize) -> Self {
        let default_w = Weights::default();
        let arr = default_w.to_array();
        
        let mut std_dev = [5.0f32; 15];
        // Give higher exploration variance to opponent weights to help discover multiplayer tactics
        for i in 10..15 {
            std_dev[i] = 10.0;
        }

        Self {
            mean: arr,
            std_dev,
            generation: 0,
            best_fitness: f32::NEG_INFINITY,
            best_weights: default_w,
        }
    }

    /// Evaluates a single set of weights by playing a few games.
    pub fn evaluate_weights(weights: &Weights, board_width: usize, num_games: usize) -> f32 {
        let mut total_score = 0.0;
        let max_pieces = 300; // Limit game length during training for speed

        for _ in 0..num_games {
            let mut state = GameState::new(board_width);
            let mut score = 0.0;

            while !state.game_over && state.pieces_placed < max_pieces {
                if let Some((best_move, use_hold)) = find_best_move(&state, None, weights, 1) {
                    if use_hold {
                        state.hold();
                    }
                    let cleared = state.do_move(best_move);
                    
                    // Reward function
                    score += cleared as f32 * 100.0;
                    if state.combo > 0 {
                        score += state.combo as f32 * 150.0;
                    }
                    if state.b2b {
                        score += 50.0;
                    }
                } else {
                    break; // Blocked
                }
            }

            // Reward for survival length (pieces placed)
            score += state.pieces_placed as f32 * 5.0;

            // Penalty for quick death
            if state.pieces_placed < 50 {
                score -= 1000.0;
            }

            total_score += score;
        }

        total_score / num_games as f32
    }

    /// Evolve population to the next generation using the Noisy Cross-Entropy Method
    pub fn evolve(&mut self, board_width: usize, num_games_eval: usize) {
        let pop_size = 16;
        let elite_size = 4;
        let mut rng = rand::thread_rng();

        // 1. Sample population from Gaussian distribution N(mean, std_dev)
        let mut candidates = Vec::new();
        for _ in 0..pop_size {
            let mut arr = [0.0f32; 15];
            for i in 0..15 {
                // Box-Muller transform for normal distribution sampling
                let u1: f32 = rng.gen::<f32>().max(1e-5);
                let u2: f32 = rng.gen::<f32>();
                let z0 = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos();
                arr[i] = self.mean[i] + z0 * self.std_dev[i];
            }
            let w = Weights::from_array(arr);
            let fitness = Self::evaluate_weights(&w, board_width, num_games_eval);
            candidates.push((w, fitness));
        }

        // 2. Sort by fitness descending
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

        // 3. Update best weights
        if candidates[0].1 > self.best_fitness {
            self.best_fitness = candidates[0].1;
            self.best_weights = candidates[0].0.clone();
        }

        // 4. Select elite candidates
        let mut elite_arrs = Vec::new();
        for i in 0..elite_size {
            elite_arrs.push(candidates[i].0.to_array());
        }

        // 5. Update mean of the distribution
        let mut new_mean = [0.0f32; 15];
        for i in 0..15 {
            let mut sum = 0.0;
            for k in 0..elite_size {
                sum += elite_arrs[k][i];
            }
            new_mean[i] = sum / elite_size as f32;
        }

        // 6. Update standard deviation of the distribution (with added noise to prevent collapse)
        let mut new_std = [0.0f32; 15];
        let noise_level = 0.3f32; // exploration noise
        for i in 0..15 {
            let mut variance_sum = 0.0;
            for k in 0..elite_size {
                let diff = elite_arrs[k][i] - new_mean[i];
                variance_sum += diff * diff;
            }
            let std = (variance_sum / elite_size as f32).sqrt();
            new_std[i] = std + noise_level;
        }

        self.mean = new_mean;
        self.std_dev = new_std;
        self.generation += 1;
    }
}

pub fn evaluate_state_recursive_meta(
    state: &GameState,
    opponent_state: Option<&GameState>,
    meta_net: &MetaPolicyNetwork,
    current_depth: usize,
    max_depth: usize,
    tt: &mut TranspositionTable,
) -> f32 {
    if state.game_over {
        return -100000.0;
    }
    if state.board.highest_row() == 0 {
        let inputs = MetaPolicyNetwork::extract_inputs(state, opponent_state);
        let weights = meta_net.forward(&inputs);
        let feat = Features::evaluate_state(state, opponent_state);
        return feat.dot_product(&weights);
    }

    let is_loud = state.combo > 0;
    let quiescence_max_extensions = 2;

    if current_depth >= max_depth {
        if is_loud && current_depth < max_depth + quiescence_max_extensions {
            // Extend search since combo is active
        } else {
            let inputs = MetaPolicyNetwork::extract_inputs(state, opponent_state);
            let weights = meta_net.forward(&inputs);
            let feat = Features::evaluate_state(state, opponent_state);
            return feat.dot_product(&weights);
        }
    }

    let hash = get_zobrist_keys().hash_state(state);
    let remaining_depth = (max_depth as isize - current_depth as isize).max(0) as u8;
    if remaining_depth > 0 {
        if let Some(cached_score) = tt.probe(hash, remaining_depth) {
            return cached_score;
        }
    }

    let mut next_branches = get_all_next_states(state);
    if next_branches.is_empty() {
        return -50000.0; // Trapped
    }

    // Dynamic weights generation for sorting in beam search
    let inputs = MetaPolicyNetwork::extract_inputs(state, opponent_state);
    let weights = meta_net.forward(&inputs);

    let beam_width = if current_depth >= max_depth {
        2
    } else if state.board.highest_row() <= 5 {
        if current_depth <= 2 { 6 } else { 4 }
    } else {
        4
    };
    if next_branches.len() > beam_width {
        next_branches.sort_by(|a, b| {
            let score_a = Features::evaluate_state(&a.0, opponent_state).dot_product(&weights);
            let score_b = Features::evaluate_state(&b.0, opponent_state).dot_product(&weights);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        next_branches.truncate(beam_width);
    }

    let mut best_score = f32::NEG_INFINITY;
    for (sim_state, _, _) in next_branches {
        let child_score = evaluate_state_recursive_meta(&sim_state, opponent_state, meta_net, current_depth + 1, max_depth, tt);
        let reward = if sim_state.combo > 0 {
            10000.0 * (sim_state.combo as f32)
        } else {
            0.0
        };
        let score = child_score + reward;
        if score > best_score {
            best_score = score;
        }
    }

    if remaining_depth > 0 {
        tt.store(hash, remaining_depth, best_score);
    }
    best_score
}

pub fn find_best_move_meta(
    state: &GameState,
    opponent_state: Option<&GameState>,
    meta_net: &MetaPolicyNetwork,
    depth: usize,
) -> Option<(Move, bool)> {
    let next_branches = get_all_next_states(state);
    if next_branches.is_empty() {
        return None;
    }

    // Pre-evaluate 1-ply scores for futility pruning at the root
    let inputs = MetaPolicyNetwork::extract_inputs(state, opponent_state);
    let weights = meta_net.forward(&inputs);

    let mut evaluated_branches: Vec<(GameState, Move, bool, f32)> = next_branches
        .into_iter()
        .map(|(sim_state, m, used_hold)| {
            let heuristic = Features::evaluate_state(&sim_state, opponent_state).dot_product(&weights);
            let reward = if sim_state.combo > 0 {
                10000.0 * (sim_state.combo as f32)
            } else {
                0.0
            };
            let score = heuristic + reward;
            (sim_state, m, used_hold, score)
        })
        .collect();

    let max_1ply = evaluated_branches
        .iter()
        .map(|x| x.3)
        .fold(f32::NEG_INFINITY, f32::max);

    let futility_delta = 40.0;
    let cutoff = max_1ply - futility_delta;
    evaluated_branches.retain(|x| x.3 >= cutoff);

    evaluated_branches.sort_by(|a, b| b.3.partial_cmp(&a.3).unwrap_or(std::cmp::Ordering::Equal));

    if evaluated_branches.len() > 12 {
        evaluated_branches.truncate(12);
    }

    let mut best_score = f32::NEG_INFINITY;
    let mut best_choice = None;

    let mut tt = TranspositionTable::new(16384);

    for (sim_state1, m, used_hold, _) in evaluated_branches {
        let child_score = evaluate_state_recursive_meta(&sim_state1, opponent_state, meta_net, 1, depth, &mut tt);
        let reward = if sim_state1.combo > 0 {
            10000.0 * (sim_state1.combo as f32)
        } else {
            0.0
        };
        let score = child_score + reward;
        if score > best_score {
            best_score = score;
            best_choice = Some((m, used_hold));
        }
    }

    best_choice
}

pub struct MetaGeneticOptimizer {
    pub mean: [f32; crate::rl::meta_agent::PARAM_COUNT],
    pub std_dev: [f32; crate::rl::meta_agent::PARAM_COUNT],
    pub generation: u32,
    pub best_fitness: f32,
    pub best_net: MetaPolicyNetwork,
}

impl MetaGeneticOptimizer {
    pub fn new() -> Self {
        let default_net = MetaPolicyNetwork::default();
        let arr = default_net.to_array();
        let std_dev = [0.5f32; crate::rl::meta_agent::PARAM_COUNT];

        Self {
            mean: arr,
            std_dev,
            generation: 0,
            best_fitness: f32::NEG_INFINITY,
            best_net: default_net,
        }
    }

    pub fn evaluate_meta_agent(meta_net: &MetaPolicyNetwork, board_width: usize, num_games: usize) -> f32 {
        let default_weights = Weights::default();
        let mut total_score = 0.0;
        let max_pieces = 150;

        for _ in 0..num_games {
            let mut state_meta = GameState::new(board_width);
            let mut state_static = GameState::new(board_width);

            let mut pieces_placed = 0;
            let mut meta_wins = 0.0;

            while !state_meta.game_over && !state_static.game_over && pieces_placed < max_pieces {
                if let Some((best_move, use_hold)) = find_best_move_meta(&state_meta, Some(&state_static), meta_net, 3) {
                    if use_hold {
                        state_meta.hold();
                    }
                    state_meta.do_move_battle(best_move, &mut state_static);
                } else {
                    state_meta.game_over = true;
                }

                if !state_static.game_over {
                    if let Some((best_move, use_hold)) = find_best_move(&state_static, Some(&state_meta), &default_weights, 3) {
                        if use_hold {
                            state_static.hold();
                        }
                        state_static.do_move_battle(best_move, &mut state_meta);
                    } else {
                        state_static.game_over = true;
                    }
                }

                pieces_placed += 1;
            }

            if state_static.game_over && !state_meta.game_over {
                meta_wins += 3000.0;
            } else if state_meta.game_over && !state_static.game_over {
                meta_wins -= 3000.0;
            }

            let attack_diff = (state_meta.score as f32) - (state_static.score as f32);
            meta_wins += attack_diff * 0.1;
            meta_wins += (state_meta.pieces_placed as f32) * 5.0;

            if state_meta.game_over {
                meta_wins -= 1000.0;
            }

            total_score += meta_wins;
        }

        total_score / num_games as f32
    }

    pub fn evolve(&mut self, board_width: usize, num_games_eval: usize) {
        let pop_size = 12;
        let elite_size = 3;
        let mut rng = rand::thread_rng();

        let mut candidates = Vec::new();
        for _ in 0..pop_size {
            let mut arr = [0.0f32; crate::rl::meta_agent::PARAM_COUNT];
            for i in 0..crate::rl::meta_agent::PARAM_COUNT {
                let u1: f32 = rng.gen::<f32>().max(1e-5);
                let u2: f32 = rng.gen::<f32>();
                let z0 = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f32::consts::PI * u2).cos();
                arr[i] = self.mean[i] + z0 * self.std_dev[i];
            }
            let net = MetaPolicyNetwork::from_array(arr);
            let fitness = Self::evaluate_meta_agent(&net, board_width, num_games_eval);
            candidates.push((net, fitness));
        }

        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        if candidates[0].1 > self.best_fitness {
            self.best_fitness = candidates[0].1;
            self.best_net = candidates[0].0.clone();
        }

        let mut elite_arrs = Vec::new();
        for i in 0..elite_size {
            elite_arrs.push(candidates[i].0.to_array());
        }

        let mut new_mean = [0.0f32; crate::rl::meta_agent::PARAM_COUNT];
        for i in 0..crate::rl::meta_agent::PARAM_COUNT {
            let mut sum = 0.0;
            for k in 0..elite_size {
                sum += elite_arrs[k][i];
            }
            new_mean[i] = sum / elite_size as f32;
        }

        let mut new_std = [0.0f32; crate::rl::meta_agent::PARAM_COUNT];
        let noise_level = 0.05f32;
        for i in 0..crate::rl::meta_agent::PARAM_COUNT {
            let mut variance_sum = 0.0;
            for k in 0..elite_size {
                let diff = elite_arrs[k][i] - new_mean[i];
                variance_sum += diff * diff;
            }
            let std = (variance_sum / elite_size as f32).sqrt();
            new_std[i] = std + noise_level;
        }

        self.mean = new_mean;
        self.std_dev = new_std;
        self.generation += 1;
    }
}

pub fn evaluate_state_recursive_original(
    state: &GameState,
    opponent_state: Option<&GameState>,
    weights: &Weights,
    current_depth: usize,
    max_depth: usize,
    tt: &mut TranspositionTable,
) -> f32 {
    if state.game_over {
        return -100000.0;
    }
    if state.board.highest_row() == 0 {
        let feat = Features::evaluate_state(state, opponent_state);
        return feat.dot_product(weights);
    }
    if current_depth >= max_depth {
        let feat = Features::evaluate_state(state, opponent_state);
        return feat.dot_product(weights);
    }

    let hash = get_zobrist_keys().hash_state(state);
    let remaining_depth = (max_depth - current_depth) as u8;
    if let Some(cached_score) = tt.probe(hash, remaining_depth) {
        return cached_score;
    }

    let mut next_branches = get_all_next_states(state);
    if next_branches.is_empty() {
        return -50000.0; // Trapped
    }

    // Apply Beam Search pruning to prevent exponential branching growth
    // Sort moves by 1-ply heuristic score and keep candidates
    let beam_width = if state.board.highest_row() <= 5 {
        if current_depth <= 2 { 6 } else { 4 }
    } else {
        4
    };
    if next_branches.len() > beam_width {
        next_branches.sort_by(|a, b| {
            let score_a = Features::evaluate_state(&a.0, opponent_state).dot_product(weights);
            let score_b = Features::evaluate_state(&b.0, opponent_state).dot_product(weights);
            score_b.partial_cmp(&score_a).unwrap_or(std::cmp::Ordering::Equal)
        });
        next_branches.truncate(beam_width);
    }

    let mut best_score = f32::NEG_INFINITY;
    for (sim_state, _, _) in next_branches {
        let child_score = evaluate_state_recursive_original(&sim_state, opponent_state, weights, current_depth + 1, max_depth, tt);
        let reward = if sim_state.combo > 0 {
            10000.0 * (sim_state.combo as f32)
        } else {
            0.0
        };
        let score = child_score + reward;
        if score > best_score {
            best_score = score;
        }
    }

    tt.store(hash, remaining_depth, best_score);
    best_score
}

pub fn find_best_move_original(
    state: &GameState,
    opponent_state: Option<&GameState>,
    weights: &Weights,
    depth: usize,
) -> Option<(Move, bool)> {
    let next_branches = get_all_next_states(state);
    if next_branches.is_empty() {
        return None;
    }

    let mut best_score = f32::NEG_INFINITY;
    let mut best_choice = None;

    let mut tt = TranspositionTable::new(16384);

    for (sim_state1, m, used_hold) in next_branches {
        let child_score = evaluate_state_recursive_original(&sim_state1, opponent_state, weights, 1, depth, &mut tt);
        let reward = if sim_state1.combo > 0 {
            10000.0 * (sim_state1.combo as f32)
        } else {
            0.0
        };
        let score = child_score + reward;
        if score > best_score {
            best_score = score;
            best_choice = Some((m, used_hold));
        }
    }

    best_choice
}
