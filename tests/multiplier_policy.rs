use four_wide_bot::{
    engine::{board::Board, header::Piece, state::GameState},
    rl::{
        agent::get_all_next_states,
        features::Weights,
        search::{find_hybrid_move, Evaluator},
    },
};
fn fixture(combo: u32, garbage: u32) -> GameState {
    let mut b = Board::new(4);
    b.rows[..6].copy_from_slice(&[7, 7, 7, 7, 3, 1]);
    GameState::from_triangle(
        b,
        Piece::T,
        Some(Piece::I),
        vec![Piece::O, Piece::I, Piece::S],
        combo,
        false,
        garbage,
    )
}
// Exhaustive reference over all legal placements. Independent of beam ranking.
fn max_damage(s: &GameState, depth: usize) -> u32 {
    if depth == 0 {
        return 0;
    }
    get_all_next_states(s)
        .into_iter()
        .filter(|(n, _, _)| !n.game_over)
        .map(|(n, _, _)| n.last_attack + max_damage(&n, depth - 1))
        .max()
        .unwrap_or(0)
}
#[test]
fn grows_multiplier_before_spending_i_without_a_combo_threshold() {
    for combo in [0, 2, 5, 9] {
        let s = fixture(combo, 0);
        let p = find_hybrid_move(&s, None, Evaluator::Static(&Weights::default()), 3).unwrap();
        let candidates = get_all_next_states(&s);
        let selected = &candidates
            .iter()
            .find(|(_, m, h)| (*m, *h) == p.choice)
            .unwrap()
            .0;
        let immediate = candidates
            .iter()
            .filter(|(n, _, _)| !n.game_over)
            .map(|(n, _, _)| n.last_attack)
            .max()
            .unwrap();
        let greedy_total = candidates
            .iter()
            .filter(|(n, _, _)| !n.game_over && n.last_attack == immediate)
            .map(|(n, _, _)| immediate + max_damage(n, 2))
            .max()
            .unwrap();
        assert!(
            selected.combo > s.combo,
            "build the multiplier with a clearing move"
        );
        assert!(selected.last_attack < immediate);
        assert!(p.expected_attack > greedy_total, "combo={combo}");
        assert_eq!(p.expected_attack, max_damage(&s, 3));
        assert_eq!(
            p.expected_attack,
            selected.last_attack + max_damage(selected, 2)
        );
    }
}
#[test]
fn ready_queue_can_be_blocked_while_the_larger_multiplied_hit_is_prepared() {
    let mut s = fixture(9, 8);
    let mut attack = 0;
    let mut peak = 0;
    for remaining in (1..=3).rev() {
        let p =
            find_hybrid_move(&s, None, Evaluator::Static(&Weights::default()), remaining).unwrap();
        if remaining == 3 {
            assert_eq!(p.expected_attack, 19);
            assert!(p.peak_attack >= 14);
        }
        s = get_all_next_states(&s)
            .into_iter()
            .find(|(_, m, h)| (*m, *h) == p.choice)
            .unwrap()
            .0;
        assert_eq!(s.last_received_garbage, 0);
        if remaining == 3 {
            assert_eq!(s.last_canceled_garbage, 2);
        }
        attack += s.last_attack;
        peak = peak.max(s.last_attack);
    }
    assert_eq!(attack, 19);
    assert!(peak >= 14);
    assert_eq!(s.incoming_garbage(), 0);
}
#[test]
fn spends_now_when_preview_contains_no_payoff_for_waiting() {
    let s = fixture(9, 0);
    let p = find_hybrid_move(&s, None, Evaluator::Static(&Weights::default()), 1).unwrap();
    assert_eq!(p.expected_attack, max_damage(&s, 1));
    let next = get_all_next_states(&s)
        .into_iter()
        .find(|(_, m, h)| (*m, *h) == p.choice)
        .unwrap()
        .0;
    assert_eq!(next.lines_cleared, 4);
    assert_eq!(next.last_attack, 13);
}
