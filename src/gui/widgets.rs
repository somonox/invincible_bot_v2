use egui::{Color32, Pos2, Rect, Stroke, Vec2};
use crate::engine::board::{Board, BOARD_HEIGHT};
use crate::engine::header::{Piece, Rotation};
use crate::engine::piece::get_piece_cells;

/// Draw a single cell at coordinates (grid_x, grid_y) on the canvas
fn draw_cell(
    painter: &egui::Painter,
    board_rect: Rect,
    cell_size: f32,
    grid_x: usize,
    grid_y: usize,
    color: Color32,
    is_ghost: bool,
    left_offset: f32,
) {
    // Invert Y because Tetris row 0 is at the bottom, but screen Y=0 is at the top
    let total_rows = 22; // Show only the bottom 22 rows to keep it focused
    if grid_y >= total_rows {
        return;
    }
    let screen_y = total_rows - 1 - grid_y;

    let x_pos = board_rect.min.x + left_offset + (grid_x as f32) * cell_size;
    let y_pos = board_rect.min.y + (screen_y as f32) * cell_size;

    let cell_rect = Rect::from_min_max(
        Pos2::new(x_pos + 1.0, y_pos + 1.0),
        Pos2::new(x_pos + cell_size - 1.0, y_pos + cell_size - 1.0),
    );

    let rounding = egui::Rounding::same(cell_size * 0.2); // Rounded blocks

    if is_ghost {
        painter.rect_stroke(
            cell_rect,
            rounding,
            Stroke::new(1.5, color.linear_multiply(0.6)),
        );
    } else {
        painter.rect_filled(cell_rect, rounding, color);
        
        let highlight_rect = Rect::from_min_max(
            Pos2::new(x_pos + 2.0, y_pos + 2.0),
            Pos2::new(x_pos + cell_size * 0.5, y_pos + cell_size * 0.5),
        );
        painter.rect_filled(
            highlight_rect,
            egui::Rounding::same(cell_size * 0.1),
            Color32::from_white_alpha(40),
        );
    }
}

/// Renders the active Tetris board, including locked blocks, the active piece, the ghost landing preview,
/// and a red danger bar representing pending garbage.
pub fn render_board(
    ui: &mut egui::Ui,
    board: &Board,
    active_piece: Option<(Piece, Rotation, i32, i32)>,
    pending_garbage: u32,
    queued_garbage: u32,
) {
    let cell_size = 22.0; // Responsive size
    let grid_width = board.width;
    let grid_height = 22; // Visible height
    
    // Add 10px margin on the left for the garbage bar
    let left_offset = 10.0f32;
    let board_size = Vec2::new(grid_width as f32 * cell_size + left_offset, grid_height as f32 * cell_size);
    
    let (rect, _response) = ui.allocate_exact_size(board_size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
 
    // Board container coordinates
    let board_rect = Rect::from_min_max(
        Pos2::new(rect.min.x + left_offset, rect.min.y),
        rect.max,
    );
 
    // Draw Board Background
    painter.rect_filled(
        board_rect,
        egui::Rounding::same(8.0),
        Color32::from_rgb(20, 20, 24),
    );
    
    // Draw Grid Lines (subtle dot grid)
    for x in 1..grid_width {
        let lx = board_rect.min.x + (x as f32) * cell_size;
        painter.line_segment(
            [Pos2::new(lx, board_rect.min.y), Pos2::new(lx, board_rect.max.y)],
            Stroke::new(1.0, Color32::from_rgb(34, 34, 40)),
        );
    }
    for y in 1..grid_height {
        let ly = board_rect.min.y + (y as f32) * cell_size;
        painter.line_segment(
            [Pos2::new(board_rect.min.x, ly), Pos2::new(board_rect.max.x, ly)],
            Stroke::new(1.0, Color32::from_rgb(34, 34, 40)),
        );
    }
 
    // Draw outer glow border
    painter.rect_stroke(
        board_rect,
        egui::Rounding::same(8.0),
        Stroke::new(1.5, Color32::from_rgb(45, 45, 55)),
    );
 
    // Draw Garbage Danger Bar (on the left edge margin)
    let total_garbage = pending_garbage + queued_garbage;
    if total_garbage > 0 {
        let max_visual_garbage = 20.0f32; // Capped height at 20 rows
        
        let red_ratio = (pending_garbage as f32 / max_visual_garbage).min(1.0);
        let red_bar_height = board_size.y * red_ratio;
        
        let total_ratio = (total_garbage as f32 / max_visual_garbage).min(1.0);
        let total_bar_height = board_size.y * total_ratio;
        
        // 1. Draw Red Bar (Active pending garbage)
        if pending_garbage > 0 {
            let red_bar_rect = Rect::from_min_max(
                Pos2::new(rect.min.x + 2.0, rect.max.y - red_bar_height),
                Pos2::new(rect.min.x + 7.0, rect.max.y),
            );
            painter.rect_filled(
                red_bar_rect,
                egui::Rounding::same(2.0),
                Color32::from_rgb(239, 68, 68), // Glowing red danger color
            );
        }
        
        // 2. Draw Yellow/Orange Bar (Buffered queued garbage)
        if queued_garbage > 0 {
            let yellow_bar_rect = Rect::from_min_max(
                Pos2::new(rect.min.x + 2.0, rect.max.y - total_bar_height),
                Pos2::new(rect.min.x + 7.0, rect.max.y - red_bar_height),
            );
            painter.rect_filled(
                yellow_bar_rect,
                egui::Rounding::same(2.0),
                Color32::from_rgb(245, 158, 11), // Glowing amber/yellow color
            );
        }
    }

    // 1. Draw locked blocks
    for y in 0..BOARD_HEIGHT {
        for x in 0..board.width {
            if (board.rows[y] & (1 << x)) != 0 {
                let color = Color32::from_rgb(110, 115, 125);
                draw_cell(&painter, rect, cell_size, x, y, color, false, left_offset);
            }
        }
    }

    // 2. Draw active piece & ghost piece
    if let Some((piece, rotation, px, py)) = active_piece {
        let mut ghost_y = py;
        while ghost_y >= 0 {
            if !board.fits(piece, rotation, px, ghost_y - 1) {
                break;
            }
            ghost_y -= 1;
        }

        let piece_color = piece.color();
        let color = Color32::from_rgb(piece_color[0], piece_color[1], piece_color[2]);

        // Draw Ghost
        let cells = get_piece_cells(piece, rotation);
        for cell in cells.iter() {
            let gx = px + cell.x;
            let gy = ghost_y + cell.y;
            if gx >= 0 && gx < board.width as i32 && gy >= 0 && gy < BOARD_HEIGHT as i32 {
                draw_cell(&painter, rect, cell_size, gx as usize, gy as usize, color, true, left_offset);
            }
        }

        // Draw Active Piece
        for cell in cells.iter() {
            let cx = px + cell.x;
            let cy = py + cell.y;
            if cx >= 0 && cx < board.width as i32 && cy >= 0 && cy < BOARD_HEIGHT as i32 {
                draw_cell(&painter, rect, cell_size, cx as usize, cy as usize, color, false, left_offset);
            }
        }
    }
}

/// Renders a small preview box for the hold or next piece queue.
pub fn render_piece_preview(ui: &mut egui::Ui, label: &str, piece: Option<Piece>) {
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(label).color(Color32::from_rgb(180, 180, 190)).size(12.0));
        
        let size = 60.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(size, size), egui::Sense::hover());
        let painter = ui.painter_at(rect);

        // Preview box background
        painter.rect_filled(
            rect,
            egui::Rounding::same(6.0),
            Color32::from_rgb(26, 26, 32),
        );
        painter.rect_stroke(
            rect,
            egui::Rounding::same(6.0),
            Stroke::new(1.0, Color32::from_rgb(45, 45, 55)),
        );

        if let Some(p) = piece {
            let p_color = p.color();
            let color = Color32::from_rgb(p_color[0], p_color[1], p_color[2]);
            
            let cell_size = 12.0;
            let center_x = rect.center().x;
            let center_y = rect.center().y;

            let offset = match p {
                Piece::I => Vec2::new(-6.0, 6.0),
                Piece::O => Vec2::new(-6.0, -6.0),
                Piece::T => Vec2::new(0.0, 0.0),
                Piece::L => Vec2::new(0.0, 0.0),
                Piece::J => Vec2::new(0.0, 0.0),
                Piece::S => Vec2::new(0.0, 0.0),
                Piece::Z => Vec2::new(0.0, 0.0),
            };

            let cells = get_piece_cells(p, Rotation::North);
            for cell in cells.iter() {
                let cx = center_x + (cell.x as f32) * cell_size + offset.x;
                let cy = center_y - (cell.y as f32) * cell_size + offset.y; 
                
                let cell_rect = Rect::from_min_max(
                    Pos2::new(cx - cell_size * 0.5 + 0.5, cy - cell_size * 0.5 + 0.5),
                    Pos2::new(cx + cell_size * 0.5 - 0.5, cy + cell_size * 0.5 - 0.5),
                );
                
                painter.rect_filled(
                    cell_rect,
                    egui::Rounding::same(cell_size * 0.2),
                    color,
                );
            }
        }
    });
}

