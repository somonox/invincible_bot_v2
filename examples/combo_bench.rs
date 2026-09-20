//! Paired, deterministic 7-bag runs. Stop at FIRST break (no resets counted).
use four_wide_bot::{
    engine::{
        board::Board,
        header::{Piece, ALL_PIECES},
        state::GameState,
    },
    rl::{
        combo_solver,
        features::Weights,
        search::{find_beam_move_for_objective, Evaluator, Objective},
    },
};
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
use serde_json::json;
use std::time::Instant;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let games = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(100usize);
    let cap = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1000usize);
    let residue = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(3usize);
    let offset = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0u64);
    let mut results = vec![Vec::new(), Vec::new()];
    let mut times = vec![Vec::new(), Vec::new()];
    let mut fallback = 0;
    let weights = Weights::default();
    for seed in offset..offset + games as u64 {
        let mut rng = StdRng::seed_from_u64(7192026 + seed);
        let mut stream = Vec::new();
        while stream.len() < cap + 10 {
            let mut bag = ALL_PIECES;
            bag.shuffle(&mut rng);
            stream.extend(bag);
        }
        for mode in 0..2 {
            let mut board = Board::new(4);
            board.spawn_height = 26;
            board.rows[0] = 7;
            if residue == 6 {
                board.rows[1] = 7;
            }
            let mut hold: Option<Piece> = None;
            let mut index = 0;
            let mut length = 0;
            for _ in 0..cap {
                let state = GameState::from_triangle(
                    board,
                    stream[index],
                    hold,
                    stream[index + 1..index + 6].to_vec(),
                    length as u32 + 1,
                    false,
                    0,
                );
                let start = Instant::now();
                let choice = if mode == 1 {
                    combo_solver::choose(&state).map(|p| p.choice).or_else(|| {
                        fallback += 1;
                        find_beam_move_for_objective(
                            &state,
                            None,
                            Evaluator::Static(&weights),
                            6,
                            Objective::Combo,
                        )
                    })
                } else {
                    find_beam_move_for_objective(
                        &state,
                        None,
                        Evaluator::Static(&weights),
                        6,
                        Objective::Combo,
                    )
                };
                times[mode].push(start.elapsed().as_secs_f64() * 1000.0);
                let Some((m, use_hold)) = choice else {
                    break;
                };
                let mut next = state.clone();
                if use_hold {
                    assert!(next.hold());
                }
                next.do_move(m);
                if next.game_over || next.combo == 0 {
                    break;
                }
                index += 1 + usize::from(use_hold && hold.is_none());
                board = next.board;
                hold = next.hold;
                length += 1;
            }
            results[mode].push(length);
        }
        if (seed - offset + 1) % 25 == 0 {
            eprintln!("{}/{games}", seed - offset + 1);
        }
    }
    for mode in 0..2 {
        results[mode].sort_unstable();
        times[mode].sort_by(f64::total_cmp);
        println!(
            "{}",
            json!({"mode":if mode==0 {"beam"} else {"table"},"games":games,"cap":cap,"residue":residue,"seed_offset":offset,"mean":results[mode].iter().sum::<usize>() as f64/games as f64,"median":results[mode][games/2],"min":results[mode][0],"max":results[mode][games-1],"reached_cap":results[mode].iter().filter(|&&x|x==cap).count(),"mean_ms":times[mode].iter().sum::<f64>()/times[mode].len() as f64,"p99_ms":times[mode][times[mode].len()*99/100],"fallbacks":if mode==1 {fallback} else {0}})
        );
    }
}
