use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
    MouseButton, MouseEventKind,
};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell as RatCell, Paragraph, Row, Table};
use ratatui::Terminal;
use std::io::{self, Stdout};

use crate::types::{
    App, CellVisibility, GameAction, GameEvent, GameStatus, LeaderboardTab, Screen, UiHover,
    BORDER_COL, BORDER_ROW, CELL_HEIGHT, CELL_WIDTH,
};

// ---------------------------------------------------------------------------
// Terminal setup / teardown
// ---------------------------------------------------------------------------

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

pub fn init() -> io::Result<Tui> {
    enable_raw_mode()?;
    execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(io::stdout());
    Terminal::new(backend)
}

pub fn restore(terminal: &mut Tui) -> io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Main render dispatch
// ---------------------------------------------------------------------------

pub fn draw(terminal: &mut Tui, app: &mut App) -> io::Result<()> {
    terminal.draw(|frame| {
        let area = frame.area();
        match &app.screen {
            Screen::MainMenu { selected } => draw_main_menu(frame, area, *selected),
            Screen::NewGameMenu { selected } => draw_new_game_menu(frame, area, *selected, app.ui_hover),
            Screen::Playing => {
                if let Some(game) = &app.game {
                    draw_game(frame, area, game, app.active_cell, app.cell_active);
                }
            }
            Screen::GameOver { won, selected } => {
                if let Some(game) = &app.game {
                    draw_game_over(frame, area, game, app.active_cell, app.cell_active, *won, *selected);
                }
            }
            Screen::Leaderboard { tab } => {
                draw_leaderboard(frame, area, &app.leaderboard, *tab, app.ui_hover);
            }
        }
        // Process each effect with its own scoped rect, then remove completed ones.
        let elapsed = app.last_frame.elapsed();
        let buf = frame.buffer_mut();
        for (effect, rect) in &mut app.effects {
            effect.process(elapsed, buf, *rect);
        }
        app.effects.retain(|(effect, _)| effect.running());
    })?;
    app.last_frame = std::time::Instant::now();
    Ok(())
}

// ---------------------------------------------------------------------------
// Main menu
// ---------------------------------------------------------------------------

pub const MAIN_MENU_ITEMS: &[&str] = &["New Game", "Leaderboard", "Quit"];

fn draw_main_menu(
    frame: &mut ratatui::Frame,
    area: Rect,
    selected: usize,
) {
    let title = Line::from(Span::styled(
        "minesweepe-rs",
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
    ));

    let mut text: Vec<Line> = vec![title, Line::from("")];
    for (i, item) in MAIN_MENU_ITEMS.iter().enumerate() {
        if i == selected {
            text.push(Line::from(Span::styled(
                format!(" ▶ {} ", item),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            )));
        } else {
            text.push(Line::from(Span::styled(
                format!("   {} ", item),
                Style::default().fg(Color::White),
            )));
        }
    }
    text.push(Line::from(""));
    text.push(Line::from(Span::styled(
        "↑/↓ navigate    Enter: select",
        Style::default().fg(Color::DarkGray),
    )));

    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(" minesweepe-rs "))
        .alignment(Alignment::Center);

    let popup = centered_rect(40, 50, area);
    frame.render_widget(para, popup);
}

// ---------------------------------------------------------------------------
// New game menu
// ---------------------------------------------------------------------------

// Menu items: index maps to a selectable option (Custom opens sub-menu later).
pub const NEW_GAME_ITEMS: &[(&str, &str)] = &[
    ("Beginner",     "9×9, 10 mines"),
    ("Intermediate", "16×16, 40 mines"),
    ("Expert",       "30×16, 99 mines"),
    ("Custom",       "choose your own"),
];

fn draw_new_game_menu(
    frame: &mut ratatui::Frame,
    area: Rect,
    selected: usize,
    ui_hover: Option<UiHover>,
) {
    let text: Vec<Line> = NEW_GAME_ITEMS
        .iter()
        .enumerate()
        .map(|(i, (name, desc))| {
            if i == selected {
                Line::from(vec![
                    Span::styled(
                        format!(" ▶ {:<14}", name),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        format!("({})", desc),
                        Style::default().fg(Color::Yellow),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::styled(
                        format!("   {:<14}", name),
                        Style::default().fg(Color::White),
                    ),
                    Span::styled(
                        format!("({})", desc),
                        Style::default().fg(Color::DarkGray),
                    ),
                ])
            }
        })
        .chain(std::iter::once(Line::from("")))
        .chain(std::iter::once({
            let back_style = if ui_hover == Some(UiHover::Back) {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
            };
            Line::from(vec![
                Span::styled("  Enter: select    ", Style::default().fg(Color::DarkGray)),
                Span::styled("[ Back ]", back_style),
            ])
        }))
        .collect();

    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL).title(" New Game "))
        .alignment(Alignment::Left);

    let popup = centered_rect(50, 60, area);
    frame.render_widget(para, popup);
}

