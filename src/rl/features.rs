use crate::engine::board::BOARD_HEIGHT;
use crate::engine::state::GameState;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Weights {
    pub holes: f32,
    pub cell_coveredness: f32,
    pub height_max: f32,
    pub height_avg: f32,
    pub bumpiness: f32,
    pub row_transitions: f32,
    pub col_transitions: f32,
    pub well_depth: f32,
    pub four_wide_well: f32,
    pub combo_reward: f32,

    // Opponent / Multiplayer features
    pub opp_height_max: f32,
    pub opp_holes: f32,
    pub opp_pending_garbage: f32,
    pub own_pending_garbage: f32,
    pub own_queued_garbage: f32,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            holes: -8.0,
            cell_coveredness: -2.0,
            height_max: -1.5,
            height_avg: -1.0,
            bumpiness: -0.5,
            row_transitions: -0.8,
            col_transitions: -0.8,
            well_depth: -0.3,
            four_wide_well: 2.5,
            combo_reward: 1.5,
            
            // Opponent weights
            opp_height_max: 0.2,
            opp_holes: 0.3,
            opp_pending_garbage: 1.5,
            own_pending_garbage: -2.5,
            own_queued_garbage: -1.2,
        }
    }
}

impl Weights {
    pub fn zero() -> Self {
        Self {
            holes: 0.0,
            cell_coveredness: 0.0,
            height_max: 0.0,
            height_avg: 0.0,
            bumpiness: 0.0,
            row_transitions: 0.0,
            col_transitions: 0.0,
            well_depth: 0.0,
            four_wide_well: 0.0,
            combo_reward: 0.0,
            opp_height_max: 0.0,
            opp_holes: 0.0,
            opp_pending_garbage: 0.0,
            own_pending_garbage: 0.0,
            own_queued_garbage: 0.0,
        }
    }

    pub fn to_array(&self) -> [f32; 15] {
        [
            self.holes,
            self.cell_coveredness,
            self.height_max,
            self.height_avg,
            self.bumpiness,
            self.row_transitions,
            self.col_transitions,
            self.well_depth,
            self.four_wide_well,
            self.combo_reward,
            self.opp_height_max,
            self.opp_holes,
            self.opp_pending_garbage,
            self.own_pending_garbage,
            self.own_queued_garbage,
        ]
    }

    pub fn from_array(arr: [f32; 15]) -> Self {
        Self {
            holes: arr[0],
            cell_coveredness: arr[1],
            height_max: arr[2],
            height_avg: arr[3],
            bumpiness: arr[4],
            row_transitions: arr[5],
            col_transitions: arr[6],
            well_depth: arr[7],
            four_wide_well: arr[8],
            combo_reward: arr[9],
            opp_height_max: arr[10],
            opp_holes: arr[11],
            opp_pending_garbage: arr[12],
            own_pending_garbage: arr[13],
            own_queued_garbage: arr[14],
        }
    }
}

pub struct Features {
    pub holes: f32,
    pub cell_coveredness: f32,
    pub height_max: f32,
    pub height_avg: f32,
    pub bumpiness: f32,
    pub row_transitions: f32,
    pub col_transitions: f32,
    pub well_depth: f32,
    pub four_wide_well: f32,
    pub combo_reward: f32,
    
    // Opponent / Multiplayer features
    pub opp_height_max: f32,
    pub opp_holes: f32,
    pub opp_pending_garbage: f32,
    pub own_pending_garbage: f32,
    pub own_queued_garbage: f32,
}

