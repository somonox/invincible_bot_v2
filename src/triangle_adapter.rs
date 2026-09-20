/// Triangle.js Adapter Protocol implementation for 4wide-bot.
///
/// This binary communicates via stdin/stdout JSON messages with the Triangle.js
/// BotWrapper, allowing the bot to play on TETR.IO through the @haelp/teto library.
///
/// Protocol:
///   Bot → Triangle: { "type": "info", "name": "...", "version": "...", "author": "..." }
///   Triangle → Bot: { "type": "config", ... }
///   Triangle → Bot: { "type": "state", "board": [...], "current": "T", "hold": "I", "queue": [...], "garbage": [...], "combo": -1, "b2b": -1 }
///   Triangle → Bot: { "type": "play", ... }
///   Bot → Triangle: { "type": "move", "keys": ["hold", "dasLeft", "rotateCW", "hardDrop"] }
///   Triangle → Bot: { "type": "pieces", "pieces": ["T", "S", ...] }

pub mod engine {
    pub mod board;
    pub mod header;
    pub mod movegen;
    pub mod piece;
    pub mod state;
}

pub mod rl {
    pub mod agent;
    pub mod combo_solver;
    pub mod features;
    pub mod meta_agent;
    pub mod search;
}

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

use crate::engine::board::{Board, BOARD_HEIGHT};
use crate::engine::header::{ComboMode, Move, Piece, SpinMode};
use crate::engine::state::{GameState, GarbagePacket};
use crate::rl::meta_agent::MetaPolicyNetwork;
use crate::rl::search::{find_funny_move, find_hybrid_move_with_expert, Evaluator};

/// Convert a piece symbol string ("T", "I", etc.) to our Piece enum.
fn piece_from_str(s: &str) -> Option<Piece> {
    match s.to_ascii_uppercase().as_str() {
        "I" => Some(Piece::I),
        "O" => Some(Piece::O),
        "T" => Some(Piece::T),
        "L" => Some(Piece::L),
        "J" => Some(Piece::J),
        "S" => Some(Piece::S),
        "Z" => Some(Piece::Z),
        _ => None,
    }
}

/// Build a GameState from the Triangle protocol's state message.
fn build_state_from_protocol(msg: &Value, width: usize) -> GameState {
    let mut board = Board::new(width);
    if let Some(rows) = msg["board"].as_array() {
        for (y, row) in rows.iter().take(BOARD_HEIGHT).enumerate() {
            if let Some(cells) = row.as_array() {
                for (x, cell) in cells.iter().take(width).enumerate() {
                    if !cell.is_null() {
                        board.rows[y] |= 1 << x;
                    }
                }
            }
        }
    }
    let piece = |v: &Value| v.as_str().and_then(piece_from_str);
    let current = piece(&msg["current"]).unwrap_or(Piece::T);
    let hold = piece(&msg["hold"]);
    let queue = msg["queue"]
        .as_array()
        .map(|a| a.iter().filter_map(piece).collect())
        .unwrap_or_default();
    let combo = msg["combo"].as_i64().unwrap_or(-1);
    let b2b = msg["b2b"].as_i64().unwrap_or(-1);
    let garbage = msg["garbage"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_f64)
                .filter(|n| n.is_finite() && *n > 0.0)
                .map(|n| n.ceil() as u32)
                .sum::<u32>()
        })
        .unwrap_or(0);
    let mut state = GameState::from_triangle(
        board,
        current,
        hold,
        queue,
        (combo + 1).max(0) as u32,
        b2b >= 0,
        garbage,
    );
    state.b2b_level = (b2b + 1).max(0) as u32;
    let context = &msg["data"]["garbageContext"];
    if let Some(packets) = context["packets"].as_array() {
        let parsed: Option<Vec<GarbagePacket>> = packets
            .iter()
            .map(|p| {
                let amount = p["amount"].as_f64()?;
                let ready = p["readyIn"].as_f64()?;
                if !amount.is_finite() || !ready.is_finite() || amount < 0.0 {
                    return None;
                }
                Some(GarbagePacket {
                    amount: amount.ceil() as u32,
                    ready_in: ready.max(0.0).ceil() as u32,
                })
            })
            .collect();
        // A malformed extension falls back to the standard protocol's queue.
        if let Some(packets) = parsed {
            state.garbage_packets = Some(packets);
            state.sync_garbage_totals();
        }
    }
    if let Some(frames) = context["framesPerPiece"].as_u64() {
        state.frames_per_piece = frames.clamp(1, 3600) as u32;
    }
    if let Some(frames) = context["nextLockFrames"].as_u64() {
        state.next_lock_frames = frames.clamp(1, 3600) as u32;
    }
    if let Some(cap) = context["cap"].as_u64() {
        state.garbage_cap = cap.min(40) as u32;
    }
    state
}

