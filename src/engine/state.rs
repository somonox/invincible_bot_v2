use rand::seq::SliceRandom;
use rand::thread_rng;
use rand::Rng;
use crate::engine::board::{Board, BOARD_HEIGHT};
use crate::engine::header::{Move, Piece, Rotation};

#[derive(Clone, Debug)]
pub struct GameState {
    pub board: Board,
    pub current: Piece,
    pub hold: Option<Piece>,
    pub queue: Vec<Piece>,
    pub hold_used: bool,

    pub combo: u32,
    pub b2b: bool,
    pub score: u32,
    pub lines_cleared: u32,
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
}

impl GameState {
    pub fn new(width: usize) -> Self {
        let mut bag = vec![
            Piece::I, Piece::O, Piece::T, Piece::L, Piece::J, Piece::S, Piece::Z
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
            b2b: false,
            score: 0,
            lines_cleared: 0,
            pieces_placed: 0,
            game_over: false,
            pending_garbage: 0,
            queued_garbage: 0,
            last_garbage_hole_x: initial_hole,
            b2b_level: 0,
            b2b_charge: 0,
            last_attack: 0,
            bag,
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
            b2b,
            score: 0,
            lines_cleared: 0,
            pieces_placed: 0,
            game_over: false,
            pending_garbage,
            queued_garbage: 0,
            last_garbage_hole_x: 0,
            b2b_level: if b2b { 1 } else { 0 },
            b2b_charge: 0,
            last_attack: 0,
            bag: Vec::new(),
        }
    }

    fn get_b2b_level(b2b_count: u32) -> u32 {
        if b2b_count == 0 {
            0
        } else if b2b_count <= 2 {
            1
        } else if b2b_count <= 7 {
            2
        } else if b2b_count <= 23 {
            3
        } else if b2b_count <= 66 {
            4
        } else {
            5
        }
    }

    pub fn refill_bag_if_needed(&mut self) {
        if self.bag.is_empty() {
            self.bag = vec![
                Piece::I, Piece::O, Piece::T, Piece::L, Piece::J, Piece::S, Piece::Z
            ];
            let mut rng = thread_rng();
            self.bag.shuffle(&mut rng);
        }
    }

