use crate::engine::{header::Move, state::GameState};

/// Resolve both placements against the garbage queues at the start of the turn.
/// Neither player may cancel or receive the other's new attack before placing.
pub fn resolve_paired_turn(a: &mut GameState, b: &mut GameState, move_a: Move, move_b: Move) {
    let mut outgoing_a = b.clone();
    let mut outgoing_b = a.clone();
    let (_, attack_a) = a.do_move_battle(move_a, &mut outgoing_a);
    let (_, attack_b) = b.do_move_battle(move_b, &mut outgoing_b);
    a.queued_garbage += attack_b;
    b.queued_garbage += attack_a;
}