fn find_path_for_move(
    board: &Board,
    _piece: Piece,
    target: Move,
    mode: SpinMode,
) -> Option<Vec<String>> {
    crate::engine::movegen::find_input_path(board, target, mode)
}

fn send_message(msg: &Value) {
    let out = serde_json::to_string(msg).unwrap();
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    writeln!(handle, "{}", out).unwrap();
    handle.flush().unwrap();
}

fn main() {
    let stdin = io::stdin();
    let reader = stdin.lock();

    // Send info message immediately
    send_message(&json!({
        "type": "info",
        "name": "4wide-bot",
        "version": "1.0.0",
        "author": "antigravity",
        "data": null
    }));

    let mut board_width: usize = 4;
    let mut board_height: i32 = 20;
    let mut spin_mode = SpinMode::All;
    let mut pc_bonus = 10;
    let mut combo_mode = ComboMode::Multiplier;
    let meta_net = if std::path::Path::new("meta_net.json").exists() {
        if let Ok(file_content) = std::fs::read_to_string("meta_net.json") {
            if let Ok(net) = serde_json::from_str::<MetaPolicyNetwork>(&file_content) {
                eprintln!(
                    "[4wide-bot] Successfully loaded trained MetaPolicyNetwork from meta_net.json"
                );
                net
            } else {
                eprintln!(
                    "[4wide-bot] Failed to parse meta_net.json. Using default MetaPolicyNetwork."
                );
                MetaPolicyNetwork::default()
            }
        } else {
            MetaPolicyNetwork::default()
        }
    } else {
        eprintln!(
            "[4wide-bot] meta_net.json not found. Using default MetaPolicyNetwork (baseline)."
        );
        MetaPolicyNetwork::default()
    };
    let lookahead_depth: usize = 6;
    let verbose_moves = std::env::var("BOT_LOG_MOVES").is_ok_and(|value| value == "1");
    let mut configured = false;
    let mut last_state: Option<Value> = None;
    let mut reported_mode = None;

    // Main message loop
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };

        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("[4wide-bot] JSON parse error: {}", e);
                continue;
            }
        };

        let msg_type = msg["type"].as_str().unwrap_or("");

        match msg_type {
            "config" => {
                if msg["kicks"].as_str() != Some("SRS-X") || msg["boardWidth"].as_u64() != Some(4) {
                    eprintln!("[4wide-bot] Unsupported configuration: requires SRS-X and width 4.");
                    std::process::exit(2);
                }
                let height = msg["boardHeight"].as_u64().unwrap_or(20);
                if ![20, 26].contains(&height) {
                    eprintln!("[4wide-bot] Unsupported board height; expected 20 or 26.");
                    std::process::exit(2);
                }
                board_height = height as i32;
                configured = true;
                pc_bonus = msg["pcGarbage"]
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .map(|v| v.clamp(0.0, 1000.0).floor() as u32)
                    .unwrap_or(10);
                combo_mode = match msg["comboTable"].as_str().unwrap_or("multiplier") {
                    "multiplier" => ComboMode::Multiplier,
                    "classic guideline" => ComboMode::Classic,
                    "modern guideline" => ComboMode::Modern,
                    _ => ComboMode::None,
                };
                spin_mode = match msg["spins"].as_str().unwrap_or("all") {
                    "all" => SpinMode::All,
                    "all-mini" => SpinMode::AllMini,
                    "all-mini+" => SpinMode::AllMiniPlus,
                    "all+" => SpinMode::AllPlus,
                    "T-spins" => SpinMode::TSpins,
                    "T-spins+" => SpinMode::TSpinsPlus,
                    "mini-only" => SpinMode::MiniOnly,
                    "handheld" => SpinMode::Handheld,
                    "stupid" => SpinMode::Stupid,
                    _ => SpinMode::None,
                };
                if msg["kicks"].as_str().is_some_and(|k| k != "SRS-X") {
                    eprintln!(
                        "This adapter uses SRS-X; room kicks differ: {}",
                        msg["kicks"]
                    );
                }
                // Read board width from config (should be 4 for 4-wide)
                if let Some(w) = msg["boardWidth"].as_u64() {
                    board_width = w as usize;
                }
                eprintln!(
                    "[4wide-bot] Config received: board {}x{}",
                    board_width, board_height
                );
            }
            "state" => {
                // Store the latest state for when play is received
                last_state = Some(msg.clone());
            }
            "pieces" => {
                // New pieces added to queue - handled via state updates
                if verbose_moves {
                    eprintln!("[4wide-bot] Pieces received");
                }
            }
            "play" => {
                if !configured {
                    eprintln!("[4wide-bot] Refusing play before a supported configuration.");
                    std::process::exit(2);
                }
                // Time to make a move!
                if let Some(ref state_msg) = last_state {
                    let mut game_state = build_state_from_protocol(state_msg, board_width);
                    game_state.board.spawn_height = board_height;
                    game_state.spin_mode = spin_mode;
                    game_state.pc_bonus = pc_bonus;
                    game_state.combo_mode = combo_mode;
                    if let Some(cap) = msg["garbageCap"].as_f64() {
                        let live_cap = cap.clamp(0.0, 40.0).floor() as u32;
                        game_state.garbage_cap =
                            if state_msg["data"]["garbageContext"]["cap"].is_u64() {
                                live_cap.min(game_state.garbage_cap)
                            } else {
                                live_cap
                            };
                    }

                    let search_start = std::time::Instant::now();
                    let funny_mode = state_msg["data"]["funnyMode"].as_bool().unwrap_or(false);
                    let expert_mode =
                        !funny_mode && state_msg["data"]["expertMode"].as_bool().unwrap_or(false);
                    if reported_mode != Some((expert_mode, funny_mode)) {
                        eprintln!(
                            "[4wide-bot] Active policy: {}",
                            if funny_mode {
                                "Funny B2B-first"
                            } else if expert_mode {
                                "Expert combo-first (table or clear-chain search)"
                            } else {
                                "Normal PC/attack"
                            }
                        );
                        reported_mode = Some((expert_mode, funny_mode));
                    }
                    let result = if funny_mode {
                        find_funny_move(
                            &game_state,
                            None,
                            Evaluator::Meta(&meta_net),
                            lookahead_depth,
                        )
                    } else {
                        find_hybrid_move_with_expert(
                            &game_state,
                            None,
                            Evaluator::Meta(&meta_net),
                            lookahead_depth,
                            expert_mode,
                        )
                    };
                    let search_duration = search_start.elapsed();

                    let path_start = std::time::Instant::now();
                    let executable = |m: Move, h: bool| {
                        find_path_for_move(&game_state.board, m.piece, m, spin_mode).map(|path| {
                            let mut keys = if h {
                                vec!["hold".to_string()]
                            } else {
                                Vec::new()
                            };
                            keys.extend(path);
                            (keys, Some(m), h)
                        })
                    };
                    let selected = result.and_then(|plan| executable(plan.choice.0, plan.choice.1));
                    if let Some(plan) = result.filter(|_| verbose_moves) {
                        eprintln!(
                            "[4wide-bot] {} (incoming {}, preview attack {}, peak {})",
                            plan.mode.label(),
                            game_state.incoming_garbage(),
                            plan.expected_attack,
                            plan.peak_attack
                        );
                    }
                    let used_plan = selected.is_some();
                    let (keys, best_move, use_hold) = selected
                        .or_else(|| {
                            eprintln!(
                                "[4wide-bot] Searching for an executable fallback placement."
                            );
                            let mut candidates = crate::rl::agent::get_all_next_states(&game_state);
                            candidates.retain(|(s, _, _)| !s.game_over);
                            candidates.sort_by_key(|(s, _, _)| {
                                (
                                    s.last_received_garbage,
                                    funny_mode && game_state.b2b && !s.b2b,
                                    std::cmp::Reverse(if funny_mode { s.b2b_level } else { 0 }),
                                    std::cmp::Reverse(expert_mode && s.combo > 0),
                                    std::cmp::Reverse(s.last_canceled_garbage),
                                    std::cmp::Reverse(s.last_perfect_clear),
                                    std::cmp::Reverse(s.combo > 0),
                                    std::cmp::Reverse(s.last_attack),
                                )
                            });
                            candidates
                                .into_iter()
                                .find_map(|(_, m, h)| executable(m, h))
                        })
                        .unwrap_or_else(|| (vec!["hardDrop".to_string()], None, false));
                    let path_duration = path_start.elapsed();

                    if verbose_moves {
                        // Print evaluation plan to stderr for debugging
                        eprintln!(
                        "[4wide-bot-AI] Planning for Current Piece: {:?}, Hold: {:?}, Next Queue: {:?}",
                        game_state.current,
                        game_state.hold,
                        &game_state.queue[..game_state.queue.len().min(5)]
                    );
                        if let Some(m) = best_move {
                            eprintln!(
                            "  -> Best Placement: Piece={:?}, Rotation={:?}, Placement X={}, Use Hold={} => Sending Keys: {:?} (search: {:?}, pathfind: {:?})",
                            m.piece, m.rotation, m.x, use_hold, keys, search_duration, path_duration
                        );
                        } else {
                            eprintln!("  -> No valid placement found! Falling back to instant hardDrop. (search: {:?})", search_duration);
                        }
                    }

                    send_message(&json!({
                        "type": "move",
                        "keys": keys,
                        "data": {
                            "expertMode": expert_mode,
                            "funnyMode": funny_mode,
                            "strategy": if used_plan { result.map(|p| p.mode.label()) } else { Some("Executable fallback".into()) },
                            "incoming": game_state.incoming_garbage(),
                            "expectedAttack": if used_plan { result.map(|p| p.expected_attack) } else { None },
                            "peakAttack": if used_plan { result.map(|p| p.peak_attack) } else { None }
                        }
                    }));
                } else {
                    // No state received yet, just hard drop
                    send_message(&json!({
                        "type": "move",
                        "keys": ["hardDrop"],
                        "data": null
                    }));
                }
            }
            _ => {
                eprintln!("[4wide-bot] Unknown message type: {}", msg_type);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn packet_extension_is_authoritative_and_preserves_timing() {
        let s = build_state_from_protocol(
            &json!({
                "current":"O", "garbage":[12], "data":{"garbageContext":{
                    "packets":[{"amount":3,"readyIn":0},{"amount":4,"readyIn":25}],
                    "framesPerPiece":32,"nextLockFrames":6,"cap":4
                }}
            }),
            4,
        );
        assert_eq!((s.pending_garbage, s.queued_garbage), (3, 4));
        assert_eq!(
            (s.frames_per_piece, s.next_lock_frames, s.garbage_cap),
            (32, 6, 4)
        );
        let s = build_state_from_protocol(
            &json!({"garbage":[3], "data":{"garbageContext":{"packets":[]}}}),
            4,
        );
        assert_eq!(s.incoming_garbage(), 0);
    }
    #[test]
    fn missing_or_invalid_extension_keeps_standard_queue_including_fractions() {
        for data in [
            json!(null),
            json!({"garbageContext":{"packets":[{"amount":2}]}}),
        ] {
            let s = build_state_from_protocol(&json!({"garbage":[1.5,2,0,-3],"data":data}), 4);
            assert_eq!(s.pending_garbage, 4);
            assert!(s.garbage_packets.is_none());
        }
    }
    #[test]
    fn protocol_combo_and_b2b_keep_the_initial_clear() {
        for (protocol, internal, display) in [(-1, 0, 0), (0, 1, 0), (1, 2, 1), (7, 8, 7)] {
            let s = build_state_from_protocol(
                &json!({"board":[],"current":"T","queue":["I"],"combo":protocol,"b2b":protocol}),
                4,
            );
            assert_eq!(s.combo, internal);
            assert_eq!(s.current_combo(), display);
            assert_eq!(s.b2b_level, internal);
            assert_eq!(s.b2b, protocol >= 0);
        }
    }
}
