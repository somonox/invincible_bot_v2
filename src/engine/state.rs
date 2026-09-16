use crate::engine::board::{Board, BOARD_HEIGHT};
use crate::engine::header::{Move, Piece, Rotation, Spin, SpinMode};
use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;

#[derive(Clone, Debug)]
pub struct GameState {
    pub board: Board,
    pub current: Piece,
    pub hold: Option<Piece>,
    pub queue: Vec<Piece>,
    pub hold_used: bool,

    /// Consecutive clearing placements; displayed combo is this minus one.
    pub combo: u32,
    pub spin_mode: SpinMode,
    pub last_spin: Spin,
    pub b2b: bool,
    pub score: u32,
    pub lines_cleared: u32,
    pub perfect_clears: u32,
    pub last_perfect_clear: bool,
    pub pieces_placed: u32,
    pub game_over: bool,

    // Multiplayer features
    pub pending_garbage: u32,
    pub queued_garbage: u32,
    pub last_garbage_hole_x: usize,
    pub b2b_level: u32,
    pub b2b_charge: u32,
    pub last_attack: u32,

    bag: Vec<Piece>,
    preview_only: bool,
    current_known: bool,
}

impl GameState {
    pub fn new(width: usize) -> Self {
        let mut bag = vec![
            Piece::I,
            Piece::O,
            Piece::T,
            Piece::L,
            Piece::J,
            Piece::S,
            Piece::Z,
        ];
        let mut rng = thread_rng();
        bag.shuffle(&mut rng);

        let initial_hole = rng.gen_range(0..width);
        let mut state = Self {
            board: Board::new(width),
            current: Piece::I, // Temporary placeholder
            hold: None,
            queue: Vec::new(),
            hold_used: false,
            combo: 0,
            spin_mode: SpinMode::All,
            last_spin: Spin::None,
            b2b: false,
            score: 0,
            lines_cleared: 0,
            perfect_clears: 0,
            last_perfect_clear: false,
            pieces_placed: 0,
            game_over: false,
            pending_garbage: 0,
            queued_garbage: 0,
            last_garbage_hole_x: initial_hole,
            b2b_level: 0,
            b2b_charge: 0,
            last_attack: 0,
            bag,
            preview_only: false,
            current_known: true,
        };

        state.refill_bag_if_needed();
        state.current = state.bag.pop().unwrap();

        for _ in 0..5 {
            state.refill_bag_if_needed();
            state.queue.push(state.bag.pop().unwrap());
        }

        state
    }

    /// Create a GameState from Triangle.js protocol data.
    /// This bypasses the internal bag system since pieces come from the TETR.IO server.
    pub fn from_triangle(
        board: Board,
        current: Piece,
        hold: Option<Piece>,
        queue: Vec<Piece>,
        combo: u32,
        b2b: bool,
        pending_garbage: u32,
    ) -> Self {
        Self {
            board,
            current,
            hold,
            queue,
            hold_used: false,
            combo,
            spin_mode: SpinMode::All,
            last_spin: Spin::None,
            b2b,
            score: 0,
            lines_cleared: 0,
            perfect_clears: 0,
            last_perfect_clear: false,
            pieces_placed: 0,
            game_over: false,
            pending_garbage,
            queued_garbage: 0,
            last_garbage_hole_x: 0,
            b2b_level: if b2b { 1 } else { 0 },
            b2b_charge: 0,
            last_attack: 0,
            bag: Vec::new(),
            preview_only: true,
            current_known: true,
        }
    }

    /// Search only the supplied preview, even in a local game with a hidden bag.
    pub fn for_search(&self) -> Self {
        let mut state = self.clone();
        state.preview_only = true;
        state.bag.clear();
        state
    }

    pub fn has_known_current(&self) -> bool {
        self.current_known
    }

    fn advance_piece(&mut self) {
        if self.queue.is_empty() {
            self.current_known = false;
            return;
        }
        self.current = self.queue.remove(0);
        self.current_known = true;
        if !self.preview_only {
            self.refill_bag_if_needed();
            self.queue.push(self.bag.pop().unwrap());
        }
    }

    pub fn current_combo(&self) -> u32 {
        self.combo.saturating_sub(1)
    }

