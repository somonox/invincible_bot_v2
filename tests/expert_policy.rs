use four_wide_bot::{
    engine::{board::Board, header::Piece, state::GameState},
    rl::{
        agent::get_all_next_states,
        combo_solver,
        features::Weights,
        search::{find_hybrid_move_with_expert, Evaluator, HybridMode},
    },
};

fn fixture(rows: &[u16], current: Piece, hold: Piece, garbage: u32) -> GameState {
    let mut board = Board::new(4);
    board.spawn_height = 26;
    board.rows[..rows.len()].copy_from_slice(rows);
    let mut state = GameState::from_triangle(
        board,
        current,
        Some(hold),
        vec![Piece::O, Piece::I, Piece::S, Piece::Z, Piece::L],
        12,
        true,
        garbage,
    );
    state.b2b_level = 20;
    state.b2b_charge = 20;
    state.pc_bonus = 1000;
    state
}

// Independent exhaustive oracle: count uninterrupted clears, never attack.
fn chain(state: &GameState, depth: usize) -> usize {
    if depth == 0 {
        return 0;
    }
    get_all_next_states(state)
        .into_iter()
        .filter(|(s, _, _)| !s.game_over && s.combo > 0)
        .map(|(s, _, _)| 1 + chain(&s, depth - 1))
        .max()
        .unwrap_or(0)
}

#[test]
fn expert_keeps_clear_chains_outside_the_table_and_under_garbage_pressure() {
    let weights = Weights::default();
    for (rows, current, hold) in [
        (&[7, 7, 7, 7, 3, 1][..], Piece::T, Piece::I),
        (&[3, 3][..], Piece::I, Piece::O),
        (&[7, 7][..], Piece::T, Piece::I),
    ] {
        for garbage in [0, 8] {
            let state = fixture(rows, current, hold, garbage);
            let expected = chain(&state, 3);
            assert!(expected > 0);
            let plan =
                find_hybrid_move_with_expert(&state, None, Evaluator::Static(&weights), 3, true)
                    .unwrap();
            let next = get_all_next_states(&state)
                .into_iter()
                .find(|(_, m, h)| (*m, *h) == plan.choice)
                .unwrap()
                .0;
            assert!(
                next.combo > 0,
                "expert broke an available clear chain: {rows:?}, garbage={garbage}"
            );
            assert_eq!(next.last_received_garbage, 0);
            assert_eq!(1 + chain(&next, 2), expected);
            assert!(
                matches!(plan.mode,HybridMode::ExpertCombo { defending, .. } if defending == (garbage>0))
            );
            if garbage > 0 || rows.len() > 2 {
                assert!(matches!(
                    plan.mode,
                    HybridMode::ExpertCombo { table: false, .. }
                ));
            }
        }
    }
}

#[test]
fn online_sized_queue_actually_activates_expert_table_and_ignores_hidden_tail() {
    let mut state = fixture(&[7, 7], Piece::T, Piece::I, 0);
    state.queue = vec![Piece::O, Piece::S, Piece::Z, Piece::L, Piece::J];
    let expected = combo_solver::choose(&state).unwrap().choice;
    state.queue.extend([Piece::I; 16]);
    let plan = find_hybrid_move_with_expert(
        &state,
        None,
        Evaluator::Static(&Weights::default()),
        6,
        true,
    )
    .unwrap();
    assert_eq!(plan.choice, expected);
    assert!(matches!(
        plan.mode,
        HybridMode::ExpertCombo {
            table: true,
            defending: false
        }
    ));
}

#[test]
fn expert_can_prepare_when_no_immediate_clear_exists() {
    let state = fixture(&[], Piece::O, Piece::T, 0);
    assert_eq!(chain(&state, 1), 0);
    let plan = find_hybrid_move_with_expert(
        &state,
        None,
        Evaluator::Static(&Weights::default()),
        3,
        true,
    )
    .unwrap();
    assert!(matches!(
        plan.mode,
        HybridMode::ExpertCombo { table: false, .. }
    ));
    assert!(get_all_next_states(&state)
        .iter()
        .any(|(s, m, h)| !s.game_over && (*m, *h) == plan.choice));
}
