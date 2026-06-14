use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};
use egui::Color32;
use egui_plot::{Line, Plot, PlotPoints};

use crate::engine::board::BOARD_HEIGHT;
use crate::engine::header::{Move, Piece, Rotation};
use crate::engine::state::GameState;
use crate::rl::features::{Features, Weights};
use crate::rl::agent::{find_best_move, update_weights_td, GeneticOptimizer};
use crate::gui::widgets::{render_board, render_piece_preview, render_queue_preview};

pub enum TrainingMessage {
    TDIteration {
        weights: Weights,
        combo: u32,
        score: u32,
        pieces_placed: u32,
    },
    GeneticGeneration {
        weights: Weights,
        generation: u32,
        best_fitness: f32,
        avg_fitness: f32,
    },
    BattleIteration {
        weights_a: Weights,
        weights_b: Weights,
        pieces_placed_a: u32,
        pieces_placed_b: u32,
        max_combo_a: u32,
        max_combo_b: u32,
        winner: String,
    },
}

pub struct TetrisApp {
    // Game mode settings
    game_mode: String, // "Singleplayer" or "1v1 Battle"

    // Game state A (Player / Bot A)
    game_state: GameState,
    active_x: i32,
    active_y: i32,
    active_rot: Rotation,
    last_gravity_time: Instant,
    gravity_interval: Duration,

    // Game state B (Bot B)
    game_state_b: GameState,
    active_x_b: i32,
    active_y_b: i32,
    active_rot_b: Rotation,

    // Bot A control
    bot_enabled: bool,
    lookahead_depth: usize,
    weights: Weights,
    manual_override: bool,
    
    // Playback speed for visual bot watch
    moves_per_second: u32,
    last_bot_move_time: Instant,

    // Bot B control & playback
    bot_b_target_move: Option<Move>,
    bot_b_needs_hold: bool,
    bot_b_animating: bool,
    bot_b_stuck_ticks: u32,
    last_bot_b_step_time: Instant,
    last_bot_b_move_time: Instant,
    weights_b: Weights,

    // Settings
    board_width: usize,

    // Keyboard DAS/ARR/SDF configurations
    das_delay_ms: u32,
    arr_interval_ms: u32,
    sdf_factor: u32, // soft drop interval in ms

    // Keyboard state
    left_held_time: Option<Instant>,
    right_held_time: Option<Instant>,
    down_held_time: Option<Instant>,
    last_horizontal_shift_time: Instant,
    last_vertical_drop_time: Instant,
    das_active_left: bool,
    das_active_right: bool,
    prev_shift_down: bool,

    // Bot step-by-step path animation (Bot A)
    bot_human_like: bool,
    bot_animating: bool,
    bot_target_move: Option<Move>,
    bot_needs_hold: bool,
    last_bot_step_time: Instant,
    bot_stuck_ticks: u32,

    // Training states
    is_training: bool,
    training_mode: String, // "TD-Learning" or "Genetic" or "Self-Play"
    learning_rate: f32,
    discount_factor: f32,
    episodes_completed: u32,
    battle_games_completed: u32,
    bot_a_wins: u32,
    bot_b_wins: u32,
    
    // Learning stats history for plotting
    combo_history: Vec<f32>,
    survival_history: Vec<f32>,
    genetic_gen: u32,
    genetic_best_fitness: f32,

    // Background channels
    tx: Sender<TrainingMessage>,
    rx: Receiver<TrainingMessage>,
    stop_signal: std::sync::Arc<std::sync::atomic::AtomicBool>,

    // Async AI thinking states
    bot_thinking: bool,
    bot_result_rx: Option<Receiver<Option<(Move, bool)>>>,
    bot_b_thinking: bool,
    bot_b_result_rx: Option<Receiver<Option<(Move, bool)>>>,
}