    /// Perform a hold action if allowed. Returns true if successful.
    pub fn hold(&mut self) -> bool {
        if self.hold_used || self.game_over {
            return false;
        }

        if let Some(held_piece) = self.hold {
            let temp = self.current;
            self.current = held_piece;
            self.hold = Some(temp);
        } else {
            self.hold = Some(self.current);
            self.refill_bag_if_needed();
            self.current = self.queue.remove(0);
            self.queue.push(self.bag.pop().unwrap());
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

        if !self.board.fits(self.current, Rotation::North, spawn_x, spawn_y) {
            // Check one row higher
            if !self.board.fits(self.current, Rotation::North, spawn_x, (spawn_y + 1).min(BOARD_HEIGHT as i32 - 1)) {
                self.game_over = true;
            }
        }

        true
    }

    /// Place a piece on the board and advance to the next piece.
    /// Returns the number of lines cleared.
    pub fn do_move(&mut self, m: Move) -> u32 {
        if self.game_over {
            self.last_attack = 0;
            return 0;
        }

        // Place piece
        self.board.place(m.piece, m.rotation, m.x, m.y);

        // Clear lines
        let cleared = self.board.clear_lines();

        let mut attack_sent = 0;

        // Update score, combo, and back-to-back
        if cleared > 0 {
            // Lines score
            let base_score = match cleared {
                1 => 100,
                2 => 300,
                3 => 500,
                4 => 800,
                _ => 1000,
            };

            let mut multiplier = 1.0;
            if cleared == 4 {
                if self.b2b {
                    multiplier = 1.5;
                    self.b2b_level += 1;
                    if self.b2b_level >= 4 {
                        self.b2b_charge = self.b2b_level; // Surge charge equals B2B count
                    }
                } else {
                    self.b2b = true;
                    self.b2b_level = 1;
                    self.b2b_charge = 0;
                }

                let b2b_tier = Self::get_b2b_level(self.b2b_level);
                let sum_power = 4 + b2b_tier;
                let combo_bonus = ((self.combo as f32) / (4.0 / sum_power as f32)).floor() as u32;
                attack_sent = sum_power + combo_bonus;
            } else {
                // Non-quad clears break B2B and trigger Surge damage
                let surge_damage = if self.b2b {
                    let charge = self.b2b_charge;
                    self.b2b = false;
                    self.b2b_level = 0;
                    self.b2b_charge = 0;
                    charge
                } else {
                    0
                };

                let base_weight = match cleared {
                    1 => 0,
                    2 => 1,
                    3 => 2,
                    _ => 0,
                };

                let attack_from_clear = if base_weight == 0 {
                    // Single clear combo scaling (TETR.IO official formula)
                    let val = (self.combo as f32 * 1.25).ln_1p();
                    val.floor() as u32
                } else {
                    let combo_bonus = ((self.combo as f32) / (4.0 / base_weight as f32)).floor() as u32;
                    base_weight + combo_bonus
                };

                attack_sent = attack_from_clear + surge_damage;
            }

            self.score += (base_score as f32 * multiplier) as u32;

            // Combo bonus
            if self.combo > 0 {
                self.score += self.combo * 50;
            }
            self.combo += 1;
            self.lines_cleared += cleared;
        } else {
            self.combo = 0;
        }

        // Check for Perfect Clear
        if self.board.highest_row() == 0 && cleared > 0 {
            attack_sent += 10;
        }

        // Garbage Canceling & Pushing
        if cleared > 0 {
            let canceled = self.pending_garbage.min(attack_sent);
            self.pending_garbage -= canceled;
        } else {
            let garbage_to_push = self.pending_garbage.min(8);
            if garbage_to_push > 0 {
                self.board.spawn_garbage(garbage_to_push as i32, self.last_garbage_hole_x as i32);
                self.pending_garbage -= garbage_to_push;
            }
        }

        self.last_attack = attack_sent;
        self.pieces_placed += 1;

        // Spawn next piece
        self.refill_bag_if_needed();
        self.current = self.queue.remove(0);
        self.queue.push(self.bag.pop().unwrap());
        self.hold_used = false;

        // Check if next piece fits at spawn position
        let spawn_x = (self.board.width as i32) / 2 - 1;
        let highest = self.board.highest_row() as i32;
        let spawn_y = if highest >= 20 {
            (highest + 1).min(BOARD_HEIGHT as i32 - 3)
        } else {
            20
        };

        if !self.board.fits(self.current, Rotation::North, spawn_x, spawn_y) {
            // Check one row higher
            if !self.board.fits(self.current, Rotation::North, spawn_x, (spawn_y + 1).min(BOARD_HEIGHT as i32 - 1)) {
                self.game_over = true;
            }
        }

        cleared
    }

    /// Place a piece on the board in 1v1 battle mode.
    /// Handles garbage canceling and calculates garbage sent to the opponent.
    /// Returns (lines_cleared, attack_sent)
    pub fn do_move_battle(&mut self, m: Move, opponent: &mut GameState) -> (u32, u32) {
        if self.game_over {
            return (0, 0);
        }

        // Place piece
        self.board.place(m.piece, m.rotation, m.x, m.y);

        // Clear lines
        let cleared = self.board.clear_lines();

        let mut attack_sent = 0;
        let mut was_b2b_active = false;

        // Update score, combo, and back-to-back
        if cleared > 0 {
            was_b2b_active = self.b2b;
            let base_score = match cleared {
                1 => 100,
                2 => 300,
                3 => 500,
                4 => 800,
                _ => 1000,
            };

            let mut score_mult = 1.0;
            if cleared == 4 {
                if self.b2b {
                    score_mult = 1.5;
                }
            }
            self.score += (base_score as f32 * score_mult) as u32;

            if self.combo > 0 {
                self.score += self.combo * 50;
            }
            self.lines_cleared += cleared;

            // TETR.IO Attack calculation rules
            if cleared == 4 {
                // Quad (B2B-eligible)
                if self.b2b {
                    self.b2b_level += 1;
                    if self.b2b_level >= 4 {
                        self.b2b_charge = self.b2b_level; // Surge charge equals B2B count
                    }
                } else {
                    self.b2b = true;
                    self.b2b_level = 1;
                    self.b2b_charge = 0;
                }
                
                let b2b_tier = Self::get_b2b_level(self.b2b_level);
                let sum_power = 4 + b2b_tier;
                let combo_bonus = ((self.combo as f32) / (4.0 / sum_power as f32)).floor() as u32;
                attack_sent = sum_power + combo_bonus;
            } else {
                // Single, Double, Triple (Non-B2B-eligible, breaks B2B and triggers Surge)
                let surge_damage = if self.b2b {
                    let charge = self.b2b_charge;
                    self.b2b = false;
                    self.b2b_level = 0;
                    self.b2b_charge = 0;
                    charge
                } else {
                    0
                };

                let base_weight = match cleared {
                    1 => 0,
                    2 => 1,
                    3 => 2,
                    _ => 0,
                };

                let attack_from_clear = if base_weight == 0 {
                    // Single clear logic using TETR.IO Zen/League logarithmic multiplier formula:
                    // Math.floor(ln(combo * 1.25 + 1))
                    let val = (self.combo as f32 * 1.25).ln_1p();
                    val.floor() as u32
                } else {
                    let combo_bonus = ((self.combo as f32) / (4.0 / base_weight as f32)).floor() as u32;
                    base_weight + combo_bonus
                };

                attack_sent = attack_from_clear + surge_damage;
            }
 
            self.combo += 1;
        } else {
            self.combo = 0;
        }

        self.pieces_placed += 1;

        // Spawn next piece
        self.refill_bag_if_needed();
        self.current = self.queue.remove(0);
        self.queue.push(self.bag.pop().unwrap());
        self.hold_used = false;

        // Check if next piece fits at spawn position
        let spawn_x = (self.board.width as i32) / 2 - 1;
        let highest = self.board.highest_row() as i32;
        let spawn_y = if highest >= 20 {
            (highest + 1).min(BOARD_HEIGHT as i32 - 3)
        } else {
            20
        };

        if !self.board.fits(self.current, Rotation::North, spawn_x, spawn_y) {
            if !self.board.fits(self.current, Rotation::North, spawn_x, (spawn_y + 1).min(BOARD_HEIGHT as i32 - 1)) {
                self.game_over = true;
            }
        }

        // Garbage Resolution
        if cleared > 0 {
            let is_b2b_clear = cleared == 4 && was_b2b_active;
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
                let base_cancel_consumed = (garbage_canceled + cancel_multiplier - 1) / cancel_multiplier;
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
                self.board.spawn_garbage(lines_to_spawn as i32, self.last_garbage_hole_x as i32);
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

        (cleared, attack_sent)
    }
}
