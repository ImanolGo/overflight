//! Geometry for the overhead sky view and drawing of the app frame.
//!
//! The screen is a polar projection centred on the zenith: the middle of the
//! circle is straight up, the edge is the horizon. Terminal cells are roughly
//! twice as tall as they are wide, so the horizontal coordinate range is
//! halved to keep the horizon a circle rather than an oval.
//!
//! The horizon, the 30°/60° rings and the trails are drawn with a ratatui
//! `Canvas` in Braille. Aircraft arrows, labels and compass letters are written
//! straight into the buffer so they land on exact cells.

use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols;
use ratatui::text::{Line, Span};
use ratatui::widgets::canvas::{Canvas, Circle, Points};
use ratatui::widgets::{Block, Paragraph, Wrap};

use crate::app::App;
use crate::geo;
use crate::providers::AircraftKind;
use crate::satellite::SatellitePosition;
use crate::sky::Body;
use crate::track::Track;

/// Terminal cells are approximately twice as tall as they are wide.
pub const CELL_ASPECT: f64 = 2.0;

/// Below this size, show a "make me bigger" message instead of the sky.
const MIN_WIDTH: u16 = 30;
const MIN_HEIGHT: u16 = 15;

/// Distance, in cells, between the horizon and the compass letters.
const COMPASS_GAP_CELLS: f64 = 1.5;

/// The eight aircraft arrows, indexed by screen heading in 45° steps from east.
const ARROWS: [char; 8] = ['→', '↗', '↑', '↖', '←', '↙', '↓', '↘'];

/// A colour as a float triple, easy to scale for fading.
type Rgb = (f64, f64, f64);

/// Which sky palette is in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkyKind {
    Day,
    Twilight,
    Night,
}

/// Colours for the current time of day.
///
/// The sky is filled with its own background colour rather than relying on the
/// terminal theme, so the view looks the same on a light or a dark terminal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub kind: SkyKind,
    pub background: Rgb,
    pub horizon: Rgb,
    pub ring: Rgb,
    pub trail: Rgb,
    pub star: Rgb,
    pub compass: Rgb,
}

const DAY_PALETTE: Palette = Palette {
    kind: SkyKind::Day,
    background: (64.0, 110.0, 168.0),
    horizon: (228.0, 238.0, 252.0),
    ring: (110.0, 158.0, 208.0),
    trail: (190.0, 220.0, 248.0),
    star: (200.0, 205.0, 220.0),
    compass: (232.0, 232.0, 238.0),
};

const TWILIGHT_PALETTE: Palette = Palette {
    kind: SkyKind::Twilight,
    background: (70.0, 54.0, 88.0),
    horizon: (240.0, 164.0, 104.0),
    ring: (126.0, 100.0, 142.0),
    trail: (206.0, 168.0, 150.0),
    star: (200.0, 205.0, 220.0),
    compass: (230.0, 220.0, 220.0),
};

const NIGHT_PALETTE: Palette = Palette {
    kind: SkyKind::Night,
    background: (7.0, 10.0, 24.0),
    horizon: (58.0, 74.0, 120.0),
    ring: (28.0, 36.0, 62.0),
    trail: (56.0, 76.0, 116.0),
    star: (200.0, 205.0, 220.0),
    compass: (200.0, 205.0, 215.0),
};

/// Sun elevation above this is day.
const DAY_SUN_ELEVATION: f64 = 0.0;
/// Sun elevation below this is night; in between is twilight.
const NIGHT_SUN_ELEVATION: f64 = -12.0;

/// Colour for the Moon and bright planets in the night sky.
const BODY_COLOR: Rgb = (245.0, 235.0, 190.0);

/// Colour for satellites.
const SATELLITE_COLOR: Rgb = (190.0, 215.0, 255.0);

impl Palette {
    /// Pick the palette for a sun elevation in degrees.
    #[must_use]
    pub fn for_sun_elevation(elevation_deg: f64) -> Self {
        if elevation_deg > DAY_SUN_ELEVATION {
            DAY_PALETTE
        } else if elevation_deg > NIGHT_SUN_ELEVATION {
            TWILIGHT_PALETTE
        } else {
            NIGHT_PALETTE
        }
    }

    /// Whether the star field should be drawn.
    #[must_use]
    pub const fn shows_stars(self) -> bool {
        matches!(self.kind, SkyKind::Night)
    }
}

