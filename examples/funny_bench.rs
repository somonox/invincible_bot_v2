//! Deterministic solo survival/B2B benchmark with five visible previews.
use four_wide_bot::{
    engine::{board::Board, header::ALL_PIECES, state::GameState},
    rl::{
        meta_agent::MetaPolicyNetwork,
        search::{find_funny_move, Evaluator},
    },
};
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
use serde_json::json;
use std::time::Instant;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let games = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(8usize);
    let cap = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(300usize);
    let net = MetaPolicyNetwork::default();
    for seed in 0..games {
        let mut rng = StdRng::seed_from_u64(20260920 + seed as u64);
        let mut stream = Vec::new();
        while stream.len() < cap + 10 {
            let mut bag = ALL_PIECES;
            bag.shuffle(&mut rng);
            stream.extend(bag);
        }
        let mut board = Board::new(4);
        board.spawn_height = 26;
        let mut state =
            GameState::from_triangle(board, stream[0], None, stream[1..6].to_vec(), 0, false, 0);
        let (mut index, mut placed, mut peak, mut max_b2b, mut breaks) = (0, 0, 0, 0, 0);
        let start = Instant::now();
        while placed < cap {
            let Some(plan) = find_funny_move(&state, None, Evaluator::Meta(&net), 6) else {
                break;
            };
            let previous_b2b = state.b2b;
            if plan.choice.1 {
                if state.hold.is_none() {
                    index += 1;
                }
                assert!(state.hold());
            }
            state.do_move(plan.choice.0);
            placed += 1;
            index += 1;
            peak = peak.max(state.board.highest_row());
            max_b2b = max_b2b.max(state.b2b_level);
            breaks += usize::from(previous_b2b && !state.b2b);
            if state.game_over || state.board.highest_row() >= 26 {
                break;
            }
            state.queue = stream[index + 1..index + 6].to_vec();
        }
        println!(
            "{}",
            json!({"seed":seed,"placed":placed,"cap":cap,"peak_height":peak,"max_b2b":max_b2b,"b2b_breaks":breaks,"ms_per_move":start.elapsed().as_secs_f64()*1000.0/placed.max(1) as f64})
        );
    }
}
