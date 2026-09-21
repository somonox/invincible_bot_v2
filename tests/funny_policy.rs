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

#[test]
fn funny_lowers_a_tall_field_even_when_it_breaks_b2b() {
    for height in [20, 26] {
        let mut state = position(&vec![3; height - 6], Piece::O, Piece::O);
        state.board.spawn_height = height as i32;
        let plan =
            find_funny_move(&state, None, Evaluator::Static(&Weights::default()), 6).unwrap();
        let next = get_all_next_states(&state)
            .into_iter()
            .find(|(_, m, h)| (*m, *h) == plan.choice)
            .unwrap()
            .0;
        assert!(!next.game_over);
        assert_eq!(next.lines_cleared, 2);
        assert!(!next.b2b);
        assert!(next.board.highest_row() < state.board.highest_row());
    }
}

#[test]
fn funny_keeps_b2b_while_lowering_a_tall_tetris_well() {
    let state = position(&[7; 22], Piece::I, Piece::O);
    let plan = find_funny_move(&state, None, Evaluator::Static(&Weights::default()), 6).unwrap();
    let next = get_all_next_states(&state)
        .into_iter()
        .find(|(_, m, h)| (*m, *h) == plan.choice)
        .unwrap()
        .0;
    assert_eq!(next.lines_cleared, 4);
    assert_eq!(next.b2b_level, 21);
    assert_eq!(next.board.highest_row(), 18);
}

#[test]
fn funny_reserves_headroom_for_garbage_that_has_not_arrived() {
    use four_wide_bot::engine::state::GarbagePacket;
    let mut state = position(&[3; 14], Piece::O, Piece::O);
    state.garbage_packets = Some(vec![GarbagePacket {
        amount: 6,
        ready_in: 600,
    }]);
    state.sync_garbage_totals();
    let plan = find_funny_move(&state, None, Evaluator::Static(&Weights::default()), 1).unwrap();
    let next = get_all_next_states(&state)
        .into_iter()
        .find(|(_, m, h)| (*m, *h) == plan.choice)
        .unwrap()
        .0;
    assert_eq!(next.last_received_garbage, 0);
    assert_eq!(next.lines_cleared, 2);
    assert!(next.last_canceled_garbage > 0);
}

#[test]
fn funny_prefers_a_clean_setup_over_a_spin_that_buries_more_holes() {
    // An immediate handheld spin is possible here, but caps extra holes.
    // Preserve the chain with an L setup instead of buying one more B2B
    // clear at the cost of a worse field for the following pieces.
    let mut state = position(&[14, 1, 9], Piece::L, Piece::S);
    state.spin_mode = SpinMode::Handheld;
    state.combo = 0;
    let plan = find_funny_move(&state, None, Evaluator::Static(&Weights::default()), 1).unwrap();
    let candidates = get_all_next_states(&state);
    let selected = &candidates
        .iter()
        .find(|(_, m, h)| (*m, *h) == plan.choice)
        .unwrap()
        .0;
    assert!(!selected.game_over && selected.b2b);
    assert_eq!(selected.b2b_level, state.b2b_level);
    assert_eq!(selected.board.holes_count(), 1);
    let evaluator = Evaluator::Static(&Weights::default());
    assert!(candidates.iter().any(|(next, _, _)| {
        !next.game_over
            && next.b2b_level > state.b2b_level
            && next.board.holes_count() > selected.board.holes_count()
            && four_wide_bot::rl::search::funny_survival_risk(next) == 0
            && evaluator.funny_position_value(selected) > evaluator.funny_position_value(next)
    }));
}
