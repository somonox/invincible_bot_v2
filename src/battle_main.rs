use egui::Color32;
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

pub mod engine {
    pub mod battle;
    pub mod board;
    pub mod header;
    pub mod movegen;
    pub mod piece;
    pub mod state;
}

pub mod rl {
    pub mod agent;
    pub mod features;
    pub mod meta_agent;
    pub mod search;
}

pub mod gui {
    pub mod widgets;
}

use crate::engine::board::BOARD_HEIGHT;
use crate::engine::header::{Move, Piece, Rotation, SpinMode};
use crate::engine::state::GameState;
use crate::gui::widgets::{render_board, render_piece_preview, render_queue_preview};
use crate::rl::features::Weights;
use crate::rl::meta_agent::MetaPolicyNetwork;
use crate::rl::search::{
    find_best_move_for_objective, find_hybrid_move, Evaluator, HybridMode, Objective,
};

struct BotPlan {
    choice: Option<(Move, bool)>,
    elapsed: Duration,
}

struct PairedPlan {
    a: BotPlan,
    b: BotPlan,
    depth: usize,
    hybrid_mode: Option<HybridMode>,
}

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

    // A turn is published only when both searches finish.
    pair_rx: Option<Receiver<PairedPlan>>,
    pair_plan: Option<PairedPlan>,
    animation_ready: [bool; 2],
    animation_stuck: [u32; 2],
    last_pair_move_time: Instant,
    last_animation_step_time: Instant,
    last_search_a: Duration,
    last_search_b: Duration,
    active_depth: usize,
    opponent_combo_threshold: u32,
    hybrid_mode: Option<HybridMode>,
    meta_net_a: MetaPolicyNetwork,
    weights_b: Weights,

    // Stats
    total_pcs_a: u32,
    total_pcs_b: u32,
    games_played: u32,
    bot_a_wins: u32,
    bot_b_wins: u32,
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

        Self::new_match(4)
    }

    fn new_match(default_width: usize) -> Self {
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
            lookahead_depth: 6,
            moves_per_second: 15,
            bot_human_like: true,
            board_width: default_width,

            pair_rx: None,
            pair_plan: None,
            animation_ready: [false; 2],
            animation_stuck: [0; 2],
            last_pair_move_time: Instant::now(),
            last_animation_step_time: Instant::now(),
            last_search_a: Duration::ZERO,
            last_search_b: Duration::ZERO,
            active_depth: 6,
            opponent_combo_threshold: crate::rl::search::DEFAULT_OPPONENT_COMBO_THRESHOLD,
            hybrid_mode: None,
            meta_net_a: MetaPolicyNetwork::default(),
            weights_b: Weights::default(),

            total_pcs_a: 0,
            total_pcs_b: 0,
            games_played: 0,
            bot_a_wins: 0,
            bot_b_wins: 0,
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
    }

    fn reset_bot_thinking(&mut self) {
        self.pair_rx = None;
        self.pair_plan = None;
        self.hybrid_mode = None;
        self.animation_ready = [false; 2];
        self.animation_stuck = [0; 2];
        self.last_pair_move_time = Instant::now();
        self.last_animation_step_time = Instant::now();
    }

    fn start_paired_search(&mut self) {
        let state_a = self.game_state.clone();
        let state_b = self.game_state_b.clone();
        let meta_net = self.meta_net_a.clone();
        let weights_b = self.weights_b.clone();
        let threshold = self.opponent_combo_threshold;
        // Empty hold consumes another visible piece. Give both algorithms the
        // same usable horizon, including when the selector requests 6-ply.
        let visible_depth = |state: &GameState| {
            let visible = 1 + state.queue.len();
            let reserve = usize::from(state.hold.is_none() && !state.hold_used && visible > 1);
            visible - reserve
        };
        let depth = self
            .lookahead_depth
            .max(1)
            .min(visible_depth(&state_a))
            .min(visible_depth(&state_b));
        self.active_depth = depth;
        let (tx, rx) = channel();
        self.pair_rx = Some(rx);
        thread::spawn(move || {
            let a = state_a.clone();
            let b = state_b.clone();
            let worker_a = thread::spawn(move || {
                let start = Instant::now();
                let choice = find_best_move_for_objective(
                    &a,
                    Some(&b),
                    Evaluator::Meta(&meta_net),
                    depth,
                    Objective::Combo,
                );
                BotPlan {
                    choice,
                    elapsed: start.elapsed(),
                }
            });
            let start = Instant::now();
            let plan = find_hybrid_move(
                &state_b,
                Some(&state_a),
                Evaluator::Static(&weights_b),
                depth,
                threshold,
            );
            let choice = plan.map(|p| p.choice);
            let hybrid_mode = plan.map(|p| p.mode);
            let b = BotPlan {
                choice,
                elapsed: start.elapsed(),
            };
            // On a worker failure, dropping tx lets the UI pause rather than hang.
            if let Ok(a) = worker_a.join() {
                let _ = tx.send(PairedPlan {
                    a,
                    b,
                    depth,
                    hybrid_mode,
                });
            }
        });
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
                "Game #{}: {} Won (A PCs: {}, B PCs: {})",
                self.games_played,
                if winner == "Bot A (Player)" {
                    "Bot A"
                } else if winner == "Bot B (Opponent)" {
                    "Bot B"
                } else {
                    "Draw"
                },
                self.game_state.perfect_clears,
                self.game_state_b.perfect_clears
            );
            self.history_log.insert(0, log_entry);
            if self.history_log.len() > 10 {
                self.history_log.truncate(10);
            }

            // Reset states
            let spin_mode = self.game_state.spin_mode;
            self.game_state = GameState::new(self.board_width);
            self.game_state_b = GameState::new(self.board_width);
            self.game_state.spin_mode = spin_mode;
            self.game_state_b.spin_mode = spin_mode;

            self.reset_active_piece();
            self.reset_active_piece_b();
            self.reset_bot_thinking();
            return;
        }

        if self.pair_plan.is_none() {
            if self.pair_rx.is_none() {
                self.start_paired_search();
            }
            let result = self.pair_rx.as_ref().unwrap().try_recv();
            match result {
                Ok(plan) => {
                    self.pair_rx = None;
                    self.active_depth = plan.depth;
                    self.hybrid_mode = plan.hybrid_mode;
                    self.last_search_a = plan.a.elapsed;
                    self.last_search_b = plan.b.elapsed;
                    let (Some((target_a, hold_a)), Some((target_b, hold_b))) =
                        (plan.a.choice, plan.b.choice)
                    else {
                        self.game_state.game_over = plan.a.choice.is_none();
                        self.game_state_b.game_over = plan.b.choice.is_none();
                        return;
                    };
                    // Both decisions come from the same turn's unmodified boards.
                    if hold_a {
                        self.game_state.hold();
                    }
                    if hold_b {
                        self.game_state_b.hold();
                    }
                    self.reset_active_piece();
                    self.reset_active_piece_b();
                    self.active_y = self.active_y.max(target_a.y);
                    self.active_y_b = self.active_y_b.max(target_b.y);
                    self.animation_ready = [false; 2];
                    self.animation_stuck = [0; 2];
                    self.last_animation_step_time = Instant::now();
                    self.pair_plan = Some(plan);
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.pair_rx = None;
                    self.paused = true;
                    self.history_log
                        .insert(0, "Search failed. Paused; reset the match to retry.".into());
                    return;
                }
            }
        }

        let plan = self.pair_plan.as_ref().unwrap();
        let target_a = plan.a.choice.unwrap().0;
        let target_b = plan.b.choice.unwrap().0;
        if !self.bot_human_like {
            self.animation_ready = [true; 2];
        } else {
            let step_ms = (400.0 / (self.moves_per_second as f32 * 6.0).max(1.0)) as u64;
            if self.last_animation_step_time.elapsed()
                >= Duration::from_millis(step_ms.clamp(5, 150))
            {
                self.last_animation_step_time = Instant::now();
                let (x, y, rotation, stuck, ready) = animate_step(
                    &self.game_state,
                    target_a,
                    (self.active_x, self.active_y, self.active_rot),
                    self.animation_stuck[0],
                );
                self.active_x = x;
                self.active_y = y;
                self.active_rot = rotation;
                self.animation_stuck[0] = stuck;
                self.animation_ready[0] = ready;
                let (x, y, rotation, stuck, ready) = animate_step(
                    &self.game_state_b,
                    target_b,
                    (self.active_x_b, self.active_y_b, self.active_rot_b),
                    self.animation_stuck[1],
                );
                self.active_x_b = x;
                self.active_y_b = y;
                self.active_rot_b = rotation;
                self.animation_stuck[1] = stuck;
                self.animation_ready[1] = ready;
            }
        }

        let interval = Duration::from_secs_f64(1.0 / self.moves_per_second.max(1) as f64);
        if self.animation_ready == [true; 2] && self.last_pair_move_time.elapsed() >= interval {
            crate::engine::battle::resolve_paired_turn(
                &mut self.game_state,
                &mut self.game_state_b,
                target_a,
                target_b,
            );
            self.total_pcs_a += u32::from(self.game_state.last_perfect_clear);
            self.total_pcs_b += u32::from(self.game_state_b.last_perfect_clear);
            self.reset_active_piece();
            self.reset_active_piece_b();
            self.reset_bot_thinking();
        }
    }
}

