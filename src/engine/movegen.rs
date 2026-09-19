use crate::engine::board::{Board, BOARD_HEIGHT};
use crate::engine::header::{Move, Piece, Rotation, Spin, SpinMode};
use crate::engine::piece::{get_piece_cells, get_srs_kicks, get_srs_kicks_i};
use std::cell::RefCell;
use std::collections::{HashSet, VecDeque};
use std::hash::{Hash, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SearchState {
    pub rotation: Rotation,
    pub x: i32,
    pub y: i32,
}

/// Call only after a successful rotation. Translation (including a nonzero drop)
/// clears this result. Fin/TST upgrades apply only to the matching quarter-turn kick.
pub fn detect_spin(board: &Board, piece: Piece, s: SearchState, fin: bool, mode: SpinMode) -> Spin {
    if mode == SpinMode::None || board.fits(piece, s.rotation, s.x, s.y - 1) {
        return Spin::None;
    }
    if mode == SpinMode::Stupid {
        return Spin::Full;
    }
    let mut corner_spin = Spin::None;
    if piece == Piece::T && mode != SpinMode::MiniOnly {
        let corners =
            [(-1, 1), (1, 1), (1, -1), (-1, -1)].map(|(dx, dy)| board.occupied(s.x + dx, s.y + dy));
        if corners.iter().filter(|&&c| c).count() >= 3 {
            let front = s.rotation as usize;
            corner_spin = if fin || (corners[front] && corners[(front + 1) % 4]) {
                Spin::Full
            } else {
                Spin::Mini
            };
        }
    }
    if mode == SpinMode::Handheld {
        if piece == Piece::T {
            return corner_spin;
        }
        // Triangle 4.2.7 cornerTable, converted from its downward Y offsets.
        let table = match piece {
            Piece::Z => [
                [(-2, -1), (1, -1), (2, 0), (-1, 0)],
                [(0, -1), (1, -2), (0, 2), (1, 1)],
                [(-2, 0), (1, 0), (2, 1), (-1, 1)],
                [(-1, -1), (0, -2), (0, 1), (-1, 2)],
            ],
            Piece::L => [
                [(-1, -1), (0, -1), (1, 1), (-1, 1)],
                [(-1, -1), (1, -1), (1, 0), (-1, 1)],
                [(-1, -1), (1, -1), (1, 1), (0, 1)],
                [(-1, 0), (1, -1), (1, 1), (-1, 1)],
            ],
            Piece::S => [
                [(-1, -1), (2, -1), (1, 0), (-2, 0)],
                [(0, -2), (1, -1), (1, 2), (0, 1)],
                [(-1, 0), (2, 0), (1, 1), (-2, 1)],
                [(-1, -2), (0, -1), (-1, 1), (0, 2)],
            ],
            Piece::J => [
                [(0, -1), (1, -1), (1, 1), (-1, 1)],
                [(-1, -1), (1, 0), (1, 1), (-1, 1)],
                [(-1, -1), (1, -1), (0, 1), (-1, 1)],
                [(-1, -1), (1, -1), (1, 1), (-1, 0)],
            ],
            _ => return Spin::None,
        };
        return if table[s.rotation as usize]
            .iter()
            .filter(|&&(dx, dy)| board.occupied(s.x + dx, s.y - dy))
            .count()
            >= 3
        {
            Spin::Full
        } else {
            Spin::None
        };
    }
    if mode == SpinMode::TSpins {
        return corner_spin;
    }
    if piece == Piece::T && matches!(mode, SpinMode::All | SpinMode::AllMini) {
        return corner_spin;
    }
    if mode == SpinMode::TSpinsPlus && piece != Piece::T {
        return Spin::None;
    }
    let immobile = [(-1, 0), (1, 0), (0, 1)]
        .iter()
        .all(|&(dx, dy)| !board.fits(piece, s.rotation, s.x + dx, s.y + dy));
    if corner_spin == Spin::Full {
        return Spin::Full;
    }
    if corner_spin == Spin::Mini {
        return Spin::Mini;
    }
    if !immobile {
        return Spin::None;
    }
    if matches!(
        mode,
        SpinMode::AllMini | SpinMode::AllMiniPlus | SpinMode::TSpinsPlus | SpinMode::MiniOnly
    ) || piece == Piece::T
    {
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
) -> impl Iterator<Item = (SearchState, Spin, &'static str)> {
    let mut result = [None; 6];
    let mut count = 0;
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
            result[count] = Some((n, Spin::None, key));
            count += 1;
        }
    }
    for (to, key) in [
        (s.rotation.rotate_cw(), "rotateCW"),
        (s.rotation.rotate_ccw(), "rotateCCW"),
        (s.rotation.rotate_180(), "rotate180"),
    ] {
        if let Some((n, spin)) = rotate(board, piece, s, to, mode) {
            result[count] = Some((n, spin, key));
            count += 1;
        }
    }
    result.into_iter().flatten()
}