// ---------------------------------------------------------------------------
// Game board
// ---------------------------------------------------------------------------

fn draw_game(
    frame: &mut ratatui::Frame,
    area: Rect,
    game: &crate::types::GameState,
    active_cell: (u16, u16),
    cell_active: bool,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    draw_status_bar(frame, chunks[0], game);
    draw_board(frame, chunks[1], &game.board, &game.status, active_cell, cell_active, &game.config);
}

/// Game over screen: board stays fully visible, result replaces the status bar,
/// and menu items appear below the board in any remaining space.
fn draw_game_over(
    frame: &mut ratatui::Frame,
    area: Rect,
    game: &crate::types::GameState,
    active_cell: (u16, u16),
    cell_active: bool,
    won: bool,
    selected: usize,
) {
    let secs = game.elapsed.as_secs();
    let time_str = format!("{:02}:{:02}", secs / 60, secs % 60);
    let (result_text, result_color) = if won {
        ("✓ You Win!", Color::Green)
    } else {
        ("✗ Game Over", Color::Red)
    };

    // Header bar: result + time (replaces status bar)
    let header_line = Line::from(vec![
        Span::styled(
            format!("  {}  ", result_text),
            Style::default().fg(result_color).add_modifier(Modifier::BOLD),
        ),
        Span::raw("  "),
        Span::styled(time_str, Style::default().fg(Color::White)),
        Span::raw("          "),
        Span::styled(
            format!("[{}]", game.difficulty.label()),
            Style::default().fg(Color::DarkGray),
        ),
    ]);
    let header = Paragraph::new(header_line)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);

    let footer_h = 3u16;

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),       // header
            Constraint::Min(0),          // board + remaining space
            Constraint::Length(footer_h), // footer menu
        ])
        .split(area);

    frame.render_widget(header, chunks[0]);
    draw_board(frame, chunks[1], &game.board, &game.status, active_cell, cell_active, &game.config);

    // Footer menu: items in a single centered line
    let items: Vec<Span> = GAME_OVER_ITEMS.iter().enumerate().flat_map(|(i, item)| {
        let style = if i == selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };
        let sep = if i > 0 {
            vec![Span::styled("  |  ", Style::default().fg(Color::DarkGray))]
        } else {
            vec![]
        };
        sep.into_iter().chain(std::iter::once(Span::styled(format!("[ {} ]", item), style)))
    }).collect();

    let footer = Paragraph::new(Line::from(items))
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);
    frame.render_widget(footer, chunks[2]);
}