fn rgb(color: Rgb) -> Color {
    Color::Rgb(color.0 as u8, color.1 as u8, color.2 as u8)
}

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

    /// Terminal cell for a direction on the sky disc.
    ///
    /// `(x, y)` come from [`geo::project`]: `+y` is north, `+x` is east in map
    /// orientation, in units of the horizon radius.
    #[must_use]
    pub fn direction_cell(&self, x: f64, y: f64) -> (u16, u16) {
        let last_col = f64::from(self.cols.saturating_sub(1));
        let last_row = f64::from(self.rows.saturating_sub(1));
        let (radius_cols, radius_rows) = self.radius_in_cells();
        let col = last_col / 2.0 + x * radius_cols;
        let row = last_row / 2.0 - y * radius_rows;
        (clamp_cell(col, last_col), clamp_cell(row, last_row))
    }

    /// Canvas coordinates for a direction on the sky disc.
    #[must_use]
    pub fn direction_canvas(&self, x: f64, y: f64) -> (f64, f64) {
        (
            self.center.0 + x * self.radius,
            self.center.1 + y * self.radius,
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

/// Draw the whole app into `frame`.
pub fn render(frame: &mut Frame, app: &App) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        render_too_small(frame, area);
        return;
    }

    if app.horizon {
        render_horizon(frame, area, app);
        return;
    }

    let [sky_area, status_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let geometry = SkyGeometry::new(sky_area.width, sky_area.height);
    let palette = Palette::for_sun_elevation(app.sun_elevation_deg);
    let (cx, cy) = geometry.center;
    let radius = geometry.radius;

    let canvas = Canvas::default()
        .background_color(rgb(palette.background))
        .x_bounds(geometry.x_bounds)
        .y_bounds(geometry.y_bounds)
        .marker(symbols::Marker::Braille)
        .paint(|ctx| {
            if palette.shows_stars() {
                for star in &app.stars {
                    let x = if app.sky_orientation {
                        -star.east
                    } else {
                        star.east
                    };
                    let (x, y) = geometry.direction_canvas(x, star.north);
                    ctx.draw(&Points {
                        coords: &[(x, y)],
                        color: dim(palette.star, star.brightness),
                    });
                }
            }

            ctx.draw(&Circle {
                x: cx,
                y: cy,
                radius,
                color: rgb(palette.horizon),
            });
            for elevation in [30.0, 60.0] {
                let fraction = (90.0 - elevation) / 90.0;
                ctx.draw(&Circle {
                    x: cx,
                    y: cy,
                    radius: radius * fraction,
                    color: rgb(palette.ring),
                });
            }

            if app.show_trails {
                for track in &app.tracks {
                    let alpha = track.alpha(app.now_s);
                    for (enu, age) in track.trail(app.now_s) {
                        if let Some((x, y)) = disc_position(&enu, app.sky_orientation) {
                            let (x, y) = geometry.direction_canvas(x, y);
                            ctx.draw(&Points {
                                coords: &[(x, y)],
                                color: trail_color(&palette, age, alpha),
                            });
                        }
                    }
                }
            }
        });

    frame.render_widget(canvas, sky_area);

    let buffer = frame.buffer_mut();
    let (origin_x, origin_y) = (sky_area.left(), sky_area.top());

    for compass in Compass::ALL {
        let (x, y) = geometry.compass_cell(compass, app.sky_orientation);
        buffer.set_string(
            origin_x + x,
            origin_y + y,
            compass.letter(),
            Style::default().fg(rgb(palette.compass)),
        );
    }

    let mut taken = vec![false; usize::from(sky_area.width) * usize::from(sky_area.height)];
    let mut labels: Vec<(u16, u16, String, Style, bool)> = Vec::new();

    // The Moon, bright planets and satellites first, so plane labels can avoid
    // them. The Moon is drawn by day too, when it is above the horizon.
    for body in &app.bodies {
        if body.elevation_deg < 0.0 {
            continue;
        }
        if palette.kind == SkyKind::Day && body.body != Body::Moon {
            continue;
        }
        let (x, y) = geo::project(body.azimuth_deg, body.elevation_deg, app.sky_orientation);
        let (col, row) = geometry.direction_cell(x, y);
        buffer.set_string(
            origin_x + col,
            origin_y + row,
            body.body.glyph().to_string(),
            Style::default().fg(rgb(BODY_COLOR)),
        );
        mark_cell(&mut taken, sky_area.width, col, row);
    }

    if palette.kind != SkyKind::Day {
        for satellite in &app.satellite_positions {
            if satellite.elevation_deg < 0.0 {
                continue;
            }
            let (x, y) = geo::project(
                satellite.azimuth_deg,
                satellite.elevation_deg,
                app.sky_orientation,
            );
            let (col, row) = geometry.direction_cell(x, y);
            let colour = if satellite.sunlit {
                rgb(SATELLITE_COLOR)
            } else {
                dim(SATELLITE_COLOR, 0.3)
            };
            let mut style = Style::default().fg(colour);
            if app.selected_satellite.as_deref() == Some(satellite.name.as_str()) {
                style = style.add_modifier(Modifier::BOLD);
            }
            buffer.set_string(origin_x + col, origin_y + row, "✦", style);
            mark_cell(&mut taken, sky_area.width, col, row);
        }
    }

    // Arrows first, then labels, so labels can avoid every arrow and each
    // other.
    for track in &app.tracks {
        let (azimuth, elevation, _) = track.az_el();
        if elevation < 0.0 {
            continue;
        }
        let (x, y) = geo::project(azimuth, elevation, app.sky_orientation);
        let (col, row) = geometry.direction_cell(x, y);
        let alpha = track.alpha(app.now_s);
        let colour = if track.unusual.is_some() {
            alert_color(alpha)
        } else {
            altitude_color(track.alt_m, alpha)
        };
        let selected = app.selected.as_deref() == Some(track.id.as_str());
        let mut style = Style::default().fg(colour);
        if selected {
            style = style.add_modifier(Modifier::BOLD);
        }
        let glyph = aircraft_glyph(track, app.sky_orientation);
        buffer.set_string(origin_x + col, origin_y + row, glyph.to_string(), style);
        mark_cell(&mut taken, sky_area.width, col, row);
        if app.show_callsigns {
            labels.push((col, row, aircraft_label(track), style, selected));
        }
    }

    place_labels(
        buffer,
        (origin_x, origin_y),
        sky_area.width,
        sky_area.height,
        &mut taken,
        &labels,
    );

    if app.tracks.is_empty() {
        render_no_data(frame, sky_area, app, &palette);
    }

    if let Some(satellite) = app.selected_satellite() {
        render_satellite_detail(frame, sky_area, satellite, &palette);
    } else if let Some(track) = app.selected_track() {
        render_detail(frame, sky_area, track, app, &palette);
    }

    if let Some(message) = app.active_notification() {
        render_notification(frame, sky_area, message, &palette);
    }

    frame.render_widget(Paragraph::new(status_line(app)), status_area);
}

/// A transient banner for an unusual aircraft.
fn render_notification(frame: &mut Frame, area: Rect, message: &str, palette: &Palette) {
    let banner = Paragraph::new(format!("⚠ {message}"))
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(alert_color(1.0))
                .bg(rgb(palette.background)),
        );
    frame.render_widget(banner, Rect::new(area.x, area.y, area.width, 1));
}

