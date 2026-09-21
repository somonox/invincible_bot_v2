//! Deterministic solo survival/B2B benchmark with five visible previews.
use four_wide_bot::{
    engine::{
        board::Board,
        header::{SpinMode, ALL_PIECES},
        movegen::find_input_path,
        state::{GameState, GarbagePacket},
    },
    rl::{
        agent::get_all_next_states,
        meta_agent::MetaPolicyNetwork,
        search::{find_funny_move_with_history, funny_survival_risk, Evaluator, FunnyHistory},
    },
};
use rand::{rngs::StdRng, seq::SliceRandom, SeedableRng};
use serde_json::json;
use std::time::Instant;

fn main() {
    let args: Vec<_> = std::env::args().collect();
    let games = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(8usize);
    let cap = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(300usize);
    let mode = args.get(3).map(String::as_str).unwrap_or("all");
    let garbage = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0u32);
    let offset = args.get(5).and_then(|s| s.parse().ok()).unwrap_or(0usize);
    let executable = args.get(6).is_some_and(|s| s == "executable");
    let net = MetaPolicyNetwork::default();
    for seed in offset..offset + games {
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
        state.spin_mode = if mode == "handheld" {
            SpinMode::Handheld
        } else {
            SpinMode::All
        };
        state.pc_bonus = 5;
        state.garbage_packets = Some(vec![]);
        state.frames_per_piece = 12;
        state.next_lock_frames = 12;
        let (mut index, mut placed, mut peak, mut max_b2b, mut breaks) = (0, 0, 0, 0, 0);
        let (mut b2b_clears, mut attack, mut canceled) = (0, 0, 0);
        let mut unreachable_plans = 0;
        let mut first_unreachable = None;
        let (
            mut setup_streak,
            mut max_setup_streak,
            mut covered_sum,
            mut height_sum,
            mut max_holes,
        ) = (0usize, 0usize, 0u64, 0usize, 0u32);
        let mut search_times = Vec::new();
        let mut history = FunnyHistory::default();
        let start = Instant::now();
        while placed < cap {
            if garbage > 0 && placed > 0 && placed % 12 == 0 {
                state.garbage_packets.as_mut().unwrap().push(GarbagePacket {
                    amount: garbage,
                    ready_in: 36,
                });
                state.sync_garbage_totals();
                state.last_garbage_hole_x = placed / 12 % 4;
            }
            let search_start = Instant::now();
            let plan =
                find_funny_move_with_history(&state, None, Evaluator::Meta(&net), 6, &mut history);
            search_times.push(search_start.elapsed().as_secs_f64() * 1000.0);
            let Some(plan) = plan else {
                break;
            };
            let mut choice = plan.choice;
            if executable && find_input_path(&state.board, choice.0, state.spin_mode).is_none() {
                unreachable_plans += 1;
                if first_unreachable.is_none() {
                    first_unreachable = Some(
                        json!({"rows": &state.board.rows[..state.board.highest_row()], "current": format!("{:?}", state.current), "hold": format!("{:?}", state.hold), "queue": format!("{:?}", state.queue), "move": format!("{:?}", choice), "placed": placed}),
                    );
                }
                // Match the adapter's existing one-ply Funny fallback. Evaluating
                // impossible placements as if executed overstates B2B continuity.
                let evaluator = Evaluator::Meta(&net);
                let mut candidates = get_all_next_states(&state);
                candidates.retain(|(s, _, _)| !s.game_over);
                candidates.sort_by(|(a, _, _), (b, _, _)| {
                    let safety = |s: &GameState| (funny_survival_risk(s), s.last_received_garbage);
                    let tie = |s: &GameState| {
                        (
                            s.b2b_level,
                            s.last_canceled_garbage,
                            s.last_perfect_clear,
                            s.combo > 0,
                            s.last_attack,
                        )
                    };
                    safety(a)
                        .cmp(&safety(b))
                        .then_with(|| {
                            evaluator
                                .funny_next_value(&state, b, history.unpaid())
                                .total_cmp(&evaluator.funny_next_value(&state, a, history.unpaid()))
                        })
                        .then_with(|| tie(b).cmp(&tie(a)))
                });
                let Some((_, m, h)) = candidates
                    .into_iter()
                    .find(|(_, m, _)| find_input_path(&state.board, *m, state.spin_mode).is_some())
                else {
                    break;
                };
                choice = (m, h);
            }
            let previous_b2b = state.b2b;
            if choice.1 {
                if state.hold.is_none() {
                    index += 1;
                }
                assert!(state.hold());
            }
            state.do_move(choice.0);
            attack += state.last_attack;
            canceled += state.last_canceled_garbage;
            b2b_clears += usize::from(state.b2b && state.combo > 0);
            placed += 1;
            setup_streak = if state.combo == 0 {
                setup_streak + 1
            } else {
                0
            };
            max_setup_streak = max_setup_streak.max(setup_streak);
            covered_sum += (state.board.holes_count() + state.board.cell_coveredness()) as u64;
            height_sum += state.board.highest_row();
            max_holes = max_holes.max(state.board.holes_count());
            index += 1;
            peak = peak.max(state.board.highest_row());
            max_b2b = max_b2b.max(state.b2b_level);
            breaks += usize::from(previous_b2b && !state.b2b);
            if state.game_over || state.board.highest_row() >= 26 {
                break;
            }
            state.queue = stream[index + 1..index + 6].to_vec();
        }
        search_times.sort_by(f64::total_cmp);
        let percentile = |p: usize| {
            search_times
                .get(search_times.len().saturating_sub(1) * p / 100)
                .copied()
                .unwrap_or(0.0)
        };
        println!(
            "{}",
            json!({"seed":seed,"mode":mode,"garbage_per_12":garbage,"placed":placed,"cap":cap,"peak_height":peak,"max_b2b":max_b2b,"b2b_breaks":breaks,"b2b_clears":b2b_clears,"attack":attack,"canceled":canceled,"executable":executable,"unreachable_plans":unreachable_plans,"first_unreachable":first_unreachable,"max_setup_streak":max_setup_streak,"covered_sum":covered_sum,"height_sum":height_sum,"max_holes":max_holes,"ms_per_move":start.elapsed().as_secs_f64()*1000.0/placed.max(1) as f64,"p95_search_ms":percentile(95),"p99_search_ms":percentile(99)})
        );
    }
}