fn draw_status_bar(
    frame: &mut ratatui::Frame,
    area: Rect,
    game: &crate::types::GameState,
) {
    let mines_left = game.remaining_mines();
    let elapsed = game.elapsed;
    let secs = elapsed.as_secs();

    let text = Line::from(vec![
        Span::styled("  💣 ", Style::default()),
        Span::styled(
            format!("{:03}", mines_left),
            Style::default()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw("          "),
        Span::styled(
            format!("{:02}:{:02}", secs / 60, secs % 60),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  [{}]", game.difficulty.label()),
            Style::default().fg(Color::DarkGray),
        ),
    ]);

    let para = Paragraph::new(text)
        .block(Block::default().borders(Borders::ALL))
        .alignment(Alignment::Center);
    frame.render_widget(para, area);
}

fn draw_board(
    frame: &mut ratatui::Frame,
    area: Rect,
    board: &crate::types::Board,
    status: &GameStatus,
    active_cell: (u16, u16),
    cell_active: bool,
    config: &crate::types::GameConfig,
) {
    let detonated = match status {
        GameStatus::Lost { detonated } => Some(*detonated),
        _ => None,
    };

    // If the active cell is a revealed number, highlight its 8 neighbours.
    let neighbour_zone: Option<(u16, u16)> = if cell_active {
        let (ax, ay) = active_cell;
        let c = board.get(ax, ay);
        if c.visibility == CellVisibility::Revealed && !c.is_mine && c.adjacent_mines > 0 {
            Some((ax, ay))
        } else {
            None
        }
    } else {
        None
    };

    let stride_w = CELL_WIDTH + BORDER_COL;   // 3
    let stride_h = CELL_HEIGHT + BORDER_ROW;  // 2
    let board_w  = config.width  * stride_w + BORDER_COL;
    let board_h  = config.height * stride_h + BORDER_ROW;
    let origin   = center_rect(board_w, board_h, area);

    let border_style = Style::default().fg(Color::DarkGray);
    let blank_style  = Style::default().fg(Color::Reset).bg(Color::Reset);

    // Returns true if this in-bounds cell should dissolve into empty space.
    // Out-of-bounds coordinates (outside the board) are never zero.
    let is_zero = |cx: i32, cy: i32| -> bool {
        if cx < 0 || cy < 0 || cx >= config.width as i32 || cy >= config.height as i32 {
            return false;
        }
        let c = board.get(cx as u16, cy as u16);
        c.visibility == CellVisibility::Revealed && !c.is_mine && c.adjacent_mines == 0
    };

    // Precompute exposure: a revealed non-mine cell is exposed if it can reach a zero
    // cell through orthogonal connections of revealed cells. Flood-fill from all zeros.
    let w = config.width as usize;
    let h = config.height as usize;
    let mut exposed = vec![false; w * h];

    // Seed: all zero cells are exposed.
    for cy in 0..h {
        for cx in 0..w {
            if is_zero(cx as i32, cy as i32) {
                exposed[cy * w + cx] = true;
            }
        }
    }

    // Propagate: a revealed non-mine cell adjacent to an exposed cell is also exposed.
    // Iterate until stable (at most w*h passes; in practice very few).
    let mut changed = true;
    while changed {
        changed = false;
        for cy in 0..h {
            for cx in 0..w {
                if exposed[cy * w + cx] { continue; }
                let c = board.get(cx as u16, cy as u16);
                if c.visibility != CellVisibility::Revealed || c.is_mine { continue; }
                // Check orthogonal neighbours for an already-exposed revealed cell.
                let neighbours = [
                    (cx.wrapping_sub(1), cy),
                    (cx + 1, cy),
                    (cx, cy.wrapping_sub(1)),
                    (cx, cy + 1),
                ];
                for (nx, ny) in neighbours {
                    if nx < w && ny < h && exposed[ny * w + nx] {
                        exposed[cy * w + cx] = true;
                        changed = true;
                        break;
                    }
                }
            }
        }
    }

    let is_exposed = |cx: i32, cy: i32| -> bool {
        if cx < 0 || cy < 0 || cx >= config.width as i32 || cy >= config.height as i32 {
            return false;
        }
        exposed[cy as usize * w + cx as usize]
    };

    // A segment is suppressed when either adjacent cell is zero OR both adjacent
    // cells are exposed (both touch open space, so the border between them is redundant).
    // Perimeter segments are never suppressed.
    let h_seg_active = |cx: i32, cy_above: i32, cy_below: i32, is_top: bool, is_bot: bool| -> bool {
        if is_top || is_bot {
            return true;
        }
        !(is_zero(cx, cy_above)
            || is_zero(cx, cy_below)
            || (is_exposed(cx, cy_above) && is_exposed(cx, cy_below)))
    };

    let v_seg_active = |cx_left: i32, cx_right: i32, cy: i32, is_lft: bool, is_rgt: bool| -> bool {
        if is_lft || is_rgt {
            return true;
        }
        !(is_zero(cx_left, cy)
            || is_zero(cx_right, cy)
            || (is_exposed(cx_left, cy) && is_exposed(cx_right, cy)))
    };

    for term_row in 0..board_h {
        let is_border_row = term_row % stride_h == 0;
        let cy = (term_row / stride_h) as i32;

        let mut spans: Vec<Span> = Vec::with_capacity((board_w * 2) as usize);

        if is_border_row {
            let is_top = term_row == 0;
            let is_bot = term_row == board_h - 1;
            let cy_above = cy - 1;
            let cy_below = cy;

            for cx in 0..=config.width as i32 {
                let is_lft = cx == 0;
                let is_rgt = cx == config.width as i32;

                let arm_w = !is_lft && h_seg_active(cx - 1, cy_above, cy_below, is_top, is_bot);
                let arm_e = !is_rgt && h_seg_active(cx,     cy_above, cy_below, is_top, is_bot);
                let arm_n = !is_top && v_seg_active(cx - 1, cx, cy_above, is_lft, is_rgt);
                let arm_s = !is_bot && v_seg_active(cx - 1, cx, cy_below, is_lft, is_rgt);

                let junction = match (arm_n, arm_s, arm_w, arm_e) {
                    (false, false, false, false) => " ",
                    (false, false, true,  true ) => "─",
                    (true,  true,  false, false) => "│",
                    (false, true,  false, true ) => "┌",
                    (false, true,  true,  false) => "┐",
                    (true,  false, false, true ) => "└",
                    (true,  false, true,  false) => "┘",
                    (false, true,  true,  true ) => "┬",
                    (true,  false, true,  true ) => "┴",
                    (true,  true,  false, true ) => "├",
                    (true,  true,  true,  false) => "┤",
                    (true,  true,  true,  true ) => "┼",
                    (true,  false, false, false) => "╵",
                    (false, true,  false, false) => "╷",
                    (false, false, true,  false) => "╴",
                    (false, false, false, true ) => "╶",
                };

                if junction == " " {
                    spans.push(Span::styled(" ", blank_style));
                } else {
                    spans.push(Span::styled(junction, border_style));
                }

                if !is_rgt {
                    if h_seg_active(cx, cy_above, cy_below, is_top, is_bot) {
                        spans.push(Span::styled("──", border_style));
                    } else {
                        spans.push(Span::styled("  ", blank_style));
                    }
                }
            }
        } else {
            // Content row: vertical borders between cells.
            for cx in 0..config.width as i32 {
                let is_lft = cx == 0;
                if v_seg_active(cx - 1, cx, cy, is_lft, false) {
                    spans.push(Span::styled("│", border_style));
                } else {
                    spans.push(Span::styled(" ", blank_style));
                }

                let cell = board.get(cx as u16, cy as u16);
                let is_active    = cell_active && (cx as u16, cy as u16) == active_cell;
                let is_detonated = detonated == Some((cx as u16, cy as u16));
                let is_neighbour = !is_active && neighbour_zone.map_or(false, |(zx, zy)| {
                    (cx as u16).abs_diff(zx) <= 1 && (cy as u16).abs_diff(zy) <= 1
                });
                spans.push(cell_span(cell, is_active, is_neighbour, is_detonated));
            }
            // Right perimeter border — always drawn.
            spans.push(Span::styled("│", border_style));
        }

        let rect = Rect::new(origin.x, origin.y + term_row, board_w, 1);
        // Explicit reset style prevents ratatui from inheriting fg/bg from
        // the buffer's previous contents (e.g. yellow from the status bar).
        frame.render_widget(
            Paragraph::new(Line::from(spans)).style(Style::reset()),
            rect,
        );
    }
}

/// Each cell is rendered as a single fullwidth Unicode character string.
/// Fullwidth chars occupy exactly 2 terminal columns, giving square cells on
/// any terminal regardless of font aspect ratio.
fn cell_span<'a>(
    cell: &crate::types::Cell,
    is_active: bool,
    is_neighbour: bool,
    is_detonated: bool,
) -> Span<'a> {
    // Fullwidth characters: each is a 2-column glyph.
    // Numbers use fullwidth digits; other states use block/emoji chars.
    // Base symbol and colours by cell state.
    let (sym, fg, bg) = match cell.visibility {
        CellVisibility::Hidden => (
            "  ",   // empty — background is the only distinction from revealed zeros
            Color::Reset,
            Color::Reset,
        ),
        CellVisibility::Flagged => (
            "🚩",
            Color::Red,
            Color::Reset,
        ),
        CellVisibility::Question => (
            "？",
            Color::Yellow,
            Color::Reset,
        ),
        CellVisibility::Revealed => {
            if cell.is_mine {
                ("💣", Color::White, Color::Reset)
            } else {
                match cell.adjacent_mines {
                    // Zero: truly empty — no symbol, terminal default background.
                    0 => ("  ", Color::Reset, Color::Reset),
                    1 => ("１", Color::Blue,    Color::Reset),
                    2 => ("２", Color::Green,   Color::Reset),
                    3 => ("３", Color::Red,     Color::Reset),
                    4 => ("４", Color::Magenta, Color::Reset),
                    5 => ("５", Color::Yellow,  Color::Reset),
                    6 => ("６", Color::Cyan,    Color::Reset),
                    7 => ("７", Color::White,   Color::Reset),
                    _ => ("８", Color::Gray,    Color::Reset),
                }
            }
        }
    };

    // Always use a neutral dark background — no yellow under any circumstances.
    // Emoji glyphs (🚩, 💣) can bleed colour on some terminals; forcing a dark
    // bg here prevents that. The fg colour is the only colour signal for content.
    let mut style = Style::default().fg(fg).bg(bg);

    if is_detonated {
        style = style.bg(Color::Red).fg(Color::White).add_modifier(Modifier::BOLD);
    } else if is_active {
        style = style.bg(Color::White).fg(Color::Black);
    } else if is_neighbour && !matches!(cell.visibility, CellVisibility::Revealed) {
        let neighbour_bg = if matches!(cell.visibility, CellVisibility::Revealed) {
            Color::Rgb(40, 50, 80)
        } else {
            Color::Rgb(70, 80, 120)
        };
        style = style.bg(neighbour_bg);
    }

    Span::styled(sym, style)
}

