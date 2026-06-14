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
    pub mod header;
    pub mod piece;
    pub mod board;
    pub mod movegen;
    pub mod state;
}

pub mod rl {
    pub mod features;
    pub mod agent;
}

use std::collections::{HashSet, VecDeque};
use std::io::{self, BufRead, Write};
use serde_json::{json, Value};

use crate::engine::board::{Board, BOARD_HEIGHT};
use crate::engine::header::{Move, Piece, Rotation};
use crate::engine::movegen::generate_moves;
use crate::engine::state::GameState;
use crate::rl::agent::find_best_move;
use crate::rl::features::Weights;

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

/// Convert our Piece enum to the protocol string.
fn piece_to_str(p: Piece) -> &'static str {
    match p {
        Piece::I => "I",
        Piece::O => "O",
        Piece::T => "T",
        Piece::L => "L",
        Piece::J => "J",
        Piece::S => "S",
        Piece::Z => "Z",
    }
}

/// Build a GameState from the Triangle protocol's state message.
/// The board width is fixed to 4 for 4-wide mode.
fn build_state_from_protocol(state_msg: &Value, board_width: usize) -> GameState {
    let mut board = Board::new(board_width);

    // Parse board: 2D array, row 0 = bottom.
    // Each cell is null (empty) or a piece symbol string.
    if let Some(rows) = state_msg["board"].as_array() {
        for (y, row) in rows.iter().enumerate() {
            if y >= BOARD_HEIGHT {
                break;
            }
            if let Some(cells) = row.as_array() {
                for (x, cell) in cells.iter().enumerate() {
                    if x >= board_width {
                        break;
                    }
                    if !cell.is_null() {
                        board.rows[y] |= 1 << x;
                    }
                }
            }
        }
    }

    // Parse current piece
    let current = state_msg["current"]
        .as_str()
        .and_then(piece_from_str)
        .unwrap_or(Piece::T);

    // Parse hold piece
    let hold = state_msg["hold"].as_str().and_then(piece_from_str);

    // Parse queue
    let queue: Vec<Piece> = state_msg["queue"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().and_then(piece_from_str))
                .collect()
        })
        .unwrap_or_default();

    // Parse combo (-1 means no combo in protocol, we store 0-based)
    let combo = state_msg["combo"].as_i64().unwrap_or(-1);
    let combo_u32 = if combo < 0 { 0 } else { combo as u32 };

    // Parse b2b (-1 means no b2b)
    let b2b = state_msg["b2b"].as_i64().unwrap_or(-1);
    let b2b_active = b2b >= 0;

    // Parse garbage (array of line counts)
    let pending_garbage: u32 = state_msg["garbage"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|v| v.as_u64()).sum::<u64>() as u32)
        .unwrap_or(0);

    GameState::from_triangle(
        board,
        current,
        hold,
        queue,
        combo_u32,
        b2b_active,
        pending_garbage,
    )
}

fn get_spawn_x(_piece: Piece, board_width: usize) -> i32 {
    (board_width as i32) / 2 - 1
}

/// Convert a (Move, use_hold) result into a key sequence for the Triangle protocol.
/// The bot computes a final placement (piece, rotation, x, y). We convert this to:
///   1. "hold" (if use_hold is true)
///   2. rotation keys ("rotateCW" / "rotateCCW")
///   3. movement keys ("dasLeft"/"dasRight" + "moveLeft"/"moveRight")
///   4. "hardDrop"
fn move_to_keys(m: Move, use_hold: bool, board_width: usize) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();

    if use_hold {
        keys.push("hold".to_string());
    }

    // Determine the spawn x position for this piece
    let spawn_x = get_spawn_x(m.piece, board_width);

    // Add rotation keys
    // Spawn rotation is always North. We need to get to m.rotation.
    let rot_count = match m.rotation {
        Rotation::North => 0,
        Rotation::East => 1,  // 1 CW
        Rotation::South => 2, // 2 CW or 1 rotate180
        Rotation::West => 3,  // 3 CW or 1 CCW
    };

    if rot_count == 3 {
        keys.push("rotateCCW".to_string());
    } else if rot_count == 2 {
        keys.push("rotate180".to_string());
    } else {
        for _ in 0..rot_count {
            keys.push("rotateCW".to_string());
        }
    }

    // Add horizontal movement keys
    let dx = m.x - spawn_x;
    if dx < 0 {
        for _ in 0..dx.abs() {
            keys.push("moveLeft".to_string());
        }
    } else if dx > 0 {
        // Move right: use dasRight first (goes to wall), then moveLeft to adjust
        // For a 4-wide board, the rightmost x depends on the piece and rotation.
        // We calculate the rightmost x by finding the max x where the piece fits.
        // But the simpler approach: just use individual moveRight steps from spawn.
        // Let's use individual moves instead for reliability:
        for _ in 0..dx {
            keys.push("moveRight".to_string());
        }
    }

    // Hard drop
    keys.push("hardDrop".to_string());

    keys
}

fn compress_keys(keys: Vec<String>) -> Vec<String> {
    let mut compressed = Vec::new();
    let mut last_was_soft_drop = false;
    for key in keys {
        if key == "softDrop" {
            if !last_was_soft_drop {
                compressed.push(key);
                last_was_soft_drop = true;
            }
        } else {
            compressed.push(key);
            last_was_soft_drop = false;
        }
    }
    compressed
}

