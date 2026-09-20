//! A bounded, layer-wide beam. Only visible pieces are searched.
use std::cmp::Ordering;
use std::collections::HashMap;

use crate::engine::{
    header::{Move, Piece},
    state::GameState,
};
use crate::rl::{
    agent::get_all_next_states,
    features::{Features, Weights},
    meta_agent::MetaPolicyNetwork,
};

const BEAM_WIDTH: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Objective {
    PerfectClear,
    Combo,
    ExpertCombo,
    AttackPc,
    AttackCombo,
    DefensePc,
    DefenseCombo,
}

impl Objective {
    fn is_attack(self) -> bool {
        matches!(
            self,
            Self::AttackPc | Self::AttackCombo | Self::DefensePc | Self::DefenseCombo
        )
    }
}

#[derive(Clone, Copy)]
pub enum Evaluator<'a> {
    Static(&'a Weights),
    Meta(&'a MetaPolicyNetwork),
}

impl Evaluator<'_> {
    fn score(&self, state: &GameState, opponent: Option<&GameState>, objective: Objective) -> f32 {
        let mut features = Features::evaluate_state(state, opponent);
        if matches!(objective, Objective::PerfectClear | Objective::DefensePc) {
            // Combo length must not overwhelm a PC setup, even with trained weights.
            features.combo_reward = 0.0;
        }
        let score = match self {
            Self::Static(weights) => features.dot_product(weights),
            Self::Meta(net) => features
                .dot_product(&net.forward(&MetaPolicyNetwork::extract_inputs(state, opponent))),
        };
        // Do not reward emptying a combo field with the generic PC bonus.
        if objective == Objective::ExpertCombo && features.height_max == 0.0 {
            score - 150.0
        } else {
            score
        }
    }
}

#[derive(Clone)]
struct Node {
    state: GameState,
    first_move: Move,
    use_hold: bool,
    chain_open: bool,
    initial_chain: usize,
    clears: usize,
    perfect_clears: usize,
    quality: f32,
    attack: u32,
    peak_attack: u32,
    received: u32,
    exposure: u64,
    first_cancel: u32,
    pc_depth: Option<usize>,
}

impl Node {
    fn compare(&self, other: &Self, objective: Objective) -> Ordering {
        if matches!(
            objective,
            Objective::DefensePc | Objective::DefenseCombo | Objective::ExpertCombo
        ) {
            let safety = other.received.cmp(&self.received);
            if safety != Ordering::Equal {
                return safety;
            }
        }
        if objective.is_attack() {
            // Compare damage over the SAME preview horizon. A later multiplied
            // spin/quad can repay small early clears; neither combo count nor
            // remaining board height may override that realized payoff.
            return self
                .attack
                .cmp(&other.attack)
                .then(other.exposure.cmp(&self.exposure))
                .then(self.initial_chain.cmp(&other.initial_chain))
                .then(self.clears.cmp(&other.clears))
                .then_with(|| {
                    if matches!(objective, Objective::AttackPc | Objective::DefensePc) {
                        self.perfect_clears.cmp(&other.perfect_clears)
                    } else {
                        Ordering::Equal
                    }
                })
                .then(self.quality.total_cmp(&other.quality));
        }
        if matches!(objective, Objective::PerfectClear | Objective::DefensePc) {
            return self
                .perfect_clears
                .cmp(&other.perfect_clears)
                .then(self.quality.total_cmp(&other.quality))
                .then(self.clears.cmp(&other.clears))
                .then(self.attack.cmp(&other.attack));
        }
        // A dead state is never inserted. Preserve the uninterrupted clear chain,
        // then favor further clears and the evaluated final board.
        self.initial_chain
            .cmp(&other.initial_chain)
            .then(self.clears.cmp(&other.clears))
            .then(self.quality.total_cmp(&other.quality))
            .then(self.attack.cmp(&other.attack))
    }
}

/// Exact equality is checked after hashing: garbage, full combo and the entire
/// visible preview must all match before two paths can be merged.
#[derive(Hash, PartialEq, Eq)]
struct Key {
    rows: [u16; 40],
    width: usize,
    spawn_height: i32,
    current: Option<Piece>,
    hold: Option<Piece>,
    queue: Vec<Piece>,
    hold_used: bool,
    combo: u32,
    b2b: bool,
    b2b_level: u32,
    b2b_charge: u32,
    pending: u32,
    queued: u32,
    garbage_hole: usize,
    packets: Option<Vec<crate::engine::state::GarbagePacket>>,
    chain_open: bool,
}

