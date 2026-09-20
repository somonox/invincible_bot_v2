use four_wide_bot::{
    engine::{
        board::Board,
        header::{Piece, SpinMode},
        state::GameState,
    },
    rl::{
        agent::get_all_next_states,
        features::Weights,
        search::{find_funny_move, find_hybrid_move_with_expert, Evaluator, HybridMode},
    },
};

fn position(rows: &[u16], current: Piece, hold: Piece) -> GameState {
    let mut board = Board::new(4);
    board.spawn_height = 26;
    board.rows[..rows.len()].copy_from_slice(rows);
    let mut state = GameState::from_triangle(board, current, Some(hold), vec![], 15, true, 0);
    state.b2b_level = 20;
    state.b2b_charge = 20;
    state.pc_bonus = 1000;
    state.spin_mode = SpinMode::None;
    state
}

#[test]
fn funny_preserves_b2b_instead_of_taking_a_high_damage_pc() {
    let state = position(&[3, 3], Piece::O, Piece::T);
    let weights = Weights::default();
    let funny = find_funny_move(&state, None, Evaluator::Static(&weights), 1).unwrap();
    let normal =
        find_hybrid_move_with_expert(&state, None, Evaluator::Static(&weights), 1, false).unwrap();
    let candidates = get_all_next_states(&state);
    let selected = &candidates
        .iter()
        .find(|(_, m, h)| (*m, *h) == funny.choice)
        .unwrap()
        .0;
    let normal = &candidates
        .iter()
        .find(|(_, m, h)| (*m, *h) == normal.choice)
        .unwrap()
        .0;
    assert!(normal.last_perfect_clear && !normal.b2b);
    assert!(selected.b2b && !selected.game_over);
    assert_eq!(selected.b2b_level, 20); // A setup preserves, but never grows, B2B.
    assert_eq!(selected.lines_cleared, 0);
    assert!(matches!(
        funny.mode,
        HybridMode::FunnyB2b { defending: false }
    ));
}

#[test]
fn funny_takes_an_available_b2b_clear_rather_than_only_stacking() {
    let state = position(&[7, 7, 7, 7], Piece::I, Piece::O);
    let plan = find_funny_move(&state, None, Evaluator::Static(&Weights::default()), 1).unwrap();
    let next = get_all_next_states(&state)
        .into_iter()
        .find(|(_, m, h)| (*m, *h) == plan.choice)
        .unwrap()
        .0;
    assert_eq!(next.lines_cleared, 4);
    assert_eq!(next.b2b_level, 21);
}

#[test]
fn funny_can_break_b2b_to_avoid_receiving_ready_garbage() {
    let mut state = position(&[3, 3], Piece::O, Piece::T);
    state.pending_garbage = 8;
    let plan = find_funny_move(&state, None, Evaluator::Static(&Weights::default()), 1).unwrap();
    let next = get_all_next_states(&state)
        .into_iter()
        .find(|(_, m, h)| (*m, *h) == plan.choice)
        .unwrap()
        .0;
    assert_eq!(next.last_received_garbage, 0);
    assert_eq!(next.last_canceled_garbage, 8);
    assert!(next.last_perfect_clear);
    assert!(matches!(
        plan.mode,
        HybridMode::FunnyB2b { defending: true }
    ));
}