// ---------------------------------------------------------------------------
// Game over overlay
// ---------------------------------------------------------------------------

pub const GAME_OVER_ITEMS: &[&str] = &["New Game", "Main Menu", "Quit"];



/// Returns the terminal Rects for the three game-over footer items.
/// Items are on a single row in the last 3 rows of the screen:
/// "[ New Game ]  |  [ Main Menu ]  |  [ Quit ]" centered.
pub fn game_over_item_rects(term_size: (u16, u16)) -> [Rect; 3] {
    // Full string: "[ New Game ]  |  [ Main Menu ]  |  [ Quit ]" = 43 chars
    // Item starts and widths within the string:
    const FULL_LEN: u16 = 43;
    const STARTS: [u16; 3] = [0, 17, 35];
    const WIDTHS: [u16; 3] = [12, 13, 8];
    let inner_w = term_size.0.saturating_sub(2);
    let left_pad = (inner_w.saturating_sub(FULL_LEN)) / 2;
    // Footer content row is term_height - 2 (inside the border)
    let row = term_size.1.saturating_sub(2);
    [0usize, 1, 2].map(|i| {
        Rect::new(1 + left_pad + STARTS[i], row, WIDTHS[i], 1)
    })
}

/// Returns the popup Rect and the row of the first main menu item.
/// Layout: border + title_line + "" = 2 lines before items.
pub fn main_menu_item_rows(area: Rect) -> (u16, Rect) {
    let popup = centered_rect_pub(40, 50, area);
    let first_item_row = popup.y + 1 + 2; // border + title + blank
    (first_item_row, popup)
}

