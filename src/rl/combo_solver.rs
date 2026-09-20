//! A separate finite-state combo solver. Physics comes from the existing SRS-X
//! move generator; no height/holes/PC heuristic is used here.
use std::collections::HashMap;
use std::sync::OnceLock;

use crate::engine::{
    board::Board,
    header::{Move, Piece, SpinMode, ALL_PIECES},
    movegen::generate_moves_with_rules,
    state::GameState,
};
use crate::rl::agent::get_all_next_states;

pub const HORIZON: usize = 128;
const FULL_BAG: usize = 127;
const EMPTY_HOLD: usize = 7;
const SCALE: f32 = 256.0;

pub struct Graph {
    pub boards: Vec<u32>,
    pub edges: Vec<[Vec<usize>; 7]>,
    ids: HashMap<u32, usize>,
}

pub fn board_code(board: &Board) -> Option<u32> {
    if board.width != 4 || board.highest_row() > 8 {
        return None;
    }
    Some((0..8).fold(0, |code, y| code | ((board.rows[y] as u32) << (4 * y))))
}

pub fn decode_board(code: u32) -> Board {
    let mut board = Board::new(4);
    for y in 0..8 {
        board.rows[y] = ((code >> (4 * y)) & 15) as u16;
    }
    board
}

impl Graph {
    /// All <=6-cell seeds in the bottom two rows, followed by their complete
    /// closure under clearing placements. No transition may be silently cut.
    pub fn build() -> Self {
        let mut graph = Self {
            boards: Vec::new(),
            edges: Vec::new(),
            ids: HashMap::new(),
        };
        for code in 0u32..256 {
            if code.count_ones() <= 6 && code & 15 != 15 && (code >> 4) & 15 != 15 {
                graph.insert(code);
            }
        }
        let mut i = 0;
        while i < graph.boards.len() {
            let board = decode_board(graph.boards[i]);
            let mut edges: [Vec<usize>; 7] = Default::default();
            for piece in ALL_PIECES {
                for m in generate_moves_with_rules(&board, piece, SpinMode::None) {
                    let mut next = board;
                    next.place(piece, m.rotation, m.x, m.y);
                    if next.clear_lines() == 0 {
                        continue;
                    }
                    let code = board_code(&next).expect("combo graph escaped its exact encoding");
                    let id = graph.insert(code);
                    edges[piece as usize].push(id);
                }
                edges[piece as usize].sort_unstable();
                edges[piece as usize].dedup();
            }
            graph.edges.push(edges);
            i += 1;
        }
        graph
    }

    fn insert(&mut self, code: u32) -> usize {
        if let Some(&id) = self.ids.get(&code) {
            return id;
        }
        let id = self.boards.len();
        self.boards.push(code);
        self.ids.insert(code, id);
        id
    }

    fn id(&self, board: &Board) -> Option<usize> {
        self.ids.get(&board_code(board)?).copied()
    }

    /// Bellman dynamic programming, not random rollouts: enumerate every legal
    /// 7-bag draw, every clearing destination and both hold decisions. V counts
    /// expected consecutive clearing placements, capped at `horizon`.
    /// The offline policy observes the drawn piece and exact bag mask but no
    /// preview. Runtime separately solves the actually visible preview.
    pub fn continuation_values(&self, horizon: usize) -> Vec<f32> {
        let mut previous = vec![0.0; self.boards.len() * 8 * 128];
        let mut next = previous.clone();
        for _ in 0..horizon {
            for board in 0..self.boards.len() {
                for hold in 0..8 {
                    for mask in 1usize..128 {
                        let mut sum = 0.0;
                        for piece in 0..7 {
                            if mask & (1 << piece) == 0 {
                                continue;
                            }
                            let remaining = draw(mask, piece);
                            let mut best =
                                self.placement_value(&previous, board, piece, hold, remaining);
                            let swapped = if hold != EMPTY_HOLD {
                                self.placement_value(&previous, board, hold, piece, remaining)
                            } else {
                                // Empty hold draws again, but allows only one placement
                                // and no second hold before that placement.
                                let mut sum = 0.0;
                                for second in 0..7 {
                                    if remaining & (1 << second) != 0 {
                                        sum += self.placement_value(
                                            &previous,
                                            board,
                                            second,
                                            piece,
                                            draw(remaining, second),
                                        );
                                    }
                                }
                                sum / remaining.count_ones() as f32
                            };
                            best = best.max(swapped);
                            sum += best;
                        }
                        next[index(board, hold, mask)] = sum / mask.count_ones() as f32;
                    }
                }
            }
            std::mem::swap(&mut previous, &mut next);
        }
        previous
    }

