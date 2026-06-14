use crate::engine::header::{Piece, Rotation};
use crate::engine::piece::get_piece_cells;

pub const BOARD_HEIGHT: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Board {
    pub width: usize,
    pub rows: [u16; BOARD_HEIGHT],
}

impl Board {
    pub fn new(width: usize) -> Self {
        debug_assert!(width <= 16);
        Self {
            width,
            rows: [0; BOARD_HEIGHT],
        }
    }

    pub fn full_row_mask(&self) -> u16 {
        (1 << self.width) - 1
    }

    pub fn occupied(&self, x: i32, y: i32) -> bool {
        if x < 0 || x >= self.width as i32 {
            return true; // Out of bounds horizontally is considered occupied/wall
        }
        if y < 0 {
            return true; // Out of bounds vertically (below floor) is occupied
        }
        if y >= BOARD_HEIGHT as i32 {
            return false; // Sky is open
        }
        (self.rows[y as usize] & (1 << x)) != 0
    }

    pub fn obstructed(&self, x: i32, y: i32) -> bool {
        self.occupied(x, y)
    }

    pub fn fits(&self, piece: Piece, rotation: Rotation, x: i32, y: i32) -> bool {
        let cells = get_piece_cells(piece, rotation);
        for cell in cells.iter() {
            let cx = x + cell.x;
            let cy = y + cell.y;
            if self.obstructed(cx, cy) {
                return false;
            }
        }
        true
    }

    pub fn place(&mut self, piece: Piece, rotation: Rotation, x: i32, y: i32) {
        let cells = get_piece_cells(piece, rotation);
        for cell in cells.iter() {
            let cx = x + cell.x;
            let cy = y + cell.y;
            if cx >= 0 && cx < self.width as i32 && cy >= 0 && cy < BOARD_HEIGHT as i32 {
                self.rows[cy as usize] |= 1 << cx;
            }
        }
    }

    pub fn clear_lines(&mut self) -> u32 {
        let mask = self.full_row_mask();
        let mut cleared = 0;
        let mut write_idx = 0;
        let mut new_rows = [0u16; BOARD_HEIGHT];

        for read_idx in 0..BOARD_HEIGHT {
            if self.rows[read_idx] == mask {
                cleared += 1;
            } else {
                new_rows[write_idx] = self.rows[read_idx];
                write_idx += 1;
            }
        }
        self.rows = new_rows;
        cleared
    }

    /// Return the height of each column.
    pub fn column_heights(&self) -> Vec<usize> {
        let mut heights = vec![0; self.width];
        for x in 0..self.width {
            let mut col_h = 0;
            for y in (0..BOARD_HEIGHT).rev() {
                if (self.rows[y] & (1 << x)) != 0 {
                    col_h = y + 1;
                    break;
                }
            }
            heights[x] = col_h;
        }
        heights
    }

    /// Return the total number of holes. A hole is an empty cell with at least one occupied cell above it in the same column.
    pub fn holes_count(&self) -> u32 {
        let mut holes = 0;
        for x in 0..self.width {
            let mut block_found = false;
            for y in (0..BOARD_HEIGHT).rev() {
                if (self.rows[y] & (1 << x)) != 0 {
                    block_found = true;
                } else if block_found {
                    holes += 1;
                }
            }
        }
        holes
    }

    /// Count cells that are directly or indirectly above a hole.
    pub fn cell_coveredness(&self) -> u32 {
        let mut covered = 0;
        for x in 0..self.width {
            let mut hole_found = false;
            let mut column_blocks_above_hole = 0;
            for y in 0..BOARD_HEIGHT {
                if (self.rows[y] & (1 << x)) == 0 {
                    hole_found = true;
                    // Reset blocks count as we look for blocks *above* a hole
                    covered += column_blocks_above_hole;
                    column_blocks_above_hole = 0;
                } else if hole_found {
                    column_blocks_above_hole += 1;
                }
            }
            covered += column_blocks_above_hole;
        }
        covered
    }

    pub fn highest_row(&self) -> usize {
        for y in (0..BOARD_HEIGHT).rev() {
            if self.rows[y] != 0 {
                return y + 1;
            }
        }
        0
    }

    pub fn spawn_garbage(&mut self, lines: i32, hole_x: i32) {
        if lines <= 0 {
            return;
        }
        let lines = lines as usize;
        // Shift rows up
        for y in (lines..BOARD_HEIGHT).rev() {
            self.rows[y] = self.rows[y - lines];
        }
        // Fill bottom lines with garbage (all blocks except hole_x)
        let mask = self.full_row_mask();
        let garbage_row = mask & !(1 << hole_x);
        for y in 0..lines.min(BOARD_HEIGHT) {
            self.rows[y] = garbage_row;
        }
    }
}