    /// Shared clear/attack accounting for search and battle. Multiplier combo and
    /// logarithmic B2B chaining follow garbageCalcV2; room-specific cancellation
    /// and the local surge approximation remain in the battle simulator.
    fn score_clear(&mut self, cleared: u32, spin: Spin) -> u32 {
        self.last_spin = spin;
        if cleared == 0 {
            self.combo = 0;
            return 0;
        }
        let eligible = cleared >= 4 || spin != Spin::None;
        let previous_b2b = self.b2b;
        let mut surge = 0;
        if eligible {
            self.b2b_level = if previous_b2b { self.b2b_level + 1 } else { 1 };
            self.b2b = true;
            self.b2b_charge = if self.b2b_level >= 4 {
                self.b2b_level
            } else {
                0
            };
        } else {
            surge = self.b2b_charge;
            self.b2b = false;
            self.b2b_level = 0;
            self.b2b_charge = 0;
        }
        let mut attack: f64 = match (spin, cleared) {
            (Spin::Full, 1) => 2.0,
            (Spin::Full, 2) => 4.0,
            (Spin::Full, 3) => 6.0,
            (Spin::Mini | Spin::Full, 4) => 10.0,
            (_, 1) => 0.0,
            (_, 2) => 1.0,
            (_, 3) => 2.0,
            (_, 4) => 4.0,
            _ => 0.0,
        };
        let b2b_index = self.b2b_level.saturating_sub(1);
        if eligible && b2b_index > 0 {
            let log = (b2b_index as f64 * 0.8).ln_1p();
            attack += (1.0 + log).floor()
                + if b2b_index == 1 {
                    0.0
                } else {
                    (1.0 + log.fract()) / 3.0
                };
        }
        let combo_index = self.combo;
        if combo_index > 0 {
            attack *= 1.0 + 0.25 * combo_index as f64;
            if combo_index > 1 {
                attack = attack.max((combo_index as f64 * 1.25).ln_1p());
            }
        }
        let base_score = match (spin, cleared) {
            (Spin::Full, n) => 400 + n * 400,
            (Spin::Mini, n) => 100 + n * 100,
            (_, 1) => 100,
            (_, 2) => 300,
            (_, 3) => 500,
            _ => 800,
        };
        self.score += if eligible && previous_b2b {
            base_score * 3 / 2
        } else {
            base_score
        };
        self.score += self.combo * 50;
        self.combo += 1;
        self.lines_cleared += cleared;
        attack.floor() as u32 + surge
    }

    pub fn refill_bag_if_needed(&mut self) {
        if self.bag.is_empty() {
            self.bag = vec![
                Piece::I,
                Piece::O,
                Piece::T,
                Piece::L,
                Piece::J,
                Piece::S,
                Piece::Z,
            ];
            let mut rng = thread_rng();
            self.bag.shuffle(&mut rng);
        }
    }

    /// Perform a hold action if allowed. Returns true if successful.
    pub fn hold(&mut self) -> bool {
        if self.hold_used
            || self.game_over
            || !self.current_known
            || (self.hold.is_none() && self.queue.is_empty())
        {
            return false;
        }

        if let Some(held_piece) = self.hold {
            let temp = self.current;
            self.current = held_piece;
            self.hold = Some(temp);
        } else {
            self.hold = Some(self.current);
            self.advance_piece();
        }

        self.hold_used = true;

        // Check if piece fits at spawn position
        let spawn_x = (self.board.width as i32) / 2 - 1;
        let highest = self.board.highest_row() as i32;
        let spawn_y = if highest >= 20 {
            (highest + 1).min(BOARD_HEIGHT as i32 - 3)
        } else {
            20
        };

        if !self
            .board
            .fits(self.current, Rotation::North, spawn_x, spawn_y)
        {
            // Check one row higher
            if !self.board.fits(
                self.current,
                Rotation::North,
                spawn_x,
                (spawn_y + 1).min(BOARD_HEIGHT as i32 - 1),
            ) {
                self.game_over = true;
            }
        }

        true
    }

    /// Place a piece on the board and advance to the next piece.
    /// Returns the number of lines cleared.
    pub fn do_move(&mut self, m: Move) -> u32 {
        self.last_perfect_clear = false;
        if self.game_over || !self.current_known {
            self.last_attack = 0;
            return 0;
        }

        // Place piece
        self.board.place(m.piece, m.rotation, m.x, m.y);

        // Clear lines
        let cleared = self.board.clear_lines();

        let mut attack_sent = self.score_clear(cleared, m.spin);

        // Check for Perfect Clear
        if self.board.highest_row() == 0 && cleared > 0 {
            self.last_perfect_clear = true;
            self.perfect_clears += 1;
            attack_sent += 10;
        }

        // Garbage Canceling & Pushing
        if cleared > 0 {
            let canceled = self.pending_garbage.min(attack_sent);
            self.pending_garbage -= canceled;
        } else {
            let garbage_to_push = self.pending_garbage.min(8);
            if garbage_to_push > 0 {
                self.board
                    .spawn_garbage(garbage_to_push as i32, self.last_garbage_hole_x as i32);
                self.pending_garbage -= garbage_to_push;
            }
        }

        self.last_attack = attack_sent;
        self.pieces_placed += 1;

        // Spawn next piece
        self.advance_piece();
        self.hold_used = false;

        if !self.current_known {
            return cleared;
        }

        // Check if next piece fits at spawn position
        let spawn_x = (self.board.width as i32) / 2 - 1;
        let highest = self.board.highest_row() as i32;
        let spawn_y = if highest >= 20 {
            (highest + 1).min(BOARD_HEIGHT as i32 - 3)
        } else {
            20
        };

        if !self
            .board
            .fits(self.current, Rotation::North, spawn_x, spawn_y)
        {
            // Check one row higher
            if !self.board.fits(
                self.current,
                Rotation::North,
                spawn_x,
                (spawn_y + 1).min(BOARD_HEIGHT as i32 - 1),
            ) {
                self.game_over = true;
            }
        }

        cleared
    }

