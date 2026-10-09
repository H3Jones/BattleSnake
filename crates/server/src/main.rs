use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::get,
    Router,
};
use battlesnake_ai::Bot;
use battlesnake_core::{apply_turn, Config, Event, GameState, Turn, Winner};
use battlesnake_proto::{BoardView, ClientMessage, EnemyView, ServerMessage, ShotResult};
use futures_util::{SinkExt, StreamExt};
use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    net::TcpListener,
    sync::{mpsc, Mutex, Notify},
    time::{sleep, sleep_until, Duration, Instant},
};

#[derive(Clone)]
struct AppState {
    rooms: Arc<Mutex<HashMap<String, Arc<Mutex<Room>>>>>,
    config: Config,
}

struct Client {
    tx: mpsc::UnboundedSender<Message>,
}

struct Room {
    clients: [Option<Client>; 2],
    bots: [Option<Bot>; 2],
    ready: [bool; 2],
    game: Option<GameState>,
    deadline: Option<Instant>,
    wake: Arc<Notify>,
}

impl Room {
    fn new() -> Self {
        Self {
            clients: [None, None],
            bots: [None, None],
            ready: [false, false],
            game: None,
            deadline: None,
            wake: Arc::new(Notify::new()),
        }
    }

    fn send(&self, player: usize, message: &ServerMessage) {
        if let Some(client) = &self.clients[player] {
            if let Ok(json) = serde_json::to_string(message) {
                let _ = client.tx.send(Message::Text(json.into()));
            }
        }
    }

    fn broadcast(&self, message: &ServerMessage) {
        for player in 0..2 {
            self.send(player, message);
        }
    }

    fn begin_if_ready(&mut self, config: &Config) -> bool {
        if self.game.is_some() || !self.ready.iter().all(|ready| *ready) {
            return false;
        }
        let Ok(game) = GameState::new(config.clone(), rand::random()) else {
            return false;
        };
        for player in 0..2 {
            self.send(
                player,
                &ServerMessage::Start {
                    your_board: board_view(&game, player),
                    enemy_view: EnemyView {
                        width: config.width,
                        height: config.height,
                        shots: Vec::new(),
                        revealed_segments: None,
                    },
                },
            );
        }
        self.game = Some(game);
        self.set_next_deadline(config);
        self.notify_turn();
        true
    }

    fn set_next_deadline(&mut self, config: &Config) {
        self.deadline =
            Some(Instant::now() + std::time::Duration::from_secs(config.turn_timer_secs));
    }

    fn notify_turn(&self) {
        let Some(game) = &self.game else {
            return;
        };
        if game.winner.is_some() {
            return;
        }
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let deadline = seconds
            + self
                .deadline
                .map(|time| time.saturating_duration_since(Instant::now()).as_secs())
                .unwrap_or_default();
        self.send(game.current_player, &ServerMessage::YourTurn { deadline });
    }

    fn finish_if_game_over(&self) {
        let Some(game) = &self.game else {
            return;
        };
        let Some(Winner::Player(winner)) = game.winner else {
            return;
        };
        let reveal = std::array::from_fn(|player| game.snakes[player].segments.clone());
        self.broadcast(&ServerMessage::GameOver { winner, reveal });
    }

    fn apply(&mut self, player: usize, turn: Turn, config: &Config, timed_out: bool) {
        self.wake.notify_one();
        let Some(game) = &mut self.game else {
            self.send(
                player,
                &ServerMessage::Error {
                    message: "Game has not started".into(),
                },
            );
            return;
        };
        if game.current_player != player || game.winner.is_some() {
            self.send(
                player,
                &ServerMessage::Error {
                    message: "It is not your turn".into(),
                },
            );
            return;
        }
        let result = {
            let game = self.game.as_mut().expect("game checked above");
            let old_shot_count = game.shots_fired[player].len();
            apply_turn(game, turn).map(|events| {
                let shooter_outcome = events.iter().find_map(|event| match event {
                    Event::Shot { outcome, .. } => Some(ShotResult::from(*outcome)),
                    _ => None,
                });
                let shot_at = (game.shots_fired[player].len() > old_shot_count)
                    .then_some(turn.target)
                    .flatten();
                let head_hit = events
                    .iter()
                    .any(|event| matches!(event, Event::HeadHit { .. }));
                (
                    shot_at,
                    shooter_outcome,
                    head_hit,
                    game.winner.is_some(),
                    [board_view(game, 0), board_view(game, 1)],
                )
            })
        };
        match result {
            Ok((shot_at, shooter_outcome, head_hit, game_over, boards)) => {
                if !head_hit {
                    for recipient in 0..2 {
                        self.send(
                            recipient,
                            &ServerMessage::TurnResult {
                                shot_at: if recipient == player { shot_at } else { None },
                                outcome: if recipient == player {
                                    shooter_outcome
                                } else {
                                    None
                                },
                                timed_out,
                                your_board: boards[recipient].clone(),
                            },
                        );
                    }
                }
                if game_over {
                    self.deadline = None;
                    self.finish_if_game_over();
                } else {
                    self.set_next_deadline(config);
                    self.notify_turn();
                }
            }
            Err(error) => self.send(
                player,
                &ServerMessage::Error {
                    message: format!("Invalid turn: {error:?}"),
                },
            ),
        }
        if self.game.as_ref().is_some_and(|game| game.winner.is_none())
            && self
                .game
                .as_ref()
                .is_some_and(|game| game.current_player == player)
        {
            self.notify_turn();
        }
    }
}

fn board_view(game: &GameState, player: usize) -> BoardView {
    BoardView {
        width: game.config.width,
        height: game.config.height,
        your_segments: game.snakes[player].segments.clone(),
        incoming_shots: game.shots_fired[1 - player].iter().copied().collect(),
    }
}

