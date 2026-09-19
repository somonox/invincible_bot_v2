use four_wide_bot::engine::{
    board::Board,
    header::{Move, Piece, Rotation, Spin, SpinMode, ALL_PIECES, ALL_ROTATIONS},
    movegen::{detect_spin, find_input_path, generate_moves, rotate, SearchState},
    piece::{get_piece_cells, get_srs_kicks, get_srs_kicks_i},
    state::GameState,
};

fn board(rows: &[u16]) -> Board {
    let mut b = Board::new(4);
    b.rows[..rows.len()].copy_from_slice(rows);
    b
}
#[test]
fn all_srs_x_transition_tests_match_reference_in_order() {
    // Independently captured @haelp/teto 4.2.7 data: coordinates there are Y-down.
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/srs_x.json")).unwrap();
    for from in ALL_ROTATIONS {
        for to in ALL_ROTATIONS {
            if from == to {
                continue;
            }
            let key = format!("{}{}", from as u8, to as u8);
            for (table, actual) in [
                ("kicks", get_srs_kicks(from, to)),
                ("i_kicks", get_srs_kicks_i(from, to)),
            ] {
                let mut expected = vec![(0, 0)];
                expected.extend(reference[table][&key].as_array().unwrap().iter().map(|v| {
                    (
                        v[0].as_i64().unwrap() as i32,
                        -(v[1].as_i64().unwrap() as i32),
                    )
                }));
                assert_eq!(actual, expected, "{table} {key}");
            }
        }
    }
}
#[test]
fn i_states_rotate_about_the_srs_half_cell_pivot() {
    let mut cells = get_piece_cells(Piece::I, Rotation::North).map(|c| (c.x, c.y));
    for rotation in [
        Rotation::East,
        Rotation::South,
        Rotation::West,
        Rotation::North,
    ] {
        cells = cells.map(|(x, y)| (y + 1, -x)); // clockwise about (0.5,-0.5)
        let mut expected = get_piece_cells(Piece::I, rotation).map(|c| (c.x, c.y));
        cells.sort();
        expected.sort();
        assert_eq!(cells, expected);
    }
}
#[test]
fn t_front_corners_and_fin_upgrade_mini() {
    let b = board(&[1, 0, 5]);
    let n = SearchState {
        rotation: Rotation::North,
        x: 1,
        y: 1,
    };
    assert_eq!(
        detect_spin(&b, Piece::T, n, false, SpinMode::All),
        Spin::Full
    );
    let s = SearchState {
        rotation: Rotation::South,
        ..n
    };
    assert_eq!(
        detect_spin(&b, Piece::T, s, false, SpinMode::All),
        Spin::Mini
    );
    assert_eq!(
        detect_spin(&b, Piece::T, s, true, SpinMode::All),
        Spin::Full
    );
    assert_eq!(
        detect_spin(&Board::new(4), Piece::T, n, true, SpinMode::All),
        Spin::None
    );
}
#[test]
fn all_piece_spins_require_four_blocked_directions_and_respect_mode() {
    let b = board(&[5, 0, 0, 0, 2]);
    let s = SearchState {
        rotation: Rotation::East,
        x: 0,
        y: 2,
    };
    assert!(b.fits(Piece::I, s.rotation, s.x, s.y));
    assert_eq!(
        detect_spin(&b, Piece::I, s, false, SpinMode::All),
        Spin::Full
    );
    assert_eq!(
        detect_spin(&b, Piece::I, s, false, SpinMode::AllMiniPlus),
        Spin::Mini
    );
    assert_eq!(
        detect_spin(&b, Piece::I, s, false, SpinMode::TSpins),
        Spin::None
    );
    assert_eq!(
        detect_spin(&board(&[5]), Piece::I, s, false, SpinMode::All),
        Spin::None
    );
}
fn clear_state(spin: Spin, lines: usize, combo: u32, b2b: u32) -> (GameState, Move) {
    let mut b = board(&vec![14; lines]);
    b.rows[6] = 8; // avoid PC bonus
    let mut s = GameState::from_triangle(b, Piece::I, None, vec![Piece::T], combo, b2b > 0, 0);
    s.b2b_level = b2b;
    let mut m = Move::new(Piece::I, Rotation::East, -1, 2);
    m.spin = spin;
    (s, m)
}
#[test]
fn spin_damage_combo_and_b2b_match_in_search_and_battle() {
    for (spin, lines, combo, b2b, expected) in [
        (Spin::None, 1, 0, 0, 0),
        (Spin::Full, 1, 0, 0, 2),
        (Spin::Mini, 1, 0, 0, 0),
        (Spin::Full, 2, 0, 0, 4),
        (Spin::Mini, 2, 0, 0, 1),
        (Spin::Full, 3, 0, 0, 6),
        (Spin::None, 4, 0, 0, 4),
        (Spin::Full, 4, 0, 0, 10),
        (Spin::None, 4, 0, 1, 5),
        (Spin::Full, 2, 4, 1, 10),
        (Spin::None, 1, 1, 0, 0),
        (Spin::None, 1, 2, 0, 1),
    ] {
        let (mut solo, m) = clear_state(spin, lines, combo, b2b);
        let mut battle = solo.clone();
        let mut opponent = GameState::new(4);
        assert_eq!(solo.do_move(m), lines as u32);
        let (_, sent) = battle.do_move_battle(m, &mut opponent);
        assert_eq!(
            solo.last_attack, expected,
            "{spin:?} {lines} combo={combo} b2b={b2b}"
        );
        assert_eq!(sent, expected);
        assert_eq!(solo.b2b, battle.b2b);
        assert_eq!(solo.b2b, spin != Spin::None || lines == 4);
        assert_eq!(solo.current_combo(), combo);
    }
}
#[test]
fn generated_spin_paths_replay_with_the_same_final_spin() {
    let boards = [
        board(&[5, 0, 0, 0, 2]),
        board(&[11, 9, 1]),
        board(&[7, 5, 4]),
        board(&[13, 9, 8]),
        board(&[1, 0, 5]),
    ];
    let mut checked = 0;
    for b in boards {
        for piece in ALL_PIECES {
            for target in generate_moves(&b, piece)
                .into_iter()
                .filter(|m| m.spin != Spin::None)
            {
                let Some(path) = find_input_path(&b, target, SpinMode::All) else {
                    continue;
                };
                let mut s = SearchState {
                    rotation: Rotation::North,
                    x: 1,
                    y: 20,
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
                            (s, spin) = rotate(&b, piece, s, to, SpinMode::All).unwrap();
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
                checked += 1;
            }
        }
    }
    assert!(checked >= 3, "checked only {checked} spin paths");
}

#[test]
fn handheld_halves_non_t_spin_damage_before_the_combo_multiplier() {
    let (mut state, m) = clear_state(Spin::Full, 2, 4, 0);
    state.spin_mode = SpinMode::Handheld;
    state.do_move(m);
    assert_eq!(state.last_attack, 4); // (4 / 2) * (1 + .25 * 4)
}
#[test]
fn handheld_corner_rules_differ_from_immobility() {
    let mut b = Board::new(6);
    // L north corners around pivot (2,1): (1,2),(2,2),(3,0),(1,0).
    b.rows[0] = (1 << 1) | (1 << 3);
    b.rows[2] = 1 << 1;
    let s = SearchState {
        rotation: Rotation::North,
        x: 2,
        y: 1,
    };
    assert!(b.fits(Piece::L, s.rotation, s.x, s.y));
    assert_eq!(
        detect_spin(&b, Piece::L, s, false, SpinMode::Handheld),
        Spin::Full
    );
    assert_eq!(
        detect_spin(&b, Piece::L, s, false, SpinMode::All),
        Spin::None
    );
    assert_eq!(
        detect_spin(&b, Piece::L, s, false, SpinMode::None),
        Spin::None
    );
}
#[test]
fn pc_bonus_and_combo_tables_are_room_settings() {
    use four_wide_bot::engine::header::ComboMode;
    for bonus in [0, 5, 10] {
        let mut b = Board::new(4);
        b.rows[0] = 3;
        b.rows[1] = 3;
        let mut s = GameState::from_triangle(b, Piece::O, None, vec![Piece::T], 0, false, 0);
        s.pc_bonus = bonus;
        s.do_move(Move::new(Piece::O, Rotation::North, 2, 0));
        assert_eq!(s.last_attack, 1 + bonus);
    }
    for (mode, expected) in [
        (ComboMode::None, 1),
        (ComboMode::Classic, 6),
        (ComboMode::Modern, 4),
        (ComboMode::Multiplier, 3),
    ] {
        let (mut s, m) = clear_state(Spin::None, 2, 11, 0);
        s.combo_mode = mode;
        s.do_move(m);
        assert_eq!(s.last_attack, expected);
    }
}
