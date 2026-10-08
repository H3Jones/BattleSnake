use rand::seq::SliceRandom;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const PLAYER_COUNT: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Coord {
    pub x: u16,
    pub y: u16,
}

impl Coord {
    fn offset(self, dir: Direction) -> Option<Self> {
        let (dx, dy) = dir.delta();
        let x = i32::from(self.x) + dx;
        let y = i32::from(self.y) + dy;
        if x < 0 || y < 0 || x > i32::from(u16::MAX) || y > i32::from(u16::MAX) {
            return None;
        }
        Some(Self {
            x: x as u16,
            y: y as u16,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

impl Direction {
    pub fn delta(self) -> (i32, i32) {
        match self {
            Self::Up => (0, -1),
            Self::Down => (0, 1),
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
        }
    }

    fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub width: u16,
    pub height: u16,
    pub start_lengths: [usize; PLAYER_COUNT],
    pub growth_every_moves: u32,
    pub turn_timer_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            width: 10,
            height: 10,
            start_lengths: [3, 4],
            growth_every_moves: 3,
            turn_timer_secs: 30,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snake {
    /// Ordered from head to tail.
    pub segments: Vec<Coord>,
    pub direction: Direction,
    moves_taken: u32,
}

impl Snake {
    pub fn moves_taken(&self) -> u32 {
        self.moves_taken
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Winner {
    Player(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameState {
    pub config: Config,
    pub snakes: [Snake; PLAYER_COUNT],
    pub current_player: usize,
    pub winner: Option<Winner>,
    /// Cells each player has already fired at on the opposing board.
    pub shots_fired: [HashSet<Coord>; PLAYER_COUNT],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Turn {
    pub dir: Direction,
    pub target: Option<Coord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShotOutcome {
    Miss,
    Sever(usize),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Moved {
        player: usize,
        head: Coord,
        grew: bool,
    },
    Shot {
        player: usize,
        target: Coord,
        outcome: ShotOutcome,
    },
    HeadHit {
        winner: usize,
        loser: usize,
        target: Coord,
    },
    Collision {
        winner: usize,
        loser: usize,
        at: Option<Coord>,
    },
}

pub type Events = Vec<Event>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    InvalidConfig,
    InvalidPlayer,
    GameOver,
    WallCollision,
    SelfCollision,
    CannotReverse,
    InvalidTarget,
    TargetAlreadyShot,
}

impl GameState {
    /// Creates a game with reproducible, independently seeded snake placement.
    pub fn new(config: Config, seed: u64) -> Result<Self, Error> {
        if config.width == 0
            || config.height == 0
            || config.growth_every_moves == 0
            || config.start_lengths.iter().any(|&length| length == 0)
            || config.width.checked_mul(config.height).is_none()
        {
            return Err(Error::InvalidConfig);
        }
        let mut rng = ChaCha8Rng::seed_from_u64(seed);
        let first = random_snake(&config, config.start_lengths[0], &mut rng)?;
        let second = random_snake(&config, config.start_lengths[1], &mut rng)?;
        Ok(Self {
            config,
            snakes: [first, second],
            current_player: 0,
            winner: None,
            shots_fired: std::array::from_fn(|_| HashSet::new()),
        })
    }
}

fn random_snake(config: &Config, length: usize, rng: &mut ChaCha8Rng) -> Result<Snake, Error> {
    let mut candidates = Vec::new();
    for y in 0..config.height {
        for x in 0..config.width {
            let head = Coord { x, y };
            for direction in [
                Direction::Up,
                Direction::Down,
                Direction::Left,
                Direction::Right,
            ] {
                let mut segments = vec![head];
                let backwards = direction.opposite();
                let mut point = head;
                let mut valid = true;
                for _ in 1..length {
                    let Some(next) = point.offset(backwards) else {
                        valid = false;
                        break;
                    };
                    if next.x >= config.width || next.y >= config.height {
                        valid = false;
                        break;
                    }
                    segments.push(next);
                    point = next;
                }
                if valid {
                    candidates.push((segments, direction));
                }
            }
        }
    }
    let Some((segments, direction)) = candidates.choose(rng).cloned() else {
        return Err(Error::InvalidConfig);
    };
    Ok(Snake {
        segments,
        direction,
        moves_taken: 0,
    })
}

/// Applies the active player's move and optional shot. A move collision is
/// resolved before a shot, so a player that suicides cannot fire that turn.
pub fn apply_turn(state: &mut GameState, turn: Turn) -> Result<Events, Error> {
    if state.current_player >= PLAYER_COUNT {
        return Err(Error::InvalidPlayer);
    }
    if state.winner.is_some() {
        return Err(Error::GameOver);
    }

    let player = state.current_player;
    let old_snake = &state.snakes[player];
    if turn.dir == old_snake.direction.opposite() {
        return Err(Error::CannotReverse);
    }

    if let Some(target) = turn.target {
        if target.x >= state.config.width || target.y >= state.config.height {
            return Err(Error::InvalidTarget);
        }
        if state.shots_fired[player].contains(&target) {
            return Err(Error::TargetAlreadyShot);
        }
    }

    let old_head = old_snake.segments[0];
    let Some(new_head) = old_head.offset(turn.dir) else {
        return resolve_collision(state, player, None);
    };
    if new_head.x >= state.config.width || new_head.y >= state.config.height {
        return resolve_collision(state, player, Some(new_head));
    }

    let grows = (old_snake.moves_taken + 1) % state.config.growth_every_moves == 0;
    let occupied_end = if grows {
        old_snake.segments.len()
    } else {
        old_snake.segments.len().saturating_sub(1)
    };
    if old_snake.segments[..occupied_end].contains(&new_head) {
        return resolve_collision(state, player, Some(new_head));
    }

    let snake = &mut state.snakes[player];
    snake.segments.insert(0, new_head);
    if !grows {
        snake.segments.pop();
    }
    snake.direction = turn.dir;
    snake.moves_taken += 1;

    let mut events = vec![Event::Moved {
        player,
        head: new_head,
        grew: grows,
    }];

    if let Some(target) = turn.target {
        state.shots_fired[player].insert(target);
        let opponent = 1 - player;
        let hit_index = state.snakes[opponent]
            .segments
            .iter()
            .position(|&segment| segment == target);
        let outcome = match hit_index {
            None => ShotOutcome::Miss,
            Some(0) => {
                state.winner = Some(Winner::Player(player));
                events.push(Event::HeadHit {
                    winner: player,
                    loser: opponent,
                    target,
                });
                ShotOutcome::Sever(1)
            }
            Some(index) => {
                let severed = state.snakes[opponent].segments.len() - index;
                state.snakes[opponent].segments.truncate(index);
                ShotOutcome::Sever(severed)
            }
        };
        events.push(Event::Shot {
            player,
            target,
            outcome,
        });
    }

    if state.winner.is_none() {
        state.current_player = 1 - player;
    }
    Ok(events)
}

fn resolve_collision(
    state: &mut GameState,
    loser: usize,
    at: Option<Coord>,
) -> Result<Events, Error> {
    let winner = 1 - loser;
    state.winner = Some(Winner::Player(winner));
    Ok(vec![Event::Collision { winner, loser, at }])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> GameState {
        GameState {
            config: Config::default(),
            snakes: [
                Snake {
                    segments: vec![
                        Coord { x: 4, y: 4 },
                        Coord { x: 3, y: 4 },
                        Coord { x: 2, y: 4 },
                    ],
                    direction: Direction::Right,
                    moves_taken: 0,
                },
                Snake {
                    segments: vec![
                        Coord { x: 7, y: 7 },
                        Coord { x: 6, y: 7 },
                        Coord { x: 5, y: 7 },
                        Coord { x: 4, y: 7 },
                    ],
                    direction: Direction::Right,
                    moves_taken: 0,
                },
            ],
            current_player: 0,
            winner: None,
            shots_fired: std::array::from_fn(|_| HashSet::new()),
        }
    }

    #[test]
    fn severs_hit_segment_and_everything_behind_it() {
        let mut game = state();
        let events = apply_turn(
            &mut game,
            Turn {
                dir: Direction::Right,
                target: Some(Coord { x: 6, y: 7 }),
            },
        )
        .unwrap();
        assert_eq!(game.snakes[1].segments, vec![Coord { x: 7, y: 7 }]);
        assert!(events.contains(&Event::Shot {
            player: 0,
            target: Coord { x: 6, y: 7 },
            outcome: ShotOutcome::Sever(3),
        }));
    }

    #[test]
    fn head_hit_wins_immediately() {
        let mut game = state();
        let events = apply_turn(
            &mut game,
            Turn {
                dir: Direction::Right,
                target: Some(Coord { x: 7, y: 7 }),
            },
        )
        .unwrap();
        assert_eq!(game.winner, Some(Winner::Player(0)));
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::HeadHit { .. })));
    }

    #[test]
    fn a_cell_cannot_be_shot_twice() {
        let mut game = state();
        let target = Coord { x: 8, y: 8 };
        game.shots_fired[0].insert(target);
        let before = game.snakes[0].segments.clone();
        assert_eq!(
            apply_turn(
                &mut game,
                Turn {
                    dir: Direction::Right,
                    target: Some(target),
                },
            ),
            Err(Error::TargetAlreadyShot)
        );
        assert_eq!(game.snakes[0].segments, before);
    }

    #[test]
    fn configured_start_lengths_are_used() {
        let game = GameState::new(Config::default(), 42).unwrap();
        assert_eq!(game.snakes[0].segments.len(), 3);
        assert_eq!(game.snakes[1].segments.len(), 4);
        assert_eq!(game, GameState::new(Config::default(), 42).unwrap());
    }

    #[test]
    fn grows_after_three_own_moves() {
        let mut game = state();
        for dir in [Direction::Right, Direction::Up, Direction::Left] {
            apply_turn(&mut game, Turn { dir, target: None }).unwrap();
            if game.winner.is_some() {
                break;
            }
            game.current_player = 0;
        }
        assert_eq!(game.snakes[0].segments.len(), 4);
    }

    #[test]
    fn wall_and_self_collisions_lose_before_firing() {
        let mut game = state();
        game.snakes[0].segments = vec![
            Coord { x: 0, y: 1 },
            Coord { x: 1, y: 1 },
            Coord { x: 1, y: 0 },
        ];
        game.snakes[0].direction = Direction::Left;
        let event = apply_turn(
            &mut game,
            Turn {
                dir: Direction::Left,
                target: Some(Coord { x: 7, y: 7 }),
            },
        )
        .unwrap();
        assert!(matches!(event[0], Event::Collision { .. }));
        assert!(game.shots_fired[0].is_empty());

        let mut game = state();
        game.snakes[0].segments = vec![
            Coord { x: 4, y: 4 },
            Coord { x: 4, y: 5 },
            Coord { x: 3, y: 5 },
            Coord { x: 3, y: 4 },
        ];
        game.snakes[0].direction = Direction::Right;
        game.snakes[0].moves_taken = 1;
        let event = apply_turn(
            &mut game,
            Turn {
                dir: Direction::Down,
                target: None,
            },
        )
        .unwrap();
        assert!(matches!(event[0], Event::Collision { .. }));
    }

    #[test]
    fn vacated_tail_cell_is_legal_unless_this_move_grows() {
        let mut game = state();
        game.snakes[0].segments = vec![
            Coord { x: 1, y: 1 },
            Coord { x: 1, y: 0 },
            Coord { x: 0, y: 0 },
            Coord { x: 0, y: 1 },
        ];
        game.snakes[0].direction = Direction::Down;
        game.snakes[0].moves_taken = 1;
        apply_turn(
            &mut game,
            Turn {
                dir: Direction::Left,
                target: None,
            },
        )
        .unwrap();
        assert_eq!(game.snakes[0].segments[0], Coord { x: 0, y: 1 });

        let mut game = state();
        game.snakes[0].segments = vec![
            Coord { x: 1, y: 1 },
            Coord { x: 1, y: 0 },
            Coord { x: 0, y: 0 },
            Coord { x: 0, y: 1 },
        ];
        game.snakes[0].direction = Direction::Down;
        game.snakes[0].moves_taken = 2;
        let event = apply_turn(
            &mut game,
            Turn {
                dir: Direction::Left,
                target: None,
            },
        )
        .unwrap();
        assert!(matches!(event[0], Event::Collision { .. }));
    }
}