    fn placement_value(
        &self,
        values: &[f32],
        board: usize,
        piece: usize,
        hold: usize,
        mask: usize,
    ) -> f32 {
        self.edges[board][piece]
            .iter()
            .map(|&next| 1.0 + values[index(next, hold, mask)])
            .fold(0.0, f32::max)
    }

    /// Greatest fixed point for a full hold with one preview. `mask` is the
    /// remaining bag AFTER the current piece was drawn. A state survives iff
    /// for EVERY next draw there EXISTS a clearing action into the surviving
    /// set. Asynchronous deletions converge to the same greatest fixed point.
    pub fn winning_with_one_preview(&self) -> [usize; 7] {
        let ix = |b: usize, h: usize, p: usize, m: usize| ((b * 7 + h) * 7 + p) * 128 + m;
        let mut alive = vec![true; self.boards.len() * 7 * 7 * 128];
        loop {
            let mut changed = false;
            for b in 0..self.boards.len() {
                for h in 0..7 {
                    for p in 0..7 {
                        for mask in 1usize..128 {
                            let slot = ix(b, h, p, mask);
                            if !alive[slot] {
                                continue;
                            }
                            for q in 0..7 {
                                if mask & (1 << q) == 0 {
                                    continue;
                                }
                                let rem = draw(mask, q);
                                if !self.edges[b][p].iter().any(|&n| alive[ix(n, h, q, rem)])
                                    && !self.edges[b][h].iter().any(|&n| alive[ix(n, p, q, rem)])
                                {
                                    alive[slot] = false;
                                    changed = true;
                                    break;
                                }
                            }
                        }
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut counts = [0; 7];
        for b in 0..self.boards.len() {
            for h in 0..7 {
                for p in 0..7 {
                    for mask in 1usize..128 {
                        if alive[ix(b, h, p, mask)] {
                            counts[self.boards[b].count_ones() as usize] += 1;
                        }
                    }
                }
            }
        }
        counts
    }

    pub fn encode(&self, values: &[f32]) -> Vec<u8> {
        assert_eq!(values.len(), self.boards.len() * 8 * 128);
        let mut bytes = b"C4TB0001".to_vec();
        bytes.extend_from_slice(&(HORIZON as u32).to_le_bytes());
        bytes.extend_from_slice(&(self.boards.len() as u32).to_le_bytes());
        assert!(self.boards.len() < u16::MAX as usize);
        for (code, edges) in self.boards.iter().zip(&self.edges) {
            bytes.extend_from_slice(&code.to_le_bytes());
            for children in edges {
                bytes.extend_from_slice(&(children.len() as u16).to_le_bytes());
                for &child in children {
                    bytes.extend_from_slice(&(child as u16).to_le_bytes());
                }
            }
        }
        for &value in values {
            bytes.extend_from_slice(&((value * SCALE).floor() as u16).to_le_bytes());
        }
        bytes
    }
}

fn index(board: usize, hold: usize, mask: usize) -> usize {
    (board * 8 + hold) * 128 + mask
}
fn draw(mask: usize, piece: usize) -> usize {
    let mask = mask ^ (1 << piece);
    if mask == 0 {
        FULL_BAG
    } else {
        mask
    }
}

struct Table {
    graph: Graph,
    values: Vec<u16>,
}
impl Table {
    fn decode(bytes: &[u8]) -> Self {
        assert_eq!(&bytes[..8], b"C4TB0001");
        assert_eq!(
            u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            HORIZON as u32
        );
        let count = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let mut at = 16;
        let mut graph = Graph {
            boards: Vec::new(),
            edges: Vec::new(),
            ids: HashMap::new(),
        };
        for _ in 0..count {
            let code = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            at += 4;
            graph.insert(code);
            let mut edges: [Vec<usize>; 7] = Default::default();
            for children in &mut edges {
                let len = u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize;
                at += 2;
                for _ in 0..len {
                    children
                        .push(u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap()) as usize);
                    at += 2;
                }
            }
            graph.edges.push(edges);
        }
        assert_eq!(bytes.len() - at, count * 8 * 128 * 2);
        let values = bytes[at..]
            .chunks_exact(2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .collect();
        Self { graph, values }
    }
}

fn table() -> &'static Table {
    static TABLE: OnceLock<Table> = OnceLock::new();
    TABLE.get_or_init(|| Table::decode(include_bytes!("../../data/combo-table.bin")))
}

/// We do not pretend to know a hidden bag boundary. Keep EVERY bag remainder
/// compatible with the observed sequence, including possible boundary wraps.
fn possible_tail_masks(sequence: &[Piece]) -> Vec<usize> {
    let mut masks: Vec<_> = (1usize..128).collect();
    for &piece in sequence {
        masks = masks
            .into_iter()
            .filter(|m| m & (1 << piece as usize) != 0)
            .map(|m| draw(m, piece as usize))
            .collect();
        masks.sort_unstable();
        masks.dedup();
    }
    masks
}

#[derive(Clone, Copy, Default)]
struct Value {
    complete: bool,
    clears: f32,
}
impl Value {
    fn better(self, other: Self) -> bool {
        (self.complete && !other.complete)
            || (self.complete == other.complete && self.clears > other.clears)
    }
}

struct Preview<'a> {
    table: &'a Table,
    sequence: &'a [Piece],
    tail_masks: &'a [usize],
    memo: HashMap<(usize, usize, usize), Value>,
}
impl Preview<'_> {
    fn solve(&mut self, board: usize, hold: usize, pos: usize) -> Value {
        if let Some(&value) = self.memo.get(&(board, hold, pos)) {
            return value;
        }
        let value = if pos == self.sequence.len() {
            // Conservative phase ranking, NOT a proof of a policy with hidden
            // phase information. The stored policy knows its bag mask.
            Value {
                complete: true,
                clears: self
                    .tail_masks
                    .iter()
                    .map(|&mask| self.table.values[index(board, hold, mask)] as f32 / SCALE)
                    .fold(f32::INFINITY, f32::min),
            }
        } else {
            let piece = self.sequence[pos] as usize;
            let mut options = vec![(piece, hold, pos + 1)];
            if hold != EMPTY_HOLD {
                options.push((hold, piece, pos + 1));
            } else if pos + 1 < self.sequence.len() {
                options.push((self.sequence[pos + 1] as usize, piece, pos + 2));
            }
            let mut best = Value::default();
            for (placed, next_hold, next_pos) in options {
                for &child in &self.table.graph.edges[board][placed] {
                    let mut value = self.solve(child, next_hold, next_pos);
                    value.clears += 1.0;
                    if value.better(best) {
                        best = value;
                    }
                }
            }
            best
        };
        self.memo.insert((board, hold, pos), value);
        value
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ComboPlan {
    pub choice: (Move, bool),
    pub continuation: f32,
}

/// Exact enumeration of clearing paths through the supplied preview, followed
/// by the offline long-horizon continuation table. No random future is sampled.
/// Unsupported boards/rules/pressure are explicitly handed back to normal play.
pub fn choose(state: &GameState) -> Option<ComboPlan> {
    if state.game_over
        || !state.has_known_current()
        || state.board.width != 4
        || !matches!(state.board.spawn_height, 20 | 26)
        || state.incoming_garbage() != 0
        || state
            .garbage_packets
            .as_ref()
            .is_some_and(|ps| ps.iter().any(|p| p.amount > 0))
        || state.queue.len() > 5
    {
        return None;
    }
    let sequence: Vec<_> = std::iter::once(state.current)
        .chain(state.queue.iter().copied())
        .collect();
    let masks = possible_tail_masks(&sequence);
    if masks.is_empty() {
        return None;
    }
    let table = table();
    // Only take over an ALREADY compact combo well. Do not spend a tall stack
    // early merely to enter this table: that would undo multiplier planning.
    table.graph.id(&state.board)?;
    let mut preview = Preview {
        table,
        sequence: &sequence,
        tail_masks: &masks,
        memo: HashMap::new(),
    };
    let mut best: Option<(ComboPlan, u32)> = None;
    for (next, m, use_hold) in get_all_next_states(state) {
        if next.game_over || next.combo == 0 || next.last_received_garbage > 0 {
            continue;
        }
        let Some(board) = table.graph.id(&next.board) else {
            continue;
        };
        let consumed = 1 + usize::from(use_hold && state.hold.is_none());
        let value = preview.solve(
            board,
            next.hold.map_or(EMPTY_HOLD, |p| p as usize),
            consumed,
        );
        if !value.complete {
            continue;
        }
        let candidate = ComboPlan {
            choice: (m, use_hold),
            continuation: value.clears + 1.0,
        };
        if best.is_none_or(|(old, attack)| {
            candidate.continuation > old.continuation
                || (candidate.continuation == old.continuation && next.last_attack > attack)
        }) {
            best = Some((candidate, next.last_attack));
        }
    }
    best.map(|(plan, _)| plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_table_is_the_complete_reachable_graph() {
        let generated = Graph::build();
        let stored = &table().graph;
        assert_eq!(generated.boards, stored.boards);
        assert_eq!(generated.edges, stored.edges);
        assert_eq!(stored.boards.len(), 1026);
        for &code in &stored.boards {
            assert!(code.count_ones() <= 6);
        }
    }

    #[test]
    fn graph_destinations_match_live_engine_at_both_spawn_heights() {
        let graph = &table().graph;
        for id in (0..graph.boards.len()).step_by(17) {
            for height in [20, 26] {
                let mut board = decode_board(graph.boards[id]);
                board.spawn_height = height;
                for p in ALL_PIECES {
                    for mode in [SpinMode::All, SpinMode::Handheld, SpinMode::None] {
                        let mut state =
                            GameState::from_triangle(board, p, None, vec![], 1, false, 0);
                        state.spin_mode = mode;
                        state.hold_used = true;
                        let mut children: Vec<_> = get_all_next_states(&state)
                            .into_iter()
                            .filter(|(s, _, _)| !s.game_over && s.combo > 0)
                            .map(|(s, _, _)| {
                                graph.id(&s.board).expect("missing legal clearing state")
                            })
                            .collect();
                        children.sort_unstable();
                        children.dedup();
                        assert_eq!(children, graph.edges[id][p as usize]);
                    }
                }
            }
        }
    }

    #[test]
    fn bag_boundaries_are_not_guessed() {
        // Seven distinct visible pieces alone do NOT reveal the boundary.
        assert_eq!(
            possible_tail_masks(&ALL_PIECES),
            vec![1, 3, 7, 15, 31, 63, 127]
        );
        // The repeated I pins a boundary; the following six finish that bag.
        let mut sequence = vec![Piece::I];
        sequence.extend(ALL_PIECES);
        assert_eq!(possible_tail_masks(&sequence), vec![FULL_BAG]);
        sequence.push(Piece::I);
        assert_eq!(possible_tail_masks(&sequence), vec![126]);
        assert!(possible_tail_masks(&[Piece::I, Piece::I, Piece::I]).is_empty());
        assert!(possible_tail_masks(&[Piece::I]).len() > 1);
    }

    #[test]
    fn one_step_value_matches_exhaustive_draw_and_empty_hold() {
        let graph = Graph::build();
        let values = graph.continuation_values(1);
        // On an empty four-column board only a horizontal I clears immediately.
        let b = graph.id(&Board::new(4)).unwrap();
        assert_eq!(values[index(b, Piece::I as usize, FULL_BAG)], 1.0);
        assert!((values[index(b, Piece::O as usize, FULL_BAG)] - 1.0 / 7.0).abs() < 1e-6);
        assert!((values[index(b, EMPTY_HOLD, FULL_BAG)] - 2.0 / 7.0).abs() < 1e-6);
        // With just O left, holding it refills the bag and gives one I chance.
        assert!((values[index(b, EMPTY_HOLD, 1 << Piece::O as usize)] - 1.0 / 7.0).abs() < 1e-6);
    }

    #[test]
    fn fixed_point_preserves_closed_cycles_and_eliminates_dead_ends() {
        let mut graph = Graph {
            boards: vec![7],
            edges: vec![std::array::from_fn(|_| vec![0])],
            ids: HashMap::from([(7, 0)]),
        };
        assert_eq!(graph.winning_with_one_preview()[3], 7 * 7 * 127);
        graph.edges[0] = Default::default();
        assert_eq!(graph.winning_with_one_preview(), [0; 7]);
    }

    #[test]
    fn lookup_never_overrides_pressure_or_unknown_rules() {
        let mut state = GameState::from_triangle(
            decode_board(7),
            Piece::I,
            Some(Piece::T),
            vec![Piece::O, Piece::S, Piece::Z, Piece::L, Piece::J],
            1,
            false,
            0,
        );
        let plan = choose(&state).unwrap();
        assert!(get_all_next_states(&state)
            .iter()
            .any(|(s, m, h)| !s.game_over && s.combo > 0 && (*m, *h) == plan.choice));
        state.pending_garbage = 1;
        assert!(choose(&state).is_none());
        state.pending_garbage = 0;
        state.board.spawn_height = 12;
        assert!(choose(&state).is_none());
        state.board.spawn_height = 26;
        state.board.width = 10;
        assert!(choose(&state).is_none());
    }

    #[test]
    fn solver_does_not_use_the_local_games_hidden_random_bag() {
        let mut a = GameState::new(4);
        a.board = decode_board(7);
        a.current = Piece::T;
        a.hold = Some(Piece::O);
        a.queue = vec![Piece::I, Piece::L, Piece::J, Piece::S, Piece::Z];
        a.combo = 4;
        let b = GameState::from_triangle(
            a.board,
            a.current,
            a.hold,
            a.queue.clone(),
            a.combo,
            a.b2b,
            0,
        );
        assert_eq!(choose(&a).map(|p| p.choice), choose(&b).map(|p| p.choice));
    }
}
