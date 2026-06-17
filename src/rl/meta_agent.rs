use crate::engine::state::GameState;
use crate::rl::features::Weights;

pub const PARAM_COUNT: usize = 191;

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MetaPolicyNetwork {
    pub w1: [[f32; 6]; 8],
    pub b1: [f32; 8],
    pub w2: [[f32; 8]; 15],
    pub b2: [f32; 15],
}

impl Default for MetaPolicyNetwork {
    fn default() -> Self {
        // By default, all parameters are zero, which means the network outputs 0.0 offsets,
        // fallback to the default static weights (Residual learning baseline).
        Self {
            w1: [[0.0f32; 6]; 8],
            b1: [0.0f32; 8],
            w2: [[0.0f32; 8]; 15],
            b2: [0.0f32; 15],
        }
    }
}

impl MetaPolicyNetwork {
    pub fn new_random() -> Self {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let mut w1 = [[0.0f32; 6]; 8];
        let mut b1 = [0.0f32; 8];
        let mut w2 = [[0.0f32; 8]; 15];
        let mut b2 = [0.0f32; 15];

        // Small random initial weights around 0
        for i in 0..8 {
            for j in 0..6 {
                w1[i][j] = rng.gen_range(-0.5..0.5);
            }
            b1[i] = rng.gen_range(-0.1..0.1);
        }
        for i in 0..15 {
            for j in 0..8 {
                w2[i][j] = rng.gen_range(-0.5..0.5);
            }
            b2[i] = rng.gen_range(-0.1..0.1);
        }

        Self { w1, b1, w2, b2 }
    }

    /// Extract 6 macro state features from current game and opponent state
    pub fn extract_inputs(state: &GameState, opponent_state: Option<&GameState>) -> [f32; 6] {
        let own_height = state.board.highest_row() as f32;
        let own_holes = state.board.holes_count() as f32;
        let own_pending = (state.pending_garbage + state.queued_garbage) as f32;

        let opp_height = opponent_state.map_or(0.0, |opp| opp.board.highest_row() as f32);
        let opp_holes = opponent_state.map_or(0.0, |opp| opp.board.holes_count() as f32);
        let opp_pending = opponent_state.map_or(0.0, |opp| (opp.pending_garbage + opp.queued_garbage) as f32);

        [
            (own_height / 20.0).min(1.5),
            (own_holes / 15.0).min(1.5),
            (own_pending / 12.0).min(1.5),
            (opp_height / 20.0).min(1.5),
            (opp_holes / 15.0).min(1.5),
            (opp_pending / 12.0).min(1.5),
        ]
    }

    /// Forward pass of the MLP to generate the customized weights
    pub fn forward(&self, inputs: &[f32; 6]) -> Weights {
        // Hidden Layer 1: Input (6) -> Hidden (8) + ReLU
        let mut hidden = [0.0f32; 8];
        for i in 0..8 {
            let mut sum = self.b1[i];
            for j in 0..6 {
                sum += self.w1[i][j] * inputs[j];
            }
            hidden[i] = sum.max(0.0); // ReLU
        }

        // Output Layer 2: Hidden (8) -> Output (15)
        let mut outputs = [0.0f32; 15];
        for i in 0..15 {
            let mut sum = self.b2[i];
            for j in 0..8 {
                sum += self.w2[i][j] * hidden[j];
            }
            outputs[i] = sum;
        }

        // Residual addition to default static weights
        let default_w = Weights::default().to_array();
        let mut final_w = [0.0f32; 15];
        for i in 0..15 {
            // Apply a clamp to the offsets to keep them stable and prevent extreme divergence
            let offset = outputs[i].clamp(-20.0, 20.0);
            final_w[i] = default_w[i] + offset;
        }

        Weights::from_array(final_w)
    }

    /// Convert the 191 parameters to a flat array
    pub fn to_array(&self) -> [f32; PARAM_COUNT] {
        let mut arr = [0.0f32; PARAM_COUNT];
        let mut idx = 0;

        for i in 0..8 {
            for j in 0..6 {
                arr[idx] = self.w1[i][j];
                idx += 1;
            }
        }
        for i in 0..8 {
            arr[idx] = self.b1[i];
            idx += 1;
        }
        for i in 0..15 {
            for j in 0..8 {
                arr[idx] = self.w2[i][j];
                idx += 1;
            }
        }
        for i in 0..15 {
            arr[idx] = self.b2[i];
            idx += 1;
        }

        arr
    }

    /// Construct the network from a flat array of 191 parameters
    pub fn from_array(arr: [f32; PARAM_COUNT]) -> Self {
        let mut w1 = [[0.0f32; 6]; 8];
        let mut b1 = [0.0f32; 8];
        let mut w2 = [[0.0f32; 8]; 15];
        let mut b2 = [0.0f32; 15];
        let mut idx = 0;

        for i in 0..8 {
            for j in 0..6 {
                w1[i][j] = arr[idx];
                idx += 1;
            }
        }
        for i in 0..8 {
            b1[i] = arr[idx];
            idx += 1;
        }
        for i in 0..15 {
            for j in 0..8 {
                w2[i][j] = arr[idx];
                idx += 1;
            }
        }
        for i in 0..15 {
            b2[i] = arr[idx];
            idx += 1;
        }

        Self { w1, b1, w2, b2 }
    }
}