/// The glyph for an aircraft: a heading arrow, or a distinct symbol for the
/// kinds that are not ordinary aeroplanes.
#[must_use]
pub fn aircraft_glyph(track: &Track, sky_orientation: bool) -> char {
    match track.kind {
        AircraftKind::Helicopter => '⊛',
        AircraftKind::Glider => '⌁',
        AircraftKind::Balloon => '◯',
        AircraftKind::Plane => screen_arrow(track.track_deg, sky_orientation),
    }
}

/// Alert colour for unusual aircraft and the notification banner.
fn alert_color(alpha: f64) -> Color {
    dim((255.0, 96.0, 96.0), alpha)
}

/// A centred hint when there is nothing to draw.
fn render_no_data(frame: &mut Frame, area: Rect, app: &App, palette: &Palette) {
    let mut lines: Vec<Line> = Vec::new();
    match &app.last_error {
        Some(error) => {
            lines.push(Line::from(format!("No aircraft from {}", app.source)));
            lines.push(Line::from(error.clone()));
            lines.push(Line::from("Try: overflight --demo"));
        }
        None => lines.push(Line::from(format!(
            "Looking for aircraft from {}…",
            app.source
        ))),
    }

    let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    let y = area.y + area.height.saturating_sub(height) / 2;
    let message = Paragraph::new(lines)
        .alignment(Alignment::Center)
        .wrap(Wrap { trim: true })
        .style(
            Style::default()
                .fg(rgb(palette.compass))
                .bg(rgb(palette.background)),
        );
    frame.render_widget(message, Rect::new(area.x, y, area.width, height));
}

/// The detail box for the selected aircraft.
fn render_detail(frame: &mut Frame, area: Rect, track: &Track, app: &App, palette: &Palette) {
    if area.width < 24 || area.height < 10 {
        return;
    }

    let (azimuth, elevation, range_m) = track.az_el();
    let altitude = track.alt_m.map_or_else(
        || "on ground".to_string(),
        |metres| {
            let (value, unit) = app.units.altitude(metres);
            format!("{value:.0} {unit}")
        },
    );
    let (speed, speed_unit) = app.units.speed(track.ground_speed_ms.unwrap_or(0.0));
    let (distance, distance_unit) = app.units.distance(range_m / 1000.0);

    let identity: Vec<&str> = [track.registration.as_deref(), track.type_code.as_deref()]
        .into_iter()
        .flatten()
        .collect();

    let mut lines = vec![Line::from(Span::styled(
        track.label().to_string(),
        Style::default().add_modifier(Modifier::BOLD),
    ))];
    if !identity.is_empty() {
        lines.push(Line::from(identity.join(" · ")));
    }
    lines.push(Line::from(format!("Altitude  {altitude}")));
    lines.push(Line::from(format!("Speed     {speed:.0} {speed_unit}")));
    lines.push(Line::from(format!(
        "Distance  {distance:.1} {distance_unit}"
    )));
    lines.push(Line::from(format!(
        "Where     {}, {elevation:.0}° up",
        compass_word(azimuth)
    )));

    render_info_box(frame, area, lines, palette);
}

