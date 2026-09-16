use four_wide_bot::engine::{
    board::Board,
    header::{Move, Piece, ALL_PIECES},
    movegen::generate_moves,
    state::GameState,
};
use four_wide_bot::rl::{
    agent::{find_best_move, find_best_move_meta, get_all_next_states},
    features::Weights,
    meta_agent::MetaPolicyNetwork,
};

fn state(rows: &[u16], current: Piece, hold: Option<Piece>, queue: &[Piece]) -> GameState {
    let mut board = Board::new(4);
    board.rows[..rows.len()].copy_from_slice(rows);
    GameState::from_triangle(board, current, hold, queue.to_vec(), 30, false, 0)
}

fn chosen_next(state: &GameState, choice: (Move, bool)) -> GameState {
    get_all_next_states(state)
        .into_iter()
        .find(|(_, m, h)| (*m, *h) == choice)
        .unwrap()
        .0
}

// Independent exhaustive goal oracle: no evaluation function or beam ranking.
fn earliest_pc(state: &GameState, depth: usize) -> Option<usize> {
    if depth == 0 {
        return None;
    }
    get_all_next_states(state)
        .into_iter()
        .filter(|(s, _, _)| !s.game_over)
        .filter_map(|(s, _, _)| {
            if s.last_perfect_clear {
                Some(1)
            } else {
                earliest_pc(&s, depth - 1).map(|n| n + 1)
            }
        })
        .min()
}

#[test]
fn selects_a_pc_even_with_extreme_combo_weights() {
    let state = state(
        &[3, 3],
        Piece::I,
        Some(Piece::O),
        &[Piece::T, Piece::S, Piece::Z],
    );
    let mut weights = Weights::default();
    weights.combo_reward = 1_000_000.0;
    let choice = find_best_move(&state, None, &weights, 3).unwrap();
    assert!(choice.1, "must hold I and complete the board with O");
    let next = chosen_next(&state, choice);
    assert!(next.last_perfect_clear);
    assert_eq!(next.board.highest_row(), 0);
}

#[test]
fn accepts_a_combo_break_to_set_up_a_two_piece_pc() {
    let state = state(
        &[],
        Piece::O,
        Some(Piece::T),
        &[Piece::O, Piece::S, Piece::Z],
    );
    let choice = find_best_move_meta(&state, None, &MetaPolicyNetwork::default(), 2).unwrap();
    let next = chosen_next(&state, choice);
    assert_eq!(next.combo, 0);
    assert!(!next.last_perfect_clear);
    assert_eq!(earliest_pc(&next, 1), Some(1));
}

#[test]
fn pc_choices_match_short_exhaustive_solutions() {
    let net = MetaPolicyNetwork::default();
    let mut checked = 0;
    for rows in [&[][..], &[3, 3][..], &[7, 1][..], &[5, 5][..]] {
        for current in ALL_PIECES {
            let state = state(
                rows,
                current,
                Some(Piece::I),
                &[Piece::O, Piece::S, Piece::T],
            );
            if let Some(expected) = earliest_pc(&state, 3) {
                let choice = find_best_move_meta(&state, None, &net, 3).unwrap();
                let next = chosen_next(&state, choice);
                let actual = if next.last_perfect_clear {
                    Some(1)
                } else {
                    earliest_pc(&next, 2).map(|n| n + 1)
                };
                assert_eq!(actual, Some(expected), "rows={rows:?} current={current:?}");
                checked += 1;
            }
        }
    }
    assert!(checked >= 10);
}

#[test]
fn battle_and_search_count_and_reward_the_same_pc_event() {
    let mut solo = state(&[3, 3], Piece::O, None, &[Piece::O, Piece::T, Piece::S]);
    solo.combo = 0;
    let mut battle = solo.clone();
    let mut opponent = state(&[], Piece::T, None, &[Piece::I]);
    let m = generate_moves(&solo.board, Piece::O)
        .into_iter()
        .find(|m| {
            let mut board = solo.board;
            board.place(m.piece, m.rotation, m.x, m.y);
            board.clear_lines();
            board.highest_row() == 0
        })
        .unwrap();
    solo.do_move(m);
    let (_, attack) = battle.do_move_battle(m, &mut opponent);
    assert_eq!(solo.perfect_clears, 1);
    assert_eq!(battle.perfect_clears, 1);
    assert!(solo.last_perfect_clear && battle.last_perfect_clear);
    assert_eq!(attack, 11); // double + 10-line PC bonus
    assert_eq!(attack, solo.last_attack);
    assert_eq!(attack, battle.last_attack);
    assert_eq!(opponent.queued_garbage, attack);
    let setup = generate_moves(&solo.board, solo.current)[0];
    solo.do_move(setup);
    assert!(!solo.last_perfect_clear);
    assert_eq!(solo.perfect_clears, 1);
}

#[test]
fn starting_empty_is_not_a_perfect_clear_event() {
    let mut state = state(&[], Piece::O, None, &[Piece::T]);
    assert_eq!(state.perfect_clears, 0);
    let m = generate_moves(&state.board, state.current)[0];
    state.do_move(m);
    assert_eq!(state.perfect_clears, 0);
    assert!(!state.last_perfect_clear);
}
