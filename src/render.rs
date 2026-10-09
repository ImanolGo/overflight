//! Geometry for the overhead "sky view" and its projection onto a terminal.
//!
//! The screen is a polar projection centred on the zenith: the middle of the
//! circle is straight up, the edge is the horizon. Terminal cells are roughly
//! twice as tall as they are wide, so the horizontal coordinate range is
//! halved to keep the horizon a circle rather than an oval.

use ratatui::Frame;
use ratatui::style::{Color, Style};
use ratatui::symbols;
use ratatui::widgets::canvas::{Canvas, Circle};

/// Terminal cells are approximately twice as tall as they are wide.
pub const CELL_ASPECT: f64 = 2.0;

/// Distance, in cells, between the horizon and the compass letters.
const COMPASS_GAP_CELLS: f64 = 1.5;

/// The coordinate system used to paint the sky into a terminal area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyGeometry {
    cols: u16,
    rows: u16,
    /// Left/right bounds of the canvas coordinate system.
    pub x_bounds: [f64; 2],
    /// Bottom/top bounds of the canvas coordinate system.
    pub y_bounds: [f64; 2],
    /// Centre of the sky, in canvas coordinates.
    pub center: (f64, f64),
    /// Radius of the horizon circle, in canvas coordinates.
    pub radius: f64,
}

impl SkyGeometry {
    /// Build the canvas geometry for a terminal area of `width` x `height` cells.
    #[must_use]
    pub fn new(width: u16, height: u16) -> Self {
        let cols = width.max(1);
        let rows = height.max(1);
        let width = f64::from(cols);
        let height = f64::from(rows);

        // One coordinate unit maps to one cell vertically. Horizontally the
        // range is `width / CELL_ASPECT`, which makes one coordinate unit the
        // same physical length along both axes.
        let x_range = width / CELL_ASPECT;
        let y_range = height;

        let center = (x_range / 2.0, y_range / 2.0);
        // Leave a little room outside the horizon for the compass letters.
        let radius = x_range.min(y_range) / 2.0 * 0.88;

        Self {
            cols,
            rows,
            x_bounds: [0.0, x_range],
            y_bounds: [0.0, y_range],
            center,
            radius,
        }
    }

    /// Radius of the horizon, measured in terminal columns and rows.
    fn radius_in_cells(&self) -> (f64, f64) {
        let cols = f64::from(self.cols);
        let rows = f64::from(self.rows);
        let x_range = self.x_bounds[1] - self.x_bounds[0];
        let y_range = self.y_bounds[1] - self.y_bounds[0];
        (
            self.radius * (cols - 1.0) / x_range,
            self.radius * (rows - 1.0) / y_range,
        )
    }

    /// Terminal cell where a compass letter is drawn.
    ///
    /// Letters sit just outside the horizon. In the default sky view east is
    /// drawn on the left, mirroring the sky as seen from below.
    #[must_use]
    pub fn compass_cell(&self, compass: Compass, sky_orientation: bool) -> (u16, u16) {
        let last_col = f64::from(self.cols.saturating_sub(1));
        let last_row = f64::from(self.rows.saturating_sub(1));
        let center_col = last_col / 2.0;
        let center_row = last_row / 2.0;
        let (radius_cols, radius_rows) = self.radius_in_cells();
        let gap = COMPASS_GAP_CELLS;

        let (col, row) = match compass {
            Compass::North => (center_col, center_row - radius_rows - gap),
            Compass::South => (center_col, center_row + radius_rows + gap),
            Compass::East if sky_orientation => (center_col - radius_cols - gap, center_row),
            Compass::West if sky_orientation => (center_col + radius_cols + gap, center_row),
            Compass::East => (center_col + radius_cols + gap, center_row),
            Compass::West => (center_col - radius_cols - gap, center_row),
        };

        (clamp_cell(col, last_col), clamp_cell(row, last_row))
    }
}

/// Round and clamp a cell coordinate into `0..=last`.
fn clamp_cell(value: f64, last: f64) -> u16 {
    value.round().clamp(0.0, last) as u16
}