/// Returns the popup Rect and the row of the first new-game menu item.
/// Layout: border = 1 line before items (no title line or blank).
pub fn new_game_menu_item_rows(area: Rect) -> (u16, Rect) {
    let popup = centered_rect_pub(50, 60, area);
    let first_item_row = popup.y + 1; // border only
    (first_item_row, popup)
}

/// Returns the full-width Rect for the leaderboard tab bar and the exact column
/// ranges for each tab label, computed to match draw_leaderboard's centered layout.
pub fn leaderboard_tab_rects(term_size: (u16, u16)) -> (Rect, [Rect; 3]) {
    // The tab line is "Beginner | Intermediate | Expert" centered in the inner block.
    // Label starts within the string: Beginner=0, Intermediate=11, Expert=26.
    // Label widths: 8, 12, 6. Full string width: 32.
    const FULL_LEN: u16 = 32;
    const STARTS: [u16; 3] = [0, 11, 26];
    const WIDTHS: [u16; 3] = [8, 12, 6];

    let tab_bar = Rect::new(0, 0, term_size.0, 3);
    let inner_w = term_size.0.saturating_sub(2);
    let left_pad = (inner_w.saturating_sub(FULL_LEN)) / 2;
    // Absolute column of each label: border(1) + left_pad + label_start_in_string
    let tabs = [0usize, 1, 2].map(|i| {
        let x = 1 + left_pad + STARTS[i];
        Rect::new(x, 1, WIDTHS[i], 1)
    });
    (tab_bar, tabs)
}

/// Returns the exact Rect for the clickable [ Back ] button in the leaderboard footer.
/// The footer line is "[ ← / → ] Switch tab    [ Back ]" (32 display cols), centered.
/// [ Back ] starts at char index 24, width 8.
pub fn leaderboard_back_rect(term_size: (u16, u16)) -> Rect {
    const FULL_LEN: u16 = 32;
    const BACK_START: u16 = 24;
    const BACK_WIDTH: u16 = 8;
    let inner_w = term_size.0.saturating_sub(2);
    let left_pad = (inner_w.saturating_sub(FULL_LEN)) / 2;
    let footer_y = term_size.1.saturating_sub(2); // inside border = last row - 1
    let x = 1 + left_pad + BACK_START;
    Rect::new(x, footer_y, BACK_WIDTH, 1)
}

// ---------------------------------------------------------------------------
// Leaderboard
// ---------------------------------------------------------------------------