fn spawn(board: &Board, piece: Piece, fast: bool) -> Option<SearchState> {
    let highest = board.highest_row() as i32;
    let mut s = SearchState {
        rotation: Rotation::North,
        x: board.width as i32 / 2 - 1,
        y: if highest >= board.spawn_height {
            (highest + 1).min(BOARD_HEIGHT as i32 - 3)
        } else {
            board.spawn_height
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
const VISITED_COUNT: usize = 3 * 4 * 20 * (BOARD_HEIGHT + 4);
const CACHE_SIZE: usize = 512;
struct MoveEntry {
    board: Board,
    piece: Piece,
    mode: SpinMode,
    moves: Vec<Move>,
}
struct MoveWorkspace {
    visited: Vec<u16>,
    epoch: u16,
    queue: Vec<(SearchState, Spin)>,
    placements: Vec<([i32; 4], Spin)>,
    cache: Vec<Option<MoveEntry>>,
}
impl MoveWorkspace {
    fn new() -> Self {
        Self {
            visited: vec![0; VISITED_COUNT],
            epoch: 0,
            queue: Vec::with_capacity(512),
            placements: Vec::with_capacity(32),
            cache: (0..CACHE_SIZE).map(|_| None).collect(),
        }
    }
}
thread_local! { static MOVE_WORKSPACE: RefCell<MoveWorkspace> = RefCell::new(MoveWorkspace::new()); }

/// Bounded per-thread cache. Equality checks guard hash collisions; spin rules,
/// full rows and width are part of the key. Combo/garbage cannot affect geometry.
pub fn generate_moves_with_rules(board: &Board, piece: Piece, mode: SpinMode) -> Vec<Move> {
    MOVE_WORKSPACE.with(|cell| {
        let mut work = cell.borrow_mut();
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        board.rows.hash(&mut hash);
        board.width.hash(&mut hash);
        board.spawn_height.hash(&mut hash);
        piece.hash(&mut hash);
        mode.hash(&mut hash);
        let slot = hash.finish() as usize % CACHE_SIZE;
        if let Some(entry) = &work.cache[slot] {
            if entry.board == *board && entry.piece == piece && entry.mode == mode {
                return entry.moves.clone();
            }
        }
        let moves = generate_uncached(board, piece, mode, &mut work);
        work.cache[slot] = Some(MoveEntry {
            board: *board,
            piece,
            mode,
            moves: moves.clone(),
        });
        moves
    })
}
fn generate_uncached(
    board: &Board,
    piece: Piece,
    mode: SpinMode,
    work: &mut MoveWorkspace,
) -> Vec<Move> {
    let Some(start) = spawn(board, piece, true) else {
        return Vec::new();
    };
    work.epoch = work.epoch.wrapping_add(1);
    if work.epoch == 0 {
        work.visited.fill(0);
        work.epoch = 1;
    }
    let epoch = work.epoch;
    let visited = &mut work.visited;
    let queue = &mut work.queue;
    let placements = &mut work.placements;
    queue.clear();
    placements.clear();
    let mut moves = Vec::with_capacity(32);
    visited[index(start, Spin::None).unwrap()] = epoch;
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
                if visited[i] != epoch {
                    visited[i] = epoch;
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

#[cfg(test)]
mod workspace_tests {
    use super::*;
    #[test]
    fn configured_height_controls_full_input_spawn() {
        let mut board = Board::new(4);
        board.spawn_height = 26;
        let start = spawn(&board, Piece::T, false).unwrap();
        assert_eq!(start.y, 26);
        board.rows[21] = 0b1000;
        for piece in crate::engine::header::ALL_PIECES {
            let moves = generate_moves_with_rules(&board, piece, SpinMode::All);
            assert!(!moves.is_empty());
            assert!(moves
                .iter()
                .any(|m| find_input_path(&board, *m, SpinMode::All).is_some()));
        }
    }

    #[test]
    fn epoch_wrap_does_not_leave_stale_visited_states() {
        let mut work = MoveWorkspace::new();
        work.epoch = u16::MAX - 1;
        let b = Board::new(4);
        let first = generate_uncached(&b, Piece::T, SpinMode::All, &mut work);
        assert_eq!(
            first,
            generate_uncached(&b, Piece::T, SpinMode::All, &mut work)
        );
        assert_eq!(work.epoch, 1);
    }
    #[test]
    fn cache_preserves_move_order_and_all_key_dimensions() {
        let mut work = MoveWorkspace::new();
        for width in [4, 10] {
            for mode in [SpinMode::All, SpinMode::AllMiniPlus, SpinMode::TSpins] {
                for piece in crate::engine::header::ALL_PIECES {
                    for rows in [[0, 0, 0], [3, 1, 0], [11, 9, 1]] {
                        let mut b = Board::new(width);
                        b.rows[..3].copy_from_slice(&rows);
                        let expected = generate_uncached(&b, piece, mode, &mut work);
                        for _ in 0..2 {
                            assert_eq!(generate_moves_with_rules(&b, piece, mode), expected);
                        }
                    }
                }
            }
        }
    }
}