/// The four cardinal directions labelled around the horizon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Compass {
    North,
    East,
    South,
    West,
}

impl Compass {
    /// All directions, in the order they are drawn.
    pub const ALL: [Compass; 4] = [Compass::North, Compass::East, Compass::South, Compass::West];

    /// The letter drawn for this direction.
    #[must_use]
    pub const fn letter(self) -> &'static str {
        match self {
            Compass::North => "N",
            Compass::East => "E",
            Compass::South => "S",
            Compass::West => "W",
        }
    }
}

/// Draw the sky view into `frame`.
pub fn render(frame: &mut Frame) {
    let area = frame.area();
    let geometry = SkyGeometry::new(area.width, area.height);
    let (cx, cy) = geometry.center;
    let radius = geometry.radius;

    let canvas = Canvas::default()
        .x_bounds(geometry.x_bounds)
        .y_bounds(geometry.y_bounds)
        .marker(symbols::Marker::Braille)
        .paint(move |ctx| {
            ctx.draw(&Circle {
                x: cx,
                y: cy,
                radius,
                color: Color::DarkGray,
            });
        });

    frame.render_widget(canvas, area);

    let buffer = frame.buffer_mut();
    let style = Style::default().fg(Color::Gray);
    for compass in Compass::ALL {
        let (x, y) = geometry.compass_cell(compass, true);
        buffer.set_string(x, y, compass.letter(), style);
    }
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    #[test]
    fn coordinate_axes_have_equal_physical_scale() {
        for (w, h) in [(80, 24), (120, 40), (40, 12), (30, 80)] {
            let g = SkyGeometry::new(w, h);
            let x_range = g.x_bounds[1] - g.x_bounds[0];
            let y_range = g.y_bounds[1] - g.y_bounds[0];
            // Cells per coordinate unit times the physical cell size.
            let physical_x = f64::from(w) / x_range;
            let physical_y = f64::from(h) / y_range * CELL_ASPECT;
            assert!(
                (physical_x - physical_y).abs() < 1e-9,
                "{w}x{h}: x={physical_x} y={physical_y}"
            );
        }
    }

    #[test]
    fn compass_letters_fit_inside_the_canvas() {
        for (w, h) in [(80, 24), (40, 12), (30, 80), (20, 20), (2, 1)] {
            let g = SkyGeometry::new(w, h);
            for compass in Compass::ALL {
                let (x, y) = g.compass_cell(compass, true);
                assert!(x < w.max(1), "{w}x{h} {compass:?} x={x}");
                assert!(y < h.max(1), "{w}x{h} {compass:?} y={y}");
            }
        }
    }

    #[test]
    fn compass_letters_sit_outside_the_horizon() {
        let g = SkyGeometry::new(80, 24);
        let center_col = 40.0;
        let center_row = 11.5;
        let (n_x, n_y) = g.compass_cell(Compass::North, true);
        let (s_x, s_y) = g.compass_cell(Compass::South, true);
        assert_eq!(f64::from(n_x), center_col);
        assert_eq!(f64::from(s_x), center_col);
        assert!(f64::from(n_y) < center_row, "north should be above centre");
        assert!(f64::from(s_y) > center_row, "south should be below centre");
        assert_eq!(n_y, 0);
        assert_eq!(s_y, 23);
    }

    #[test]
    fn east_west_swap_between_orientations() {
        let g = SkyGeometry::new(80, 24);
        let center_col = 39.5;
        assert!(
            f64::from(g.compass_cell(Compass::East, true).0) < center_col,
            "east should be left in sky mode"
        );
        assert!(
            f64::from(g.compass_cell(Compass::East, false).0) > center_col,
            "east should be right in map mode"
        );
    }

    #[test]
    fn renders_horizon_and_compass_letters() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(render).unwrap();
        let buffer = terminal.backend().buffer();

        let mut text = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                text.push_str(buffer[(x, y)].symbol());
            }
        }

        for letter in ["N", "E", "S", "W"] {
            assert!(text.contains(letter), "missing compass letter {letter}");
        }
        assert!(
            text.chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)),
            "expected braille dots for the horizon circle"
        );
    }
}
