use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::{Duration, Instant};
use egui::Color32;

pub mod engine {
    pub mod header;
    pub mod piece;
    pub mod board;
    pub mod movegen;
    pub mod state;
}

pub mod rl {
    pub mod features;
    pub mod agent;
    pub mod meta_agent;
}

pub mod gui {
    pub mod widgets;
}

use crate::engine::board::BOARD_HEIGHT;
use crate::engine::header::{Move, Piece, Rotation};
use crate::engine::state::GameState;
use crate::rl::features::Weights;
use crate::rl::agent::{find_best_move_meta, find_best_move_original};
use crate::rl::meta_agent::MetaPolicyNetwork;
use crate::gui::widgets::{render_board, render_piece_preview, render_queue_preview};

struct BattleApp {
    // Game states
    game_state: GameState,
    active_x: i32,
    active_y: i32,
    active_rot: Rotation,

    game_state_b: GameState,
    active_x_b: i32,
    active_y_b: i32,
    active_rot_b: Rotation,

    // Simulation settings & controls
    paused: bool,
    lookahead_depth: usize,
    moves_per_second: u32,
    bot_human_like: bool,
    board_width: usize,

    // Bot A AI control
    bot_thinking: bool,
    bot_result_rx: Option<Receiver<Option<(Move, bool)>>>,
    bot_target_move: Option<Move>,
    bot_needs_hold: bool,
    bot_animating: bool,
    bot_stuck_ticks: u32,
    last_bot_move_time: Instant,
    last_bot_step_time: Instant,
    meta_net_a: MetaPolicyNetwork,
    current_weights_a: Weights,

    // Bot B AI control
    bot_b_thinking: bool,
    bot_b_result_rx: Option<Receiver<Option<(Move, bool)>>>,
    bot_b_target_move: Option<Move>,
    bot_b_needs_hold: bool,
    bot_b_animating: bool,
    bot_b_stuck_ticks: u32,
    last_bot_b_move_time: Instant,
    last_bot_b_step_time: Instant,
    weights_b: Weights,

    // Stats
    games_played: u32,
    bot_a_wins: u32,
    bot_b_wins: u32,
    max_combo_achieved_a: u32,
    max_combo_achieved_b: u32,
    overall_max_combo_a: u32,
    overall_max_combo_b: u32,
    history_log: Vec<String>,
}