/// Draw the bordered detail box, if the area is big enough.
fn render_info_box(frame: &mut Frame, area: Rect, lines: Vec<Line<'_>>, palette: &Palette) {
    if area.width < 24 || area.height < 10 {
        return;
    }
    let width = 32.min(area.width.saturating_sub(2));
    let height = u16::try_from(lines.len())
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    let detail_area = Rect::new(area.x + 1, area.y + 1, width, height);
    let style = Style::default()
        .fg(rgb(palette.compass))
        .bg(rgb(palette.background));
    let block = Block::bordered().title(" Selected ").style(style);
    frame.render_widget(Paragraph::new(lines).block(block).style(style), detail_area);
}

/// The detail box for the selected satellite.
fn render_satellite_detail(
    frame: &mut Frame,
    area: Rect,
    satellite: &SatellitePosition,
    palette: &Palette,
) {
    let lines = vec![
        Line::from(Span::styled(
            satellite.name.clone(),
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("Satellite"),
        Line::from(format!("Altitude  {:.0} km", satellite.altitude_km)),
        Line::from(format!(
            "Where     {}, {:.0}° up",
            compass_word(satellite.azimuth_deg),
            satellite.elevation_deg
        )),
        Line::from(if satellite.sunlit {
            "Status    visible now"
        } else {
            "Status    in Earth's shadow"
        }),
    ];
    render_info_box(frame, area, lines, palette);
}

/// The eight-point compass name for an azimuth.
#[must_use]
pub fn compass_word(azimuth_deg: f64) -> &'static str {
    const WORDS: [&str; 8] = [
        "north",
        "north-east",
        "east",
        "south-east",
        "south",
        "south-west",
        "west",
        "north-west",
    ];
    let index = (((azimuth_deg.rem_euclid(360.0) + 22.5) / 45.0).floor() as usize) % WORDS.len();
    WORDS[index]
}

/// Project an ENU position onto the sky disc, or `None` if it is out of range.
fn disc_position(enu: &[f64; 3], sky_orientation: bool) -> Option<(f64, f64)> {
    let [east, north, up] = *enu;
    let ground = east.hypot(north);
    let azimuth = east.atan2(north).to_degrees().rem_euclid(360.0);
    let elevation = up.atan2(ground).to_degrees();
    if elevation < 0.0 {
        return None;
    }
    Some(geo::project(azimuth, elevation, sky_orientation))
}

/// Screen-space arrow for a true track.
///
/// The sky view mirrors east onto the left, so a plane heading east points
/// left on screen there and right in map orientation.
#[must_use]
pub fn screen_arrow(track_deg: Option<f64>, sky_orientation: bool) -> char {
    let Some(track_deg) = track_deg else {
        return '·';
    };
    let track = track_deg.to_radians();
    let east = track.sin() * if sky_orientation { -1.0 } else { 1.0 };
    let north = track.cos();
    arrow_from_angle(north.atan2(east).to_degrees())
}

/// The arrow nearest to a screen angle, measured counter-clockwise from east.
fn arrow_from_angle(angle_deg: f64) -> char {
    let angle = angle_deg.rem_euclid(360.0);
    ARROWS[((angle / 45.0).round() as usize) % ARROWS.len()]
}

/// Colour for an aircraft by altitude band, dimmed by `alpha`.
fn altitude_color(alt_m: Option<f64>, alpha: f64) -> Color {
    let (r, g, b) = match alt_m {
        Some(altitude) if altitude < 3_000.0 => (255.0, 176.0, 84.0),
        Some(altitude) if altitude > 9_000.0 => (200.0, 230.0, 255.0),
        _ => (235.0, 235.0, 235.0),
    };
    dim((r, g, b), alpha)
}

/// Fading trail dot colour.
fn trail_color(palette: &Palette, age_s: f64, alpha: f64) -> Color {
    let freshness = (1.0 - age_s / 60.0).clamp(0.0, 1.0);
    dim(palette.trail, freshness * alpha)
}

fn dim((r, g, b): Rgb, alpha: f64) -> Color {
    let alpha = alpha.clamp(0.0, 1.0);
    Color::Rgb((r * alpha) as u8, (g * alpha) as u8, (b * alpha) as u8)
}

/// Label plus a climbing/descending marker.
fn aircraft_label(track: &crate::track::Track) -> String {
    let mut label = track.label().to_string();
    match track.vertical_rate_ms {
        Some(rate) if rate > 0.5 => label.push('+'),
        Some(rate) if rate < -0.5 => label.push('-'),
        _ => {}
    }
    label
}

/// Mark a single cell as occupied, if it is inside the area.
fn mark_cell(taken: &mut [bool], width: u16, col: u16, row: u16) {
    let index = usize::from(row) * usize::from(width) + usize::from(col);
    if let Some(cell) = taken.get_mut(index) {
        *cell = true;
    }
}

/// Draw callsign labels, avoiding cells that are already taken.
///
/// Each entry is the arrow's cell, the label, its style, and whether it belongs
/// to the selected aircraft. Tries the right of the arrow, then the left, then
/// a row up or down; the selected aircraft is drawn regardless.
fn place_labels(
    buffer: &mut Buffer,
    origin: (u16, u16),
    width: u16,
    height: u16,
    taken: &mut [bool],
    entries: &[(u16, u16, String, Style, bool)],
) {
    let free = |taken: &[bool], x: u16, y: u16, len: u16| -> bool {
        if y >= height || x.saturating_add(len) > width {
            return false;
        }
        (x..x + len).all(|cx| {
            let index = usize::from(y) * usize::from(width) + usize::from(cx);
            !taken[index]
        })
    };

    for (col, row, label, style, selected) in entries {
        let (col, row) = (*col, *row);
        let len = u16::try_from(label.chars().count()).unwrap_or(u16::MAX);
        if len == 0 {
            continue;
        }
        let candidates = [
            (col.saturating_add(2), row),
            (col.saturating_sub(len.saturating_add(1)), row),
            (col.saturating_add(2), row.saturating_sub(1)),
            (col.saturating_add(2), row.saturating_add(1)),
        ];
        let placed = candidates
            .iter()
            .copied()
            .find(|&(x, y)| free(taken, x, y, len))
            .or_else(|| selected.then_some((col.saturating_add(2), row)));

        if let Some((x, y)) = placed {
            buffer.set_string(origin.0 + x, origin.1 + y, label, *style);
            if y < height {
                for cx in x..x.saturating_add(len) {
                    if cx < width {
                        let index = usize::from(y) * usize::from(width) + usize::from(cx);
                        taken[index] = true;
                    }
                }
            }
        }
    }
}

/// The bottom status line.
fn status_line(app: &App) -> Line<'static> {
    let base = Style::default().fg(Color::DarkGray);
    let mut spans = vec![Span::styled(
        format!("{} · {} aircraft", app.source, app.live_count()),
        base,
    )];
    match app.last_update_s {
        Some(updated) => spans.push(Span::styled(
            format!(" · updated {:.1}s ago", (app.now_s - updated).max(0.0)),
            base,
        )),
        None => spans.push(Span::styled(" · waiting for data", base)),
    }
    if app.satellites_stale {
        spans.push(Span::styled(" · satellite data out of date", base));
    }
    if let Some(error) = &app.last_error {
        spans.push(Span::styled(
            format!(" · {error}"),
            Style::default().fg(Color::Rgb(90, 70, 70)),
        ));
    }
    Line::from(spans)
}

