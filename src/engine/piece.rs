use crate::engine::header::{Coordinates, Piece, Rotation};

/// Get the 4 local coordinates of a piece at a given rotation.
pub fn get_piece_cells(piece: Piece, rotation: Rotation) -> [Coordinates; 4] {
    // We define a pivot at (0, 0) and 3 other cell offsets.
    let make_coords = |c0: (i32, i32), c1: (i32, i32), c2: (i32, i32), c3: (i32, i32)| {
        [
            Coordinates::new(c0.0, c0.1),
            Coordinates::new(c1.0, c1.1),
            Coordinates::new(c2.0, c2.1),
            Coordinates::new(c3.0, c3.1),
        ]
    };

    match piece {
        Piece::I => match rotation {
            Rotation::North => make_coords((-1, 0), (0, 0), (1, 0), (2, 0)),
            Rotation::East => make_coords((1, -1), (1, 0), (1, 1), (1, 2)),
            Rotation::South => make_coords((2, 1), (1, 1), (0, 1), (-1, 1)),
            Rotation::West => make_coords((0, 2), (0, 1), (0, 0), (0, -1)),
        },
        Piece::O => {
            // O piece has no rotation shifts under standard simplified SRS
            make_coords((0, 0), (1, 0), (0, 1), (1, 1))
        }
        Piece::T => match rotation {
            Rotation::North => make_coords((-1, 0), (0, 0), (1, 0), (0, 1)),
            Rotation::East => make_coords((0, 1), (0, 0), (0, -1), (1, 0)),
            Rotation::South => make_coords((1, 0), (0, 0), (-1, 0), (0, -1)),
            Rotation::West => make_coords((0, -1), (0, 0), (0, 1), (-1, 0)),
        },
        Piece::L => match rotation {
            Rotation::North => make_coords((-1, 0), (0, 0), (1, 0), (1, 1)),
            Rotation::East => make_coords((0, 1), (0, 0), (0, -1), (1, -1)),
            Rotation::South => make_coords((1, 0), (0, 0), (-1, 0), (-1, -1)),
            Rotation::West => make_coords((0, -1), (0, 0), (0, 1), (-1, 1)),
        },
        Piece::J => match rotation {
            Rotation::North => make_coords((-1, 0), (0, 0), (1, 0), (-1, 1)),
            Rotation::East => make_coords((0, 1), (0, 0), (0, -1), (1, 1)),
            Rotation::South => make_coords((1, 0), (0, 0), (-1, 0), (1, -1)),
            Rotation::West => make_coords((0, -1), (0, 0), (0, 1), (-1, -1)),
        },
        Piece::S => match rotation {
            Rotation::North => make_coords((-1, 0), (0, 0), (0, 1), (1, 1)),
            Rotation::East => make_coords((0, 1), (0, 0), (1, 0), (1, -1)),
            Rotation::South => make_coords((1, 0), (0, 0), (0, -1), (-1, -1)),
            Rotation::West => make_coords((0, -1), (0, 0), (-1, 0), (-1, 1)),
        },
        Piece::Z => match rotation {
            Rotation::North => make_coords((-1, 1), (0, 0), (0, 1), (1, 0)),
            Rotation::East => make_coords((1, 1), (0, 0), (1, 0), (0, -1)),
            Rotation::South => make_coords((1, -1), (0, 0), (0, -1), (-1, 0)),
            Rotation::West => make_coords((-1, -1), (0, 0), (-1, 0), (0, 1)),
        },
    }
}

/// Standard SRS kick translation tests for normal pieces (T, L, J, S, Z).
/// Returns offsets for rotation transition (from, to).
pub fn get_srs_kicks(from: Rotation, to: Rotation) -> &'static [(i32, i32)] {
    match (from, to) {
        (Rotation::North, Rotation::East) => &[(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
        (Rotation::East, Rotation::North) => &[(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
        (Rotation::East, Rotation::South) => &[(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
        (Rotation::South, Rotation::East) => &[(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
        (Rotation::South, Rotation::West) => &[(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
        (Rotation::West, Rotation::South) => &[(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
        (Rotation::West, Rotation::North) => &[(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
        (Rotation::North, Rotation::West) => &[(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
        _ => &[(0, 0)],
    }
}

/// SRS kick translation tests for the I piece.
pub fn get_srs_kicks_i(from: Rotation, to: Rotation) -> &'static [(i32, i32)] {
    match (from, to) {
        (Rotation::North, Rotation::East) => &[(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
        (Rotation::East, Rotation::North) => &[(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
        (Rotation::East, Rotation::South) => &[(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
        (Rotation::South, Rotation::East) => &[(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
        (Rotation::South, Rotation::West) => &[(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
        (Rotation::West, Rotation::South) => &[(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
        (Rotation::West, Rotation::North) => &[(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
        (Rotation::North, Rotation::West) => &[(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
        _ => &[(0, 0)],
    }
}
