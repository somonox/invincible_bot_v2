use four_wide_bot::engine::{
    board::Board,
    header::Piece,
    state::{GameState, GarbagePacket},
};
use four_wide_bot::rl::{
    agent::get_all_next_states,
    features::Weights,
    search::{find_hybrid_move, pc_residue_possible, Evaluator, HybridMode},
};
fn state(rows: &[u16], piece: Piece, queue: &[Piece]) -> GameState {
    let mut board = Board::new(4);
    board.rows[..rows.len()].copy_from_slice(rows);
    GameState::from_triangle(board, piece, Some(Piece::T), queue.to_vec(), 8, false, 0)
}
fn plan(s: &GameState, depth: usize) -> four_wide_bot::rl::search::HybridPlan {
    find_hybrid_move(s, None, Evaluator::Static(&Weights::default()), depth).unwrap()
}
fn chosen(s: &GameState, c: (four_wide_bot::engine::header::Move, bool)) -> GameState {
    get_all_next_states(s)
        .into_iter()
        .find(|(_, m, h)| (*m, *h) == c)
        .unwrap()
        .0
}
#[test]
fn opponent_combo_no_longer_triggers_switching() {
    let s = state(&[], Piece::O, &[Piece::O, Piece::S]);
    let mut opp = s.clone();
    for combo in [0, 5, 20, 100] {
        opp.combo = combo + 1;
        let hybrid =
            find_hybrid_move(&s, Some(&opp), Evaluator::Static(&Weights::default()), 2).unwrap();
        assert_eq!(hybrid.mode, HybridMode::PerfectClear { placements: 2 });
        assert!(get_all_next_states(&chosen(&s, hybrid.choice))
            .iter()
            .any(|(s, _, _)| s.last_perfect_clear));
    }
}
#[test]
fn residue_is_preserved_and_rechecked_after_garbage() {
    let mut s = state(&[3, 1], Piece::T, &[Piece::I, Piece::S, Piece::O]);
    assert!(!pc_residue_possible(&s));
    assert_eq!(plan(&s, 3).mode, HybridMode::ComboResidue);
    s.pending_garbage = 8;
    assert!(matches!(
        plan(&s, 3).mode,
        HybridMode::GarbageDefense { pc: false, .. }
    ));
    s.board.spawn_garbage(3, 0);
    assert!(pc_residue_possible(&s));
}
#[test]
fn ready_and_queued_garbage_both_enable_defense_and_immediate_pc_cancels() {
    for queued in [false, true] {
        let mut s = state(&[3, 3], Piece::O, &[Piece::S]);
        if queued {
            s.queued_garbage = 8;
        } else {
            s.pending_garbage = 8;
        }
        let p = plan(&s, 2);
        assert!(matches!(
            p.mode,
            HybridMode::GarbageDefense {
                pc: true,
                canceled_next: 8,
                received: 0
            }
        ));
        let next = chosen(&s, p.choice);
        assert!(next.last_perfect_clear);
        assert_eq!(next.last_canceled_garbage, 8);
        assert_eq!(next.incoming_garbage(), 0);
    }
}
#[test]
fn delayed_packet_allows_pc_before_garbage_arrives() {
    let mut s = state(&[], Piece::O, &[Piece::O, Piece::S]);
    s.hold_used = true;
    s.garbage_packets = Some(vec![GarbagePacket {
        amount: 8,
        ready_in: 40,
    }]);
    s.next_lock_frames = 6;
    s.frames_per_piece = 30;
    s.sync_garbage_totals();
    let p = plan(&s, 2);
    assert!(matches!(
        p.mode,
        HybridMode::GarbageDefense {
            pc: true,
            received: 0,
            ..
        }
    ));
    let next = chosen(&s, p.choice);
    assert_eq!(next.last_received_garbage, 0);
    assert!(get_all_next_states(&next)
        .iter()
        .any(|(s, _, _)| s.last_perfect_clear && s.last_canceled_garbage == 8));
}
#[test]
fn arriving_garbage_uses_deadline_and_cap_without_counting_as_cancel() {
    let mut s = state(&[], Piece::O, &[Piece::O]);
    s.hold_used = true;
    s.garbage_packets = Some(vec![GarbagePacket {
        amount: 9,
        ready_in: 5,
    }]);
    s.next_lock_frames = 5;
    s.garbage_cap = 3;
    s.sync_garbage_totals();
    for (next, _, _) in get_all_next_states(&s) {
        assert_eq!(next.last_received_garbage, 3);
        assert_eq!(next.last_canceled_garbage, 0);
        assert_eq!(next.incoming_garbage(), 6);
    }
    s.next_lock_frames = 4;
    assert!(get_all_next_states(&s)
        .iter()
        .all(|(n, _, _)| n.last_received_garbage == 0 && n.queued_garbage == 9));
}
#[test]
fn queued_garbage_can_be_canceled_before_arrival_and_zero_attack_only_blocks() {
    let mut s = state(&[7], Piece::I, &[Piece::O]);
    s.combo = 0;
    s.hold_used = true;
    s.queued_garbage = 8;
    let next = get_all_next_states(&s)
        .into_iter()
        .find(|(n, _, _)| n.lines_cleared == 1)
        .unwrap()
        .0;
    assert_eq!(next.last_attack, 0);
    assert_eq!(next.last_received_garbage, 0);
    assert_eq!(next.last_canceled_garbage, 0);
    assert_eq!(next.pending_garbage, 8);
    s.combo = 8;
    let next = get_all_next_states(&s)
        .into_iter()
        .find(|(n, _, _)| n.lines_cleared == 1)
        .unwrap()
        .0;
    assert!(next.last_canceled_garbage > 0);
    assert_eq!(next.pending_garbage, 8 - next.last_canceled_garbage);
}
#[test]
fn empty_queue_can_plan_pc_and_unavailable_pc_uses_multiplier_attack() {
    let mut s = state(&[3, 3], Piece::T, &[Piece::S]);
    s.hold_used = true;
    let p = plan(&s, 1);
    assert_eq!(p.mode, HybridMode::ComboMultiplier);
    assert_eq!(
        p.expected_attack,
        get_all_next_states(&s)
            .iter()
            .filter(|(n, _, _)| !n.game_over)
            .map(|(n, _, _)| n.last_attack)
            .max()
            .unwrap()
    );
    let mut s = state(&[], Piece::O, &[Piece::O, Piece::S]);
    s.pending_garbage = 4;
    assert!(matches!(
        plan(&s, 2).mode,
        HybridMode::GarbageDefense { .. }
    ));
    s.pending_garbage = 0;
    assert_eq!(plan(&s, 2).mode, HybridMode::PerfectClear { placements: 2 });
}
#[test]
fn residue_check_uses_width_gcd_and_terminal_states_have_no_plan() {
    let mut s = state(&[1], Piece::I, &[Piece::O]);
    s.board.width = 10;
    assert!(!pc_residue_possible(&s));
    s.board.rows[0] = 3;
    assert!(pc_residue_possible(&s));
    s.game_over = true;
    assert!(find_hybrid_move(&s, None, Evaluator::Static(&Weights::default()), 3).is_none());
}

#[test]
fn ready_queue_replaces_a_three_piece_pc_setup_with_immediate_cancellation() {
    let mut s = state(&[11, 2], Piece::T, &[Piece::O, Piece::I, Piece::L]);
    let opening = plan(&s, 3);
    assert_eq!(opening.mode, HybridMode::PerfectClear { placements: 3 });
    assert_eq!(chosen(&s, opening.choice).combo, 0);
    s.pending_garbage = 8;
    let defended = plan(&s, 3);
    assert!(matches!(
        defended.mode,
        HybridMode::GarbageDefense {
            pc: false,
            canceled_next: 2,
            received: 0
        }
    ));
    let next = chosen(&s, defended.choice);
    assert_eq!(next.last_canceled_garbage, 2);
    assert_eq!(next.last_received_garbage, 0);
    assert_ne!(opening.choice, defended.choice);
}