fn find_path_for_move(board: &Board, piece: Piece, target: Move) -> Option<Vec<String>> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    let spawn_x = get_spawn_x(piece, board.width);
    let highest_row = board.highest_row() as i32;
    let spawn_y = if highest_row >= 20 {
        (highest_row + 1).min(BOARD_HEIGHT as i32 - 3)
    } else {
        20
    };

    let start_state = crate::engine::movegen::SearchState {
        rotation: Rotation::North,
        x: spawn_x,
        y: spawn_y,
    };

    if board.fits(piece, start_state.rotation, start_state.x, start_state.y) {
        queue.push_back((start_state, Vec::new()));
        visited.insert(start_state);
    } else {
        return None;
    }

    while let Some((current, path)) = queue.pop_front() {
        let x = current.x;
        let y = current.y;
        let rot = current.rotation;

        if x == target.x && y == target.y && rot == target.rotation {
            let mut final_path = path;
            final_path.push("hardDrop".to_string());
            return Some(compress_keys(final_path));
        }

        // Try Clockwise Rotation
        let cw_rot = rot.rotate_cw();
        let kicks = if piece == Piece::I {
            crate::engine::piece::get_srs_kicks_i(rot, cw_rot)
        } else {
            crate::engine::piece::get_srs_kicks(rot, cw_rot)
        };

        for &(dx, dy) in kicks.iter() {
            let next_x = x + dx;
            let next_y = y + dy;
            if board.fits(piece, cw_rot, next_x, next_y) {
                let rot_state = crate::engine::movegen::SearchState { rotation: cw_rot, x: next_x, y: next_y };
                if !visited.contains(&rot_state) {
                    visited.insert(rot_state);
                    let mut next_path = path.clone();
                    next_path.push("rotateCW".to_string());
                    queue.push_back((rot_state, next_path));
                }
                break;
            }
        }

        // Try Counter-Clockwise Rotation
        let ccw_rot = rot.rotate_ccw();
        let kicks = if piece == Piece::I {
            crate::engine::piece::get_srs_kicks_i(rot, ccw_rot)
        } else {
            crate::engine::piece::get_srs_kicks(rot, ccw_rot)
        };

        for &(dx, dy) in kicks.iter() {
            let next_x = x + dx;
            let next_y = y + dy;
            if board.fits(piece, ccw_rot, next_x, next_y) {
                let rot_state = crate::engine::movegen::SearchState { rotation: ccw_rot, x: next_x, y: next_y };
                if !visited.contains(&rot_state) {
                    visited.insert(rot_state);
                    let mut next_path = path.clone();
                    next_path.push("rotateCCW".to_string());
                    queue.push_back((rot_state, next_path));
                }
                break;
            }
        }

        // Try Left
        let left = crate::engine::movegen::SearchState { rotation: rot, x: x - 1, y };
        if !visited.contains(&left) && board.fits(piece, left.rotation, left.x, left.y) {
            visited.insert(left);
            let mut next_path = path.clone();
            next_path.push("moveLeft".to_string());
            queue.push_back((left, next_path));
        }

        // Try Right
        let right = crate::engine::movegen::SearchState { rotation: rot, x: x + 1, y };
        if !visited.contains(&right) && board.fits(piece, right.rotation, right.x, right.y) {
            visited.insert(right);
            let mut next_path = path.clone();
            next_path.push("moveRight".to_string());
            queue.push_back((right, next_path));
        }

        // Try Soft Drop
        let down = crate::engine::movegen::SearchState { rotation: rot, x, y: y - 1 };
        if y > 0 && !visited.contains(&down) && board.fits(piece, down.rotation, down.x, down.y) {
            visited.insert(down);
            let mut next_path = path.clone();
            next_path.push("softDrop".to_string());
            queue.push_back((down, next_path));
        }
    }

    None
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
    let weights = Weights::default();
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
                    let game_state = build_state_from_protocol(state_msg, board_width);

                    let search_start = std::time::Instant::now();
                    let result = find_best_move(&game_state, None, &weights, lookahead_depth);
                    let search_duration = search_start.elapsed();

                    let mut path_duration = std::time::Duration::from_secs(0);
                    let (keys, best_move, use_hold) = if let Some((best_move, use_hold)) = result {
                        let current_piece = if use_hold {
                            match game_state.hold {
                                Some(p) => p,
                                None => *game_state.queue.first().unwrap_or(&game_state.current),
                            }
                        } else {
                            game_state.current
                        };

                        let path_start = std::time::Instant::now();
                        let keys = if let Some(path_keys) = find_path_for_move(&game_state.board, current_piece, best_move) {
                            let mut k = if use_hold { vec!["hold".to_string()] } else { Vec::new() };
                            k.extend(path_keys);
                            k
                        } else {
                            move_to_keys(best_move, use_hold, board_width)
                        };
                        path_duration = path_start.elapsed();
                        (keys, Some(best_move), use_hold)
                    } else {
                        // Fallback: just hard drop
                        (vec!["hardDrop".to_string()], None, false)
                    };

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

