pub mod engine {
    pub mod header;
    pub mod piece;
    pub mod board;
    pub mod movegen;
    pub mod state;
    pub mod battle;
}

pub mod rl {
    pub mod features;
    pub mod agent;
    pub mod meta_agent;
    pub mod search;
}

#[cfg(feature = "gui")]
pub mod gui {
    pub mod widgets;
}