/// The side-on horizon view: a skyline, looking north, with planes rising over
/// it and their height above the horizon shown directly.
fn render_horizon(frame: &mut Frame, area: Rect, app: &App) {
    let [sky_area, status_area] =
        Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let palette = Palette::for_sun_elevation(app.sun_elevation_deg);
    frame.render_widget(
        Block::default().style(Style::default().bg(rgb(palette.background))),
        sky_area,
    );

    let view = app.view_azimuth_deg;

    {
        let buffer = frame.buffer_mut();
        let (origin_x, origin_y) = (sky_area.left(), sky_area.top());
        draw_skyline(buffer, sky_area, &palette);

        let mut left = 0usize;
        let mut right = 0usize;
        let mut taken = vec![false; usize::from(sky_area.width) * usize::from(sky_area.height)];
        let mut labels: Vec<(u16, u16, String, Style, bool)> = Vec::new();

        for track in &app.tracks {
            let (azimuth, elevation, _) = track.az_el();
            if !(0.0..=90.0).contains(&elevation) {
                continue;
            }
            let mut delta = (azimuth - view).rem_euclid(360.0);
            if delta > 180.0 {
                delta -= 360.0;
            }
            if delta > HORIZON_FOV_HALF {
                right += 1;
                continue;
            }
            if delta < -HORIZON_FOV_HALF {
                left += 1;
                continue;
            }

            let Some((col, row)) = horizon_cell(sky_area, azimuth, elevation, view) else {
                continue;
            };
            let alpha = track.alpha(app.now_s);
            let colour = if track.unusual.is_some() {
                alert_color(alpha)
            } else {
                altitude_color(track.alt_m, alpha)
            };
            let selected = app.selected.as_deref() == Some(track.id.as_str());
            let mut style = Style::default().fg(colour);
            if selected {
                style = style.add_modifier(Modifier::BOLD);
            }
            buffer.set_string(
                origin_x + col,
                origin_y + row,
                horizon_arrow(track, view).to_string(),
                style,
            );
            mark_cell(&mut taken, sky_area.width, col, row);
            if app.show_callsigns {
                labels.push((col, row, aircraft_label(track), style, selected));
            }
        }

        place_labels(
            buffer,
            (origin_x, origin_y),
            sky_area.width,
            sky_area.height,
            &mut taken,
            &labels,
        );

        let edge_style = Style::default().fg(rgb(palette.compass));
        buffer.set_string(
            origin_x,
            origin_y,
            format!("looking {}", compass_word(view)),
            edge_style,
        );
        let edge_row = origin_y + sky_area.height / 2;
        if left > 0 {
            buffer.set_string(origin_x, edge_row, format!("‹ {left}"), edge_style);
        }
        if right > 0 {
            let label = format!("{right} ›");
            let width = u16::try_from(label.chars().count()).unwrap_or(0);
            let col = sky_area.width.saturating_sub(width);
            buffer.set_string(origin_x + col, edge_row, label, edge_style);
        }
    }

    if app.tracks.is_empty() {
        render_no_data(frame, sky_area, app, &palette);
    }
    if let Some(satellite) = app.selected_satellite() {
        render_satellite_detail(frame, sky_area, satellite, &palette);
    } else if let Some(track) = app.selected_track() {
        render_detail(frame, sky_area, track, app, &palette);
    }
    if let Some(message) = app.active_notification() {
        render_notification(frame, sky_area, message, &palette);
    }
    frame.render_widget(Paragraph::new(status_line(app)), status_area);
}

