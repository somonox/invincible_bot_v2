use crate::engine::header::{Piece, Rotation};
use crate::engine::piece::get_piece_cells;

pub const BOARD_HEIGHT: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Board {
    pub width: usize,
    /// Visible field height used as the spawn baseline; storage includes headroom.
    pub spawn_height: i32,
    pub rows: [u16; BOARD_HEIGHT],
}

impl Board {
    pub fn new(width: usize) -> Self {
        debug_assert!(width <= 16);
        Self {
            width,
            spawn_height: 20,
            rows: [0; BOARD_HEIGHT],
        }
    }

    pub fn full_row_mask(&self) -> u16 {
        ((1u32 << self.width) - 1) as u16
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
        self.column_heights_array()[..self.width].to_vec()
    }

    pub fn column_heights_array(&self) -> [usize; 16] {
        let mut heights = [0; 16];
        let mut missing = self.full_row_mask();
        for y in (0..BOARD_HEIGHT).rev() {
            let mut first = self.rows[y] & missing;
            missing &= !first;
            while first != 0 {
                let x = first.trailing_zeros() as usize;
                heights[x] = y + 1;
                first &= first - 1;
            }
            if missing == 0 {
                break;
            }
        }
        heights
    }

    /// Count holes in all columns together with row bit operations.
    pub fn holes_count(&self) -> u32 {
        let mut above = 0u16;
        let mask = self.full_row_mask();
        self.rows
            .iter()
            .rev()
            .map(|&row| {
                let holes = (above & !row & mask).count_ones();
                above |= row;
                holes
            })
            .sum()
    }

    /// Preserve the original definition: occupied cells above any empty cell.
    pub fn cell_coveredness(&self) -> u32 {
        let mut empty_below = 0u16;
        let mask = self.full_row_mask();
        self.rows
            .iter()
            .map(|&row| {
                let covered = (row & empty_below & mask).count_ones();
                empty_below |= !row;
                covered
            })
            .sum()
    }

    pub fn transitions(&self, check_height: usize) -> (u32, u32) {
        #[cfg(all(target_arch = "aarch64", target_feature = "neon", feature = "arm-neon"))]
        if (1..16).contains(&self.width) {
            // ARM64 NEON loads exactly five groups of eight rows. Width 16
            // needs a 17th horizontal boundary bit and uses the scalar path.
            return unsafe { self.transitions_neon(check_height) };
        }
        self.transitions_scalar(check_height)
    }

    fn transitions_scalar(&self, check_height: usize) -> (u32, u32) {
        if self.width == 0 {
            return (0, 0);
        }
        let mask = self.full_row_mask() as u32;
        let walls = (1u32 << self.width) | 1;
        let mut horizontal = 0;
        let mut vertical = 0;
        let mut previous = 0;
        for (y, &bits) in self.rows.iter().enumerate() {
            let row = bits as u32 & mask;
            if y < check_height {
                horizontal += (((row << 1) ^ row ^ walls) & ((mask << 1) | 1)).count_ones();
            }
            vertical += (row ^ previous).count_ones();
            previous = row;
        }
        (horizontal, vertical)
    }

    #[cfg(all(target_arch = "aarch64", target_feature = "neon", feature = "arm-neon"))]
    #[target_feature(enable = "neon")]
    unsafe fn transitions_neon(&self, check_height: usize) -> (u32, u32) {
        use std::arch::aarch64::*;
        let mask = vdupq_n_u16(self.full_row_mask());
        let horizontal_mask = vdupq_n_u16((self.full_row_mask() << 1) | 1);
        let walls = vdupq_n_u16((1u16 << self.width) | 1);
        let lane_numbers = [0u16, 1, 2, 3, 4, 5, 6, 7];
        let lanes = vld1q_u16(lane_numbers.as_ptr());
        let limit = vdupq_n_u16(check_height.min(BOARD_HEIGHT) as u16);
        let mut previous = vdupq_n_u16(0);
        let (mut horizontal, mut vertical) = (0u32, 0u32);
        for start in (0..BOARD_HEIGHT).step_by(8) {
            // start <= 32, so all 8 u16 values lie inside the 40-row array.
            let rows = vandq_u16(vld1q_u16(self.rows.as_ptr().add(start)), mask);
            let preceding = vextq_u16::<7>(previous, rows);
            let differences = veorq_u16(rows, preceding);
            vertical += vaddvq_u8(vcntq_u8(vreinterpretq_u8_u16(differences))) as u32;
            let horizontal_bits = vandq_u16(
                veorq_u16(veorq_u16(vshlq_n_u16::<1>(rows), rows), walls),
                horizontal_mask,
            );
            let active = vcltq_u16(vaddq_u16(lanes, vdupq_n_u16(start as u16)), limit);
            horizontal += vaddvq_u8(vcntq_u8(vreinterpretq_u8_u16(vandq_u16(
                horizontal_bits,
                active,
            )))) as u32;
            previous = rows;
        }
        (horizontal, vertical)
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