impl TetrisApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Customize styling for premium aesthetics
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(18, 18, 22);
        visuals.window_fill = Color32::from_rgb(26, 26, 32);
        visuals.widgets.active.bg_fill = Color32::from_rgb(124, 58, 237); // Royal Purple accent
        visuals.widgets.hovered.bg_fill = Color32::from_rgb(109, 40, 217);
        visuals.widgets.inactive.bg_fill = Color32::from_rgb(34, 34, 42);
        visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);
        visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);
        visuals.widgets.active.rounding = egui::Rounding::same(6.0);
        cc.egui_ctx.set_visuals(visuals);

        let (tx, rx) = channel();
        let stop_signal = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

        let default_width = 4;
        let mut app = Self {
            game_mode: "Singleplayer".to_string(),

            game_state: GameState::new(default_width),
            active_x: 0,
            active_y: 20,
            active_rot: Rotation::North,
            last_gravity_time: Instant::now(),
            gravity_interval: Duration::from_millis(800),

            game_state_b: GameState::new(default_width),
            active_x_b: 0,
            active_y_b: 20,
            active_rot_b: Rotation::North,

            bot_enabled: true,
            lookahead_depth: 4,
            weights: Weights::default(),
            manual_override: false,

            moves_per_second: 5,
            last_bot_move_time: Instant::now(),

            bot_b_target_move: None,
            bot_b_needs_hold: false,
            bot_b_animating: false,
            bot_b_stuck_ticks: 0,
            last_bot_b_step_time: Instant::now(),
            last_bot_b_move_time: Instant::now(),
            weights_b: Weights::default(),

            board_width: default_width,
            
            das_delay_ms: 140,
            arr_interval_ms: 20,
            sdf_factor: 25, // soft drop every 25ms

            left_held_time: None,
            right_held_time: None,
            down_held_time: None,
            last_horizontal_shift_time: Instant::now(),
            last_vertical_drop_time: Instant::now(),
            das_active_left: false,
            das_active_right: false,
            prev_shift_down: false,

            bot_human_like: true,
            bot_animating: false,
            bot_target_move: None,
            bot_needs_hold: false,
            last_bot_step_time: Instant::now(),
            bot_stuck_ticks: 0,

            is_training: false,
            training_mode: "TD-Learning".to_string(),
            learning_rate: 0.05,
            discount_factor: 0.90,
            episodes_completed: 0,
            battle_games_completed: 0,
            bot_a_wins: 0,
            bot_b_wins: 0,

            combo_history: Vec::new(),
            survival_history: Vec::new(),
            genetic_gen: 0,
            genetic_best_fitness: 0.0,

            tx,
            rx,
            stop_signal,

            bot_thinking: false,
            bot_result_rx: None,
            bot_b_thinking: false,
            bot_b_result_rx: None,
        };

        app.reset_active_piece();
        app.reset_active_piece_b();
        app
    }

    fn reset_active_piece(&mut self) {
        self.active_rot = Rotation::North;
        self.active_x = (self.board_width as i32) / 2 - 1;
        let highest = self.game_state.board.highest_row() as i32;
        self.active_y = if highest >= 20 {
            (highest + 1).min(BOARD_HEIGHT as i32 - 3)
        } else {
            20
        };
        self.last_gravity_time = Instant::now();
    }

    fn reset_active_piece_b(&mut self) {
        self.active_rot_b = Rotation::North;
        self.active_x_b = (self.board_width as i32) / 2 - 1;
        let highest = self.game_state_b.board.highest_row() as i32;
        self.active_y_b = if highest >= 20 {
            (highest + 1).min(BOARD_HEIGHT as i32 - 3)
        } else {
            20
        };
        self.last_bot_b_move_time = Instant::now();
    }

    fn reset_bot_thinking(&mut self) {
        self.bot_thinking = false;
        self.bot_result_rx = None;
        self.bot_b_thinking = false;
        self.bot_b_result_rx = None;
        self.bot_target_move = None;
        self.bot_b_target_move = None;
        self.bot_animating = false;
        self.bot_b_animating = false;
    }

    fn try_shift_x(&mut self, dx: i32) -> bool {
        let new_x = self.active_x + dx;
        if self.game_state.board.fits(self.game_state.current, self.active_rot, new_x, self.active_y) {
            self.active_x = new_x;
            true
        } else {
            false
        }
    }

    fn handle_direction(&mut self, is_left: bool) {
        let now = Instant::now();
        let das_delay = Duration::from_millis(self.das_delay_ms as u64);
        let arr_interval = Duration::from_millis(self.arr_interval_ms as u64);
        let dx = if is_left { -1 } else { 1 };

        let held_time = if is_left { &mut self.left_held_time } else { &mut self.right_held_time };
        let das_active = if is_left { &mut self.das_active_left } else { &mut self.das_active_right };

        if held_time.is_none() {
            *held_time = Some(now);
            *das_active = false;
            self.try_shift_x(dx);
            self.last_horizontal_shift_time = now;
        } else {
            let start = held_time.unwrap();
            if !*das_active {
                if start.elapsed() >= das_delay {
                    *das_active = true;
                    self.try_shift_x(dx);
                    self.last_horizontal_shift_time = now;
                }
            } else {
                if self.arr_interval_ms == 0 {
                    while self.try_shift_x(dx) {}
                } else if self.last_horizontal_shift_time.elapsed() >= arr_interval {
                    self.try_shift_x(dx);
                    self.last_horizontal_shift_time = now;
                }
            }
        }
    }

    fn handle_human_input(&mut self, ctx: &egui::Context) {
        if self.game_state.game_over || self.bot_enabled {
            return;
        }

        // Horizontal Shift (DAS & ARR)
        let left_down = ctx.input(|i| i.key_down(egui::Key::ArrowLeft));
        let right_down = ctx.input(|i| i.key_down(egui::Key::ArrowRight));

        if left_down && right_down {
            let left_time = self.left_held_time.unwrap_or(Instant::now());
            let right_time = self.right_held_time.unwrap_or(Instant::now());
            if left_time > right_time {
                self.handle_direction(true);
                self.right_held_time = None;
                self.das_active_right = false;
            } else {
                self.handle_direction(false);
                self.left_held_time = None;
                self.das_active_left = false;
            }
        } else if left_down {
            self.handle_direction(true);
            self.right_held_time = None;
            self.das_active_right = false;
        } else if right_down {
            self.handle_direction(false);
            self.left_held_time = None;
            self.das_active_left = false;
        } else {
            self.left_held_time = None;
            self.das_active_left = false;
            self.right_held_time = None;
            self.das_active_right = false;
        }

        // Soft Drop (SDF Factor)
        let down_down = ctx.input(|i| i.key_down(egui::Key::ArrowDown));
        if down_down {
            let now = Instant::now();
            if self.down_held_time.is_none() {
                self.down_held_time = Some(now);
                if self.game_state.board.fits(self.game_state.current, self.active_rot, self.active_x, self.active_y - 1) {
                    self.active_y -= 1;
                }
                self.last_vertical_drop_time = now;
                self.last_gravity_time = now;
            } else {
                let soft_drop_interval = Duration::from_millis(self.sdf_factor as u64);
                if self.last_vertical_drop_time.elapsed() >= soft_drop_interval {
                    if self.game_state.board.fits(self.game_state.current, self.active_rot, self.active_x, self.active_y - 1) {
                        self.active_y -= 1;
                    }
                    self.last_vertical_drop_time = now;
                    self.last_gravity_time = now;
                }
            }
        } else {
            self.down_held_time = None;
        }

        // Rotations & Hold (discrete presses)
        let mut rotated = false;
        let mut new_rot = self.active_rot;

        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) || ctx.input(|i| i.key_pressed(egui::Key::X)) {
            new_rot = self.active_rot.rotate_cw();
            rotated = true;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Z)) {
            new_rot = self.active_rot.rotate_ccw();
            rotated = true;
        }
        let shift_down = ctx.input(|i| i.modifiers.shift);
        if shift_down && !self.prev_shift_down {
            if self.game_state.hold() {
                self.reset_active_piece();
            }
        }
        self.prev_shift_down = shift_down;

        if rotated && self.active_rot != new_rot {
            let kicks = if self.game_state.current == Piece::I {
                crate::engine::piece::get_srs_kicks_i(self.active_rot, new_rot)
            } else {
                crate::engine::piece::get_srs_kicks(self.active_rot, new_rot)
            };

            for &(dx, dy) in kicks.iter() {
                let kx = self.active_x + dx;
                let ky = self.active_y + dy;
                if self.game_state.board.fits(self.game_state.current, new_rot, kx, ky) {
                    self.active_x = kx;
                    self.active_y = ky;
                    self.active_rot = new_rot;
                    break;
                }
            }
        }

        // Hard Drop
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            let mut gy = self.active_y;
            while gy >= 0 {
                if !self.game_state.board.fits(self.game_state.current, self.active_rot, self.active_x, gy - 1) {
                    break;
                }
                gy -= 1;
            }
            let m = Move::new(self.game_state.current, self.active_rot, self.active_x, gy);
            if self.game_mode == "1v1 Battle" {
                self.game_state.do_move_battle(m, &mut self.game_state_b);
            } else {
                self.game_state.do_move(m);
            }
            self.reset_active_piece();
        }
    }

    fn try_shift_x_b(&mut self, dx: i32) -> bool {
        let new_x = self.active_x_b + dx;
        if self.game_state_b.board.fits(self.game_state_b.current, self.active_rot_b, new_x, self.active_y_b) {
            self.active_x_b = new_x;
            true
        } else {
            false
        }
    }

    fn update_game_logic(&mut self) {
        if self.game_mode == "1v1 Battle" {
            // If game is over for either, restart the battle
            if self.game_state.game_over || self.game_state_b.game_over {
                self.game_state = GameState::new(self.board_width);
                self.game_state_b = GameState::new(self.board_width);
                self.reset_active_piece();
                self.reset_active_piece_b();
                self.reset_bot_thinking();
                return;
            }

            // --- BOT A (Player Board) ---
            if self.bot_enabled {
                if self.bot_human_like {
                    if self.bot_target_move.is_none() {
                        let interval_ms = (1000.0 / self.moves_per_second as f32) as u64;
                        if self.last_bot_move_time.elapsed() >= Duration::from_millis(interval_ms) {
                            if !self.bot_thinking {
                                let state_clone = self.game_state.clone();
                                let opp_clone = self.game_state_b.clone();
                                let weights_clone = self.weights.clone();
                                let depth = self.lookahead_depth;
                                let (tx, rx) = channel();
                                self.bot_result_rx = Some(rx);
                                self.bot_thinking = true;
                                thread::spawn(move || {
                                    let res = find_best_move(&state_clone, Some(&opp_clone), &weights_clone, depth);
                                    let _ = tx.send(res);
                                });
                            }

                            let mut got_result = false;
                            let mut result = None;
                            if self.bot_thinking {
                                if let Some(rx) = &self.bot_result_rx {
                                    if let Ok(res) = rx.try_recv() {
                                        got_result = true;
                                        result = res;
                                    }
                                }
                            }

                            if got_result {
                                self.bot_thinking = false;
                                self.bot_result_rx = None;
                                if let Some((best_move, use_hold)) = result {
                                    self.bot_target_move = Some(best_move);
                                    self.bot_needs_hold = use_hold;
                                    self.bot_animating = true;
                                    self.last_bot_step_time = Instant::now();
                                    self.bot_stuck_ticks = 0;
                                    if self.active_y < best_move.y {
                                        self.active_y = best_move.y;
                                    }
                                }
                            }
                        }
                    }

                    if let Some(target) = self.bot_target_move {
                        let step_delay_ms = (400.0 / (self.moves_per_second as f32 * 6.0).max(1.0)) as u64;
                        let step_delay = Duration::from_millis(step_delay_ms.clamp(5, 150));

                        if self.last_bot_step_time.elapsed() >= step_delay {
                            self.last_bot_step_time = Instant::now();
                            let prev_state = (self.active_x, self.active_y, self.active_rot);

                            if self.bot_needs_hold {
                                self.game_state.hold();
                                self.reset_active_piece();
                                self.bot_needs_hold = false;
                                if self.active_y < target.y {
                                    self.active_y = target.y;
                                }
                                self.bot_stuck_ticks = 0;
                                return;
                            }

                            if self.active_rot != target.rotation {
                                let new_rot = self.active_rot.rotate_cw();
                                let kicks = if self.game_state.current == Piece::I {
                                    crate::engine::piece::get_srs_kicks_i(self.active_rot, new_rot)
                                } else {
                                    crate::engine::piece::get_srs_kicks(self.active_rot, new_rot)
                                };

                                for &(dx, dy) in kicks.iter() {
                                    let kx = self.active_x + dx;
                                    let ky = self.active_y + dy;
                                    if self.game_state.board.fits(self.game_state.current, new_rot, kx, ky) {
                                        self.active_x = kx;
                                        self.active_y = ky;
                                        self.active_rot = new_rot;
                                        break;
                                    }
                                }

                                if (self.active_x, self.active_y, self.active_rot) == prev_state {
                                    self.bot_stuck_ticks += 1;
                                    if self.bot_stuck_ticks >= 3 {
                                        self.game_state.do_move_battle(target, &mut self.game_state_b);
                                        self.reset_active_piece();
                                        self.bot_target_move = None;
                                        self.bot_animating = false;
                                        self.bot_stuck_ticks = 0;
                                        self.last_bot_move_time = Instant::now();
                                    }
                                } else {
                                    self.bot_stuck_ticks = 0;
                                }
                                return;
                            }

                            if self.active_x < target.x {
                                self.try_shift_x(1);
                            } else if self.active_x > target.x {
                                self.try_shift_x(-1);
                            }

                            if self.active_x == target.x && self.active_rot == target.rotation {
                                if self.active_y > target.y {
                                    self.active_y -= 1;
                                }
                            }

                            let current_state = (self.active_x, self.active_y, self.active_rot);
                            if current_state == prev_state {
                                self.bot_stuck_ticks += 1;
                                if self.bot_stuck_ticks >= 3 {
                                    self.game_state.do_move_battle(target, &mut self.game_state_b);
                                    self.reset_active_piece();
                                    self.bot_target_move = None;
                                    self.bot_animating = false;
                                    self.bot_stuck_ticks = 0;
                                    self.last_bot_move_time = Instant::now();
                                    return;
                                }
                            } else {
                                self.bot_stuck_ticks = 0;
                            }

                            if self.active_x == target.x && self.active_rot == target.rotation && self.active_y == target.y {
                                self.game_state.do_move_battle(target, &mut self.game_state_b);
                                self.reset_active_piece();
                                self.bot_target_move = None;
                                self.bot_animating = false;
                                self.bot_stuck_ticks = 0;
                                self.last_bot_move_time = Instant::now();
                            }
                        }
                    }
                } else {
                    let interval_ms = (1000.0 / self.moves_per_second as f32) as u64;
                    if self.last_bot_move_time.elapsed() >= Duration::from_millis(interval_ms) {
                        if !self.bot_thinking {
                            let state_clone = self.game_state.clone();
                            let opp_clone = self.game_state_b.clone();
                            let weights_clone = self.weights.clone();
                            let depth = self.lookahead_depth;
                            let (tx, rx) = channel();
                            self.bot_result_rx = Some(rx);
                            self.bot_thinking = true;
                            thread::spawn(move || {
                                let res = find_best_move(&state_clone, Some(&opp_clone), &weights_clone, depth);
                                let _ = tx.send(res);
                            });
                        }

                        let mut got_result = false;
                        let mut result = None;
                        if self.bot_thinking {
                            if let Some(rx) = &self.bot_result_rx {
                                if let Ok(res) = rx.try_recv() {
                                    got_result = true;
                                    result = res;
                                }
                            }
                        }

                        if got_result {
                            self.bot_thinking = false;
                            self.bot_result_rx = None;
                            if let Some((best_move, use_hold)) = result {
                                if use_hold {
                                    self.game_state.hold();
                                }
                                self.game_state.do_move_battle(best_move, &mut self.game_state_b);
                                self.reset_active_piece();
                                self.last_bot_move_time = Instant::now();
                            }
                        }
                    }
                }
            } else {
                // Human gravity for Player A in 1v1 Battle Mode
                if self.last_gravity_time.elapsed() >= self.gravity_interval {
                    if self.game_state.board.fits(self.game_state.current, self.active_rot, self.active_x, self.active_y - 1) {
                        self.active_y -= 1;
                        self.last_gravity_time = Instant::now();
                    } else {
                        let m = Move::new(self.game_state.current, self.active_rot, self.active_x, self.active_y);
                        self.game_state.do_move_battle(m, &mut self.game_state_b);
                        self.reset_active_piece();
                    }
                }
            }

            // --- BOT B (Opponent Board) ---
            if self.bot_human_like {
                if self.bot_b_target_move.is_none() {
                    let interval_ms = (1000.0 / self.moves_per_second as f32) as u64;
                    if self.last_bot_b_move_time.elapsed() >= Duration::from_millis(interval_ms) {
                        if !self.bot_b_thinking {
                            let state_clone = self.game_state_b.clone();
                            let opp_clone = self.game_state.clone();
                            let weights_clone = self.weights_b.clone();
                            let depth = self.lookahead_depth;
                            let (tx, rx) = channel();
                            self.bot_b_result_rx = Some(rx);
                            self.bot_b_thinking = true;
                            thread::spawn(move || {
                                let res = find_best_move(&state_clone, Some(&opp_clone), &weights_clone, depth);
                                let _ = tx.send(res);
                            });
                        }

                        let mut got_result = false;
                        let mut result = None;
                        if self.bot_b_thinking {
                            if let Some(rx) = &self.bot_b_result_rx {
                                if let Ok(res) = rx.try_recv() {
                                    got_result = true;
                                    result = res;
                                }
                            }
                        }

                        if got_result {
                            self.bot_b_thinking = false;
                            self.bot_b_result_rx = None;
                            if let Some((best_move, use_hold)) = result {
                                self.bot_b_target_move = Some(best_move);
                                self.bot_b_needs_hold = use_hold;
                                self.bot_b_animating = true;
                                self.last_bot_b_step_time = Instant::now();
                                self.bot_b_stuck_ticks = 0;
                                if self.active_y_b < best_move.y {
                                    self.active_y_b = best_move.y;
                                }
                            }
                        }
                    }
                }

                if let Some(target) = self.bot_b_target_move {
                    let step_delay_ms = (400.0 / (self.moves_per_second as f32 * 6.0).max(1.0)) as u64;
                    let step_delay = Duration::from_millis(step_delay_ms.clamp(5, 150));

                    if self.last_bot_b_step_time.elapsed() >= step_delay {
                        self.last_bot_b_step_time = Instant::now();
                        let prev_state = (self.active_x_b, self.active_y_b, self.active_rot_b);

                        if self.bot_b_needs_hold {
                            self.game_state_b.hold();
                            self.reset_active_piece_b();
                            self.bot_b_needs_hold = false;
                            if self.active_y_b < target.y {
                                self.active_y_b = target.y;
                            }
                            self.bot_b_stuck_ticks = 0;
                            return;
                        }

                        if self.active_rot_b != target.rotation {
                            let new_rot = self.active_rot_b.rotate_cw();
                            let kicks = if self.game_state_b.current == Piece::I {
                                crate::engine::piece::get_srs_kicks_i(self.active_rot_b, new_rot)
                            } else {
                                crate::engine::piece::get_srs_kicks(self.active_rot_b, new_rot)
                            };

                            for &(dx, dy) in kicks.iter() {
                                let kx = self.active_x_b + dx;
                                let ky = self.active_y_b + dy;
                                if self.game_state_b.board.fits(self.game_state_b.current, new_rot, kx, ky) {
                                    self.active_x_b = kx;
                                    self.active_y_b = ky;
                                    self.active_rot_b = new_rot;
                                    break;
                                }
                            }

                            if (self.active_x_b, self.active_y_b, self.active_rot_b) == prev_state {
                                self.bot_b_stuck_ticks += 1;
                                if self.bot_b_stuck_ticks >= 3 {
                                    self.game_state_b.do_move_battle(target, &mut self.game_state);
                                    self.reset_active_piece_b();
                                    self.bot_b_target_move = None;
                                    self.bot_b_animating = false;
                                    self.bot_b_stuck_ticks = 0;
                                    self.last_bot_b_move_time = Instant::now();
                                }
                            } else {
                                self.bot_b_stuck_ticks = 0;
                            }
                            return;
                        }

                        if self.active_x_b < target.x {
                            self.try_shift_x_b(1);
                        } else if self.active_x_b > target.x {
                            self.try_shift_x_b(-1);
                        }

                        if self.active_x_b == target.x && self.active_rot_b == target.rotation {
                            if self.active_y_b > target.y {
                                self.active_y_b -= 1;
                            }
                        }

                        let current_state = (self.active_x_b, self.active_y_b, self.active_rot_b);
                        if current_state == prev_state {
                            self.bot_b_stuck_ticks += 1;
                            if self.bot_b_stuck_ticks >= 3 {
                                self.game_state_b.do_move_battle(target, &mut self.game_state);
                                self.reset_active_piece_b();
                                self.bot_b_target_move = None;
                                self.bot_b_animating = false;
                                self.bot_b_stuck_ticks = 0;
                                self.last_bot_b_move_time = Instant::now();
                                return;
                            }
                        } else {
                            self.bot_b_stuck_ticks = 0;
                        }

                        if self.active_x_b == target.x && self.active_rot_b == target.rotation && self.active_y_b == target.y {
                            self.game_state_b.do_move_battle(target, &mut self.game_state);
                            self.reset_active_piece_b();
                            self.bot_b_target_move = None;
                            self.bot_b_animating = false;
                            self.bot_b_stuck_ticks = 0;
                            self.last_bot_b_move_time = Instant::now();
                        }
                    }
                }
            } else {
                let interval_ms = (1000.0 / self.moves_per_second as f32) as u64;
                if self.last_bot_b_move_time.elapsed() >= Duration::from_millis(interval_ms) {
                    if !self.bot_b_thinking {
                        let state_clone = self.game_state_b.clone();
                        let opp_clone = self.game_state.clone();
                        let weights_clone = self.weights_b.clone();
                        let depth = self.lookahead_depth;
                        let (tx, rx) = channel();
                        self.bot_b_result_rx = Some(rx);
                        self.bot_b_thinking = true;
                        thread::spawn(move || {
                            let res = find_best_move(&state_clone, Some(&opp_clone), &weights_clone, depth);
                            let _ = tx.send(res);
                        });
                    }

                    let mut got_result = false;
                    let mut result = None;
                    if self.bot_b_thinking {
                        if let Some(rx) = &self.bot_b_result_rx {
                            if let Ok(res) = rx.try_recv() {
                                got_result = true;
                                result = res;
                            }
                        }
                    }

                    if got_result {
                        self.bot_b_thinking = false;
                        self.bot_b_result_rx = None;
                        if let Some((best_move, use_hold)) = result {
                            if use_hold {
                                self.game_state_b.hold();
                            }
                            self.game_state_b.do_move_battle(best_move, &mut self.game_state);
                            self.reset_active_piece_b();
                            self.last_bot_b_move_time = Instant::now();
                        }
                    }
                }
            }
        } else {
            // Singleplayer Logic
            if self.game_state.game_over {
                return;
            }

            if self.bot_enabled {
                if self.bot_human_like {
                    if self.bot_target_move.is_none() {
                        let interval_ms = (1000.0 / self.moves_per_second as f32) as u64;
                        if self.last_bot_move_time.elapsed() >= Duration::from_millis(interval_ms) {
                            if !self.bot_thinking {
                                let state_clone = self.game_state.clone();
                                let weights_clone = self.weights.clone();
                                let depth = self.lookahead_depth;
                                let (tx, rx) = channel();
                                self.bot_result_rx = Some(rx);
                                self.bot_thinking = true;
                                thread::spawn(move || {
                                    let res = find_best_move(&state_clone, None, &weights_clone, depth);
                                    let _ = tx.send(res);
                                });
                            }

                            let mut got_result = false;
                            let mut result = None;
                            if self.bot_thinking {
                                if let Some(rx) = &self.bot_result_rx {
                                    if let Ok(res) = rx.try_recv() {
                                        got_result = true;
                                        result = res;
                                    }
                                }
                            }

                            if got_result {
                                self.bot_thinking = false;
                                self.bot_result_rx = None;
                                if let Some((best_move, use_hold)) = result {
                                    self.bot_target_move = Some(best_move);
                                    self.bot_needs_hold = use_hold;
                                    self.bot_animating = true;
                                    self.last_bot_step_time = Instant::now();
                                    self.bot_stuck_ticks = 0;
                                    if self.active_y < best_move.y {
                                        self.active_y = best_move.y;
                                    }
                                } else {
                                    self.game_state.game_over = true;
                                }
                            }
                        }
                    }

                    if let Some(target) = self.bot_target_move {
                        let step_delay_ms = (400.0 / (self.moves_per_second as f32 * 6.0).max(1.0)) as u64;
                        let step_delay = Duration::from_millis(step_delay_ms.clamp(5, 150));

                        if self.last_bot_step_time.elapsed() >= step_delay {
                            self.last_bot_step_time = Instant::now();
                            let prev_state = (self.active_x, self.active_y, self.active_rot);

                            if self.bot_needs_hold {
                                self.game_state.hold();
                                self.reset_active_piece();
                                self.bot_needs_hold = false;
                                if self.active_y < target.y {
                                    self.active_y = target.y;
                                }
                                self.bot_stuck_ticks = 0;
                                return;
                            }

                            if self.active_rot != target.rotation {
                                let new_rot = self.active_rot.rotate_cw();
                                let kicks = if self.game_state.current == Piece::I {
                                    crate::engine::piece::get_srs_kicks_i(self.active_rot, new_rot)
                                } else {
                                    crate::engine::piece::get_srs_kicks(self.active_rot, new_rot)
                                };

                                for &(dx, dy) in kicks.iter() {
                                    let kx = self.active_x + dx;
                                    let ky = self.active_y + dy;
                                    if self.game_state.board.fits(self.game_state.current, new_rot, kx, ky) {
                                        self.active_x = kx;
                                        self.active_y = ky;
                                        self.active_rot = new_rot;
                                        break;
                                    }
                                }

                                if (self.active_x, self.active_y, self.active_rot) == prev_state {
                                    self.bot_stuck_ticks += 1;
                                    if self.bot_stuck_ticks >= 3 {
                                        self.game_state.do_move(target);
                                        self.reset_active_piece();
                                        self.bot_target_move = None;
                                        self.bot_animating = false;
                                        self.bot_stuck_ticks = 0;
                                        self.last_bot_move_time = Instant::now();
                                    }
                                } else {
                                    self.bot_stuck_ticks = 0;
                                }
                                return;
                            }

                            if self.active_x < target.x {
                                self.try_shift_x(1);
                            } else if self.active_x > target.x {
                                self.try_shift_x(-1);
                            }

                            if self.active_x == target.x && self.active_rot == target.rotation {
                                if self.active_y > target.y {
                                    self.active_y -= 1;
                                }
                            }

                            let current_state = (self.active_x, self.active_y, self.active_rot);
                            if current_state == prev_state {
                                self.bot_stuck_ticks += 1;
                                if self.bot_stuck_ticks >= 3 {
                                    self.game_state.do_move(target);
                                    self.reset_active_piece();
                                    self.bot_target_move = None;
                                    self.bot_animating = false;
                                    self.bot_stuck_ticks = 0;
                                    self.last_bot_move_time = Instant::now();
                                    return;
                                }
                            } else {
                                self.bot_stuck_ticks = 0;
                            }

                            if self.active_x == target.x && self.active_rot == target.rotation && self.active_y == target.y {
                                self.game_state.do_move(target);
                                self.reset_active_piece();
                                self.bot_target_move = None;
                                self.bot_animating = false;
                                self.bot_stuck_ticks = 0;
                                self.last_bot_move_time = Instant::now();
                            }
                        }
                    }
                } else {
                    let interval_ms = (1000.0 / self.moves_per_second as f32) as u64;
                    if self.last_bot_move_time.elapsed() >= Duration::from_millis(interval_ms) {
                        if !self.bot_thinking {
                            let state_clone = self.game_state.clone();
                            let weights_clone = self.weights.clone();
                            let depth = self.lookahead_depth;
                            let (tx, rx) = channel();
                            self.bot_result_rx = Some(rx);
                            self.bot_thinking = true;
                            thread::spawn(move || {
                                let res = find_best_move(&state_clone, None, &weights_clone, depth);
                                let _ = tx.send(res);
                            });
                        }

                        let mut got_result = false;
                        let mut result = None;
                        if self.bot_thinking {
                            if let Some(rx) = &self.bot_result_rx {
                                if let Ok(res) = rx.try_recv() {
                                    got_result = true;
                                    result = res;
                                }
                            }
                        }

                        if got_result {
                            self.bot_thinking = false;
                            self.bot_result_rx = None;
                            if let Some((best_move, use_hold)) = result {
                                if use_hold {
                                    self.game_state.hold();
                                }
                                self.game_state.do_move(best_move);
                                self.reset_active_piece();
                                self.last_bot_move_time = Instant::now();
                            } else {
                                self.game_state.game_over = true;
                            }
                        }
                    }
                }
            } else {
                // Human gravity
                if self.last_gravity_time.elapsed() >= self.gravity_interval {
                    if self.game_state.board.fits(self.game_state.current, self.active_rot, self.active_x, self.active_y - 1) {
                        self.active_y -= 1;
                        self.last_gravity_time = Instant::now();
                    } else {
                        let m = Move::new(self.game_state.current, self.active_rot, self.active_x, self.active_y);
                        if self.game_mode == "1v1 Battle" {
                            self.game_state.do_move_battle(m, &mut self.game_state_b);
                        } else {
                            self.game_state.do_move(m);
                        }
                        self.reset_active_piece();
                    }
                }
            }
        }
    }

    fn check_background_training_thread(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                TrainingMessage::TDIteration { weights, combo, pieces_placed, .. } => {
                    self.episodes_completed += 1;
                    if !self.manual_override {
                        self.weights = weights;
                    }
                    
                    // Simple moving average smoothing for plots (smoothing factor = 0.05)
                    let smoothing = 0.05f32;
                    let last_c = self.combo_history.last().copied().unwrap_or(0.0);
                    let last_s = self.survival_history.last().copied().unwrap_or(0.0);
                    
                    self.combo_history.push(last_c * (1.0 - smoothing) + combo as f32 * smoothing);
                    self.survival_history.push(last_s * (1.0 - smoothing) + pieces_placed as f32 * smoothing);
                    
                    // Cap history to keep graph fast
                    if self.combo_history.len() > 1000 {
                        self.combo_history.remove(0);
                        self.survival_history.remove(0);
                    }
                }
                TrainingMessage::GeneticGeneration { weights, generation, best_fitness, .. } => {
                    self.genetic_gen = generation;
                    self.genetic_best_fitness = best_fitness;
                    if !self.manual_override {
                        self.weights = weights;
                    }
                    
                    self.combo_history.push(best_fitness / 100.0); // scaled for visualization
                    self.survival_history.push(best_fitness);
                    
                    if self.combo_history.len() > 1000 {
                        self.combo_history.remove(0);
                        self.survival_history.remove(0);
                    }
                }
                TrainingMessage::BattleIteration {
                    weights_a,
                    weights_b,
                    max_combo_a,
                    max_combo_b,
                    winner,
                    ..
                } => {
                    self.battle_games_completed += 1;
                    if !self.manual_override {
                        self.weights = weights_a;
                    }
                    self.weights_b = weights_b;

                    if winner == "Bot A" {
                        self.bot_a_wins += 1;
                    } else if winner == "Bot B" {
                        self.bot_b_wins += 1;
                    }

                    let smoothing = 0.05f32;
                    let last_c = self.combo_history.last().copied().unwrap_or(0.0);
                    let last_s = self.survival_history.last().copied().unwrap_or(0.0);
                    
                    self.combo_history.push(last_c * (1.0 - smoothing) + max_combo_a as f32 * smoothing);
                    self.survival_history.push(last_s * (1.0 - smoothing) + max_combo_b as f32 * smoothing);
                    
                    if self.combo_history.len() > 1000 {
                        self.combo_history.remove(0);
                        self.survival_history.remove(0);
                    }
                }
            }
        }
    }

    fn start_background_training(&mut self) {
        if self.is_training {
            return;
        }

        self.is_training = true;
        self.stop_signal.store(false, std::sync::atomic::Ordering::Relaxed);

        let tx = self.tx.clone();
        let stop_signal = self.stop_signal.clone();
        let training_mode = self.training_mode.clone();
        let board_width = self.board_width;
        let lr = self.learning_rate;
        let df = self.discount_factor;
        let initial_weights = self.weights.clone();
        let initial_weights_b = self.weights_b.clone();
        let depth = self.lookahead_depth;

        thread::spawn(move || {
            if training_mode == "TD-Learning" {
                let mut local_weights = initial_weights;
                while !stop_signal.load(std::sync::atomic::Ordering::Relaxed) {
                    let mut state = GameState::new(board_width);
                    let mut max_combo = 0;
                    
                    while !state.game_over && state.pieces_placed < 1000 {
                        // TD Step
                        let current_features = Features::evaluate_state(&state, None);
                        if let Some((best_move, use_hold)) = find_best_move(&state, None, &local_weights, depth) {
                            if use_hold {
                                state.hold();
                            }
                            let cleared = state.do_move(best_move);
                            max_combo = max_combo.max(state.combo);

                            // Compute reward
                            let mut reward = cleared as f32 * 50.0 + state.combo as f32 * 100.0 + 2.0;
                            if state.game_over {
                                reward = -500.0;
                            }
                            
                            let next_features = Features::evaluate_state(&state, None);
                            update_weights_td(&mut local_weights, &current_features, &next_features, reward, lr, df);
                        } else {
                            state.game_over = true;
                        }
                    }

                    if tx.send(TrainingMessage::TDIteration {
                        weights: local_weights.clone(),
                        combo: max_combo,
                        score: state.score,
                        pieces_placed: state.pieces_placed,
                    }).is_err() {
                        break;
                    }
                }
            } else if training_mode == "Genetic" {
                // Genetic evolution
                let mut optimizer = GeneticOptimizer::new(16);
                optimizer.best_weights = initial_weights;
                while !stop_signal.load(std::sync::atomic::Ordering::Relaxed) {
                    optimizer.evolve(board_width, 2);
                    if tx.send(TrainingMessage::GeneticGeneration {
                        weights: optimizer.best_weights.clone(),
                        generation: optimizer.generation,
                        best_fitness: optimizer.best_fitness,
                        avg_fitness: 0.0,
                    }).is_err() {
                        break;
                    }
                }
            } else if training_mode == "Self-Play" {
                let mut local_weights_a = initial_weights;
                let mut local_weights_b = initial_weights_b;
                while !stop_signal.load(std::sync::atomic::Ordering::Relaxed) {
                    let mut state_a = GameState::new(board_width);
                    let mut state_b = GameState::new(board_width);
                    let mut max_combo_a = 0;
                    let mut max_combo_b = 0;
                    
                    while !state_a.game_over && !state_b.game_over && state_a.pieces_placed < 1000 && state_b.pieces_placed < 1000 {
                        // --- Bot A step ---
                        let current_features_a = Features::evaluate_state(&state_a, Some(&state_b));
                        if let Some((best_move_a, use_hold_a)) = find_best_move(&state_a, Some(&state_b), &local_weights_a, depth) {
                            if use_hold_a {
                                state_a.hold();
                            }
                            let (cleared_a, _) = state_a.do_move_battle(best_move_a, &mut state_b);
                            max_combo_a = max_combo_a.max(state_a.combo);
                            
                            // TD update for A
                            let mut reward_a = cleared_a as f32 * 50.0 + state_a.combo as f32 * 100.0 + 2.0;
                            if state_a.game_over {
                                reward_a = -1000.0;
                            } else if state_b.game_over {
                                reward_a = 1000.0;
                            }
                            let next_features_a = Features::evaluate_state(&state_a, Some(&state_b));
                            update_weights_td(&mut local_weights_a, &current_features_a, &next_features_a, reward_a, lr, df);
                        } else {
                            state_a.game_over = true;
                        }

                        if state_a.game_over || state_b.game_over {
                            break;
                        }

                        // --- Bot B step ---
                        let current_features_b = Features::evaluate_state(&state_b, Some(&state_a));
                        if let Some((best_move_b, use_hold_b)) = find_best_move(&state_b, Some(&state_a), &local_weights_b, depth) {
                            if use_hold_b {
                                state_b.hold();
                            }
                            let (cleared_b, _) = state_b.do_move_battle(best_move_b, &mut state_a);
                            max_combo_b = max_combo_b.max(state_b.combo);
                            
                            // TD update for B
                            let mut reward_b = cleared_b as f32 * 50.0 + state_b.combo as f32 * 100.0 + 2.0;
                            if state_b.game_over {
                                reward_b = -1000.0;
                            } else if state_a.game_over {
                                reward_b = 1000.0;
                            }
                            let next_features_b = Features::evaluate_state(&state_b, Some(&state_a));
                            update_weights_td(&mut local_weights_b, &current_features_b, &next_features_b, reward_b, lr, df);
                        } else {
                            state_b.game_over = true;
                        }
                    }

                    let winner = if state_a.game_over && state_b.game_over {
                        "Draw".to_string()
                    } else if state_a.game_over {
                        "Bot B".to_string()
                    } else if state_b.game_over {
                        "Bot A".to_string()
                    } else {
                        "Draw".to_string()
                    };

                    if tx.send(TrainingMessage::BattleIteration {
                        weights_a: local_weights_a.clone(),
                        weights_b: local_weights_b.clone(),
                        pieces_placed_a: state_a.pieces_placed,
                        pieces_placed_b: state_b.pieces_placed,
                        max_combo_a,
                        max_combo_b,
                        winner,
                    }).is_err() {
                        break;
                    }
                }
            }
        });
    }

    fn stop_background_training(&mut self) {
        if self.is_training {
            self.stop_signal.store(true, std::sync::atomic::Ordering::Relaxed);
            self.is_training = false;
        }
    }
}