#[tokio::main]
async fn main() {
    let bind = std::env::args()
        .skip_while(|arg| arg != "--bind")
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:7777".to_owned());
    let address: SocketAddr = bind.parse().expect("invalid --bind address");
    let state = AppState {
        rooms: Arc::new(Mutex::new(HashMap::new())),
        config: Config::default(),
    };
    let web_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../web");
    let app = Router::new()
        .route("/ws", get(websocket))
        .fallback_service(tower_http::services::ServeDir::new(web_dir))
        .with_state(state);
    let listener = TcpListener::bind(address).await.expect("failed to bind");
    println!("BattleSnake server listening on ws://{address}/ws");
    axum::serve(listener, app).await.expect("server failed");
}

async fn websocket(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut writer, mut reader) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
    tokio::spawn(async move {
        while let Some(message) = rx.recv().await {
            if writer.send(message).await.is_err() {
                break;
            }
        }
    });

    let Some(Ok(Message::Text(first))) = reader.next().await else {
        return;
    };
    let Ok(ClientMessage::Join {
        room,
        name: _,
        vs_bot,
    }) = serde_json::from_str(&first)
    else {
        send_error(&tx, "First message must be Join");
        return;
    };
    let room_code = if room.trim().is_empty() {
        thread_rng()
            .sample_iter(&Alphanumeric)
            .take(6)
            .map(char::from)
            .collect::<String>()
            .to_uppercase()
    } else {
        room.trim().to_uppercase()
    };

    let room_handle = {
        let mut rooms = state.rooms.lock().await;
        rooms
            .entry(room_code.clone())
            .or_insert_with(|| Arc::new(Mutex::new(Room::new())))
            .clone()
    };

    let player = {
        let mut room = room_handle.lock().await;
        let Some(player) = room.clients.iter().position(Option::is_none) else {
            send_error(&tx, "Room is full");
            return;
        };
        room.clients[player] = Some(Client { tx: tx.clone() });
        if vs_bot && player == 0 && room.clients[1].is_none() {
            // The bot's outbound channel is closed; messages to it are dropped.
            let (bot_tx, _) = mpsc::unbounded_channel::<Message>();
            room.clients[1] = Some(Client { tx: bot_tx });
            room.bots[1] = Some(Bot::default());
            room.ready[1] = true;
        }
        room.send(
            player,
            &ServerMessage::Joined {
                room: room_code.clone(),
                player,
            },
        );
        player
    };

    loop {
        let Some(Ok(message)) = reader.next().await else {
            break;
        };
        let Message::Text(text) = message else {
            continue;
        };
        let Ok(message) = serde_json::from_str::<ClientMessage>(&text) else {
            send_error(&tx, "Malformed client message");
            continue;
        };
        match message {
            ClientMessage::Join { .. } => send_error(&tx, "Already joined"),
            ClientMessage::Ready => {
                let started = {
                    let mut room = room_handle.lock().await;
                    room.ready[player] = true;
                    room.begin_if_ready(&state.config)
                };
                if started {
                    spawn_timeout(room_handle.clone(), state.config.clone());
                }
            }
            ClientMessage::Turn { dir, target } => {
                room_handle
                    .lock()
                    .await
                    .apply(player, Turn { dir, target }, &state.config, false);
            }
            ClientMessage::Resign => {
                resign(&room_handle, player).await;
                break;
            }
        }
    }

    let mut room = room_handle.lock().await;
    if room.game.as_ref().is_some_and(|game| game.winner.is_none()) {
        resign_locked(&mut room, player);
    }
    room.clients[player] = None;
}

fn spawn_timeout(room: Arc<Mutex<Room>>, config: Config) {
    tokio::spawn(async move {
        loop {
            let (deadline, wake, bot_to_move) = {
                let room = room.lock().await;
                let Some(game) = &room.game else { return };
                if game.winner.is_some() {
                    return;
                }
                let Some(deadline) = room.deadline else {
                    return;
                };
                (deadline, room.wake.clone(), room.bots[game.current_player])
            };
            if let Some(bot) = bot_to_move {
                // Short pause so the bot's move doesn't feel instantaneous.
                sleep(Duration::from_millis(700)).await;
                let mut room = room.lock().await;
                if room.deadline != Some(deadline) {
                    continue;
                }
                let Some(game) = &room.game else { return };
                if game.winner.is_some() {
                    return;
                }
                let player = game.current_player;
                let turn = bot.choose_turn(game, player, &mut thread_rng());
                room.apply(player, turn, &config, false);
                continue;
            }
            tokio::select! {
                _ = sleep_until(deadline) => {}
                _ = wake.notified() => continue,
            }
            let mut room = room.lock().await;
            if room.deadline != Some(deadline) {
                continue;
            }
            let Some(game) = &room.game else { return };
            if game.winner.is_some() {
                return;
            }
            let player = game.current_player;
            let direction = game.snakes[player].direction;
            room.apply(
                player,
                Turn {
                    dir: direction,
                    target: None,
                },
                &config,
                true,
            );
        }
    });
}

async fn resign(handle: &Arc<Mutex<Room>>, player: usize) {
    let mut room = handle.lock().await;
    resign_locked(&mut room, player);
}

fn resign_locked(room: &mut Room, player: usize) {
    let Some(game) = &mut room.game else { return };
    if game.winner.is_none() {
        game.winner = Some(Winner::Player(1 - player));
        room.deadline = None;
        room.wake.notify_one();
        room.finish_if_game_over();
    }
}

fn send_error(tx: &mpsc::UnboundedSender<Message>, message: &str) {
    if let Ok(json) = serde_json::to_string(&ServerMessage::Error {
        message: message.into(),
    }) {
        let _ = tx.send(Message::Text(json.into()));
    }
}