/// Renders a vertical stack preview of the next 5 pieces in the queue.
pub fn render_queue_preview(ui: &mut egui::Ui, label: &str, queue: &[Piece]) {
    ui.vertical_centered(|ui| {
        ui.label(egui::RichText::new(label).color(Color32::from_rgb(180, 180, 190)).size(12.0));
        ui.add_space(4.0);

        // Render up to 5 pieces vertically
        let limit = 5.min(queue.len());
        for i in 0..limit {
            let piece = queue[i];
            let size = 42.0; // Slightly smaller size for the vertical stack
            let (rect, _) = ui.allocate_exact_size(Vec2::new(size, size), egui::Sense::hover());
            let painter = ui.painter_at(rect);

            // Preview box background
            painter.rect_filled(
                rect,
                egui::Rounding::same(4.0),
                Color32::from_rgb(26, 26, 32),
            );
            painter.rect_stroke(
                rect,
                egui::Rounding::same(4.0),
                Stroke::new(1.0, Color32::from_rgb(45, 45, 55)),
            );

            let p_color = piece.color();
            let color = Color32::from_rgb(p_color[0], p_color[1], p_color[2]);
            
            let cell_size = 8.0;
            let center_x = rect.center().x;
            let center_y = rect.center().y;

            let offset = match piece {
                Piece::I => Vec2::new(-4.0, 4.0),
                Piece::O => Vec2::new(-4.0, -4.0),
                _ => Vec2::new(0.0, 0.0),
            };

            let cells = get_piece_cells(piece, Rotation::North);
            for cell in cells.iter() {
                let cx = center_x + (cell.x as f32) * cell_size + offset.x;
                let cy = center_y - (cell.y as f32) * cell_size + offset.y; 
                
                let cell_rect = Rect::from_min_max(
                    Pos2::new(cx - cell_size * 0.5 + 0.5, cy - cell_size * 0.5 + 0.5),
                    Pos2::new(cx + cell_size * 0.5 - 0.5, cy + cell_size * 0.5 - 0.5),
                );
                
                painter.rect_filled(
                    cell_rect,
                    egui::Rounding::same(cell_size * 0.2),
                    color,
                );
            }
            ui.add_space(4.0);
        }
    });
}
