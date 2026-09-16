use four_wide_bot::engine::{
    battle::resolve_paired_turn,
    board::Board,
    header::{Move, Piece, Rotation},
    state::GameState,
};

fn quad_state() -> GameState {
    let mut board = Board::new(4);
    board.rows[..4].fill(0b1110);
    GameState::from_triangle(
        board,
        Piece::I,
        Some(Piece::T),
        vec![Piece::O, Piece::S],
        0,
        false,
        0,
    )
}

#[test]
fn simultaneous_attacks_do_not_give_the_second_player_free_cancellation() {
    let mut a = quad_state();
    let mut b = a.clone();
    let quad = Move::new(Piece::I, Rotation::East, -1, 2);
    resolve_paired_turn(&mut a, &mut b, quad, quad);
    assert_eq!((a.pieces_placed, b.pieces_placed), (1, 1));
    assert_eq!(a.board, b.board);
    assert_eq!(a.score, b.score);
    assert!(a.queued_garbage > 0);
    assert_eq!(a.queued_garbage, b.queued_garbage);
    assert_eq!((a.pending_garbage, b.pending_garbage), (0, 0));
}

#[test]
fn existing_garbage_is_resolved_before_either_new_attack_arrives() {
    let mut a = quad_state();
    a.pending_garbage = 2;
    a.queued_garbage = 1;
    let mut b = a.clone();
    let quad = Move::new(Piece::I, Rotation::East, -1, 2);
    resolve_paired_turn(&mut a, &mut b, quad, quad);
    assert_eq!((a.pending_garbage, b.pending_garbage), (0, 0));
    assert!(a.queued_garbage > 0);
    assert_eq!(a.queued_garbage, b.queued_garbage);
}
