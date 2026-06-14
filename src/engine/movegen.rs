use crate::engine::header::{Move, Piece, Rotation};
use crate::engine::board::{Board, BOARD_HEIGHT};
use crate::engine::piece::{get_srs_kicks, get_srs_kicks_i};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SearchState {
    pub rotation: Rotation,
    pub x: i32,
    pub y: i32,
}

pub fn generate_moves(board: &Board, piece: Piece) -> Vec<Move> {
    let mut lock_moves = Vec::with_capacity(32);

    let mut visited = [false; 2560];
    let mut lock_states = [false; 2560];
    
    let mut queue = [SearchState { rotation: Rotation::North, x: 0, y: 0 }; 1024];
    let mut head = 0;
    let mut tail = 0;

    let spawn_x = (board.width as i32) / 2 - 1;
    let highest_row = board.highest_row() as i32;
    let spawn_y = if highest_row >= 20 {
        (highest_row + 1).min(BOARD_HEIGHT as i32 - 3)
    } else {
        20
    };

    let start_state = SearchState {
        rotation: Rotation::North,
        x: spawn_x,
        y: spawn_y,
    };

    let state_index = |rot: Rotation, x: i32, y: i32| -> usize {
        let rot_idx = match rot {
            Rotation::North => 0,
            Rotation::East => 1,
            Rotation::South => 2,
            Rotation::West => 3,
        };
        let x_idx = (x + 2).clamp(0, 15) as usize;
        let y_idx = y.clamp(0, 39) as usize;
        rot_idx * 640 + x_idx * 40 + y_idx
    };

    if board.fits(piece, start_state.rotation, start_state.x, start_state.y) {
        queue[tail] = start_state;
        tail += 1;
        visited[state_index(start_state.rotation, start_state.x, start_state.y)] = true;
    } else {
        return lock_moves;
    }

    while head < tail {
        let current = queue[head];
        head += 1;

        let x = current.x;
        let y = current.y;
        let rot = current.rotation;

        // Check if this is a locking position (cannot move down)
        if !board.fits(piece, rot, x, y - 1) {
            let idx = state_index(rot, x, y);
            if !lock_states[idx] {
                lock_states[idx] = true;
                lock_moves.push(Move::new(piece, rot, x, y));
            }
        }

        // Try Left
        let left_x = x - 1;
        let left_idx = state_index(rot, left_x, y);
        if !visited[left_idx] && board.fits(piece, rot, left_x, y) {
            visited[left_idx] = true;
            if tail < 1024 {
                queue[tail] = SearchState { rotation: rot, x: left_x, y };
                tail += 1;
            }
        }

        // Try Right
        let right_x = x + 1;
        let right_idx = state_index(rot, right_x, y);
        if !visited[right_idx] && board.fits(piece, rot, right_x, y) {
            visited[right_idx] = true;
            if tail < 1024 {
                queue[tail] = SearchState { rotation: rot, x: right_x, y };
                tail += 1;
            }
        }

        // Try Soft Drop
        let down_y = y - 1;
        if y > 0 {
            let down_idx = state_index(rot, x, down_y);
            if !visited[down_idx] && board.fits(piece, rot, x, down_y) {
                visited[down_idx] = true;
                if tail < 1024 {
                    queue[tail] = SearchState { rotation: rot, x, y: down_y };
                    tail += 1;
                }
            }
        }

        // Try Clockwise Rotation
        let cw_rot = rot.rotate_cw();
        let kicks = if piece == Piece::I {
            get_srs_kicks_i(rot, cw_rot)
        } else {
            get_srs_kicks(rot, cw_rot)
        };

        for &(dx, dy) in kicks.iter() {
            let next_x = x + dx;
            let next_y = y + dy;
            if board.fits(piece, cw_rot, next_x, next_y) {
                let rot_idx = state_index(cw_rot, next_x, next_y);
                if !visited[rot_idx] {
                    visited[rot_idx] = true;
                    if tail < 1024 {
                        queue[tail] = SearchState { rotation: cw_rot, x: next_x, y: next_y };
                        tail += 1;
                    }
                }
                break;
            }
        }

        // Try Counter-Clockwise Rotation
        let ccw_rot = rot.rotate_ccw();
        let kicks = if piece == Piece::I {
            get_srs_kicks_i(rot, ccw_rot)
        } else {
            get_srs_kicks(rot, ccw_rot)
        };

        for &(dx, dy) in kicks.iter() {
            let next_x = x + dx;
            let next_y = y + dy;
            if board.fits(piece, ccw_rot, next_x, next_y) {
                let rot_idx = state_index(ccw_rot, next_x, next_y);
                if !visited[rot_idx] {
                    visited[rot_idx] = true;
                    if tail < 1024 {
                        queue[tail] = SearchState { rotation: ccw_rot, x: next_x, y: next_y };
                        tail += 1;
                    }
                }
                break;
            }
        }
    }

    lock_moves
}
