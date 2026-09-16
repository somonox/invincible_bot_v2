use four_wide_bot::engine::{
    board::{Board, BOARD_HEIGHT},
    header::{Move, Piece, Rotation, ALL_PIECES},
    movegen::{generate_moves, SearchState},
    piece::{get_piece_cells, get_srs_kicks, get_srs_kicks_i},
    state::GameState,
};
use four_wide_bot::rl::{
    agent::{find_best_move, find_best_move_meta, get_all_next_states},
    features::Weights,
    meta_agent::MetaPolicyNetwork,
};
use std::collections::{HashSet, VecDeque};

fn position(rows: &[u16], current: Piece, hold: Option<Piece>, queue: &[Piece]) -> GameState {
    let mut board = Board::new(4);
    board.rows[..rows.len()].copy_from_slice(rows);
    GameState::from_triangle(board, current, hold, queue.to_vec(), 8, false, 0)
}

fn placement(m: Move) -> [i32; 4] {
    let mut cells = get_piece_cells(m.piece, m.rotation).map(|c| (m.y + c.y) * 16 + m.x + c.x);
    cells.sort_unstable();
    cells
}

// Deliberately slow reference: full spawn-height BFS, hash-set visitation, and
// no array indexing, sky shortcut, queue cap or rotation deduplication.
fn reference_placements(board: &Board, piece: Piece) -> HashSet<[i32; 4]> {
    let highest = board.highest_row() as i32;
    let start = SearchState {
        rotation: Rotation::North,
        x: board.width as i32 / 2 - 1,
        y: if highest >= 20 {
            (highest + 1).min(BOARD_HEIGHT as i32 - 3)
        } else {
            20
        },
    };
    let mut found = HashSet::new();
    if !board.fits(piece, start.rotation, start.x, start.y) {
        return found;
    }
    let mut queue = VecDeque::from([start]);
    let mut seen = HashSet::from([start]);
    while let Some(s) = queue.pop_front() {
        if !board.fits(piece, s.rotation, s.x, s.y - 1) {
            found.insert(placement(Move::new(piece, s.rotation, s.x, s.y)));
        }
        let mut next = vec![
            SearchState { x: s.x - 1, ..s },
            SearchState { x: s.x + 1, ..s },
            SearchState { y: s.y - 1, ..s },
        ];
        for rotation in [
            s.rotation.rotate_cw(),
            s.rotation.rotate_ccw(),
            s.rotation.rotate_180(),
        ] {
            let kicks = if piece == Piece::I {
                get_srs_kicks_i(s.rotation, rotation)
            } else {
                get_srs_kicks(s.rotation, rotation)
            };
            for &(dx, dy) in kicks {
                let candidate = SearchState {
                    rotation,
                    x: s.x + dx,
                    y: s.y + dy,
                };
                if board.fits(piece, rotation, candidate.x, candidate.y) {
                    next.push(candidate);
                    break;
                }
            }
        }
        for candidate in next {
            if (-2..BOARD_HEIGHT as i32 + 2).contains(&candidate.y)
                && board.fits(piece, candidate.rotation, candidate.x, candidate.y)
                && seen.insert(candidate)
            {
                queue.push_back(candidate);
            }
        }
    }
    found
}

#[test]
fn fast_movegen_matches_full_spawn_bfs_without_duplicate_placements() {
    for width in [4, 10] {
        let mut boards = vec![Board::new(width)];
        for rows in [
            vec![3, 1],
            vec![11, 9, 1],
            vec![7, 5, 4],
            vec![13, 9, 8],
            vec![14; 22],
        ] {
            let mut board = Board::new(width);
            board.rows[..rows.len()].copy_from_slice(&rows);
            boards.push(board);
        }
        for board in boards {
            for piece in ALL_PIECES {
                let actual = generate_moves(&board, piece);
                let unique: HashSet<_> = actual.iter().copied().map(placement).collect();
                let spin_unique: HashSet<_> =
                    actual.iter().map(|m| (placement(*m), m.spin)).collect();
                assert_eq!(spin_unique.len(), actual.len(), "duplicate {piece:?}");
                assert_eq!(
                    unique,
                    reference_placements(&board, piece),
                    "width={width} piece={piece:?} rows={:?}",
                    board.rows
                );
            }
        }
    }
}