fn draw_leaderboard(
    frame: &mut ratatui::Frame,
    area: Rect,
    leaderboard: &crate::types::Leaderboard,
    tab: LeaderboardTab,
    ui_hover: Option<UiHover>,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    // Tab bar — highlight active tab and hovered tab
    let tabs_line = Line::from(vec![
        tab_span("Beginner",     tab == LeaderboardTab::Beginner,     ui_hover == Some(UiHover::Tab(0))),
        Span::raw(" | "),
        tab_span("Intermediate", tab == LeaderboardTab::Intermediate, ui_hover == Some(UiHover::Tab(1))),
        Span::raw(" | "),
        tab_span("Expert",       tab == LeaderboardTab::Expert,       ui_hover == Some(UiHover::Tab(2))),
    ]);
    let tabs = Paragraph::new(tabs_line)
        .block(Block::default().borders(Borders::ALL).title(" Leaderboard "))
        .alignment(Alignment::Center);
    frame.render_widget(tabs, chunks[0]);

    // Entries table
    let difficulty = tab.difficulty();
    let entries = leaderboard.get(difficulty);

    let rows: Vec<Row> = entries
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let secs = e.time.as_secs();
            let time_str = format!("{:02}:{:02}.{:03}", secs / 60, secs % 60, e.time.subsec_millis());
            let date_str = e.achieved_at.format("%Y-%m-%d").to_string();
            Row::new(vec![
                RatCell::from(format!("  {:>2}.", i + 1)),
                RatCell::from(time_str).style(Style::default().fg(Color::Cyan)),
                RatCell::from(date_str).style(Style::default().fg(Color::DarkGray)),
            ])
        })
        .collect();

    let empty_row = vec![Row::new(vec![RatCell::from("  No entries yet.")])];
    let display_rows = if rows.is_empty() { empty_row } else { rows };

    let table = Table::new(
        display_rows,
        [Constraint::Length(6), Constraint::Length(12), Constraint::Fill(1)],
    )
    .header(Row::new(vec!["  #", "Time", "Date"]).style(
        Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    ))
    .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM));
    frame.render_widget(table, chunks[1]);

    // Footer: hint text + clickable [ Back ] button
    let back_style = if ui_hover == Some(UiHover::Back) {
        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
    };
    let footer = Paragraph::new(Line::from(vec![
        Span::styled("[ ← / → ] Switch tab    ", Style::default().fg(Color::DarkGray)),
        Span::styled("[ Back ]", back_style),
    ]))
    .block(Block::default().borders(Borders::ALL))
    .alignment(Alignment::Center);
    frame.render_widget(footer, chunks[2]);
}