impl BattleApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Customize styling for premium dark aesthetics
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

        let default_width = 4;
        let mut app = Self {
            game_state: GameState::new(default_width),
            active_x: 0,
            active_y: 20,
            active_rot: Rotation::North,

            game_state_b: GameState::new(default_width),
            active_x_b: 0,
            active_y_b: 20,
            active_rot_b: Rotation::North,

            paused: false,
            lookahead_depth: 4,
            moves_per_second: 15,
            bot_human_like: true,
            board_width: default_width,

            bot_thinking: false,
            bot_result_rx: None,
            bot_target_move: None,
            bot_needs_hold: false,
            bot_animating: false,
            bot_stuck_ticks: 0,
            last_bot_move_time: Instant::now(),
            last_bot_step_time: Instant::now(),
            meta_net_a: MetaPolicyNetwork::new_random(),
            current_weights_a: Weights::default(),

            bot_b_thinking: false,
            bot_b_result_rx: None,
            bot_b_target_move: None,
            bot_b_needs_hold: false,
            bot_b_animating: false,
            bot_b_stuck_ticks: 0,
            last_bot_b_move_time: Instant::now(),
            last_bot_b_step_time: Instant::now(),
            weights_b: Weights::default(),

            games_played: 0,
            bot_a_wins: 0,
            bot_b_wins: 0,
            max_combo_achieved_a: 0,
            max_combo_achieved_b: 0,
            overall_max_combo_a: 0,
            overall_max_combo_b: 0,
            history_log: Vec::new(),
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
        self.last_bot_move_time = Instant::now();
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
        if self.paused {
            return;
        }

        // If game is over, record winner and reset
        if self.game_state.game_over || self.game_state_b.game_over {
            let winner = if self.game_state.game_over && self.game_state_b.game_over {
                "Draw".to_string()
            } else if self.game_state.game_over {
                "Bot B (Opponent)".to_string()
            } else {
                "Bot A (Player)".to_string()
            };

            if winner == "Bot A (Player)" {
                self.bot_a_wins += 1;
            } else if winner == "Bot B (Opponent)" {
                self.bot_b_wins += 1;
            }

            self.games_played += 1;
            
            // Add to history log
            let log_entry = format!(
                "Game #{}: {} Won (A Combo: {} max, B Combo: {} max)",
                self.games_played,
                if winner == "Bot A (Player)" { "Bot A" } else if winner == "Bot B (Opponent)" { "Bot B" } else { "Draw" },
                self.max_combo_achieved_a,
                self.max_combo_achieved_b
            );
            self.history_log.insert(0, log_entry);
            if self.history_log.len() > 10 {
                self.history_log.truncate(10);
            }

            // Reset states
            self.game_state = GameState::new(self.board_width);
            self.game_state_b = GameState::new(self.board_width);
            
            self.max_combo_achieved_a = 0;
            self.max_combo_achieved_b = 0;

            self.reset_active_piece();
            self.reset_active_piece_b();
            self.reset_bot_thinking();
            return;
        }

        self.max_combo_achieved_a = self.max_combo_achieved_a.max(self.game_state.combo);
        self.max_combo_achieved_b = self.max_combo_achieved_b.max(self.game_state_b.combo);
        self.overall_max_combo_a = self.overall_max_combo_a.max(self.game_state.combo);
        self.overall_max_combo_b = self.overall_max_combo_b.max(self.game_state_b.combo);

        // --- BOT A Step ---
        if self.bot_human_like {
            if self.bot_target_move.is_none() {
                let interval_ms = (1000.0 / self.moves_per_second as f32) as u64;
                if self.last_bot_move_time.elapsed() >= Duration::from_millis(interval_ms) {
                    if !self.bot_thinking {
                        let state_clone = self.game_state.clone();
                        let opp_clone = self.game_state_b.clone();
                        
                        // Dynamically evaluate weights for visualization
                        let inputs = MetaPolicyNetwork::extract_inputs(&state_clone, Some(&opp_clone));
                        self.current_weights_a = self.meta_net_a.forward(&inputs);

                        let meta_net_clone = self.meta_net_a.clone();
                        let depth = self.lookahead_depth;
                        let (tx, rx) = channel();
                        self.bot_result_rx = Some(rx);
                        self.bot_thinking = true;
                        thread::spawn(move || {
                            let res = find_best_move_meta(&state_clone, Some(&opp_clone), &meta_net_clone, depth);
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

                    // Dynamically evaluate weights for visualization
                    let inputs = MetaPolicyNetwork::extract_inputs(&state_clone, Some(&opp_clone));
                    self.current_weights_a = self.meta_net_a.forward(&inputs);

                    let meta_net_clone = self.meta_net_a.clone();
                    let depth = self.lookahead_depth;
                    let (tx, rx) = channel();
                    self.bot_result_rx = Some(rx);
                    self.bot_thinking = true;
                    thread::spawn(move || {
                        let res = find_best_move_meta(&state_clone, Some(&opp_clone), &meta_net_clone, depth);
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

        // --- BOT B Step ---
        if self.bot_human_like {
            if self.bot_b_target_move.is_none() {
                let interval_ms = (1000.0 / self.moves_per_second as f32) as u64;
                if self.last_bot_b_move_time.elapsed() >= Duration::from_millis(interval_ms) {
                    if !self.bot_b_thinking {
                        let state_clone = self.game_state_b.clone();
                        let opp_clone = self.game_state.clone();
                        let weights_clone = self.weights_b.clone();
                        let (tx, rx) = channel();
                        self.bot_b_result_rx = Some(rx);
                        self.bot_b_thinking = true;
                        thread::spawn(move || {
                            let res = find_best_move_original(&state_clone, Some(&opp_clone), &weights_clone, 6);
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
                    let (tx, rx) = channel();
                    self.bot_b_result_rx = Some(rx);
                    self.bot_b_thinking = true;
                    thread::spawn(move || {
                        let res = find_best_move_original(&state_clone, Some(&opp_clone), &weights_clone, 6);
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
    }
}

impl eframe::App for BattleApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();

        self.update_game_logic();

        // Top Header Panel
        egui::TopBottomPanel::top("header_panel").show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.heading(
                    egui::RichText::new("⚔️ ANTIGRAVITY 1v1 BOT BATTLE")
                        .size(24.0)
                        .strong()
                        .color(Color32::from_rgb(139, 92, 246)),
                );
                ui.label(
                    egui::RichText::new("Real-Time 4-Wide RL Bot vs Bot Battle Arena")
                        .color(Color32::from_rgb(140, 140, 160))
                        .size(13.0),
                );
                ui.add_space(8.0);
            });
        });

        // Left Panel: Bot A Arena
        egui::SidePanel::left("bot_a_panel")
            .width_range(280.0..=320.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.vertical_centered(|ui| {
                    ui.heading("🤖 BOT A (Player)");
                    ui.add_space(5.0);
                    let active_piece_opt = if self.game_state.game_over {
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
                    ui.label(format!("Lines Cleared: {}", self.game_state.lines_cleared));
                    let combo_text = if self.game_state.combo > 0 {
                        egui::RichText::new(format!("Current Combo: {} 🔥", self.game_state.combo - 1))
                            .color(Color32::from_rgb(245, 158, 11))
                            .strong()
                    } else {
                        egui::RichText::new("Current Combo: 0").color(Color32::GRAY)
                    };
                    ui.label(combo_text);
                    ui.label(format!("Round Max Combo: {}", self.max_combo_achieved_a));
                    ui.label(format!("Overall Max Combo: {}", self.overall_max_combo_a));
                });
            });

        // Right Panel: Bot B Arena
        egui::SidePanel::right("bot_b_panel")
            .width_range(280.0..=320.0)
            .show(ctx, |ui| {
                ui.add_space(10.0);
                ui.vertical_centered(|ui| {
                    ui.heading("🤖 BOT B (Opponent)");
                    ui.add_space(5.0);
                    let active_piece_opt_b = if self.game_state_b.game_over {
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
                    ui.label(format!("Lines Cleared: {}", self.game_state_b.lines_cleared));
                    let combo_text = if self.game_state_b.combo > 0 {
                        egui::RichText::new(format!("Current Combo: {} 🔥", self.game_state_b.combo - 1))
                            .color(Color32::from_rgb(245, 158, 11))
                            .strong()
                    } else {
                        egui::RichText::new("Current Combo: 0").color(Color32::GRAY)
                    };
                    ui.label(combo_text);
                    ui.label(format!("Round Max Combo: {}", self.max_combo_achieved_b));
                    ui.label(format!("Overall Max Combo: {}", self.overall_max_combo_b));
                });
            });

        // Center Control Dashboard
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            ui.heading("🎛️ Control Panel");
            ui.separator();

            // Stats Group
            ui.group(|ui| {
                ui.columns(3, |cols| {
                    cols[0].vertical_centered(|ui| {
                        ui.label("Games Played");
                        ui.heading(format!("{}", self.games_played));
                    });

                    // Win rate calculations
                    let total_wins = self.bot_a_wins + self.bot_b_wins;
                    let a_rate = if total_wins > 0 {
                        (self.bot_a_wins as f32 / total_wins as f32) * 100.0
                    } else {
                        0.0
                    };
                    let b_rate = if total_wins > 0 {
                        (self.bot_b_wins as f32 / total_wins as f32) * 100.0
                    } else {
                        0.0
                    };

                    cols[1].vertical_centered(|ui| {
                        ui.label("Bot A Wins");
                        ui.heading(format!("{} ({:.1}%)", self.bot_a_wins, a_rate));
                    });

                    cols[2].vertical_centered(|ui| {
                        ui.label("Bot B Wins");
                        ui.heading(format!("{} ({:.1}%)", self.bot_b_wins, b_rate));
                    });
                });
            });

            ui.add_space(15.0);

            // Simulation controls
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    let pause_btn_text = if self.paused { "▶ Resume" } else { "⏸ Pause" };
                    if ui.button(pause_btn_text).clicked() {
                        self.paused = !self.paused;
                    }

                    if ui.button("🔄 Reset Match").clicked() {
                        self.game_state = GameState::new(self.board_width);
                        self.game_state_b = GameState::new(self.board_width);
                        self.max_combo_achieved_a = 0;
                        self.max_combo_achieved_b = 0;
                        self.reset_active_piece();
                        self.reset_active_piece_b();
                        self.reset_bot_thinking();
                    }

                    if ui.button("🧼 Reset Win Stats").clicked() {
                        self.games_played = 0;
                        self.bot_a_wins = 0;
                        self.bot_b_wins = 0;
                        self.overall_max_combo_a = 0;
                        self.overall_max_combo_b = 0;
                        self.history_log.clear();
                    }
                });

                ui.add_space(10.0);

                // Speed Slider
                ui.horizontal(|ui| {
                    ui.label("Simulation Speed:");
                    ui.add(egui::Slider::new(&mut self.moves_per_second, 1..=100).text("moves/sec"));
                });

                // Human-like animation checkbox
                ui.checkbox(&mut self.bot_human_like, "Animate Movements (Human-like DAS/Rotation)");

                ui.add_space(10.0);

                // AI Lookahead Depth
                ui.horizontal(|ui| {
                    ui.label("AI Lookahead Depth:");
                    ui.radio_value(&mut self.lookahead_depth, 4, "4-Ply");
                    ui.radio_value(&mut self.lookahead_depth, 5, "5-Ply");
                    ui.radio_value(&mut self.lookahead_depth, 6, "6-Ply");
                });
            });

            ui.add_space(15.0);

            // Dynamic Weights Grid
            ui.heading("🧠 Meta-Agent Dynamic Weights (Bot A)");
            ui.separator();
            ui.group(|ui| {
                ui.label(egui::RichText::new("Comparing Bot A (Meta-Agent, dynamic) vs Bot B (Static Agent, fixed)").italics().color(Color32::GRAY));
                ui.add_space(5.0);
                egui::Grid::new("weights_grid").striped(true).show(ui, |ui| {
                    ui.label(egui::RichText::new("Feature").strong());
                    ui.label(egui::RichText::new("Bot A (Dynamic)").strong());
                    ui.label(egui::RichText::new("Bot B (Static)").strong());
                    ui.end_row();

                    let w_a = self.current_weights_a.to_array();
                    let w_b = self.weights_b.to_array();
                    let names = [
                        "Holes", "Cell Coveredness", "Height Max", "Height Avg",
                        "Bumpiness", "Row Transitions", "Col Transitions", "Well Depth",
                        "4-Wide Well", "Combo Reward", "Opp Height Max", "Opp Holes",
                        "Opp Pending Garbage", "Own Pending Garbage", "Own Queued Garbage"
                    ];

                    for i in 0..15 {
                        ui.label(names[i]);
                        let diff = w_a[i] - w_b[i];
                        let color = if diff.abs() < 0.01 {
                            Color32::GRAY
                        } else if diff > 0.0 {
                            Color32::from_rgb(52, 211, 153) // Green for higher
                        } else {
                            Color32::from_rgb(248, 113, 113) // Red for lower
                        };
                        ui.label(egui::RichText::new(format!("{:.2}", w_a[i])).color(color));
                        ui.label(format!("{:.2}", w_b[i]));
                        ui.end_row();
                    }
                });
            });

            ui.add_space(15.0);

            // Battle Log History
            ui.heading("📜 Recent Matches");
            ui.separator();
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                if self.history_log.is_empty() {
                    ui.label(egui::RichText::new("No matches completed yet. Let the bots battle!").italics().color(Color32::GRAY));
                } else {
                    for entry in &self.history_log {
                        ui.label(entry);
                    }
                }
            });
        });
    }
}

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1100.0, 850.0])
            .with_title("Antigravity 1v1 Bot Battle Arena")
            .with_min_inner_size([1000.0, 750.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Antigravity 1v1 Bot Battle Arena",
        native_options,
        Box::new(|cc| Box::new(BattleApp::new(cc))),
    )
}
