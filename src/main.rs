use std::time::{Duration, Instant};

use crossterm::event::{self, Event};

use ratatui::layout::Rect;
use minesweepe_rs::game;
use minesweepe_rs::game::{detonate_effect, reveal_effect};
use minesweepe_rs::leaderboard;
use minesweepe_rs::tui;
use minesweepe_rs::tui::{GAME_OVER_ITEMS, MAIN_MENU_ITEMS, NEW_GAME_ITEMS};
use minesweepe_rs::types::{
    App, CellVisibility, Difficulty, GameAction, GameState, GameStatus, LeaderboardTab, Screen, UiHover,
    BORDER_COL, BORDER_ROW, CELL_HEIGHT, CELL_WIDTH,
};

const TICK_RATE: Duration = Duration::from_millis(250);
const EFFECT_TICK: Duration = Duration::from_millis(16); // ~60fps when effects running

fn main() -> anyhow::Result<()> {
    let lb = leaderboard::load();
    let mut app = App::new(lb);
    let mut terminal = tui::init()?;

    let result = run(&mut terminal, &mut app);

    tui::restore(&mut terminal)?;
    leaderboard::save(&app.leaderboard);

    if let Err(e) = result {
        eprintln!("Error: {e}");
    }
    Ok(())
}

fn run(terminal: &mut tui::Tui, app: &mut App) -> anyhow::Result<()> {
    let mut last_tick = Instant::now();

    loop {
        tui::draw(terminal, app)?;

        // Use faster tick while effects are running for smooth animation.
        let tick = if !app.effects.is_empty() { EFFECT_TICK } else { TICK_RATE };
        let timeout = tick.saturating_sub(last_tick.elapsed());
        if event::poll(timeout)? {
            let raw = event::read()?;
            handle_event(app, &raw);
        }

        if last_tick.elapsed() >= TICK_RATE {
            tick_timer(app);
            last_tick = Instant::now();
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Timer
// ---------------------------------------------------------------------------

fn tick_timer(app: &mut App) {
    if let Some(game) = &mut app.game {
        if game.status == GameStatus::Playing {
            game.elapsed += TICK_RATE;
        }
    }
}

// ---------------------------------------------------------------------------
// Event handling
// ---------------------------------------------------------------------------

fn term_size() -> (u16, u16) {
    crossterm::terminal::size().unwrap_or((80, 24))
}

fn handle_event(app: &mut App, event: &Event) {
    let (tw, th) = term_size();

    let board_origin = app.game.as_ref().map_or((0, 0), |g| {
        tui::board_origin(tw, th, &g.config)
    });
    let board_size = app.game.as_ref().map_or((0, 0), |g| {
        (g.config.width, g.config.height)
    });

    let Some(ev) = tui::translate_event(event, board_origin, board_size, (tw, th), &app.screen) else {
        return;
    };

    // UI hover actions — update ui_hover and return, never dispatch further.
    match ev.action {
        GameAction::HoverBack  => { app.ui_hover = Some(UiHover::Back);  return; }
        GameAction::HoverStart => { app.ui_hover = Some(UiHover::Start); return; }
        GameAction::ClearUiHover => { app.ui_hover = None; return; }
        GameAction::MenuHover(i) => {
            // On leaderboard this means tab hover; on other screens it's menu item hover.
            // Only set ui_hover here for leaderboard; other screens handle it in dispatch.
            if matches!(app.screen, Screen::Leaderboard { .. }) {
                app.ui_hover = Some(UiHover::Tab(i));
                return;
            }
            // Fall through for menu screens — handled as before.
        }
        _ => {}
    }

    // Mouse move on board: take over active_cell.
    if ev.action == GameAction::MoveCursor(0, 0) {
        if let Some(pos) = ev.board_pos {
            app.mouse_controlling = true;
            app.active_cell = pos;
            app.cell_active = true;
        }
        return;
    }

    if let Some(pos) = ev.board_pos {
        app.mouse_controlling = true;
        app.active_cell = pos;
        app.cell_active = true;
    } else {
        app.mouse_controlling = false;
    }
    // Clear ui_hover on any confirmed action.
    app.ui_hover = None;

    dispatch(app, ev.action, ev.board_pos);
}

// ---------------------------------------------------------------------------
// Action dispatch
// ---------------------------------------------------------------------------

fn dispatch(app: &mut App, action: GameAction, board_pos: Option<(u16, u16)>) {
    match app.screen.clone() {
        Screen::MainMenu { selected }    => dispatch_main_menu(app, action, selected),
        Screen::NewGameMenu { selected } => dispatch_new_game_menu(app, action, selected),
        Screen::CustomGame { field, width, height, mines } => {
            dispatch_custom_game(app, action, field, width, height, mines);
        }
        Screen::Playing                  => dispatch_playing(app, action, board_pos),
        Screen::GameOver { selected, won } => dispatch_game_over(app, action, selected, won),
        Screen::Leaderboard { tab }      => dispatch_leaderboard(app, action, tab),
    }
}

fn dispatch_main_menu(app: &mut App, action: GameAction, selected: usize) {
    fn confirm(app: &mut App, idx: usize) {
        match idx {
            0 => app.screen = Screen::NewGameMenu { selected: 0 },
            1 => app.screen = Screen::Leaderboard { tab: LeaderboardTab::Beginner },
            _ => app.should_quit = true,
        }
    }
    let item_count = MAIN_MENU_ITEMS.len();
    match action {
        GameAction::Quit => app.should_quit = true,
        GameAction::MoveCursor(0, -1) => {
            app.screen = Screen::MainMenu { selected: selected.saturating_sub(1) };
        }
        GameAction::MoveCursor(0, 1) => {
            app.screen = Screen::MainMenu { selected: (selected + 1).min(item_count - 1) };
        }
        GameAction::Reveal            => confirm(app, selected),
        GameAction::MenuSelect(idx)   => confirm(app, idx),
        GameAction::MenuHover(idx)    => app.screen = Screen::MainMenu { selected: idx },
        _ => {}
    }
}

fn dispatch_new_game_menu(app: &mut App, action: GameAction, selected: usize) {
    fn confirm(app: &mut App, idx: usize) {
        match idx {
            0 => start_game(app, Difficulty::Beginner),
            1 => start_game(app, Difficulty::Intermediate),
            2 => start_game(app, Difficulty::Expert),
            _ => app.screen = Screen::CustomGame {
                field: 0,
                width: String::new(),
                height: String::new(),
                mines: String::new(),
            },
        }
    }
    let item_count = NEW_GAME_ITEMS.len();
    match action {
        GameAction::OpenMenu | GameAction::Quit => {
            app.screen = Screen::MainMenu { selected: 0 };
        }
        GameAction::MoveCursor(0, -1) => {
            app.screen = Screen::NewGameMenu { selected: selected.saturating_sub(1) };
        }
        GameAction::MoveCursor(0, 1) => {
            app.screen = Screen::NewGameMenu { selected: (selected + 1).min(item_count - 1) };
        }
        GameAction::Reveal          => confirm(app, selected),
        GameAction::MenuSelect(idx) => confirm(app, idx),
        GameAction::MenuHover(idx)  => app.screen = Screen::NewGameMenu { selected: idx },
        _ => {}
    }
}

fn set_custom_game_screen(app: &mut App, field: usize, width: String, height: String, mines: String) {
    app.screen = Screen::CustomGame { field, width, height, mines };
}

fn dispatch_custom_game(
    app: &mut App,
    action: GameAction,
    field: usize,
    width: String,
    height: String,
    mines: String,
) {
    // 5 tab stops: 0=Width, 1=Height, 2=Mines, 3=Start, 4=Back
    const TOTAL_STOPS: usize = 5;

    let validate = |w: &str, h: &str, m: &str| -> Option<Difficulty> {
        let w: u16 = w.parse().unwrap_or(0);
        let h: u16 = h.parse().unwrap_or(0);
        let m: u16 = m.parse().unwrap_or(0);
        let max_mines = w.saturating_mul(h).saturating_sub(9);
        if w >= 4 && h >= 4 && m >= 1 && m <= max_mines {
            Some(Difficulty::Custom(w, h, m))
        } else {
            None
        }
    };

    match action {
        GameAction::OpenMenu | GameAction::Quit => {
            app.screen = Screen::NewGameMenu { selected: 3 };
        }
        // Tab: cycle through all 5 stops forward
        GameAction::MoveCursor(0, 1) => {
            set_custom_game_screen(app, (field + 1) % TOTAL_STOPS, width, height, mines);
        }
        // Shift-tab / up: cycle backward
        GameAction::MoveCursor(0, -1) => {
            set_custom_game_screen(app, (field + TOTAL_STOPS - 1) % TOTAL_STOPS, width, height, mines);
        }
        GameAction::Reveal => {
            match field {
                // On Start button: attempt to start
                3 => { if let Some(d) = validate(&width, &height, &mines) { start_game(app, d); } }
                // On Back button: go back
                4 => { app.screen = Screen::NewGameMenu { selected: 3 }; }
                // On a text field: advance to next empty field, or start if all filled
                _ => {
                    let fields_arr = [&width, &height, &mines];
                    let next_empty = (1..3)
                        .map(|i| (field + i) % 3)
                        .find(|&i| fields_arr[i].is_empty());
                    if let Some(next) = next_empty {
                        set_custom_game_screen(app, next, width, height, mines);
                    } else {
                        if let Some(d) = validate(&width, &height, &mines) { start_game(app, d); }
                    }
                }
            }
        }
        // Backspace: only on text fields
        GameAction::Backspace if field < 3 => {
            let mut fields = [width, height, mines];
            fields[field].pop();
            let [w, h, m] = fields;
            set_custom_game_screen(app, field, w, h, m);
        }
        // Digit input: only on text fields
        GameAction::TypeChar(c) if c.is_ascii_digit() && field < 3 => {
            let mut fields = [width, height, mines];
            if fields[field].len() < 3 {
                fields[field].push(c);
            }
            let [w, h, m] = fields;
            set_custom_game_screen(app, field, w, h, m);
        }
        // Mouse click on a field row selects it
        GameAction::MenuSelect(idx) | GameAction::MenuHover(idx) if idx < 3 => {
            set_custom_game_screen(app, idx, width, height, mines);
        }
        _ => {}
    }
}

fn dispatch_playing(app: &mut App, action: GameAction, board_pos: Option<(u16, u16)>) {
    match action {
        GameAction::Quit | GameAction::OpenMenu => {
            app.screen = Screen::MainMenu { selected: 0 };
            return;
        }
        GameAction::MoveCursor(dx, dy) => {
            if let Some(game) = &app.game {
                let new_pos = game::move_cursor(app.active_cell, dx, dy, &game.config);
                app.active_cell = new_pos;
                app.cell_active = true;
            }
            return;
        }
        _ => {}
    }

    // Mouse actions use the clicked position; keyboard actions use active_cell.
    let target = board_pos.unwrap_or(app.active_cell);

    let Some(game) = &mut app.game else { return };

    let newly_revealed: Vec<(u16, u16)> = match action {
        GameAction::Reveal => {
            let cell = game.board.get(target.0, target.1);
            if cell.visibility == CellVisibility::Revealed {
                game::chord_reveal(game, target.0, target.1)
            } else {
                game::reveal(game, target.0, target.1)
            }
        }
        GameAction::CycleFlag => {
            let cell = game.board.get(target.0, target.1);
            if cell.visibility == CellVisibility::Revealed {
                game::chord_flag(game, target.0, target.1);
                vec![]
            } else {
                game::cycle_flag(game, target.0, target.1);
                vec![]
            }
        }
        _ => vec![],
    };

    if !newly_revealed.is_empty() {
        let (tw, th) = term_size();
        if let Some(game) = &app.game {
            let origin = tui::board_origin(tw, th, &game.config);
            let stride_w = CELL_WIDTH + BORDER_COL;
            let stride_h = CELL_HEIGHT + BORDER_ROW;
            let count = newly_revealed.len();
            for (cx, cy) in &newly_revealed {
                let x = origin.0 + 1 + cx * stride_w;
                let y = origin.1 + 1 + cy * stride_h;
                let rect = Rect::new(x, y, CELL_WIDTH, CELL_HEIGHT);
                app.effects.push((reveal_effect(count), rect));
            }
        }
    }

    check_game_over(app);
}

fn dispatch_game_over(app: &mut App, action: GameAction, selected: usize, won: bool) {
    fn confirm(app: &mut App, idx: usize) {
        match idx {
            0 => {
                if let Some(game) = &app.game {
                    let difficulty = game.difficulty;
                    start_game(app, difficulty);
                }
            }
            1 => app.screen = Screen::MainMenu { selected: 0 },
            _ => app.should_quit = true,
        }
    }
    let item_count = GAME_OVER_ITEMS.len();
    match action {
        GameAction::Quit     => app.should_quit = true,
        GameAction::OpenMenu => app.screen = Screen::MainMenu { selected: 0 },
        GameAction::MoveCursor(0, -1) => {
            app.screen = Screen::GameOver { won, selected: selected.saturating_sub(1) };
        }
        GameAction::MoveCursor(0, 1) => {
            app.screen = Screen::GameOver { won, selected: (selected + 1).min(item_count - 1) };
        }
        GameAction::Reveal          => confirm(app, selected),
        GameAction::MenuSelect(idx) => confirm(app, idx),
        GameAction::MenuHover(idx)  => app.screen = Screen::GameOver { won, selected: idx },
        _ => {}
    }
}

fn dispatch_leaderboard(app: &mut App, action: GameAction, tab: LeaderboardTab) {
    match action {
        GameAction::OpenMenu | GameAction::Quit => app.screen = Screen::MainMenu { selected: 0 },
        GameAction::MoveCursor(1, 0)            => app.screen = Screen::Leaderboard { tab: tab.next() },
        GameAction::MoveCursor(-1, 0)           => app.screen = Screen::Leaderboard { tab: tab.prev() },
        GameAction::MenuSelect(idx) => {
            let new_tab = match idx {
                0 => LeaderboardTab::Beginner,
                1 => LeaderboardTab::Intermediate,
                _ => LeaderboardTab::Expert,
            };
            app.screen = Screen::Leaderboard { tab: new_tab };
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------
// Game lifecycle helpers
// ---------------------------------------------------------------------------

fn start_game(app: &mut App, difficulty: Difficulty) {
    app.game = Some(GameState::new(difficulty));
    app.active_cell = (0, 0);
    app.cell_active = false;
    app.mouse_controlling = true;
    app.screen = Screen::Playing;
}

fn check_game_over(app: &mut App) {
    let Some(game) = &app.game else { return };
    match game.status {
        GameStatus::Won => {
            let elapsed = game.elapsed;
            let difficulty = game.difficulty;
            leaderboard::submit(&mut app.leaderboard, difficulty, elapsed);
            app.screen = Screen::GameOver { won: true, selected: 0 };
        }
        GameStatus::Lost { .. } => {
            let (tw, th) = term_size();
            let det_rect = tui::board_rect(tw, th, &game.config);
            app.effects.push((detonate_effect(), det_rect));
            app.screen = Screen::GameOver { won: false, selected: 0 };
        }
        _ => {}
    }
}
