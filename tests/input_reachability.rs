use four_wide_bot::{
    engine::{
        board::Board,
        header::{Move, Piece, Rotation, Spin, SpinMode, ALL_PIECES},
        movegen::{
            find_input_path, generate_input_moves, generate_moves_with_rules, rotate, SearchState,
        },
        piece::get_piece_cells,
        state::GameState,
    },
    rl::{
        features::Weights,
        search::{find_funny_move, Evaluator},
    },
};
use std::collections::HashSet;

fn geometry(m: Move) -> ([i32; 4], u8) {
    let mut cells = get_piece_cells(m.piece, m.rotation).map(|c| (m.y + c.y) * 16 + m.x + c.x);
    cells.sort_unstable();
    (cells, m.spin as u8)
}

#[test]
fn sonic_moves_are_complete_and_replayable_without_midair_drops() {
    for height in [20, 26] {
        for rows in [&[0][..], &[11, 9, 1], &[14, 7, 12, 8, 12, 8, 12, 12]] {
            let mut b = Board::new(4);
            b.spawn_height = height;
            b.rows[..rows.len()].copy_from_slice(rows);
            for mode in [SpinMode::All, SpinMode::Handheld, SpinMode::None] {
                for piece in ALL_PIECES {
                    let expected: HashSet<_> = generate_moves_with_rules(&b, piece, mode)
                        .into_iter()
                        .filter(|m| find_input_path(&b, *m, mode).is_some())
                        .map(geometry)
                        .collect();
                    let actual = generate_input_moves(&b, piece, mode);
                    assert_eq!(
                        actual.iter().copied().map(geometry).collect::<HashSet<_>>(),
                        expected
                    );
                    // Alternate modes to exercise cache separation, then replay
                    // every emitted path with full drops, preserving final spin.
                    generate_moves_with_rules(&b, piece, mode);
                    assert_eq!(actual, generate_input_moves(&b, piece, mode));
                    for target in actual {
                        let path = find_input_path(&b, target, mode).expect("executable placement");
                        let mut s = SearchState {
                            rotation: Rotation::North,
                            x: 1,
                            y: height,
                        };
                        let mut spin = Spin::None;
                        for key in path {
                            match key.as_str() {
                                "rotateCW" | "rotateCCW" | "rotate180" => {
                                    let to = match key.as_str() {
                                        "rotateCW" => s.rotation.rotate_cw(),
                                        "rotateCCW" => s.rotation.rotate_ccw(),
                                        _ => s.rotation.rotate_180(),
                                    };
                                    (s, spin) = rotate(&b, piece, s, to, mode).unwrap();
                                }
                                "moveLeft" | "moveRight" => {
                                    s.x += if key == "moveLeft" { -1 } else { 1 };
                                    spin = Spin::None;
                                }
                                "softDrop" | "hardDrop" => {
                                    while b.fits(piece, s.rotation, s.x, s.y - 1) {
                                        s.y -= 1;
                                        spin = Spin::None;
                                    }
                                }
                                _ => panic!("unexpected input"),
                            }
                            assert!(b.fits(piece, s.rotation, s.x, s.y));
                        }
                        assert_eq!(
                            (s.rotation, s.x, s.y, spin),
                            (target.rotation, target.x, target.y, target.spin)
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn funny_does_not_plan_the_midair_s_spin_from_seed_500() {
    let mut board = Board::new(4);
    board.spawn_height = 26;
    board.rows[..8].copy_from_slice(&[14, 7, 12, 8, 12, 8, 12, 12]);
    let impossible = Move {
        piece: Piece::S,
        rotation: Rotation::South,
        x: 1,
        y: 5,
        spin: Spin::Full,
    };
    assert!(generate_moves_with_rules(&board, Piece::S, SpinMode::Handheld).contains(&impossible));
    assert!(find_input_path(&board, impossible, SpinMode::Handheld).is_none());
    assert!(!generate_input_moves(&board, Piece::S, SpinMode::Handheld).contains(&impossible));
    let mut state = GameState::from_triangle(
        board,
        Piece::J,
        Some(Piece::S),
        vec![Piece::Z, Piece::S, Piece::I, Piece::T, Piece::L],
        0,
        true,
        0,
    );
    state.spin_mode = SpinMode::Handheld;
    state.b2b_level = 20;
    for depth in [1, 3, 6] {
        let choice = find_funny_move(&state, None, Evaluator::Static(&Weights::default()), depth)
            .unwrap()
            .choice;
        assert!(find_input_path(&state.board, choice.0, state.spin_mode).is_some());
    }
}