// This is a visual interpolation; only the validated target is committed, and
// both boards wait for the slower animation before the shared placement event.
fn animate_step(
    state: &GameState,
    target: Move,
    pose: (i32, i32, Rotation),
    stuck: u32,
) -> (i32, i32, Rotation, u32, bool) {
    let (mut x, mut y, mut rotation) = pose;
    if pose == (target.x, target.y, target.rotation) {
        return (x, y, rotation, stuck, true);
    }
    if rotation != target.rotation {
        let next = target.rotation;
        let kicks = if state.current == Piece::I {
            crate::engine::piece::get_srs_kicks_i(rotation, next)
        } else {
            crate::engine::piece::get_srs_kicks(rotation, next)
        };
        for &(dx, dy) in kicks {
            if state.board.fits(state.current, next, x + dx, y + dy) {
                x += dx;
                y += dy;
                rotation = next;
                break;
            }
        }
    } else {
        let dx = (target.x - x).signum();
        if state.board.fits(state.current, rotation, x + dx, y) {
            x += dx;
        }
        if x == target.x {
            y = target.y;
        }
    }
    let stuck = if pose == (x, y, rotation) {
        stuck + 1
    } else {
        0
    };
    if stuck >= 3 {
        return (target.x, target.y, target.rotation, stuck, true);
    }
    (
        x,
        y,
        rotation,
        stuck,
        (x, y, rotation) == (target.x, target.y, target.rotation),
    )
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
                    egui::RichText::new("COMBO vs HYBRID")
                        .size(24.0)
                        .strong()
                        .color(Color32::from_rgb(139, 92, 246)),
                );
                ui.label(
                    egui::RichText::new(
                        "Left: Combo | Right: PC + Combo | Equal depth | Synchronized turns",
                    )
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
                ui.vertical_centered(|ui| {
                    ui.heading("BOT A (Combo)");
                    ui.label(
                        egui::RichText::new(format!(
                            "Current Combo: {}",
                            self.game_state.current_combo()
                        ))
                        .size(24.0)
                        .strong()
                        .color(Color32::from_rgb(245, 158, 11)),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "Perfect Clears: {}",
                            self.game_state.perfect_clears
                        ))
                        .size(20.0)
                        .strong()
                        .color(Color32::from_rgb(52, 211, 153)),
                    );
                    ui.label(format!(
                        "Spin: {:?} | Attack: {}",
                        self.game_state.last_spin, self.game_state.last_attack
                    ));
                });
                egui::ScrollArea::vertical()
                    .id_source("arena_a")
                    .show(ui, |ui| {
                        ui.add_space(10.0);
                        ui.vertical_centered(|ui| {
                            ui.add_space(5.0);
                            let active_piece_opt = if self.game_state.game_over {
                                None
                            } else {
                                Some((
                                    self.game_state.current,
                                    self.active_rot,
                                    self.active_x,
                                    self.active_y,
                                ))
                            };
                            render_board(
                                ui,
                                &self.game_state.board,
                                active_piece_opt,
                                self.game_state.pending_garbage,
                                self.game_state.queued_garbage,
                            );
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
                            ui.label(format!("Session PCs: {}", self.total_pcs_a));
                            ui.label(format!("Pieces placed: {}", self.game_state.pieces_placed));
                            if self.game_state.last_perfect_clear {
                                ui.label(
                                    egui::RichText::new("PERFECT CLEAR!")
                                        .strong()
                                        .color(Color32::from_rgb(245, 158, 11)),
                                );
                            }
                        });
                    });
            });

        // Right Panel: Bot B Arena
        egui::SidePanel::right("bot_b_panel")
            .width_range(280.0..=320.0)
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.heading("BOT B (PC + Combo)");
                    ui.label(
                        egui::RichText::new(format!(
                            "Current Combo: {}",
                            self.game_state_b.current_combo()
                        ))
                        .size(24.0)
                        .strong()
                        .color(Color32::from_rgb(245, 158, 11)),
                    );
                    ui.label(
                        egui::RichText::new(format!(
                            "Perfect Clears: {}",
                            self.game_state_b.perfect_clears
                        ))
                        .size(20.0)
                        .strong()
                        .color(Color32::from_rgb(52, 211, 153)),
                    );
                    ui.label(format!(
                        "Spin: {:?} | Attack: {}",
                        self.game_state_b.last_spin, self.game_state_b.last_attack
                    ));
                });
                egui::ScrollArea::vertical()
                    .id_source("arena_b")
                    .show(ui, |ui| {
                        ui.add_space(10.0);
                        ui.vertical_centered(|ui| {
                            ui.add_space(5.0);
                            let active_piece_opt_b = if self.game_state_b.game_over {
                                None
                            } else {
                                Some((
                                    self.game_state_b.current,
                                    self.active_rot_b,
                                    self.active_x_b,
                                    self.active_y_b,
                                ))
                            };
                            render_board(
                                ui,
                                &self.game_state_b.board,
                                active_piece_opt_b,
                                self.game_state_b.pending_garbage,
                                self.game_state_b.queued_garbage,
                            );
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
                            ui.label(format!(
                                "Lines Cleared: {}",
                                self.game_state_b.lines_cleared
                            ));
                            ui.label(format!("Session PCs: {}", self.total_pcs_b));
                            ui.label(format!(
                                "Pieces placed: {}",
                                self.game_state_b.pieces_placed
                            ));
                            if self.game_state_b.last_perfect_clear {
                                ui.label(
                                    egui::RichText::new("PERFECT CLEAR!")
                                        .strong()
                                        .color(Color32::from_rgb(245, 158, 11)),
                                );
                            }
                        });
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
                    let pause_btn_text = if self.paused {
                        "▶ Resume"
                    } else {
                        "⏸ Pause"
                    };
                    if ui.button(pause_btn_text).clicked() {
                        self.paused = !self.paused;
                    }

                    if ui.button("🔄 Reset Match").clicked() {
                        let spin_mode = self.game_state.spin_mode;
                        self.game_state = GameState::new(self.board_width);
                        self.game_state_b = GameState::new(self.board_width);
                        self.game_state.spin_mode = spin_mode;
                        self.game_state_b.spin_mode = spin_mode;
                        self.reset_active_piece();
                        self.reset_active_piece_b();
                        self.reset_bot_thinking();
                    }

                    if ui.button("🧼 Reset Win Stats").clicked() {
                        self.games_played = 0;
                        self.bot_a_wins = 0;
                        self.bot_b_wins = 0;
                        self.total_pcs_a = 0;
                        self.total_pcs_b = 0;
                        self.history_log.clear();
                    }
                });

                ui.add_space(10.0);

                // Speed Slider
                ui.horizontal(|ui| {
                    ui.label("Paired turn speed:");
                    ui.add(
                        egui::Slider::new(&mut self.moves_per_second, 1..=100).text("turns/sec"),
                    );
                });

                // Human-like animation checkbox
                ui.checkbox(
                    &mut self.bot_human_like,
                    "Animate both bots before placing together",
                );

                ui.add_space(10.0);

                // AI Lookahead Depth
                ui.horizontal(|ui| {
                    ui.label("Both bots — search depth:");
                    ui.radio_value(&mut self.lookahead_depth, 4, "4-Ply");
                    ui.radio_value(&mut self.lookahead_depth, 5, "5-Ply");
                    ui.radio_value(&mut self.lookahead_depth, 6, "6-Ply");
                });
            });

            ui.add_space(15.0);

            ui.label(format!(
                "Active turn: {}-ply for both bots",
                self.active_depth
            ));
            ui.label("Next turn uses the selected depth, limited to both previews.");
            ui.label(if self.pair_rx.is_some() {
                "Waiting for both searches..."
            } else if self.pair_plan.is_some() {
                "Both plans ready — synchronized placement"
            } else {
                "Ready for next turn"
            });
            ui.label(format!(
                "Last search: A {:.1} ms | B {:.1} ms",
                self.last_search_a.as_secs_f64() * 1000.0,
                self.last_search_b.as_secs_f64() * 1000.0
            ));

            ui.heading("Left: Combo / Right: Adaptive PC + Combo");
            ui.label("B pressures low combos with PC; otherwise keeps a combo.");
            ui.label(
                self.hybrid_mode
                    .map(|m| m.label())
                    .unwrap_or_else(|| "B: deciding next strategy...".into()),
            );
            ui.add(
                egui::Slider::new(&mut self.opponent_combo_threshold, 1..=20)
                    .text("Opponent combo: switch to combo"),
            );
            ui.label("Threshold changes apply next turn. High combos favor keeping the chain.");
            ui.label("Current Combo: first clear = 0, second consecutive clear = 1.");
            ui.label("Rotation: SRS-X (90 and 180 degrees)");
            let mut mode = self.game_state.spin_mode;
            egui::ComboBox::from_label("Spin bonuses")
                .selected_text(format!("{:?}", mode))
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut mode, SpinMode::All, "All (full mino spins)");
                    ui.selectable_value(&mut mode, SpinMode::AllMiniPlus, "All-Mini+");
                    ui.selectable_value(&mut mode, SpinMode::TSpins, "T-spins");
                });
            if mode != self.game_state.spin_mode {
                self.game_state.spin_mode = mode;
                self.game_state_b.spin_mode = mode;
                self.reset_bot_thinking();
                self.reset_active_piece();
                self.reset_active_piece_b();
            }
            ui.add_space(15.0);

            // Battle Log History
            ui.heading("📜 Recent Matches");
            ui.separator();
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    if self.history_log.is_empty() {
                        ui.label(
                            egui::RichText::new("No matches completed yet. Let the bots battle!")
                                .italics()
                                .color(Color32::GRAY),
                        );
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
            .with_title("Combo vs Hybrid Arena")
            .with_min_inner_size([1000.0, 750.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Combo vs Hybrid Arena",
        native_options,
        Box::new(|cc| Box::new(BattleApp::new(cc))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{board::Board, movegen::generate_moves};

    fn controlled_app(animated: bool) -> BattleApp {
        let mut app = BattleApp::new_match(4);
        app.game_state = GameState::from_triangle(
            Board::new(4),
            Piece::I,
            Some(Piece::O),
            vec![Piece::T, Piece::S, Piece::Z],
            0,
            false,
            0,
        );
        app.game_state_b = GameState::from_triangle(
            Board::new(4),
            Piece::T,
            Some(Piece::O),
            vec![Piece::I, Piece::S, Piece::Z],
            0,
            false,
            0,
        );
        app.bot_human_like = animated;
        app.reset_active_piece();
        app.reset_active_piece_b();
        app.last_pair_move_time = Instant::now() - Duration::from_secs(2);
        app
    }

    fn plan(app: &BattleApp) -> PairedPlan {
        let a = generate_moves(&app.game_state.board, app.game_state.current)
            .into_iter()
            .find(|m| m.rotation == Rotation::North)
            .unwrap();
        let b = generate_moves(&app.game_state_b.board, app.game_state_b.current)
            .into_iter()
            .find(|m| m.rotation == Rotation::South)
            .unwrap();
        PairedPlan {
            a: BotPlan {
                choice: Some((a, false)),
                elapsed: Duration::from_millis(1),
            },
            b: BotPlan {
                choice: Some((b, false)),
                elapsed: Duration::from_millis(200),
            },
            depth: 3,
            hybrid_mode: None,
        }
    }

    fn deliver(app: &mut BattleApp, plan: PairedPlan) {
        let (tx, rx) = channel();
        tx.send(plan).unwrap();
        app.pair_rx = Some(rx);
    }

    #[test]
    fn no_placement_until_both_plans_are_published() {
        let mut app = controlled_app(false);
        let (tx, rx) = channel();
        app.pair_rx = Some(rx);
        app.update_game_logic();
        assert_eq!(
            (app.game_state.pieces_placed, app.game_state_b.pieces_placed),
            (0, 0)
        );
        tx.send(plan(&app)).unwrap();
        app.update_game_logic();
        assert_eq!(
            (app.game_state.pieces_placed, app.game_state_b.pieces_placed),
            (1, 1)
        );
        assert!(app.pair_rx.is_none() && app.pair_plan.is_none());
        assert_eq!((app.total_pcs_a, app.total_pcs_b), (1, 0));
        app.paused = true;
        app.update_game_logic();
        assert_eq!((app.total_pcs_a, app.total_pcs_b), (1, 0));
    }

    #[test]
    fn faster_animation_waits_for_slower_animation() {
        let mut app = controlled_app(true);
        let ready = plan(&app);
        deliver(&mut app, ready);
        app.update_game_logic();
        let mut observed_wait = false;
        for _ in 0..20 {
            app.last_animation_step_time = Instant::now() - Duration::from_secs(1);
            app.update_game_logic();
            assert_eq!(app.game_state.pieces_placed, app.game_state_b.pieces_placed);
            if app.animation_ready[0] != app.animation_ready[1] {
                observed_wait = true;
                assert_eq!(app.game_state.pieces_placed, 0);
            }
            if app.game_state.pieces_placed == 1 {
                break;
            }
        }
        assert!(observed_wait);
        assert_eq!(
            (app.game_state.pieces_placed, app.game_state_b.pieces_placed),
            (1, 1)
        );
    }

    #[test]
    fn shared_speed_limit_and_hold_apply_to_both_placements() {
        let mut app = controlled_app(false);
        app.moves_per_second = 1;
        app.last_pair_move_time = Instant::now();
        let mut ready = plan(&app);
        ready.a.choice = Some((generate_moves(&app.game_state.board, Piece::O)[0], true));
        deliver(&mut app, ready);
        app.update_game_logic();
        assert_eq!(
            (app.game_state.pieces_placed, app.game_state_b.pieces_placed),
            (0, 0)
        );
        assert_eq!(app.game_state.current, Piece::O);
        app.last_pair_move_time = Instant::now() - Duration::from_secs(2);
        app.update_game_logic();
        assert_eq!(
            (app.game_state.pieces_placed, app.game_state_b.pieces_placed),
            (1, 1)
        );
        assert_eq!(app.game_state.hold, Some(Piece::I));
    }

    #[test]
    fn missing_move_is_terminal_instead_of_retrying_forever() {
        let mut app = controlled_app(false);
        let mut ready = plan(&app);
        ready.b.choice = None;
        deliver(&mut app, ready);
        app.update_game_logic();
        assert!(!app.game_state.game_over && app.game_state_b.game_over);
        assert_eq!(
            (app.game_state.pieces_placed, app.game_state_b.pieces_placed),
            (0, 0)
        );
    }

    #[test]
    fn changing_depth_cannot_split_an_in_flight_pair() {
        let mut app = controlled_app(false);
        app.lookahead_depth = 1;
        app.start_paired_search();
        app.lookahead_depth = 6;
        let ready = app
            .pair_rx
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(ready.depth, 1);
        assert!(ready.a.choice.is_some() && ready.b.choice.is_some());
        app.start_paired_search();
        let ready = app
            .pair_rx
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert_eq!(ready.depth, 4); // only current + three visible pieces
    }
    #[test]
    fn paired_worker_uses_combo_left_and_hybrid_right() {
        let mut app = controlled_app(false);
        app.lookahead_depth = 3;
        app.game_state.board.rows[0] = 7;
        app.game_state.board.rows[1] = 1;
        let expected_a = find_best_move_for_objective(
            &app.game_state,
            Some(&app.game_state_b),
            Evaluator::Meta(&app.meta_net_a),
            3,
            Objective::Combo,
        );
        let expected_b = find_hybrid_move(
            &app.game_state_b,
            Some(&app.game_state),
            Evaluator::Static(&app.weights_b),
            3,
            app.opponent_combo_threshold,
        );
        app.start_paired_search();
        let ready = app
            .pair_rx
            .take()
            .unwrap()
            .recv_timeout(Duration::from_secs(10))
            .unwrap();
        assert_eq!(ready.a.choice, expected_a);
        assert_eq!(ready.b.choice, expected_b.map(|p| p.choice));
        assert_eq!(ready.hybrid_mode, expected_b.map(|p| p.mode));
        assert_eq!(ready.depth, 3);
    }
}