/// A deterministic little skyline along the bottom of a horizon view.
fn draw_skyline(buffer: &mut Buffer, area: Rect, palette: &Palette) {
    let style = Style::default()
        .fg(rgb(palette.ring))
        .bg(rgb(palette.background));
    let base = area.bottom().saturating_sub(1);
    for x in 0..area.width {
        buffer.set_string(area.left() + x, base, "▁", style);
    }

    const HEIGHTS: [u16; 16] = [1, 3, 2, 5, 1, 2, 4, 1, 3, 2, 6, 1, 2, 3, 1, 4];
    let mut column = 1u16;
    let mut index = 0usize;
    while column + 1 < area.width {
        let height = HEIGHTS[index % HEIGHTS.len()];
        for level in 0..height {
            if base > area.top() + level {
                buffer.set_string(area.left() + column, base - level, "█", style);
            }
        }
        column += 2;
        index += 1;
    }
}

/// Half the horizontal field of view in the horizon mode, in degrees.
const HORIZON_FOV_HALF: f64 = 65.0;

/// Screen cell for a direction in the horizon view, or `None` if it is behind
/// us or below the horizon.
fn horizon_cell(
    area: Rect,
    azimuth_deg: f64,
    elevation_deg: f64,
    view_azimuth: f64,
) -> Option<(u16, u16)> {
    if !(0.0..=90.0).contains(&elevation_deg) {
        return None;
    }
    let mut delta = (azimuth_deg - view_azimuth).rem_euclid(360.0);
    if delta > 180.0 {
        delta -= 360.0;
    }
    if delta.abs() > HORIZON_FOV_HALF {
        return None;
    }

    let width = f64::from(area.width);
    let x = width / 2.0 + delta / HORIZON_FOV_HALF * (width / 2.0 - 2.0);
    let base = f64::from(area.height.saturating_sub(3));
    let y = base - elevation_deg / 90.0 * (base - 1.0);

    Some((
        clamp_cell(x, f64::from(area.width.saturating_sub(1))),
        clamp_cell(y, f64::from(area.height.saturating_sub(1))),
    ))
}

/// Side-on arrow: east is to the right, and the vertical rate tilts it up or
/// down. A plane moving mostly towards or away from the viewer gets a dot.
fn horizon_arrow(track: &Track, view_azimuth: f64) -> char {
    let Some(track_deg) = track.track_deg else {
        return '·';
    };
    let relative = (track_deg - view_azimuth).to_radians();
    if relative.cos().abs() > 0.7 {
        return '•';
    }
    let across = relative.sin();
    let speed = track.ground_speed_ms.unwrap_or(0.0).max(1.0);
    let climb = track.vertical_rate_ms.unwrap_or(0.0) / speed;
    arrow_from_angle(climb.atan2(across).to_degrees())
}

