//! Offline, fixed-piece-stream benchmark. No TETR.IO account or network needed.
use four_wide_bot::engine::{board::Board, header::Piece, state::GameState};
use four_wide_bot::rl::{agent::find_best_move_meta, meta_agent::MetaPolicyNetwork};
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
use serde_json::json;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let games = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(8usize);
    let moves = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(80usize);
    let depth = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(6usize);
    let mode = args.get(4).map(String::as_str).unwrap_or("pc");
    let net = MetaPolicyNetwork::default();
    let mut decisions = std::collections::BTreeMap::<String, usize>::new();
    let opponent_combo = args.get(5).and_then(|s| s.parse::<u32>().ok());
    let mut opponent = GameState::new(4);
    if let Some(combo) = opponent_combo {
        opponent.combo = combo + 1;
    }
    let mut total_pcs = 0usize;
    let mut times = Vec::new();
    let mut records = Vec::new();
    let mut total_clears = 0usize;
    let mut total_breaks = 0usize;
    for seed in 0..games {
        let mut rng = StdRng::seed_from_u64(seed as u64 + 20260916);
        let mut stream = Vec::new();
        while stream.len() < moves + 16 {
            let mut bag = [
                Piece::I,
                Piece::O,
                Piece::T,
                Piece::L,
                Piece::J,
                Piece::S,
                Piece::Z,
            ];
            bag.shuffle(&mut rng);
            stream.extend(bag);
        }
        let mut board = Board::new(4);
        // Three-cell residue, as used in a four-wide combo well.
        if mode.ends_with("-residue") {
            board.rows[0] = 0b0011;
            board.rows[1] = 0b0001;
        }
        let mut pcs = 0;
        let mut hold = None;
        let mut combo = 0;
        let mut index = 0;
        let mut max_combo = 0;
        let mut placed = 0;
        let mut clears = 0;
        let mut breaks = 0;
        for _ in 0..moves {
            let mut state = GameState::from_triangle(
                board,
                stream[index],
                hold,
                stream[index + 1..index + 6].to_vec(),
                combo,
                false,
                0,
            );
            let start = Instant::now();
            let choice = if mode.starts_with("hybrid") {
                let plan = four_wide_bot::rl::search::find_hybrid_move(
                    &state,
                    opponent_combo.map(|_| &opponent),
                    four_wide_bot::rl::search::Evaluator::Meta(&net),
                    depth,
                );
                if let Some(plan) = plan {
                    *decisions.entry(plan.mode.label()).or_default() += 1;
                }
                plan.map(|p| p.choice)
            } else if mode.starts_with("pc") {
                find_best_move_meta(&state, None, &net, depth)
            } else {
                four_wide_bot::rl::search::find_best_move_for_objective(
                    &state,
                    None,
                    four_wide_bot::rl::search::Evaluator::Meta(&net),
                    depth,
                    four_wide_bot::rl::search::Objective::Combo,
                )
            };
            times.push(start.elapsed().as_secs_f64() * 1000.0);
            let Some((m, use_hold)) = choice else { break };
            if use_hold {
                if hold.is_none() {
                    index += 1;
                }
                state.hold();
            }
            let lines = state.do_move(m);
            placed += 1;
            pcs += usize::from(state.last_perfect_clear);
            if lines > 0 {
                clears += 1;
            }
            if combo > 0 && lines == 0 {
                breaks += 1;
            }
            max_combo = max_combo.max(state.combo);
            board = state.board;
            hold = state.hold;
            combo = state.combo;
            index += 1;
            if state.game_over {
                break;
            }
        }
        total_pcs += pcs;
        total_clears += clears;
        total_breaks += breaks;
        records.push(
            json!({"seed": seed, "placed": placed, "clear_moves": clears,
            "perfect_clears": pcs, "combo_breaks": breaks, "max_combo": max_combo}),
        );
        eprintln!(
            "seed={seed} pcs={pcs} placed={placed} clears={clears} breaks={breaks} max_combo={max_combo}"
        );
    }
    times.sort_by(f64::total_cmp);
    let n = times.len();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "games": records, "depth": depth, "decisions":decisions,"opponent_combo":opponent_combo, "searches": n, "objective": mode, "perfect_clears": total_pcs,
            "clear_moves": total_clears, "combo_breaks": total_breaks,
            "p50_ms": times[n / 2], "p95_ms": times[(n * 95 / 100).min(n - 1)],
            "mean_ms": times.iter().sum::<f64>() / n as f64,
        }))
        .unwrap()
    );
}
