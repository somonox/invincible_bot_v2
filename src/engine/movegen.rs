use crate::engine::board::{Board, BOARD_HEIGHT};
use crate::engine::header::{Move, Piece, Rotation, Spin, SpinMode};
use crate::engine::piece::{get_piece_cells, get_srs_kicks, get_srs_kicks_i};
use std::collections::{HashSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SearchState {
    pub rotation: Rotation,
    pub x: i32,
    pub y: i32,
}

/// Call only after a successful rotation. Translation (including a nonzero drop)
/// clears this result. Fin/TST upgrades apply only to the matching quarter-turn kick.
pub fn detect_spin(board: &Board, piece: Piece, s: SearchState, fin: bool, mode: SpinMode) -> Spin {
    if board.fits(piece, s.rotation, s.x, s.y - 1) {
        return Spin::None;
    }
    if piece == Piece::T {
        let corners =
            [(-1, 1), (1, 1), (1, -1), (-1, -1)].map(|(dx, dy)| board.occupied(s.x + dx, s.y + dy));
        if corners.iter().filter(|&&c| c).count() >= 3 {
            let front = s.rotation as usize;
            return if fin || (corners[front] && corners[(front + 1) % 4]) {
                Spin::Full
            } else {
                Spin::Mini
            };
        }
        if mode != SpinMode::AllMiniPlus {
            return Spin::None;
        }
    } else if mode == SpinMode::TSpins {
        return Spin::None;
    }
    let immobile = [(-1, 0), (1, 0), (0, 1)]
        .iter()
        .all(|&(dx, dy)| !board.fits(piece, s.rotation, s.x + dx, s.y + dy));
    if !immobile {
        Spin::None
    } else if mode == SpinMode::AllMiniPlus {
        Spin::Mini
    } else {
        Spin::Full
    }
}

pub fn rotate(
    board: &Board,
    piece: Piece,
    s: SearchState,
    to: Rotation,
    mode: SpinMode,
) -> Option<(SearchState, Spin)> {
    let kicks = if piece == Piece::I {
        get_srs_kicks_i(s.rotation, to)
    } else {
        get_srs_kicks(s.rotation, to)
    };
    for &(dx, dy) in kicks {
        let n = SearchState {
            rotation: to,
            x: s.x + dx,
            y: s.y + dy,
        };
        if board.fits(piece, to, n.x, n.y) {
            let fin = matches!(
                (s.rotation, to, dx, dy),
                (Rotation::North | Rotation::South, Rotation::East, -1, -2)
                    | (Rotation::North | Rotation::South, Rotation::West, 1, -2)
            );
            return Some((n, detect_spin(board, piece, n, fin, mode)));
        }
    }
    None
}

fn neighbors(
    board: &Board,
    piece: Piece,
    s: SearchState,
    mode: SpinMode,
    sonic: bool,
) -> Vec<(SearchState, Spin, &'static str)> {
    let mut result = Vec::with_capacity(6);
    for (dx, dy, key) in [
        (-1, 0, "moveLeft"),
        (1, 0, "moveRight"),
        (0, -1, "softDrop"),
    ] {
        let mut n = SearchState {
            x: s.x + dx,
            y: s.y + dy,
            ..s
        };
        if board.fits(piece, n.rotation, n.x, n.y) {
            if sonic && dy == -1 {
                while board.fits(piece, n.rotation, n.x, n.y - 1) {
                    n.y -= 1;
                }
            }
            result.push((n, Spin::None, key));
        }
    }
    for (to, key) in [
        (s.rotation.rotate_cw(), "rotateCW"),
        (s.rotation.rotate_ccw(), "rotateCCW"),
        (s.rotation.rotate_180(), "rotate180"),
    ] {
        if let Some((n, spin)) = rotate(board, piece, s, to, mode) {
            result.push((n, spin, key));
        }
    }
    result
}

fn spawn(board: &Board, piece: Piece, fast: bool) -> Option<SearchState> {
    let highest = board.highest_row() as i32;
    let mut s = SearchState {
        rotation: Rotation::North,
        x: board.width as i32 / 2 - 1,
        y: if highest >= 20 {
            (highest + 1).min(BOARD_HEIGHT as i32 - 3)
        } else {
            20
        },
    };
    if !board.fits(piece, s.rotation, s.x, s.y) {
        return None;
    }
    if fast {
        s.y = s.y.min(highest + 4);
    }
    Some(s)
}
fn index(s: SearchState, spin: Spin) -> Option<usize> {
    if !(-2..18).contains(&s.x) || !(-2..BOARD_HEIGHT as i32 + 2).contains(&s.y) {
        return None;
    }
    Some(
        ((spin as usize * 4 + s.rotation as usize) * 20 + (s.x + 2) as usize) * (BOARD_HEIGHT + 4)
            + (s.y + 2) as usize,
    )
}

pub fn generate_moves(board: &Board, piece: Piece) -> Vec<Move> {
    generate_moves_with_rules(board, piece, SpinMode::All)
}
pub fn generate_moves_with_rules(board: &Board, piece: Piece, mode: SpinMode) -> Vec<Move> {
    let Some(start) = spawn(board, piece, true) else {
        return Vec::new();
    };
    let mut visited = vec![false; 3 * 4 * 20 * (BOARD_HEIGHT + 4)];
    let mut queue = Vec::with_capacity(512);
    let mut placements = Vec::new();
    let mut moves = Vec::with_capacity(32);
    visited[index(start, Spin::None).unwrap()] = true;
    queue.push((start, Spin::None));
    let mut head = 0;
    while head < queue.len() {
        let (s, spin) = queue[head];
        head += 1;
        if !board.fits(piece, s.rotation, s.x, s.y - 1) {
            let mut cells =
                get_piece_cells(piece, s.rotation).map(|c| (s.y + c.y) * 16 + s.x + c.x);
            cells.sort_unstable();
            if !placements.contains(&(cells, spin)) {
                placements.push((cells, spin));
                moves.push(Move {
                    piece,
                    rotation: s.rotation,
                    x: s.x,
                    y: s.y,
                    spin,
                });
            }
        }
        for (n, spin, _) in neighbors(board, piece, s, mode, false) {
            if let Some(i) = index(n, spin) {
                if !visited[i] {
                    visited[i] = true;
                    queue.push((n, spin));
                }
            }
        }
    }
    moves
}

/// Triangle softDrop is a sonic drop. Never compress one-cell drops across a
/// path that actually requires a midair turn, or lose the final rotation/spin.
pub fn find_input_path(board: &Board, target: Move, mode: SpinMode) -> Option<Vec<String>> {
    let start = spawn(board, target.piece, false)?;
    let mut seen = HashSet::from([(start, Spin::None)]);
    let mut queue = VecDeque::from([(start, Spin::None, Vec::new())]);
    while let Some((s, spin, path)) = queue.pop_front() {
        if (s.rotation, s.x, s.y, spin) == (target.rotation, target.x, target.y, target.spin) {
            let mut result = path;
            result.push("hardDrop".to_string());
            return Some(result);
        }
        for (n, spin, key) in neighbors(board, target.piece, s, mode, true) {
            if index(n, spin).is_some() && seen.insert((n, spin)) {
                let mut p = path.clone();
                p.push(key.to_string());
                queue.push_back((n, spin, p));
            }
        }
    }
    None
}