    /// Place a piece on the board in 1v1 battle mode.
    /// Handles garbage canceling and calculates garbage sent to the opponent.
    /// Returns (lines_cleared, attack_sent)
    pub fn do_move_battle(&mut self, m: Move, opponent: &mut GameState) -> (u32, u32) {
        self.last_perfect_clear = false;
        if self.game_over || !self.current_known {
            self.last_attack = 0;
            return (0, 0);
        }

        // Place piece
        self.board.place(m.piece, m.rotation, m.x, m.y);

        // Clear lines
        let cleared = self.board.clear_lines();

        let was_b2b_active = self.b2b;
        let mut attack_sent = self.score_clear(cleared, m.spin);

        // Apply the same perfect-clear bonus as the search/single-player engine,
        // before cancellation and before sending the remaining attack.
        if cleared > 0 && self.board.highest_row() == 0 {
            self.last_perfect_clear = true;
            self.perfect_clears += 1;
            attack_sent += 10;
        }

        self.pieces_placed += 1;

        // Spawn next piece
        self.advance_piece();
        self.hold_used = false;

        // Check if next piece fits at spawn position
        let spawn_x = (self.board.width as i32) / 2 - 1;
        let highest = self.board.highest_row() as i32;
        let spawn_y = if highest >= 20 {
            (highest + 1).min(BOARD_HEIGHT as i32 - 3)
        } else {
            20
        };

        if self.current_known
            && !self
                .board
                .fits(self.current, Rotation::North, spawn_x, spawn_y)
        {
            if !self.board.fits(
                self.current,
                Rotation::North,
                spawn_x,
                (spawn_y + 1).min(BOARD_HEIGHT as i32 - 1),
            ) {
                self.game_over = true;
            }
        }

        // Garbage Resolution
        if cleared > 0 {
            let is_b2b_clear = (cleared >= 4 || m.spin != Spin::None) && was_b2b_active;
            let cancel_multiplier = if self.pieces_placed <= 14 { 2 } else { 1 };

            let mut base_cancel = attack_sent;
            if is_b2b_clear {
                base_cancel += 1;
            }

            let mut cancel_power = base_cancel * cancel_multiplier;
            let original_garbage = self.pending_garbage + self.queued_garbage;

            // 1. Cancel own pending garbage first
            let cancel_pending = self.pending_garbage.min(cancel_power);
            self.pending_garbage -= cancel_pending;
            cancel_power -= cancel_pending;

            // 2. Cancel own queued garbage next
            let cancel_queued = self.queued_garbage.min(cancel_power);
            self.queued_garbage -= cancel_queued;

            // Calculate remaining attack sent to opponent
            let garbage_canceled = original_garbage - (self.pending_garbage + self.queued_garbage);
            let mut remaining_attack = attack_sent;

            if garbage_canceled > 0 {
                let base_cancel_consumed =
                    (garbage_canceled + cancel_multiplier - 1) / cancel_multiplier;
                let mut attack_consumed = base_cancel_consumed;
                if is_b2b_clear {
                    attack_consumed = attack_consumed.saturating_sub(1);
                }
                remaining_attack = remaining_attack.saturating_sub(attack_consumed);
            }

            // 3. Send remaining attack to opponent's queue (buffered)
            if remaining_attack > 0 {
                opponent.queued_garbage += remaining_attack;
            }

            attack_sent = remaining_attack;
        } else {
            // If we did not clear lines, pending garbage rises from the bottom! (Capped at 8 lines per turn - TETR.IO cap)
            if self.pending_garbage > 0 {
                let mut rng = thread_rng();

                // Randomly shift garbage hole column occasionally to avoid making it too easy to downstack
                if rng.gen::<f32>() < 0.3 {
                    self.last_garbage_hole_x = rng.gen_range(0..self.board.width);
                }

                let lines_to_spawn = self.pending_garbage.min(8);
                self.board
                    .spawn_garbage(lines_to_spawn as i32, self.last_garbage_hole_x as i32);
                self.pending_garbage -= lines_to_spawn;

                // Check if topped out by garbage rising
                let new_highest = self.board.highest_row();
                if new_highest >= BOARD_HEIGHT - 3 {
                    self.game_over = true;
                }
            }
        }

        // At the end of the turn, transfer queued garbage to pending garbage
        self.pending_garbage += self.queued_garbage;
        self.queued_garbage = 0;

        self.last_attack = attack_sent;
        (cleared, attack_sent)
    }
}
