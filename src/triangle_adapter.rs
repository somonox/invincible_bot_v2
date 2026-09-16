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
    pub mod features;
    pub mod meta_agent;
    pub mod search;
}

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};

use crate::engine::board::{Board, BOARD_HEIGHT};
use crate::engine::header::{Move, Piece, SpinMode};
use crate::engine::state::GameState;
use crate::rl::agent::find_best_move_meta;
use crate::rl::meta_agent::MetaPolicyNetwork;

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
        .map(|a| a.iter().filter_map(Value::as_u64).sum::<u64>() as u32)
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
    let mut spin_mode = SpinMode::All;
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
    let mut last_state: Option<Value> = None;

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
                spin_mode = match msg["spins"].as_str().unwrap_or("all") {
                    "all-mini+" => SpinMode::AllMiniPlus,
                    "T-spins" => SpinMode::TSpins,
                    "all" => SpinMode::All,
                    other => {
                        eprintln!("Unsupported spin mode {other}; using All.");
                        SpinMode::All
                    }
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
                    board_width,
                    msg["boardHeight"].as_u64().unwrap_or(20)
                );
            }
            "state" => {
                // Store the latest state for when play is received
                last_state = Some(msg.clone());
            }
            "pieces" => {
                // New pieces added to queue - handled via state updates
                eprintln!("[4wide-bot] Pieces received");
            }
            "play" => {
                // Time to make a move!
                if let Some(ref state_msg) = last_state {
                    let mut game_state = build_state_from_protocol(state_msg, board_width);
                    game_state.spin_mode = spin_mode;

                    let search_start = std::time::Instant::now();
                    let result = find_best_move_meta(&game_state, None, &meta_net, lookahead_depth);
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
                    let selected = result.and_then(|(m, h)| executable(m, h));
                    let (keys, best_move, use_hold) = selected
                        .or_else(|| {
                            eprintln!(
                                "[4wide-bot] Searching for an executable fallback placement."
                            );
                            let mut candidates = crate::rl::agent::get_all_next_states(&game_state);
                            candidates.retain(|(s, _, _)| !s.game_over);
                            candidates.sort_by_key(|(s, _, _)| {
                                std::cmp::Reverse((s.last_perfect_clear, s.last_attack))
                            });
                            candidates
                                .into_iter()
                                .find_map(|(_, m, h)| executable(m, h))
                        })
                        .unwrap_or_else(|| (vec!["hardDrop".to_string()], None, false));
                    let path_duration = path_start.elapsed();

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

                    send_message(&json!({
                        "type": "move",
                        "keys": keys,
                        "data": null
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
