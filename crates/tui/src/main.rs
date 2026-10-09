use battlesnake_core::{Coord, Direction};
use battlesnake_proto::{BoardView, ClientMessage, EnemyView, ServerMessage, ShotResult};
use crossterm::{
    event::{self, Event as TerminalEvent, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use futures_util::{SinkExt, StreamExt};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction as LayoutDirection, Layout},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::{error::Error, io};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::{connect_async, tungstenite::Message};

struct Ui {
    room: String,
    player: Option<usize>,
    your_board: Option<BoardView>,
    enemy_view: Option<EnemyView>,
    direction: Direction,
    target: Coord,
    your_turn: bool,
    status: String,
}

impl Default for Ui {
    fn default() -> Self {
        Self {
            room: String::new(),
            player: None,
            your_board: None,
            enemy_view: None,
            direction: Direction::Right,
            target: Coord { x: 0, y: 0 },
            your_turn: false,
            status: String::new(),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let address = args
        .next()
        .unwrap_or_else(|| "ws://127.0.0.1:7777/ws".to_owned());
    let room = args.next().unwrap_or_default();
    let name = args.next().unwrap_or_else(|| "Player".to_owned());
    let (socket, _) = connect_async(&address).await?;
    let (mut writer, mut reader) = socket.split();
    writer
        .send(Message::Text(
            serde_json::to_string(&ClientMessage::Join { room, name })?.into(),
        ))
        .await?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    let mut ui = Ui {
        status: "Connected. Press R when both players are ready; arrows aim, WASD steers.".into(),
        ..Ui::default()
    };

    let result = loop {
        terminal.draw(|frame| draw(frame, &ui))?;

        match timeout(Duration::from_millis(20), reader.next()).await {
            Ok(None) => break Ok(()),
            Ok(Some(Err(error))) => break Err(error.into()),
            Ok(Some(Ok(Message::Text(text)))) => {
                match serde_json::from_str::<ServerMessage>(&text) {
                    Ok(ServerMessage::Joined { room, player }) => {
                        ui.room = room;
                        ui.player = Some(player);
                        ui.status = format!("Joined room {} as player {}.", ui.room, player + 1);
                    }
                    Ok(ServerMessage::Start {
                        your_board,
                        enemy_view,
                    }) => {
                        ui.target = Coord {
                            x: your_board.width / 2,
                            y: your_board.height / 2,
                        };
                        ui.direction = heading(&your_board).unwrap_or(ui.direction);
                        ui.your_board = Some(your_board);
                        ui.enemy_view = Some(enemy_view);
                        ui.status = "Game started.".into();
                    }
                    Ok(ServerMessage::YourTurn { deadline }) => {
                        ui.your_turn = true;
                        ui.status = format!("Your turn (deadline {deadline}).");
                    }
                    Ok(ServerMessage::TurnResult {
                        shot_at,
                        outcome,
                        timed_out,
                        your_board,
                    }) => {
                        ui.your_turn = false;
                        ui.direction = heading(&your_board).unwrap_or(ui.direction);
                        ui.your_board = Some(your_board);
                        if let (Some(target), Some(outcome)) = (shot_at, outcome) {
                            if let Some(view) = &mut ui.enemy_view {
                                view.shots
                                    .push(battlesnake_proto::ShotRecord { target, outcome });
                            }
                            ui.status = format!("Shot result: {}", outcome_text(outcome));
                        } else if timed_out {
                            ui.status = "Turn timed out; continued straight without firing.".into();
                        } else {
                            ui.status = "Opponent took their turn.".into();
                        }
                    }
                    Ok(ServerMessage::GameOver { winner, reveal }) => {
                        ui.your_turn = false;
                        if let Some(player) = ui.player {
                            if let Some(board) = &mut ui.your_board {
                                board.your_segments = reveal[player].clone();
                            }
                            if let Some(enemy) = &mut ui.enemy_view {
                                enemy.revealed_segments = Some(reveal[1 - player].clone());
                            }
                        }
                        ui.status = format!(
                            "Game over. Player {} wins. Revealed board lengths: {}, {}.",
                            winner + 1,
                            reveal[0].len(),
                            reveal[1].len()
                        );
                    }
                    Ok(ServerMessage::Error { message }) => {
                        if message.starts_with("Invalid turn:") {
                            ui.your_turn = true;
                        }
                        ui.status = message;
                    }
                    Err(error) => ui.status = format!("Invalid server response: {error}"),
                }
            }
            _ => {}
        }

        if event::poll(Duration::from_millis(10))? {
            let TerminalEvent::Key(key) = event::read()? else {
                continue;
            };
            match key.code {
                KeyCode::Char('q' | 'Q') => break Ok(()),
                KeyCode::Char('r' | 'R') if ui.player.is_some() => {
                    send(&mut writer, ClientMessage::Ready).await?;
                    ui.status = "Ready; waiting for the other player.".into();
                }
                KeyCode::Char('w' | 'W') => ui.direction = Direction::Up,
                KeyCode::Char('s' | 'S') => ui.direction = Direction::Down,
                KeyCode::Char('a' | 'A') => ui.direction = Direction::Left,
                KeyCode::Char('d' | 'D') => ui.direction = Direction::Right,
                KeyCode::Up => ui.target.y = ui.target.y.saturating_sub(1),
                KeyCode::Down => ui.target.y = ui.target.y.saturating_add(1),
                KeyCode::Left => ui.target.x = ui.target.x.saturating_sub(1),
                KeyCode::Right => ui.target.x = ui.target.x.saturating_add(1),
                KeyCode::Enter if ui.your_turn => {
                    ui.your_turn = false;
                    send(
                        &mut writer,
                        ClientMessage::Turn {
                            dir: ui.direction,
                            target: Some(ui.target),
                        },
                    )
                    .await?;
                }
                KeyCode::Char(' ') if ui.your_turn => {
                    ui.your_turn = false;
                    send(
                        &mut writer,
                        ClientMessage::Turn {
                            dir: ui.direction,
                            target: None,
                        },
                    )
                    .await?;
                }
                _ => {}
            }
            clamp_target(&mut ui);
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

async fn send<W>(writer: &mut W, message: ClientMessage) -> Result<(), Box<dyn Error>>
where
    W: futures_util::Sink<Message> + Unpin,
    W::Error: Error + 'static,
{
    writer
        .send(Message::Text(serde_json::to_string(&message)?.into()))
        .await?;
    Ok(())
}

fn clamp_target(ui: &mut Ui) {
    if let Some(board) = &ui.your_board {
        ui.target.x = ui.target.x.min(board.width.saturating_sub(1));
        ui.target.y = ui.target.y.min(board.height.saturating_sub(1));
    }
}

fn draw(frame: &mut ratatui::Frame<'_>, ui: &Ui) {
    let chunks = Layout::default()
        .direction(LayoutDirection::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(frame.area());
    let boards = Layout::default()
        .direction(LayoutDirection::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[0]);
    let own = ui
        .your_board
        .as_ref()
        .map(|board| board_text(board, ui.target, false, ui.direction))
        .unwrap_or_else(|| "Waiting for both players to ready up.".into());
    let enemy = ui
        .enemy_view
        .as_ref()
        .map(|view| enemy_text(view, ui.target))
        .unwrap_or_else(|| "Enemy board is hidden.".into());
    frame.render_widget(
        Paragraph::new(own).block(Block::default().title("Your board").borders(Borders::ALL)),
        boards[0],
    );
    frame.render_widget(
        Paragraph::new(enemy).block(
            Block::default()
                .title("Enemy fog of war")
                .borders(Borders::ALL),
        ),
        boards[1],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "Room: {} | {} | Direction: {:?} | WASD steer, arrows aim, Enter fire, Space skip shot, R ready, Q quit",
            ui.room,
            ui.status,
            ui.direction
        ))
        .block(Block::default().borders(Borders::ALL)),
        chunks[1],
    );
}

fn heading(board: &BoardView) -> Option<Direction> {
    let [head, neck, ..] = board.your_segments[..] else {
        return None;
    };
    match (
        i32::from(head.x) - i32::from(neck.x),
        i32::from(head.y) - i32::from(neck.y),
    ) {
        (0, -1) => Some(Direction::Up),
        (0, 1) => Some(Direction::Down),
        (-1, 0) => Some(Direction::Left),
        (1, 0) => Some(Direction::Right),
        _ => None,
    }
}

fn board_text(board: &BoardView, target: Coord, show_target: bool, direction: Direction) -> String {
    let mut rows = Vec::new();
    for y in 0..board.height {
        let mut row = String::new();
        for x in 0..board.width {
            let point = Coord { x, y };
            let mark = if show_target && point == target {
                '+'
            } else if board.your_segments.first() == Some(&point) {
                match direction {
                    Direction::Up => '^',
                    Direction::Down => 'v',
                    Direction::Left => '<',
                    Direction::Right => '>',
                }
            } else if board.your_segments.contains(&point) {
                'o'
            } else if board.incoming_shots.contains(&point) {
                'x'
            } else {
                '.'
            };
            row.push(mark);
            row.push(' ');
        }
        rows.push(row);
    }
    rows.join("\n")
}

fn enemy_text(view: &EnemyView, target: Coord) -> String {
    let mut rows = Vec::new();
    for y in 0..view.height {
        let mut row = String::new();
        for x in 0..view.width {
            let point = Coord { x, y };
            let mark = if point == target {
                '+'
            } else if view
                .revealed_segments
                .as_ref()
                .is_some_and(|segments| segments.first() == Some(&point))
            {
                '@'
            } else if view
                .revealed_segments
                .as_ref()
                .is_some_and(|segments| segments.contains(&point))
            {
                'o'
            } else {
                match view
                    .shots
                    .iter()
                    .find(|shot| shot.target == point)
                    .map(|shot| shot.outcome)
                {
                    None => '.',
                    Some(ShotResult::Miss) => '-',
                    Some(ShotResult::Sever(length)) => {
                        char::from_digit(length.min(9) as u32, 10).unwrap_or('#')
                    }
                }
            };
            row.push(mark);
            row.push(' ');
        }
        rows.push(row);
    }
    rows.join("\n")
}

fn outcome_text(outcome: ShotResult) -> String {
    match outcome {
        ShotResult::Miss => "Miss".into(),
        ShotResult::Sever(length) => format!("Sever({length})"),
    }
}
