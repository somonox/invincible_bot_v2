use four_wide_bot::{
    engine::{
        board::Board,
        header::{Piece, SpinMode},
        state::GameState,
    },
    rl::{
        agent::get_all_next_states,
        features::Weights,
        search::{
            find_funny_move, find_funny_move_with_history, find_hybrid_move_with_expert, Evaluator,
            FunnyHistory, HybridMode,
        },
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
fn funny_takes_a_pc_as_b2b_progress_and_a_clean_field() {
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
    assert!(normal.last_perfect_clear && normal.b2b);
    assert!(selected.b2b && !selected.game_over);
    assert!(selected.last_perfect_clear);
    assert_eq!(selected.b2b_level, 21);
    assert_eq!(selected.lines_cleared, 2);
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
fn funny_cancels_ready_garbage_with_a_b2b_increasing_pc() {
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
    assert_eq!(next.b2b_level, 21);
    assert!(matches!(
        plan.mode,
        HybridMode::FunnyB2b { defending: true }
    ));
}

#[test]
fn funny_still_preserves_b2b_when_an_ordinary_clear_is_not_a_pc() {
    let state = position(&[3, 3, 1], Piece::O, Piece::T);
    let plan = find_funny_move(&state, None, Evaluator::Static(&Weights::default()), 1).unwrap();
    let candidates = get_all_next_states(&state);
    assert!(candidates
        .iter()
        .any(|(s, _, _)| s.lines_cleared > 0 && !s.last_perfect_clear && !s.b2b));
    let next = &candidates
        .iter()
        .find(|(_, m, h)| (*m, *h) == plan.choice)
        .unwrap()
        .0;
    assert!(next.b2b && !next.game_over);
    assert_eq!(next.b2b_level, 20);
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

#[test]
fn funny_builds_a_roof_then_recovers_it_with_executable_spins() {
    use four_wide_bot::engine::movegen::find_input_path;
    let mut state = position(&[7, 7, 3], Piece::J, Piece::T);
    state.spin_mode = SpinMode::Handheld;
    state.pc_bonus = 5;
    state.combo = 1;
    state.b2b = false;
    state.b2b_level = 0;
    state.b2b_charge = 0;
    state.queue = vec![Piece::S, Piece::Z, Piece::I, Piece::O, Piece::T];
    let weights = Weights::default();
    assert_eq!(state.board.holes_count(), 0);
    for turn in 0..3 {
        let choice = find_funny_move(&state, None, Evaluator::Static(&weights), 6)
            .unwrap()
            .choice;
        assert!(find_input_path(&state.board, choice.0, state.spin_mode).is_some());
        if choice.1 {
            assert!(state.hold());
        }
        state.do_move(choice.0);
        assert!(!state.game_over);
        if turn == 0 {
            assert_eq!(state.combo, 0, "the useful roof starts with a non-clear");
            assert!(
                state.board.holes_count() > 0,
                "do not ban deliberate overhangs"
            );
        }
    }
    assert_eq!(state.board.holes_count(), 0);
    assert_eq!(state.board.cell_coveredness(), 0);
    assert!(
        state.b2b_level >= 2,
        "the roof must pay back through actual B2B clears"
    );
}

#[test]
fn aged_hole_free_walls_are_recovered_on_either_side() {
    // A small B2B sacrifice must become preferable to repeatedly preserving a
    // twelve-row wall. Mirror the board so this cannot just swap left/right.
    for row in [3, 12] {
        let mut state = position(&[row; 12], Piece::O, Piece::O);
        state.pc_bonus = 5;
        let mut history = FunnyHistory::default();
        for pieces in 0..32 {
            state.pieces_placed = pieces;
            history.observe(&state);
        }
        assert!(history.unpaid() > 0, "solid shelves must carry debt too");
        let plan = find_funny_move_with_history(
            &state,
            None,
            Evaluator::Static(&Weights::default()),
            1,
            &mut history,
        )
        .unwrap();
        let next = get_all_next_states(&state)
            .into_iter()
            .find(|(_, m, h)| (*m, *h) == plan.choice)
            .unwrap()
            .0;
        assert_eq!(next.lines_cleared, 2);
        assert_eq!(next.board.highest_row(), 10);
        assert!(!next.b2b);
    }
}

#[test]
fn history_counts_placements_not_requests_and_repays_real_recovery() {
    let mut state = position(&[3; 12], Piece::O, Piece::O);
    let mut history = FunnyHistory::default();
    history.observe(&state);
    state.pieces_placed = 1;
    history.observe(&state);
    let debt = history.unpaid();
    assert!(debt > 0);
    for _ in 0..10 {
        history.observe(&state);
    }
    assert_eq!(history.unpaid(), debt);
    state.pieces_placed = 2;
    state.combo = 1; // A spin/clear that leaves the wall is not recovery.
    history.observe(&state);
    assert!(history.unpaid() > debt);
    state.board.rows.fill(0);
    state.pieces_placed = 3;
    history.observe(&state);
    assert_eq!(history.unpaid(), 0);
    state.board.rows[..12].fill(3);
    state.pieces_placed = 4;
    history.observe(&state);
    assert!(history.unpaid() > 0);
    state.pieces_placed = 0; // New round.
    history.observe(&state);
    assert_eq!(history.unpaid(), 0);
    state.pieces_placed = 8; // Missing intermediate observations are not invented.
    history.observe(&state);
    assert_eq!(history.unpaid(), 0);
}