fn tab_span(label: &'static str, active: bool, hovered: bool) -> Span<'static> {
    if active {
        Span::styled(
            label,
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        )
    } else if hovered {
        Span::styled(
            label,
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(label, Style::default().fg(Color::DarkGray))
    }
}

// ---------------------------------------------------------------------------
// Input translation
// ---------------------------------------------------------------------------

/// Translates a raw crossterm Event into an optional GameEvent, bundling the
/// logical action with the board-space coordinate for mouse events.
pub fn translate_event(
    event: &Event,
    board_origin: (u16, u16),
    board_size: (u16, u16),
    term_size: (u16, u16),
    screen: &Screen,
) -> Option<GameEvent> {
    match event {
        Event::Key(key) => translate_key(key, screen).map(|action| GameEvent {
            action,
            board_pos: None,
        }),
        Event::Mouse(mouse) => {
            // For the game-over overlay, intercept clicks on the menu items
            // before they reach the board underneath.
            if let Screen::GameOver { .. } = screen {
                // Check footer items first; otherwise allow board mouse events through.
                if let Some(ev) = translate_game_over_click(mouse, term_size) {
                    return Some(ev);
                }
                // Board is fully visible — allow hover/move to update active cell.
                return translate_mouse(mouse, board_origin, board_size);
            }
            if let Screen::MainMenu { .. } = screen {
                return translate_main_menu_click(mouse, term_size);
            }
            if let Screen::NewGameMenu { .. } = screen {
                return translate_new_game_menu_click(mouse, term_size);
            }
            if let Screen::Leaderboard { .. } = screen {
                return translate_leaderboard_click(mouse, term_size);
            }
            translate_mouse(mouse, board_origin, board_size)
        }
        _ => None,
    }
}

/// Hit-tests mouse events against the main menu items.
fn translate_main_menu_click(
    mouse: &crossterm::event::MouseEvent,
    term_size: (u16, u16),
) -> Option<GameEvent> {
    let area = Rect::new(0, 0, term_size.0, term_size.1);
    let (first_row, popup) = main_menu_item_rows(area);
    let last_row = first_row + MAIN_MENU_ITEMS.len() as u16 - 1;

    if mouse.column < popup.x || mouse.column >= popup.x + popup.width {
        return None;
    }

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if mouse.row >= first_row && mouse.row <= last_row {
                let idx = (mouse.row - first_row) as usize;
                Some(GameEvent { action: GameAction::SelectGameOverItem(idx), board_pos: None })
            } else {
                None
            }
        }
        MouseEventKind::Moved => {
            if mouse.row >= first_row && mouse.row <= last_row {
                let idx = (mouse.row - first_row) as usize;
                Some(GameEvent { action: GameAction::HoverGameOverItem(idx), board_pos: None })
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Hit-tests mouse events against the new game menu items and [ Back ] button.
fn translate_new_game_menu_click(
    mouse: &crossterm::event::MouseEvent,
    term_size: (u16, u16),
) -> Option<GameEvent> {
    let area = Rect::new(0, 0, term_size.0, term_size.1);
    let (first_row, popup) = new_game_menu_item_rows(area);
    let last_row = first_row + NEW_GAME_ITEMS.len() as u16 - 1;
    // Hint line: border(1) + 4 items + blank = row 6 inside popup → popup.y + 6
    // "[ Back ]" starts at char 19 inside the line → popup.x + 1 + 19
    let back_row = popup.y + 6;
    let back_x   = popup.x + 1 + 19;
    let back_w   = 8u16;

    if mouse.column < popup.x || mouse.column >= popup.x + popup.width {
        return None;
    }

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // Precise [ Back ] hit
            if mouse.row == back_row
                && mouse.column >= back_x
                && mouse.column < back_x + back_w
            {
                return Some(GameEvent { action: GameAction::OpenMenu, board_pos: None });
            }
            if mouse.row >= first_row && mouse.row <= last_row {
                let idx = (mouse.row - first_row) as usize;
                Some(GameEvent { action: GameAction::SelectGameOverItem(idx), board_pos: None })
            } else {
                None
            }
        }
        MouseEventKind::Moved => {
            if mouse.row == back_row
                && mouse.column >= back_x
                && mouse.column < back_x + back_w
            {
                return Some(GameEvent { action: GameAction::HoverBack, board_pos: None });
            }
            if mouse.row >= first_row && mouse.row <= last_row {
                let idx = (mouse.row - first_row) as usize;
                Some(GameEvent { action: GameAction::HoverGameOverItem(idx), board_pos: None })
            } else {
                Some(GameEvent { action: GameAction::ClearUiHover, board_pos: None })
            }
        }
        _ => None,
    }
}

/// Hit-tests mouse events against the leaderboard tabs and footer.
fn translate_leaderboard_click(
    mouse: &crossterm::event::MouseEvent,
    term_size: (u16, u16),
) -> Option<GameEvent> {
    let (tab_bar, tab_rects) = leaderboard_tab_rects(term_size);
    let back = leaderboard_back_rect(term_size);

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if mouse.row == back.y
                && mouse.column >= back.x
                && mouse.column < back.x + back.width
            {
                return Some(GameEvent { action: GameAction::OpenMenu, board_pos: None });
            }
            if mouse.row >= tab_bar.y && mouse.row < tab_bar.y + tab_bar.height {
                for (i, rect) in tab_rects.iter().enumerate() {
                    if mouse.column >= rect.x && mouse.column < rect.x + rect.width {
                        return Some(GameEvent {
                            action: GameAction::SelectGameOverItem(i),
                            board_pos: None,
                        });
                    }
                }
            }
            None
        }
        MouseEventKind::Moved => {
            if mouse.row == back.y
                && mouse.column >= back.x
                && mouse.column < back.x + back.width
            {
                return Some(GameEvent { action: GameAction::HoverBack, board_pos: None });
            }
            if mouse.row >= tab_bar.y && mouse.row < tab_bar.y + tab_bar.height {
                for (i, rect) in tab_rects.iter().enumerate() {
                    if mouse.column >= rect.x && mouse.column < rect.x + rect.width {
                        return Some(GameEvent {
                            action: GameAction::HoverGameOverItem(i),
                            board_pos: None,
                        });
                    }
                }
            }
            Some(GameEvent { action: GameAction::ClearUiHover, board_pos: None })
        }
        _ => None,
    }
}

/// Hit-tests mouse events against the game-over footer items.
/// Items are on a single row in the footer bar at the bottom of the screen.
fn translate_game_over_click(
    mouse: &crossterm::event::MouseEvent,
    term_size: (u16, u16),
) -> Option<GameEvent> {
    let rects = game_over_item_rects(term_size);
    let footer_row = term_size.1.saturating_sub(2);

    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            if mouse.row == footer_row {
                for (i, rect) in rects.iter().enumerate() {
                    if mouse.column >= rect.x && mouse.column < rect.x + rect.width {
                        return Some(GameEvent {
                            action: GameAction::SelectGameOverItem(i),
                            board_pos: None,
                        });
                    }
                }
            }
            None
        }
        MouseEventKind::Moved => {
            if mouse.row == footer_row {
                for (i, rect) in rects.iter().enumerate() {
                    if mouse.column >= rect.x && mouse.column < rect.x + rect.width {
                        return Some(GameEvent {
                            action: GameAction::HoverGameOverItem(i),
                            board_pos: None,
                        });
                    }
                }
            }
            None
        }
        _ => None,
    }
}

