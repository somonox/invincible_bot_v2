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
    DefensePc,
    DefenseCombo,
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
        match self {
            Self::Static(weights) => features.dot_product(weights),
            Self::Meta(net) => features
                .dot_product(&net.forward(&MetaPolicyNetwork::extract_inputs(state, opponent))),
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
    received: u32,
    exposure: u64,
    first_cancel: u32,
    pc_depth: Option<usize>,
}

impl Node {
    fn compare(&self, other: &Self, objective: Objective) -> Ordering {
        if matches!(objective, Objective::DefensePc | Objective::DefenseCombo) {
            let safety = other
                .received
                .cmp(&self.received)
                .then(other.exposure.cmp(&self.exposure));
            if safety != Ordering::Equal {
                return safety;
            }
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
    // Score each state once, before sorting; no feature extraction in comparator.
    nodes.sort_by(|a, b| b.compare(a, objective));
    nodes.truncate(width);
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
    PerfectClear {
        placements: usize,
    },
    ComboResidue,
    GarbageDefense {
        pc: bool,
        canceled_next: u32,
        received: u32,
    },
    ComboNoVisiblePc,
}
impl HybridMode {
    pub fn label(self) -> String {
        match self {
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
            Self::ComboNoVisiblePc => "Combo: no PC found in preview".into(),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HybridPlan {
    pub choice: (Move, bool),
    pub mode: HybridMode,
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

/// Keep the pure objectives available for comparison. A PC probe must actually
/// reach a PC: its ordinary board-evaluation fallback is never a hybrid PC plan.
pub fn find_hybrid_move(
    state: &GameState,
    opponent: Option<&GameState>,
    evaluator: Evaluator<'_>,
    depth: usize,
) -> Option<HybridPlan> {
    if state.incoming_garbage() > 0 {
        let allow_pc = pc_residue_possible(state);
        let objective = if allow_pc {
            Objective::DefensePc
        } else {
            Objective::DefenseCombo
        };
        // Compare complete horizons. Early return on the first PC hides the
        // danger of remaining garbage after that PC breaks the clear chain.
        return search(state, opponent, evaluator, depth, objective).map(|found| HybridPlan {
            choice: found.choice,
            mode: HybridMode::GarbageDefense {
                pc: allow_pc && found.pc_depth.is_some(),
                canceled_next: found.first_cancel,
                received: found.received,
            },
        });
    }
    let mode = if !pc_residue_possible(state) {
        HybridMode::ComboResidue
    } else {
        if let Some(found) = search(state, opponent, evaluator, depth, Objective::PerfectClear) {
            if let Some(placements) = found.pc_depth {
                return Some(HybridPlan {
                    choice: found.choice,
                    mode: HybridMode::PerfectClear { placements },
                });
            }
        }
        HybridMode::ComboNoVisiblePc
    };
    find_best_move_for_objective(state, opponent, evaluator, depth, Objective::Combo)
        .map(|choice| HybridPlan { choice, mode })
}

#[derive(Clone, Copy)]
struct SearchResult {
    choice: (Move, bool),
    pc_depth: Option<usize>,
    first_cancel: u32,
    received: u32,
}

pub fn find_best_move_for_objective(
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
        if next.game_over {
            continue;
        }
        let cleared = next.combo > 0;
        let quality = evaluator.score(&next, opponent, objective);
        let perfect_clears = usize::from(next.last_perfect_clear);
        frontier.push(Node {
            attack: next.last_attack,
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