impl eframe::App for TetrisApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Continuous repaint if game is running or training to ensure smooth animation and graphs
        ctx.request_repaint();

        self.handle_human_input(ctx);
        if !self.is_training {
            self.update_game_logic();
        } else {
            self.check_background_training_thread();
        }

        // Layout panels
        egui::TopBottomPanel::top("header_panel").show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.heading(
                    egui::RichText::new("🚀 ANTIGRAVITY 4-WIDE RL BOT")
                        .size(24.0)
                        .strong()
                        .color(Color32::from_rgb(139, 92, 246)),
                );
                ui.label(
                    egui::RichText::new("Reinforcement Learning Optimized Tetris Bot Dashboard")
                        .color(Color32::from_rgb(140, 140, 160))
                        .size(13.0),
                );
                ui.add_space(8.0);
            });
        });

        // Left Sidebar: Game Board & Stats
        let left_panel_width = if self.game_mode == "1v1 Battle" {
            560.0..=660.0
        } else {
            280.0..=320.0
        };
        egui::SidePanel::left("left_game_panel")
            .width_range(left_panel_width)
            .show(ctx, |ui| {
                if self.game_mode == "1v1 Battle" {
                    ui.columns(2, |cols| {
                        // Left Column: Player / Bot A
                        cols[0].vertical(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("Player A / Bot A");
                                let active_piece_opt = if self.game_state.game_over || self.is_training {
                                    None
                                } else {
                                    Some((self.game_state.current, self.active_rot, self.active_x, self.active_y))
                                };
                                render_board(ui, &self.game_state.board, active_piece_opt, self.game_state.pending_garbage, self.game_state.queued_garbage);
                            });

                            ui.add_space(10.0);
                            ui.columns(2, |sub_cols| {
                                sub_cols[0].vertical(|ui| {
                                    render_piece_preview(ui, "HOLD A", self.game_state.hold);
                                });
                                sub_cols[1].vertical(|ui| {
                                    render_queue_preview(ui, "NEXT A", &self.game_state.queue);
                                });
                            });

                            ui.add_space(10.0);
                            ui.group(|ui| {
                                ui.label(format!("Score: {}", self.game_state.score));
                                ui.label(format!("Lines: {}", self.game_state.lines_cleared));
                                let combo_text = if self.game_state.combo > 0 {
                                    egui::RichText::new(format!("Combo: {} 🔥", self.game_state.combo - 1))
                                        .color(Color32::from_rgb(245, 158, 11))
                                        .strong()
                                } else {
                                    egui::RichText::new("Combo: 0").color(Color32::GRAY)
                                };
                                ui.label(combo_text);
                            });
                        });

                        // Right Column: Bot B
                        cols[1].vertical(|ui| {
                            ui.vertical_centered(|ui| {
                                ui.heading("Bot B (Opponent)");
                                let active_piece_opt_b = if self.game_state_b.game_over || self.is_training {
                                    None
                                } else {
                                    Some((self.game_state_b.current, self.active_rot_b, self.active_x_b, self.active_y_b))
                                };
                                render_board(ui, &self.game_state_b.board, active_piece_opt_b, self.game_state_b.pending_garbage, self.game_state_b.queued_garbage);
                            });

                            ui.add_space(10.0);
                            ui.columns(2, |sub_cols| {
                                sub_cols[0].vertical(|ui| {
                                    render_piece_preview(ui, "HOLD B", self.game_state_b.hold);
                                });
                                sub_cols[1].vertical(|ui| {
                                    render_queue_preview(ui, "NEXT B", &self.game_state_b.queue);
                                });
                            });

                            ui.add_space(10.0);
                            ui.group(|ui| {
                                ui.label(format!("Score: {}", self.game_state_b.score));
                                ui.label(format!("Lines: {}", self.game_state_b.lines_cleared));
                                let combo_text = if self.game_state_b.combo > 0 {
                                    egui::RichText::new(format!("Combo: {} 🔥", self.game_state_b.combo - 1))
                                        .color(Color32::from_rgb(245, 158, 11))
                                        .strong()
                                } else {
                                    egui::RichText::new("Combo: 0").color(Color32::GRAY)
                                };
                                ui.label(combo_text);
                            });
                        });
                    });
                } else {
                    ui.add_space(10.0);
                    ui.vertical_centered(|ui| {
                        let active_piece_opt = if self.game_state.game_over || self.is_training {
                            None
                        } else {
                            Some((self.game_state.current, self.active_rot, self.active_x, self.active_y))
                        };
                        render_board(ui, &self.game_state.board, active_piece_opt, self.game_state.pending_garbage, self.game_state.queued_garbage);
                    });

                    ui.add_space(15.0);
                    
                    // Hold and Next queue preview
                    ui.columns(2, |columns| {
                        columns[0].vertical(|ui| {
                            render_piece_preview(ui, "HOLD", self.game_state.hold);
                        });
                        columns[1].vertical(|ui| {
                            render_queue_preview(ui, "NEXT", &self.game_state.queue);
                        });
                    });

                    ui.add_space(15.0);

                    // Stats Dashboard
                    ui.group(|ui| {
                        ui.heading("📊 Live Stats");
                        ui.separator();
                        
                        ui.horizontal(|ui| {
                            ui.label("Status:");
                            let status_text = if self.is_training {
                                egui::RichText::new("Training Bot...").color(Color32::from_rgb(245, 158, 11))
                            } else if self.game_state.game_over {
                                egui::RichText::new("GAME OVER").color(Color32::from_rgb(239, 68, 68))
                            } else {
                                egui::RichText::new("Playing").color(Color32::from_rgb(16, 185, 129))
                            };
                            ui.label(status_text.strong());
                        });

                        ui.label(format!("Score: {}", self.game_state.score));
                        ui.label(format!("Lines Cleared: {}", self.game_state.lines_cleared));
                        
                        let combo_text = if self.game_state.combo > 0 {
                            egui::RichText::new(format!("Combo: {} 🔥", self.game_state.combo - 1))
                                .color(Color32::from_rgb(245, 158, 11))
                                .strong()
                        } else {
                            egui::RichText::new("Combo: 0").color(Color32::GRAY)
                        };
                        ui.label(combo_text);

                        ui.label(format!("B2B Active: {}", if self.game_state.b2b { "Yes" } else { "No" }));
                        ui.label(format!("Pieces Placed: {}", self.game_state.pieces_placed));
                    });
                }

                ui.add_space(10.0);
                
                // Game controls
                ui.horizontal(|ui| {
                    if ui.button("🔄 Restart Game").clicked() {
                        self.game_state = GameState::new(self.board_width);
                        self.game_state_b = GameState::new(self.board_width);
                        self.reset_active_piece();
                        self.reset_active_piece_b();
                        self.reset_bot_thinking();
                    }
                    if ui.button("⏹ Stop Training").clicked() {
                        self.stop_background_training();
                    }
                });
            });

        // Right Panel: Bot Settings & Live Weights
        egui::SidePanel::right("right_agent_panel")
            .width_range(300.0..=350.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.heading("🧠 RL Agent & Optimizer");
                ui.separator();

                ui.checkbox(&mut self.bot_enabled, "Bot Controlled Play");
                if self.bot_enabled {
                    ui.checkbox(&mut self.bot_human_like, "Human-like Animation");
                }
                ui.checkbox(&mut self.manual_override, "Manual Weights Override");

                ui.add_space(10.0);
                ui.label("Lookahead Depth:");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut self.lookahead_depth, 1, "1-Ply");
                    ui.radio_value(&mut self.lookahead_depth, 2, "2-Ply");
                    ui.radio_value(&mut self.lookahead_depth, 3, "3-Ply");
                    ui.radio_value(&mut self.lookahead_depth, 4, "4-Ply");
                    ui.radio_value(&mut self.lookahead_depth, 5, "5-Ply");
                });

                ui.add_space(10.0);
                ui.label(format!("Bot Playback Speed: {} PPS (Pieces/sec)", self.moves_per_second));
                ui.add(egui::Slider::new(&mut self.moves_per_second, 1..=40));

                ui.add_space(10.0);

                // Keyboard handling configurations
                ui.group(|ui| {
                    ui.heading("⌨ Keyboard Settings (Pro)");
                    ui.add(egui::Slider::new(&mut self.das_delay_ms, 50..=300).text("DAS Delay (ms)"));
                    ui.add(egui::Slider::new(&mut self.arr_interval_ms, 0..=80).text("ARR Speed (ms)"));
                    ui.add(egui::Slider::new(&mut self.sdf_factor, 2..=100).text("Soft Drop Delay (ms)"));
                    ui.label(egui::RichText::new("💡 Tip: Set ARR to 0 for instant wall shift!")
                        .color(Color32::from_rgb(160, 160, 170))
                        .size(10.5));
                });

                ui.separator();
                
                // Board configurations
                ui.heading("⚙ Game Configuration");
                ui.horizontal(|ui| {
                    ui.label("Board Width:");
                    if ui.radio_value(&mut self.board_width, 4, "4 (Downstack Practice)").clicked() {
                        self.game_state = GameState::new(self.board_width);
                        self.game_state_b = GameState::new(self.board_width);
                        self.reset_active_piece();
                        self.reset_active_piece_b();
                        self.stop_background_training();
                        self.reset_bot_thinking();
                    }
                    if ui.radio_value(&mut self.board_width, 10, "10 (4-Wide Stack)").clicked() {
                        self.game_state = GameState::new(self.board_width);
                        self.game_state_b = GameState::new(self.board_width);
                        self.reset_active_piece();
                        self.reset_active_piece_b();
                        self.stop_background_training();
                        self.reset_bot_thinking();
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Game Mode:");
                    if ui.radio_value(&mut self.game_mode, "Singleplayer".to_string(), "Singleplayer").clicked() {
                        self.game_state = GameState::new(self.board_width);
                        self.game_state_b = GameState::new(self.board_width);
                        self.reset_active_piece();
                        self.reset_active_piece_b();
                        self.stop_background_training();
                        self.reset_bot_thinking();
                    }
                    if ui.radio_value(&mut self.game_mode, "1v1 Battle".to_string(), "1v1 Battle").clicked() {
                        self.game_state = GameState::new(self.board_width);
                        self.game_state_b = GameState::new(self.board_width);
                        self.reset_active_piece();
                        self.reset_active_piece_b();
                        self.stop_background_training();
                        self.reset_bot_thinking();
                    }
                });

                ui.add_space(10.0);

                // Training Controls
                ui.group(|ui| {
                    ui.heading("🎯 Background Trainer");
                    ui.horizontal(|ui| {
                        ui.label("Method:");
                        ui.radio_value(&mut self.training_mode, "TD-Learning".to_string(), "TD-Learning");
                        ui.radio_value(&mut self.training_mode, "Genetic".to_string(), "Genetic");
                        ui.radio_value(&mut self.training_mode, "Self-Play".to_string(), "Self-Play");
                    });

                    if self.training_mode == "TD-Learning" || self.training_mode == "Self-Play" {
                        ui.add(egui::Slider::new(&mut self.learning_rate, 0.001..=0.2).text("Learning Rate (α)"));
                        ui.add(egui::Slider::new(&mut self.discount_factor, 0.5..=0.99).text("Discount Factor (γ)"));
                    }

                    ui.add_space(5.0);
                    
                    if !self.is_training {
                        if ui.button("🔥 Start Fast Training (Uncapped)").clicked() {
                            self.start_background_training();
                        }
                    } else {
                        if ui.button("⏹ Stop Training").clicked() {
                            self.stop_background_training();
                        }
                    }

                    if self.training_mode == "TD-Learning" {
                        ui.label(format!("Games Completed: {}", self.episodes_completed));
                    } else if self.training_mode == "Genetic" {
                        ui.label(format!("Generations Evolved: {}", self.genetic_gen));
                        ui.label(format!("Best Fitness: {:.1}", self.genetic_best_fitness));
                    } else if self.training_mode == "Self-Play" {
                        ui.label(format!("Battles Completed: {}", self.battle_games_completed));
                        ui.label(format!("Bot A Wins: {}", self.bot_a_wins));
                        ui.label(format!("Bot B Wins: {}", self.bot_b_wins));
                        let total_wins = (self.bot_a_wins + self.bot_b_wins).max(1);
                        ui.label(format!("Bot A Win Rate: {:.1}%", (self.bot_a_wins as f32 / total_wins as f32) * 100.0));
                    }
                });

                ui.add_space(10.0);

                // Live weights parameters
                ui.group(|ui| {
                    ui.heading("🎚 Live Heuristic Weights");
                    ui.label("Values show what the bot prioritizes (positive is good, negative is bad)");
                    ui.separator();

                    let mut arr = self.weights.to_array();
                    let labels = [
                        "Holes penalty", "Cell coveredness", "Max Height penalty", "Average Height",
                        "Bumpiness", "Row transitions", "Column transitions", "Well depth",
                        "4-wide Well reward", "Combo reward", "Opponent Height", "Opponent Holes",
                        "Opponent Garbage", "Own Garbage penalty", "Own Queued penalty"
                    ];

                    let mut changed = false;
                    for i in 0..15 {
                        ui.horizontal(|ui| {
                            ui.label(labels[i]);
                            ui.add_space(10.0);
                            if self.manual_override {
                                if ui.add(egui::Slider::new(&mut arr[i], -150.0..=150.0)).changed() {
                                    changed = true;
                                }
                            } else {
                                ui.label(egui::RichText::new(format!("{:.2}", arr[i])).strong().color(Color32::from_rgb(162, 230, 255)));
                            }
                        });
                    }

                    if changed && self.manual_override {
                        self.weights = Weights::from_array(arr);
                    }

                    if ui.button("🔄 Reset Weights").clicked() {
                        self.weights = Weights::default();
                    }
                });
            });

        // Center/Bottom Panel: Plots and Learning curve
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("📈 Performance Dashboard");
            ui.label("Real-time learning curves showing smoothed average combo and survival metrics");
            
            ui.add_space(10.0);
            
            let mut combo_points = Vec::new();
            let mut survival_points = Vec::new();
            
            for (idx, &val) in self.combo_history.iter().enumerate() {
                combo_points.push([idx as f64, val as f64]);
            }
            for (idx, &val) in self.survival_history.iter().enumerate() {
                survival_points.push([idx as f64, val as f64]);
            }

            let name_a = if self.training_mode == "Self-Play" {
                "Bot A Smoothed Max Combo"
            } else {
                "Smoothed Max Combo"
            };
            let name_b = if self.training_mode == "Self-Play" {
                "Bot B Smoothed Max Combo"
            } else {
                "Smoothed Pieces Placed"
            };

            let combo_line = Line::new(PlotPoints::new(combo_points))
                .color(Color32::from_rgb(245, 158, 11))
                .name(name_a);
            let survival_line = Line::new(PlotPoints::new(survival_points))
                .color(Color32::from_rgb(139, 92, 246))
                .name(name_b);

            Plot::new("performance_plot")
                .view_aspect(2.2)
                .legend(egui_plot::Legend::default())
                .show(ui, |plot_ui| {
                    plot_ui.line(combo_line);
                    plot_ui.line(survival_line);
                });
        });
    }
}
