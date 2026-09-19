use four_wide_bot::engine::board::{Board, BOARD_HEIGHT};
use rand::{rngs::StdRng, Rng, SeedableRng};
#[test]
fn bit_operations_match_cell_by_cell_reference_for_every_width() {
    let mut rng = StdRng::seed_from_u64(20260920);
    for width in 1..=16 {
        for sample in 0..100 {
            let mut b = Board::new(width);
            let height = sample % 41;
            for row in &mut b.rows[..height] {
                *row = rng.gen::<u16>() & ((1u32 << width) - 1) as u16;
            }
            let mut heights = vec![0; width];
            let (mut holes, mut covered, mut vertical) = (0, 0, 0);
            for x in 0..width {
                let mut above = false;
                for y in (0..BOARD_HEIGHT).rev() {
                    if b.rows[y] & (1 << x) != 0 {
                        above = true;
                        heights[x] = heights[x].max(y + 1);
                    } else if above {
                        holes += 1;
                    }
                }
                let (mut empty, mut blocks) = (false, 0);
                let mut previous = false;
                for y in 0..BOARD_HEIGHT {
                    let occupied = b.rows[y] & (1 << x) != 0;
                    if !occupied {
                        empty = true;
                        covered += blocks;
                        blocks = 0;
                    } else if empty {
                        blocks += 1;
                    }
                    if occupied != previous {
                        vertical += 1;
                    }
                    previous = occupied;
                }
                covered += blocks;
            }
            let limit = (heights.iter().max().unwrap() + 2).min(BOARD_HEIGHT);
            let mut horizontal = 0;
            for y in 0..limit {
                let mut previous = true;
                for x in 0..width {
                    let occupied = b.rows[y] & (1 << x) != 0;
                    if occupied != previous {
                        horizontal += 1;
                    }
                    previous = occupied;
                }
                if !previous {
                    horizontal += 1;
                }
            }
            assert_eq!(b.column_heights(), heights);
            assert_eq!(b.holes_count(), holes);
            assert_eq!(b.cell_coveredness(), covered);
            assert_eq!(
                b.transitions(limit),
                (horizontal, vertical),
                "width={width} sample={sample}"
            );
        }
    }
}