fn translate_key(
    key: &crossterm::event::KeyEvent,
    _screen: &Screen,
) -> Option<GameAction> {
    use crossterm::event::KeyEventKind;
    // Only process Press events to avoid double-firing on repeat/release.
    if key.kind != KeyEventKind::Press { return None; }
    match key.code {
        KeyCode::Char('q') | KeyCode::Char('Q') => Some(GameAction::Quit),
        KeyCode::Esc                             => Some(GameAction::OpenMenu),
        KeyCode::Enter | KeyCode::Char(' ')      => Some(GameAction::Reveal),
        KeyCode::Char('f') | KeyCode::Char('F')  => Some(GameAction::CycleFlag),
        KeyCode::Up    | KeyCode::Char('k')      => Some(GameAction::MoveCursor(0, -1)),
        KeyCode::Down  | KeyCode::Char('j')      => Some(GameAction::MoveCursor(0, 1)),
        KeyCode::Left  | KeyCode::Char('h')      => Some(GameAction::MoveCursor(-1, 0)),
        KeyCode::Right | KeyCode::Char('l')      => Some(GameAction::MoveCursor(1, 0)),
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            Some(GameAction::Quit)
        }
        _ => None,
    }
}

fn translate_mouse(
    mouse: &crossterm::event::MouseEvent,
    board_origin: (u16, u16),
    board_size: (u16, u16),
) -> Option<GameEvent> {
    // Translate from terminal space to board space.
    // Subtract 1 to skip the leading border column/row, then divide by stride.
    let col = mouse.column.checked_sub(board_origin.0)?;
    let row = mouse.row.checked_sub(board_origin.1)?;

    let stride_w = CELL_WIDTH + BORDER_COL; // 3
    let stride_h = CELL_HEIGHT + BORDER_ROW; // 2

    // Clicks on border rows/cols snap to the nearest cell — just divide by stride.
    let bx = (col / stride_w).min(board_size.0.saturating_sub(1));
    let by = (row / stride_h).min(board_size.1.saturating_sub(1));

    let action = match mouse.kind {
        MouseEventKind::Down(MouseButton::Left)  => GameAction::Reveal,
        MouseEventKind::Down(MouseButton::Right) => GameAction::CycleFlag,
        // Mouse move updates hover position but triggers no action.
        MouseEventKind::Moved => GameAction::MoveCursor(0, 0),
        _ => return None,
    };

    Some(GameEvent { action, board_pos: Some((bx, by)) })
}

/// Given the current terminal size and board config, returns the top-left
/// corner of the board widget in terminal space. Used for mouse → board-space
/// translation.
/// Computes the terminal Rect covering the entire rendered board (excluding status bar).
/// Used to scope tachyonfx effects to the board area.
pub fn board_rect(term_width: u16, term_height: u16, config: &crate::types::GameConfig) -> Rect {
    let origin = board_origin(term_width, term_height, config);
    let stride_w = CELL_WIDTH + BORDER_COL;
    let stride_h = CELL_HEIGHT + BORDER_ROW;
    let w = config.width  * stride_w + BORDER_COL;
    let h = config.height * stride_h + BORDER_ROW;
    Rect::new(origin.0, origin.1, w, h)
}

pub fn board_origin(
    term_width: u16,
    term_height: u16,
    config: &crate::types::GameConfig,
) -> (u16, u16) {
    // Must exactly match draw_board's board_w/board_h calculation.
    let stride_w = CELL_WIDTH + BORDER_COL;
    let stride_h = CELL_HEIGHT + BORDER_ROW;
    let board_w  = config.width  * stride_w + BORDER_COL;
    let board_h  = config.height * stride_h + BORDER_ROW;
    // Status bar takes 3 rows; board is centered in the remaining space.
    let available_h = term_height.saturating_sub(3);

    let ox = (term_width.saturating_sub(board_w)) / 2;
    let oy = 3 + (available_h.saturating_sub(board_h)) / 2;
    (ox, oy)
}

// ---------------------------------------------------------------------------
// Layout helpers
// ---------------------------------------------------------------------------

/// Returns a centered Rect of the given percentage dimensions within `area`.
pub fn centered_rect_pub(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    centered_rect(percent_x, percent_y, area)
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vert = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vert[1])[1]
}

/// Returns a Rect of exact dimensions centered within `area`.
fn center_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