fn render_too_small(frame: &mut Frame, area: Rect) {
    let message = format!(
        "Terminal too small ({}x{}). Make me bigger — at least {MIN_WIDTH}x{MIN_HEIGHT}.",
        area.width, area.height
    );
    frame.render_widget(
        Paragraph::new(message).style(Style::default().fg(Color::Gray)),
        area,
    );
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    use super::*;
    use crate::app::App;
    use crate::providers::Provider;
    use crate::providers::fixture::FixtureProvider;

    #[test]
    fn coordinate_axes_have_equal_physical_scale() {
        for (w, h) in [(80, 24), (120, 40), (40, 12), (30, 80)] {
            let g = SkyGeometry::new(w, h);
            let x_range = g.x_bounds[1] - g.x_bounds[0];
            let y_range = g.y_bounds[1] - g.y_bounds[0];
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
        let center_col = 40.0;
        assert!(
            f64::from(g.direction_cell(1.0, 0.0).0) > center_col,
            "map east should be right"
        );
        assert!(
            f64::from(g.direction_cell(-1.0, 0.0).0) < center_col,
            "sky east should be left"
        );
    }

    #[test]
    fn arrow_mirrors_east_in_sky_orientation() {
        assert_eq!(screen_arrow(Some(90.0), true), '←');
        assert_eq!(screen_arrow(Some(90.0), false), '→');
        assert_eq!(screen_arrow(Some(0.0), true), '↑');
        assert_eq!(screen_arrow(Some(270.0), true), '→');
        assert_eq!(screen_arrow(Some(180.0), false), '↓');
        assert_eq!(screen_arrow(None, true), '·');
    }

    #[test]
    fn special_kinds_get_distinct_glyphs() {
        use crate::geo::GeoPoint;
        use crate::providers::{Aircraft, AircraftKind};
        use crate::track::Track;

        let make = |kind| {
            let aircraft = Aircraft {
                id: "abc123".to_string(),
                kind,
                lat: 52.5,
                lon: 13.4,
                ..Aircraft::default()
            };
            Track::new(&aircraft, GeoPoint::new(52.52, 13.40, 0.0), 0.0)
        };

        assert_eq!(aircraft_glyph(&make(AircraftKind::Helicopter), true), '⊛');
        assert_eq!(aircraft_glyph(&make(AircraftKind::Glider), true), '⌁');
        assert_eq!(aircraft_glyph(&make(AircraftKind::Balloon), true), '◯');
        let plane = make(AircraftKind::Plane);
        assert_eq!(
            aircraft_glyph(&plane, true),
            screen_arrow(plane.track_deg, true)
        );
    }

    #[test]
    fn renders_horizon_and_compass_letters() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let app = App::new(
            crate::providers::Query {
                lat: 52.52,
                lon: 13.40,
                radius_km: 80.0,
                alt_m: 0.0,
            },
            "test",
        );
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        for letter in ["N", "E", "S", "W"] {
            assert!(text.contains(letter), "missing compass letter {letter}");
        }
        assert!(
            text.chars().any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)),
            "expected braille dots for the horizon"
        );
    }

    #[test]
    fn an_empty_sky_shows_a_hint() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let mut app = App::new(
            crate::providers::Query {
                lat: 52.52,
                lon: 13.40,
                radius_km: 80.0,
                alt_m: 0.0,
            },
            "airplanes.live",
        );

        terminal.draw(|frame| render(frame, &app)).unwrap();
        let waiting = buffer_text(terminal.backend().buffer());
        assert!(waiting.contains("Looking for aircraft"), "{waiting}");

        app.set_error("airplanes.live returned an error");
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let failed = buffer_text(terminal.backend().buffer());
        assert!(failed.contains("No aircraft from airplanes.live"));
        assert!(failed.contains("returned an error"));
        assert!(failed.contains("--demo"));
    }

    #[test]
    fn the_moon_is_drawn_by_day_but_planets_are_not() {
        use crate::providers::Aircraft;
        use crate::sky::{Body, BodyPosition};

        let mut app = App::new(
            crate::providers::Query {
                lat: 52.52,
                lon: 13.40,
                radius_km: 80.0,
                alt_m: 0.0,
            },
            "test",
        );
        // One aircraft, so the empty-sky hint is not drawn over the bodies.
        let aircraft = Aircraft {
            id: "abc123".to_string(),
            lat: 52.6,
            lon: 13.5,
            alt_m: Some(10_000.0),
            ..Aircraft::default()
        };
        app.apply(&[aircraft], 0.0);
        app.update(0.0, 0.0);

        app.sun_elevation_deg = 30.0; // daytime
        app.bodies = vec![
            BodyPosition {
                body: Body::Moon,
                azimuth_deg: 90.0,
                elevation_deg: 45.0,
            },
            BodyPosition {
                body: Body::Mars,
                azimuth_deg: 90.0,
                elevation_deg: 30.0,
            },
        ];

        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains('☾'), "the Moon should be drawn by day");
        assert!(!text.contains('♂'), "planets stay for twilight and night");
    }

    #[test]
    fn small_terminals_get_a_message() {
        let mut terminal = Terminal::new(TestBackend::new(20, 8)).unwrap();
        let app = App::new(
            crate::providers::Query {
                lat: 0.0,
                lon: 0.0,
                radius_km: 1.0,
                alt_m: 0.0,
            },
            "test",
        );
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("too small"), "{text}");
    }

    #[test]
    fn compass_words_cover_the_eight_points() {
        assert_eq!(compass_word(0.0), "north");
        assert_eq!(compass_word(45.0), "north-east");
        assert_eq!(compass_word(90.0), "east");
        assert_eq!(compass_word(180.0), "south");
        assert_eq!(compass_word(270.0), "west");
        assert_eq!(compass_word(350.0), "north");
        assert_eq!(compass_word(23.0), "north-east");
    }

    #[test]
    fn palette_follows_the_sun() {
        assert_eq!(Palette::for_sun_elevation(30.0).kind, SkyKind::Day);
        assert_eq!(Palette::for_sun_elevation(-5.0).kind, SkyKind::Twilight);
        assert_eq!(Palette::for_sun_elevation(-30.0).kind, SkyKind::Night);
        assert!(!Palette::for_sun_elevation(30.0).shows_stars());
        assert!(!Palette::for_sun_elevation(-5.0).shows_stars());
        assert!(Palette::for_sun_elevation(-30.0).shows_stars());
    }

    fn demo_app(utc: f64) -> App {
        let mut provider = FixtureProvider::embedded().unwrap();
        let query = provider.query();
        let mut app = App::new(query, "demo");
        app.set_satellites(crate::satellite::embedded().unwrap_or_default(), utc);
        // Two polls five seconds apart, so dead reckoning, easing and the trail
        // are all exercised at a fixed time.
        let first = provider.fetch(&query).unwrap();
        app.apply(&first, 0.0);
        app.update(0.0, utc);
        let second = provider.fetch(&query).unwrap();
        app.apply(&second, 5.0);
        app.update(5.0, utc);
        app.update(5.6, utc);
        app
    }

    #[test]
    fn place_labels_avoids_collisions() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 40, 10));
        let style = Style::default();
        let mut taken = vec![false; 40 * 10];
        let entries = vec![
            (5_u16, 5_u16, "AAA".to_string(), style, false),
            (5_u16, 5_u16, "BBB".to_string(), style, false),
        ];
        place_labels(&mut buffer, (0, 0), 40, 10, &mut taken, &entries);

        let text: String = (0..10)
            .flat_map(|y| (0..40).map(move |x| (x, y)))
            .map(|(x, y)| buffer[(x, y)].symbol())
            .collect();
        assert!(text.contains("AAA"), "{text}");
        assert!(text.contains("BBB"), "{text}");
    }

    #[test]
    fn horizon_view_projects_around_north() {
        let area = Rect::new(0, 0, 100, 30);
        let north = horizon_cell(area, 0.0, 45.0, 0.0).unwrap();
        assert!((f64::from(north.0) - 50.0).abs() < 1.0, "centred");
        assert!(horizon_cell(area, 90.0, 45.0, 0.0).is_none(), "behind us");
        assert!(
            horizon_cell(area, 0.0, -5.0, 0.0).is_none(),
            "below horizon"
        );
        let low = horizon_cell(area, 0.0, 0.0, 0.0).unwrap();
        let high = horizon_cell(area, 0.0, 80.0, 0.0).unwrap();
        assert!(high.1 < low.1, "higher elevation is a smaller row");
    }

    #[test]
    fn horizon_arrows_rise_when_climbing() {
        use crate::geo::GeoPoint;
        use crate::providers::Aircraft;
        use crate::track::Track;

        let make = |track_deg: f64, climb: f64| {
            let aircraft = Aircraft {
                id: "abc123".to_string(),
                lat: 52.53,
                lon: 13.40,
                track_deg: Some(track_deg),
                ground_speed_ms: Some(100.0),
                vertical_rate_ms: Some(climb),
                ..Aircraft::default()
            };
            Track::new(&aircraft, GeoPoint::new(52.52, 13.40, 0.0), 0.0)
        };
        // Eastbound, level: points right.
        assert_eq!(horizon_arrow(&make(90.0, 0.0), 0.0), '→');
        // Climbing at the same rate as it travels: points up-right.
        assert_eq!(horizon_arrow(&make(90.0, 100.0), 0.0), '↗');
        // Flying towards or away from the viewer: a dot, not an arrow.
        assert_eq!(horizon_arrow(&make(0.0, 0.0), 0.0), '•');
        assert_eq!(horizon_arrow(&make(180.0, 0.0), 0.0), '•');
    }

    #[test]
    fn satellite_detail_box_renders() {
        let utc = crate::sun::parse_rfc3339_seconds("2024-06-21T00:00:00Z").unwrap() as f64;
        let mut app = demo_app(utc);
        app.selected_satellite = app
            .satellite_positions
            .first()
            .map(|position| position.name.clone());
        assert!(app.selected_satellite().is_some(), "demo has a satellite");

        let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Satellite"), "{text}");
        assert!(text.contains("Status"), "{text}");
    }

    #[test]
    fn snapshot_demo_horizon_is_stable() {
        let utc = crate::sun::parse_rfc3339_seconds("2024-06-21T00:00:00Z").unwrap() as f64;
        let mut app = demo_app(utc);
        app.horizon = true;
        let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        insta::assert_snapshot!("demo_horizon", buffer_text(terminal.backend().buffer()));
    }

    #[test]
    fn snapshot_demo_day_sky_is_stable() {
        let utc = crate::sun::parse_rfc3339_seconds("2024-06-21T12:00:00Z").unwrap() as f64;
        let app = demo_app(utc);
        let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        insta::assert_snapshot!("demo_day_sky", buffer_text(terminal.backend().buffer()));
    }

    #[test]
    fn snapshot_demo_night_sky_is_stable() {
        let utc = crate::sun::parse_rfc3339_seconds("2024-06-21T00:00:00Z").unwrap() as f64;
        let app = demo_app(utc);
        let mut terminal = Terminal::new(TestBackend::new(100, 32)).unwrap();
        terminal.draw(|frame| render(frame, &app)).unwrap();
        insta::assert_snapshot!("demo_night_sky", buffer_text(terminal.backend().buffer()));
    }

    fn buffer_text(buffer: &Buffer) -> String {
        let mut text = String::new();
        for y in 0..buffer.area.height {
            let mut line = String::new();
            for x in 0..buffer.area.width {
                line.push_str(buffer[(x, y)].symbol());
            }
            text.push_str(line.trim_end());
            text.push('\n');
        }
        text
    }
}
