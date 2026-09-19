#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Piece {
    I = 0,
    O = 1,
    T = 2,
    L = 3,
    J = 4,
    S = 5,
    Z = 6,
}

pub const ALL_PIECES: [Piece; 7] = [
    Piece::I,
    Piece::O,
    Piece::T,
    Piece::L,
    Piece::J,
    Piece::S,
    Piece::Z,
];

impl Piece {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Piece::I,
            1 => Piece::O,
            2 => Piece::T,
            3 => Piece::L,
            4 => Piece::J,
            5 => Piece::S,
            6 => Piece::Z,
            _ => Piece::T,
        }
    }

    pub fn color(self) -> [u8; 3] {
        // Aesthetic modern HSL/RGB colors for Tetris pieces (soft/vibrant, not fully saturated)
        match self {
            Piece::I => [0, 180, 216], // Sleek Cyan
            Piece::O => [255, 195, 0], // Sleek Yellow
            Piece::T => [162, 0, 255], // Deep Purple
            Piece::L => [255, 109, 0], // Bright Orange
            Piece::J => [0, 114, 255], // Royal Blue
            Piece::S => [56, 176, 0],  // Emerald Green
            Piece::Z => [224, 30, 90], // Crimson Red
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Rotation {
    North = 0,
    East = 1,
    South = 2,
    West = 3,
}

pub const ALL_ROTATIONS: [Rotation; 4] = [
    Rotation::North,
    Rotation::East,
    Rotation::South,
    Rotation::West,
];

impl Rotation {
    pub fn from_u8(v: u8) -> Self {
        match v {
            0 => Rotation::North,
            1 => Rotation::East,
            2 => Rotation::South,
            3 => Rotation::West,
            _ => Rotation::North,
        }
    }

    pub fn rotate_cw(self) -> Self {
        match self {
            Rotation::North => Rotation::East,
            Rotation::East => Rotation::South,
            Rotation::South => Rotation::West,
            Rotation::West => Rotation::North,
        }
    }

    pub fn rotate_ccw(self) -> Self {
        match self {
            Rotation::North => Rotation::West,
            Rotation::East => Rotation::North,
            Rotation::South => Rotation::East,
            Rotation::West => Rotation::South,
        }
    }

    pub fn rotate_180(self) -> Self {
        match self {
            Rotation::North => Rotation::South,
            Rotation::East => Rotation::West,
            Rotation::South => Rotation::North,
            Rotation::West => Rotation::East,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Coordinates {
    pub x: i32,
    pub y: i32,
}

impl Coordinates {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

impl std::ops::Add for Coordinates {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
        }
    }
}

impl std::ops::Sub for Coordinates {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum Spin {
    #[default]
    None,
    Mini,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SpinMode {
    #[default]
    All,
    AllMiniPlus,
    AllMini,
    AllPlus,
    TSpins,
    TSpinsPlus,
    MiniOnly,
    Handheld,
    Stupid,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ComboMode {
    #[default]
    Multiplier,
    Classic,
    Modern,
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Move {
    pub spin: Spin,
    pub piece: Piece,
    pub rotation: Rotation,
    pub x: i32,
    pub y: i32,
}

impl Move {
    pub fn new(piece: Piece, rotation: Rotation, x: i32, y: i32) -> Self {
        Self {
            piece,
            rotation,
            x,
            y,
            spin: Spin::None,
        }
    }
}
