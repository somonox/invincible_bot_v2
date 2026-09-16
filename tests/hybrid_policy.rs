use four_wide_bot::engine::{board::Board, header::Piece, state::GameState};
use four_wide_bot::rl::{
    agent::get_all_next_states,
    features::Weights,
    search::{
        find_best_move_for_objective, find_hybrid_move, pc_residue_possible, Evaluator, HybridMode,
        Objective,
    },
};

fn state(rows: &[u16], piece: Piece, queue: &[Piece]) -> GameState {
    let mut board = Board::new(4);
    board.rows[..rows.len()].copy_from_slice(rows);
    GameState::from_triangle(board, piece, Some(Piece::T), queue.to_vec(), 8, false, 0)
}
fn opponent(combo: u32) -> GameState {
    let mut s = state(&[], Piece::T, &[Piece::I]);
    s.combo = combo + 1;
    s
}
fn chosen(s: &GameState, c: (four_wide_bot::engine::header::Move, bool)) -> GameState {
    get_all_next_states(s)
        .into_iter()
        .find(|(_, m, h)| (*m, *h) == c)
        .unwrap()
        .0
}
#[test]
fn impossible_residue_uses_exact_combo_policy_even_with_pending_garbage() {
    for pending in [0, 3, 8] {
        let mut s = state(&[3, 1], Piece::T, &[Piece::I, Piece::S, Piece::O]);
        s.pending_garbage = pending;
        let w = Weights::default();
        let opp = opponent(0);
        let hybrid = find_hybrid_move(&s, Some(&opp), Evaluator::Static(&w), 3, 6).unwrap();
        assert_eq!(hybrid.mode, HybridMode::ComboResidue);
        assert_eq!(
            hybrid.choice,
            find_best_move_for_objective(
                &s,
                Some(&opp),
                Evaluator::Static(&w),
                3,
                Objective::Combo
            )
            .unwrap()
        );
        assert!(chosen(&s, hybrid.choice).combo > 0);
    }
}
#[test]
fn low_opponent_combo_allows_a_two_move_pc_setup() {
    let s = state(&[], Piece::O, &[Piece::O, Piece::S]);
    let w = Weights::default();
    for combo in [0, 5] {
        let opp = opponent(combo);
        let hybrid = find_hybrid_move(&s, Some(&opp), Evaluator::Static(&w), 2, 6).unwrap();
        assert_eq!(hybrid.mode, HybridMode::PerfectClear { placements: 2 });
        let next = chosen(&s, hybrid.choice);
        assert!(!next.last_perfect_clear);
        assert!(get_all_next_states(&next)
            .iter()
            .any(|(s, _, _)| s.last_perfect_clear));
    }
}
#[test]
fn high_opponent_combo_switches_to_combo_at_the_exact_threshold() {
    let s = state(&[], Piece::O, &[Piece::O, Piece::S]);
    let w = Weights::default();
    for combo in [6, 20] {
        let opp = opponent(combo);
        let hybrid = find_hybrid_move(&s, Some(&opp), Evaluator::Static(&w), 2, 6).unwrap();
        assert_eq!(hybrid.mode, HybridMode::ComboPressure);
        assert_eq!(
            hybrid.choice,
            find_best_move_for_objective(
                &s,
                Some(&opp),
                Evaluator::Static(&w),
                2,
                Objective::Combo
            )
            .unwrap()
        );
    }
}
#[test]
fn immediate_pc_does_not_override_full_horizon_combo_under_pressure() {
    let s = state(&[3, 3], Piece::O, &[Piece::S]);
    let w = Weights::default();
    let opp = opponent(20);
    assert!(get_all_next_states(&s)
        .iter()
        .any(|(s, _, _)| s.last_perfect_clear));
    let hybrid = find_hybrid_move(&s, Some(&opp), Evaluator::Static(&w), 2, 6).unwrap();
    assert_eq!(hybrid.mode, HybridMode::ComboPressure);
    assert_eq!(
        hybrid.choice,
        find_best_move_for_objective(&s, Some(&opp), Evaluator::Static(&w), 2, Objective::Combo)
            .unwrap()
    );
}
#[test]
fn failed_pc_probe_falls_back_to_combo_instead_of_pc_board_evaluation() {
    let mut s = state(&[3, 3], Piece::T, &[Piece::S]);
    s.hold_used = true;
    let w = Weights::default();
    let opp = opponent(0);
    assert!(pc_residue_possible(&s));
    assert!(get_all_next_states(&s)
        .iter()
        .all(|(s, _, _)| !s.last_perfect_clear));
    let hybrid = find_hybrid_move(&s, Some(&opp), Evaluator::Static(&w), 1, 6).unwrap();
    assert_eq!(hybrid.mode, HybridMode::ComboNoVisiblePc);
    assert_eq!(
        hybrid.choice,
        find_best_move_for_objective(&s, Some(&opp), Evaluator::Static(&w), 1, Objective::Combo)
            .unwrap()
    );
}
#[test]
fn garbage_and_opponent_changes_are_reconsidered_each_turn() {
    let mut s = state(&[7], Piece::I, &[Piece::O, Piece::S]);
    assert!(!pc_residue_possible(&s));
    s.board.spawn_garbage(3, 0);
    assert!(pc_residue_possible(&s));
    let s = state(&[], Piece::O, &[Piece::O, Piece::S]);
    let w = Weights::default();
    let high = find_hybrid_move(&s, Some(&opponent(8)), Evaluator::Static(&w), 2, 6).unwrap();
    let low = find_hybrid_move(&s, Some(&opponent(1)), Evaluator::Static(&w), 2, 6).unwrap();
    assert_eq!(high.mode, HybridMode::ComboPressure);
    assert_eq!(low.mode, HybridMode::PerfectClear { placements: 2 });
}
#[test]
fn residue_check_uses_width_gcd_and_terminal_states_have_no_plan() {
    let mut s = state(&[1], Piece::I, &[Piece::O]);
    s.board.width = 10;
    assert!(!pc_residue_possible(&s));
    s.board.rows[0] = 3;
    assert!(pc_residue_possible(&s));
    s.game_over = true;
    assert!(find_hybrid_move(&s, None, Evaluator::Static(&Weights::default()), 3, 6).is_none());
}