impl Key {
    fn of(node: &Node) -> Self {
        let s = &node.state;
        Self {
            rows: s.board.rows,
            width: s.board.width,
            spawn_height: s.board.spawn_height,
            current: s.has_known_current().then_some(s.current),
            hold: s.hold,
            queue: s.queue.clone(),
            hold_used: s.hold_used,
            combo: s.combo,
            b2b: s.b2b,
            b2b_level: s.b2b_level,
            b2b_charge: s.b2b_charge,
            pending: s.pending_garbage,
            queued: s.queued_garbage,
            garbage_hole: s.last_garbage_hole_x,
            packets: s.garbage_packets.clone(),
            chain_open: node.chain_open,
        }
    }
}

fn retain_best(nodes: &mut Vec<Node>, width: usize, objective: Objective) {
    if nodes.len() <= width {
        nodes.sort_by(|a, b| b.compare(a, objective));
        return;
    }
    let budgets = if objective.is_attack() {
        vec![
            (objective, width / 2),
            (Objective::Combo, width / 4),
            (Objective::PerfectClear, width - width / 2 - width / 4),
        ]
    } else {
        vec![(objective, width)]
    };
    let mut keep = vec![false; nodes.len()];
    let mut indices = Vec::with_capacity(nodes.len());
    for (ranking, quota) in budgets {
        if quota == 0 {
            continue;
        }
        indices.clear();
        indices.extend((0..nodes.len()).filter(|&i| !keep[i]));
        // Previous stable full sorts broke ties by primary rank, then original
        // insertion order. Make that order explicit before partial selection.
        let compare = |&a: &usize, &b: &usize| {
            nodes[b]
                .compare(&nodes[a], ranking)
                .then_with(|| nodes[b].compare(&nodes[a], objective))
                .then(a.cmp(&b))
        };
        if quota < indices.len() {
            indices.select_nth_unstable_by(quota, compare);
        }
        for &i in indices.iter().take(quota) {
            keep[i] = true;
        }
    }
    let mut index = 0;
    nodes.retain(|_| {
        let retained = keep[index];
        index += 1;
        retained
    });
    // Sort only the survivors. Stable ties still use original insertion order.
    nodes.sort_by(|a, b| b.compare(a, objective));
}