impl Features {
    pub fn evaluate_state(state: &GameState, opponent: Option<&GameState>) -> Self {
        let board = &state.board;
        let heights = board.column_heights();
        let max_h = heights.iter().copied().max().unwrap_or(0);
        let avg_h = if board.width > 0 {
            heights.iter().sum::<usize>() as f32 / board.width as f32
        } else {
            0.0
        };

        // Holes and Coveredness
        let holes = board.holes_count() as f32;
        let cell_coveredness = board.cell_coveredness() as f32;

        // Bumpiness
        let mut bumpiness = 0.0;
        if board.width > 1 {
            for i in 0..(board.width - 1) {
                bumpiness += (heights[i] as i32 - heights[i + 1] as i32).abs() as f32;
            }
        }

        // Row Transitions (horizontal state changes)
        let mut row_transitions = 0.0;
        let check_limit_y = (max_h + 2).min(BOARD_HEIGHT);
        for y in 0..check_limit_y {
            let mut prev_cell = true; // Board boundary is considered filled
            for x in 0..board.width {
                let cell = (board.rows[y] & (1 << x)) != 0;
                if cell != prev_cell {
                    row_transitions += 1.0;
                }
                prev_cell = cell;
            }
            if !prev_cell {
                row_transitions += 1.0; // Transition to right wall
            }
        }

        // Column Transitions (vertical state changes)
        let mut col_transitions = 0.0;
        for x in 0..board.width {
            let mut prev_cell = false; // Below floor is considered filled
            for y in 0..BOARD_HEIGHT {
                let cell = (board.rows[y] & (1 << x)) != 0;
                if cell != prev_cell {
                    col_transitions += 1.0;
                }
                prev_cell = cell;
            }
        }

        // Well Depths
        let mut well_depth = 0.0;
        for x in 0..board.width {
            let left_h = if x > 0 { heights[x - 1] as i32 } else { BOARD_HEIGHT as i32 };
            let right_h = if x < board.width - 1 { heights[x + 1] as i32 } else { BOARD_HEIGHT as i32 };
            let own_h = heights[x] as i32;
            let surrounding_min = left_h.min(right_h);
            if surrounding_min > own_h {
                well_depth += (surrounding_min - own_h) as f32;
            }
        }

        // Four-wide Well Score (for 10-column mode)
        let four_wide_well = if board.width == 10 {
            let left_well_avg = (heights[0] + heights[1] + heights[2] + heights[3]) as f32 / 4.0;
            let left_rest_avg = (heights[4] + heights[5] + heights[6] + heights[7] + heights[8] + heights[9]) as f32 / 6.0;
            let right_well_avg = (heights[6] + heights[7] + heights[8] + heights[9]) as f32 / 4.0;
            let right_rest_avg = (heights[0] + heights[1] + heights[2] + heights[3] + heights[4] + heights[5]) as f32 / 6.0;

            let left_diff = left_rest_avg - left_well_avg;
            let right_diff = right_rest_avg - right_well_avg;

            left_diff.max(right_diff).max(0.0)
        } else {
            0.0
        };

        // Evaluate Opponent state features
        let opp_height_max = if let Some(opp) = opponent {
            opp.board.highest_row() as f32
        } else {
            0.0
        };

        let opp_holes = if let Some(opp) = opponent {
            opp.board.holes_count() as f32
        } else {
            0.0
        };

        let opp_pending_garbage = if let Some(opp) = opponent {
            (opp.pending_garbage + opp.queued_garbage) as f32
        } else {
            0.0
        };

        let own_pending_garbage = state.pending_garbage as f32;
        let own_queued_garbage = state.queued_garbage as f32;

        Self {
            holes,
            cell_coveredness,
            height_max: max_h as f32,
            height_avg: avg_h,
            bumpiness,
            row_transitions,
            col_transitions,
            well_depth,
            four_wide_well,
            combo_reward: (state.combo as f32).powf(1.8),
            opp_height_max,
            opp_holes,
            opp_pending_garbage,
            own_pending_garbage,
            own_queued_garbage,
        }
    }

    pub fn dot_product(&self, weights: &Weights) -> f32 {
        let pc_bonus = if self.height_max == 0.0 { 150.0 } else { 0.0 };
        pc_bonus
            + self.holes * weights.holes
            + self.cell_coveredness * weights.cell_coveredness
            + self.height_max * weights.height_max
            + self.height_avg * weights.height_avg
            + self.bumpiness * weights.bumpiness
            + self.row_transitions * weights.row_transitions
            + self.col_transitions * weights.col_transitions
            + self.well_depth * weights.well_depth
            + self.four_wide_well * weights.four_wide_well
            + self.combo_reward * weights.combo_reward
            + self.opp_height_max * weights.opp_height_max
            + self.opp_holes * weights.opp_holes
            + self.opp_pending_garbage * weights.opp_pending_garbage
            + self.own_pending_garbage * weights.own_pending_garbage
            + self.own_queued_garbage * weights.own_queued_garbage
    }

    pub fn to_array(&self) -> [f32; 15] {
        [
            self.holes,
            self.cell_coveredness,
            self.height_max,
            self.height_avg,
            self.bumpiness,
            self.row_transitions,
            self.col_transitions,
            self.well_depth,
            self.four_wide_well,
            self.combo_reward,
            self.opp_height_max,
            self.opp_holes,
            self.opp_pending_garbage,
            self.own_pending_garbage,
            self.own_queued_garbage,
        ]
    }
}