#[test]
fn search_never_invents_hidden_pieces() {
    let state = position(&[3, 1], Piece::T, Some(Piece::O), &[Piece::I, Piece::S]);
    let first = get_all_next_states(&state);
    let second = get_all_next_states(&state);
    assert_eq!(first.len(), second.len());
    for ((a, ma, ha), (b, mb, hb)) in first.iter().zip(&second) {
        assert_eq!((ma, ha), (mb, hb));
        assert_eq!(a.board, b.board);
        assert_eq!(a.current, Piece::I);
        assert_eq!(a.queue, vec![Piece::S]);
        assert_eq!(a.queue, b.queue);
    }
    assert_eq!(state.queue, vec![Piece::I, Piece::S]);
}

#[test]
fn empty_hold_consumes_preview_and_exhaustion_is_a_leaf() {
    let mut state = position(&[], Piece::T, None, &[Piece::I]);
    assert!(state.hold());
    assert_eq!(state.current, Piece::I);
    assert!(state.queue.is_empty());
    let m = generate_moves(&state.board, state.current)[0];
    state.do_move(m);
    assert!(!state.has_known_current());
    assert!(!state.game_over);
    assert!(get_all_next_states(&state).is_empty());
    assert!(!state.hold());
    let mut no_preview = position(&[], Piece::T, None, &[]);
    assert!(!no_preview.hold());
    assert!(find_best_move_meta(&no_preview, None, &MetaPolicyNetwork::default(), 6).is_some());
}

// Exhaustive continuation oracle: no heuristic, beam or learned weights.
fn longest_chain(state: &GameState, depth: usize) -> usize {
    if depth == 0 {
        return 0;
    }
    get_all_next_states(state)
        .into_iter()
        .filter(|(s, _, _)| !s.game_over && s.combo > 0)
        .map(|(s, _, _)| 1 + longest_chain(&s, depth - 1))
        .max()
        .unwrap_or(0)
}

#[test]
fn selected_moves_preserve_exhaustively_verified_clear_chains() {
    let net = MetaPolicyNetwork::default();
    for rows in [&[3, 1][..], &[7][..], &[11, 9, 1][..], &[][..]] {
        for current in ALL_PIECES {
            let state = position(
                rows,
                current,
                Some(Piece::I),
                &[Piece::O, Piece::S, Piece::T],
            );
            let expected = longest_chain(&state, 3);
            let choice = four_wide_bot::rl::search::find_best_move_for_objective(
                &state,
                None,
                four_wide_bot::rl::search::Evaluator::Meta(&net),
                3,
                four_wide_bot::rl::search::Objective::Combo,
            )
            .unwrap();
            let (next, _, _) = get_all_next_states(&state)
                .into_iter()
                .find(|(_, m, h)| (*m, *h) == choice)
                .unwrap();
            let actual = if next.combo > 0 {
                1 + longest_chain(&next, 2)
            } else {
                0
            };
            assert_eq!(actual, expected, "rows={rows:?} piece={current:?}");
        }
    }
}

#[test]
fn perfect_clear_keeps_the_preview_for_the_next_turn() {
    let state = position(&[], Piece::I, Some(Piece::O), &[Piece::I, Piece::O]);
    assert_eq!(longest_chain(&state, 2), 2);
    let choice = find_best_move_meta(&state, None, &MetaPolicyNetwork::default(), 2).unwrap();
    let (next, _, _) = get_all_next_states(&state)
        .into_iter()
        .find(|(_, m, h)| (*m, *h) == choice)
        .unwrap();
    assert_eq!(next.board.highest_row(), 0);
    assert_eq!(longest_chain(&next, 1), 1);
}

#[test]
fn repeated_search_and_zero_meta_network_agree_with_static_evaluator() {
    let state = position(
        &[3, 1],
        Piece::S,
        None,
        &[Piece::O, Piece::T, Piece::I, Piece::Z, Piece::L],
    );
    let expected = find_best_move(&state, None, &Weights::default(), 6);
    for _ in 0..3 {
        assert_eq!(
            find_best_move_meta(&state, None, &MetaPolicyNetwork::default(), 6),
            expected
        );
    }
}

#[test]
fn local_game_keeps_refilling_but_search_cannot_see_its_bag() {
    let mut local = GameState::new(4);
    let preview = local.queue.clone();
    for (next, _, used_hold) in get_all_next_states(&local) {
        assert!(next.queue.len() <= preview.len() - 1);
        if !used_hold {
            assert_eq!(next.queue, preview[1..]);
        }
    }
    let m = generate_moves(&local.board, local.current)[0];
    local.do_move(m);
    assert_eq!(local.queue.len(), 5);
    assert!(local.has_known_current());
}