pub fn find_best_move(
    state: &GameState,
    opponent: Option<&GameState>,
    evaluator: Evaluator<'_>,
    depth: usize,
) -> Option<(Move, bool)> {
    find_best_move_for_objective(state, opponent, evaluator, depth, Objective::PerfectClear)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HybridMode {
    ExpertCombo {
        table: bool,
        defending: bool,
    },
    PerfectClear {
        placements: usize,
    },
    ComboResidue,
    GarbageDefense {
        pc: bool,
        canceled_next: u32,
        received: u32,
    },
    ComboMultiplier,
}
impl HybridMode {
    pub fn label(self) -> String {
        match self {
            Self::ExpertCombo { table, defending } => format!(
                "Expert combo: {}{}",
                if table {
                    "continuation table"
                } else {
                    "clear-chain search"
                },
                if defending { " (garbage defense)" } else { "" }
            ),
            Self::PerfectClear { placements } => format!("PC in {placements} placements"),
            Self::ComboResidue => "Combo: PC needs a change in garbage".into(),
            Self::GarbageDefense {
                pc,
                canceled_next,
                received,
            } => format!(
                "{} defense: cancel {canceled_next} next, receive {received} in preview",
                if pc { "PC" } else { "Combo" }
            ),
            Self::ComboMultiplier => "Combo: multiplier attack plan".into(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HybridPlan {
    pub choice: (Move, bool),
    pub mode: HybridMode,
    pub expected_attack: u32,
    pub peak_attack: u32,
}

/// Necessary condition only: cells + 4*n - width*lines = 0 requires the current
/// cell count to be divisible by gcd(width,4). Incoming garbage is not assumed;
/// re-evaluate from the actual board after every placement.
pub fn pc_residue_possible(state: &GameState) -> bool {
    let (mut a, mut b) = (state.board.width as u32, 4);
    while b != 0 {
        (a, b) = (b, a % b);
    }
    let cells: u32 = state
        .board
        .rows
        .iter()
        .map(|r| (r & state.board.full_row_mask()).count_ones())
        .sum();
    cells % a == 0
}

/// Optimize realized multiplier damage over a common visible horizon. Report
/// PC only when the selected continuation actually reaches one.
pub fn find_hybrid_move(
    state: &GameState,
    opponent: Option<&GameState>,
    evaluator: Evaluator<'_>,
    depth: usize,
) -> Option<HybridPlan> {
    find_hybrid_move_internal(state, opponent, evaluator, depth, true)
}

/// Expert is a distinct combo-first policy, including setup and fallback.
/// The existing GUI hybrid entry point keeps its PC/attack policy.
pub fn find_hybrid_move_with_expert(
    state: &GameState,
    opponent: Option<&GameState>,
    evaluator: Evaluator<'_>,
    depth: usize,
    expert: bool,
) -> Option<HybridPlan> {
    if expert {
        let mut visible = state.for_search();
        visible.queue.truncate(5);
        let table = crate::rl::combo_solver::choose(&visible);
        return search_with_root(
            &visible,
            opponent,
            evaluator,
            depth,
            Objective::ExpertCombo,
            table.map(|p| p.choice),
        )
        .map(|found| HybridPlan {
            choice: found.choice,
            mode: HybridMode::ExpertCombo {
                table: table.is_some(),
                defending: state.incoming_garbage() > 0,
            },
            expected_attack: found.attack,
            peak_attack: found.peak_attack,
        });
    }
    find_hybrid_move_internal(state, opponent, evaluator, depth, false)
}

fn find_hybrid_move_internal(
    state: &GameState,
    opponent: Option<&GameState>,
    evaluator: Evaluator<'_>,
    depth: usize,
    use_table: bool,
) -> Option<HybridPlan> {
    let allow_pc = state.pc_bonus > 0 && pc_residue_possible(state);
    let pressure = state.incoming_garbage() > 0;
    let objective = match (pressure, allow_pc) {
        (true, true) => Objective::DefensePc,
        (true, false) => Objective::DefenseCombo,
        (false, true) => Objective::AttackPc,
        (false, false) => Objective::AttackCombo,
    };
    search(state, opponent, evaluator, depth, objective).map(|mut found| {
        // Keep queued-garbage defense and an already selected PC. In a quiet
        // combo phase, use the separate table solver's clearing continuation.
        // Re-evaluate the chosen root through the existing attack simulator so
        // the public attack/cancel/PC diagnostics describe the actual choice.
        if use_table && !pressure && found.pc_depth.is_none() {
            if let Some(plan) = crate::rl::combo_solver::choose(state) {
                if plan.choice != found.choice {
                    if let Some(restricted) = search_with_root(
                        state,
                        opponent,
                        evaluator,
                        depth,
                        objective,
                        Some(plan.choice),
                    ) {
                        found = restricted;
                    }
                }
            }
        }
        let mode = if pressure {
            HybridMode::GarbageDefense {
                pc: allow_pc && found.pc_depth.is_some(),
                canceled_next: found.first_cancel,
                received: found.received,
            }
        } else if !allow_pc {
            HybridMode::ComboResidue
        } else if let Some(placements) = found.pc_depth {
            HybridMode::PerfectClear { placements }
        } else {
            HybridMode::ComboMultiplier
        };
        HybridPlan {
            choice: found.choice,
            mode,
            expected_attack: found.attack,
            peak_attack: found.peak_attack,
        }
    })
}

#[derive(Clone, Copy)]
struct SearchResult {
    choice: (Move, bool),
    pc_depth: Option<usize>,
    first_cancel: u32,
    received: u32,
    attack: u32,
    peak_attack: u32,
}

pub fn find_best_move_for_objective(
    state: &GameState,
    opponent: Option<&GameState>,
    evaluator: Evaluator<'_>,
    depth: usize,
    objective: Objective,
) -> Option<(Move, bool)> {
    if objective == Objective::Combo {
        if let Some(plan) = crate::rl::combo_solver::choose(state) {
            return Some(plan.choice);
        }
    }
    find_beam_move_for_objective(state, opponent, evaluator, depth, objective)
}

/// Original beam kept as a fallback and as a reproducible benchmark baseline.
pub fn find_beam_move_for_objective(
    state: &GameState,
    opponent: Option<&GameState>,
    evaluator: Evaluator<'_>,
    depth: usize,
    objective: Objective,
) -> Option<(Move, bool)> {
    search(state, opponent, evaluator, depth, objective).map(|result| result.choice)
}

fn search(
    state: &GameState,
    opponent: Option<&GameState>,
    evaluator: Evaluator<'_>,
    depth: usize,
    objective: Objective,
) -> Option<SearchResult> {
    search_with_root(state, opponent, evaluator, depth, objective, None)
}

fn search_with_root(
    state: &GameState,
    opponent: Option<&GameState>,
    evaluator: Evaluator<'_>,
    depth: usize,
    objective: Objective,
    root: Option<(Move, bool)>,
) -> Option<SearchResult> {
    if state.game_over || !state.has_known_current() {
        return None;
    }
    // An empty hold can consume one additional preview piece. Use a common
    // horizon for all root choices so hold is not unfairly compared at less depth.
    let visible = 1 + state.queue.len();
    let reserve = usize::from(state.hold.is_none() && !state.hold_used && visible > 1);
    let horizon = depth.max(1).min(visible - reserve);
    let mut frontier = Vec::new();
    for (next, m, use_hold) in get_all_next_states(state) {
        if root.is_some_and(|choice| choice != (m, use_hold)) {
            continue;
        }
        if next.game_over {
            continue;
        }
        let cleared = next.combo > 0;
        let quality = evaluator.score(&next, opponent, objective);
        let perfect_clears = usize::from(next.last_perfect_clear);
        frontier.push(Node {
            attack: next.last_attack,
            peak_attack: next.last_attack,
            received: next.last_received_garbage,
            exposure: next.incoming_garbage() as u64,
            first_cancel: next.last_canceled_garbage,
            pc_depth: next.last_perfect_clear.then_some(1),
            state: next,
            first_move: m,
            use_hold,
            chain_open: cleared,
            initial_chain: usize::from(cleared),
            clears: usize::from(cleared),
            perfect_clears,
            quality,
        });
    }
    retain_best(&mut frontier, BEAM_WIDTH, objective);
    let mut fallback = frontier.first().map(|n| SearchResult {
        choice: (n.first_move, n.use_hold),
        pc_depth: n.pc_depth,
        first_cancel: n.first_cancel,
        received: n.received,
        attack: n.attack,
        peak_attack: n.peak_attack,
    });
    if objective == Objective::PerfectClear
        && frontier.first().is_some_and(|n| n.perfect_clears > 0)
    {
        return fallback;
    }
    for layer in 1..horizon {
        let mut next_layer: Vec<Node> = Vec::new();
        let mut seen: HashMap<Key, usize> = HashMap::new();
        for node in &frontier {
            for (next, _, _) in get_all_next_states(&node.state) {
                if next.game_over {
                    continue;
                }
                let cleared = next.combo > 0;
                let chain_open = node.chain_open && cleared;
                let perfect_clears = node.perfect_clears + usize::from(next.last_perfect_clear);
                let mut candidate = Node {
                    attack: node.attack + next.last_attack,
                    peak_attack: node.peak_attack.max(next.last_attack),
                    received: node.received + next.last_received_garbage,
                    exposure: node.exposure + next.incoming_garbage() as u64,
                    first_cancel: node.first_cancel,
                    pc_depth: node
                        .pc_depth
                        .or_else(|| next.last_perfect_clear.then_some(layer + 1)),
                    state: next,
                    first_move: node.first_move,
                    use_hold: node.use_hold,
                    chain_open,
                    initial_chain: node.initial_chain + usize::from(chain_open),
                    clears: node.clears + usize::from(cleared),
                    perfect_clears,
                    quality: 0.0,
                };
                let key = Key::of(&candidate);
                if let Some(&index) = seen.get(&key) {
                    candidate.quality = next_layer[index].quality;
                    if candidate.compare(&next_layer[index], objective) == Ordering::Greater {
                        next_layer[index] = candidate;
                    }
                } else {
                    candidate.quality = evaluator.score(&candidate.state, opponent, objective);
                    seen.insert(key, next_layer.len());
                    next_layer.push(candidate);
                }
            }
        }
        if next_layer.is_empty() {
            break;
        }
        retain_best(&mut next_layer, BEAM_WIDTH, objective);
        fallback = next_layer.first().map(|n| SearchResult {
            choice: (n.first_move, n.use_hold),
            pc_depth: n.pc_depth,
            first_cancel: n.first_cancel,
            received: n.received,
            attack: n.attack,
            peak_attack: n.peak_attack,
        });
        // Breadth by placement depth: the first layer with a PC is the soonest
        // discovered solution. Prefer completing it over keeping a combo alive.
        if objective == Objective::PerfectClear
            && next_layer.first().is_some_and(|n| n.perfect_clears > 0)
        {
            return fallback;
        }
        frontier = next_layer;
    }
    fallback
}

#[cfg(test)]
mod selection_tests {
    use super::*;
    #[test]
    fn expert_switch_gates_the_table_without_changing_the_normal_beam() {
        use crate::engine::{board::Board, header::ALL_PIECES};
        let weights = Weights::default();
        let mut differing = 0;
        for current in ALL_PIECES {
            for hold in ALL_PIECES {
                let mut board = Board::new(4);
                board.rows[..2].copy_from_slice(&[7, 7]);
                let queue = (1..6)
                    .map(|i| ALL_PIECES[(current as usize + i) % 7])
                    .collect();
                let state =
                    GameState::from_triangle(board, current, Some(hold), queue, 5, false, 0);
                let normal = find_hybrid_move_with_expert(
                    &state,
                    None,
                    Evaluator::Static(&weights),
                    6,
                    false,
                )
                .unwrap();
                let baseline = search(
                    &state,
                    None,
                    Evaluator::Static(&weights),
                    6,
                    Objective::AttackCombo,
                )
                .unwrap();
                assert_eq!(normal.choice, baseline.choice);
                if let Some(table) = crate::rl::combo_solver::choose(&state) {
                    let expert = find_hybrid_move_with_expert(
                        &state,
                        None,
                        Evaluator::Static(&weights),
                        6,
                        true,
                    )
                    .unwrap();
                    assert_eq!(expert.choice, table.choice);
                    differing += usize::from(expert.choice != normal.choice);
                }
            }
        }
        assert!(
            differing > 0,
            "fixture must distinguish table and beam choices"
        );
    }
    fn reference(nodes: &mut Vec<Node>, width: usize, objective: Objective) {
        nodes.sort_by(|a, b| b.compare(a, objective));
        if !objective.is_attack() || nodes.len() <= width {
            nodes.truncate(width);
            return;
        }
        let mut keep = vec![false; nodes.len()];
        for (ranking, quota) in [
            (objective, width / 2),
            (Objective::Combo, width / 4),
            (Objective::PerfectClear, width - width / 2 - width / 4),
        ] {
            let mut indices: Vec<_> = (0..nodes.len()).filter(|&i| !keep[i]).collect();
            indices.sort_by(|&a, &b| nodes[b].compare(&nodes[a], ranking));
            for i in indices.into_iter().take(quota) {
                keep[i] = true;
            }
        }
        let mut i = 0;
        nodes.retain(|_| {
            let k = keep[i];
            i += 1;
            k
        });
    }
    #[test]
    fn partial_selection_preserves_full_stable_sort_including_ties() {
        let state = GameState::new(4);
        for all_tied in [false, true] {
            let nodes: Vec<Node> = (0..256)
                .map(|i| Node {
                    state: state.clone(),
                    first_move: Move::new(Piece::T, crate::engine::header::Rotation::North, i, 0),
                    use_hold: false,
                    chain_open: true,
                    initial_chain: if all_tied { 0 } else { i as usize % 5 },
                    clears: i as usize % 3,
                    perfect_clears: 0,
                    quality: if all_tied { 0.0 } else { (i % 7) as f32 },
                    attack: if all_tied { 0 } else { i as u32 % 11 },
                    peak_attack: 0,
                    received: 0,
                    exposure: 0,
                    first_cancel: 0,
                    pc_depth: None,
                })
                .collect();
            for objective in [
                Objective::Combo,
                Objective::ExpertCombo,
                Objective::PerfectClear,
                Objective::AttackPc,
                Objective::AttackCombo,
                Objective::DefensePc,
                Objective::DefenseCombo,
            ] {
                for width in [0, 1, 2, 7, 64, 256] {
                    let mut expected = nodes.clone();
                    let mut actual = nodes.clone();
                    reference(&mut expected, width, objective);
                    retain_best(&mut actual, width, objective);
                    assert_eq!(
                        actual.iter().map(|n| n.first_move.x).collect::<Vec<_>>(),
                        expected.iter().map(|n| n.first_move.x).collect::<Vec<_>>()
                    );
                }
            }
        }
    }
}
