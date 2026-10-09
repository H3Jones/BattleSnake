//! Bot opponent. Only uses information a human player would have: its own
//! snake and the cells it has already fired at.

use battlesnake_core::{Coord, Direction, GameState, Turn};
use rand::seq::SliceRandom;
use rand::Rng;

const ALL_DIRECTIONS: [Direction; 4] = [
    Direction::Up,
    Direction::Down,
    Direction::Left,
    Direction::Right,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum MovementBehaviour {
    /// Prefer the direction with the longest unobstructed line of sight.
    #[default]
    Longest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShootingBehaviour {
    /// Fire at a uniformly random cell not yet shot at.
    #[default]
    Random,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Bot {
    pub movement: MovementBehaviour,
    pub shooting: ShootingBehaviour,
}

impl Bot {
    pub fn choose_turn(&self, state: &GameState, player: usize, rng: &mut impl Rng) -> Turn {
        let dir = match self.movement {
            MovementBehaviour::Longest => longest_direction(state, player, rng),
        };
        let target = match self.shooting {
            ShootingBehaviour::Random => random_target(state, player, rng),
        };
        Turn { dir, target }
    }
}

fn opposite(dir: Direction) -> Direction {
    match dir {
        Direction::Up => Direction::Down,
        Direction::Down => Direction::Up,
        Direction::Left => Direction::Right,
        Direction::Right => Direction::Left,
    }
}

/// Number of free cells in a straight line from the head in `dir`.
fn line_of_sight(state: &GameState, player: usize, dir: Direction) -> usize {
    let snake = &state.snakes[player];
    let grows = (snake.moves_taken() + 1) % state.config.growth_every_moves == 0;
    // The tail vacates on a non-growing move, so it is not an obstacle.
    let blocked_len = if grows {
        snake.segments.len()
    } else {
        snake.segments.len() - 1
    };
    let blocked = &snake.segments[..blocked_len];
    let (dx, dy) = dir.delta();
    let mut x = i32::from(snake.segments[0].x);
    let mut y = i32::from(snake.segments[0].y);
    let mut free = 0;
    loop {
        x += dx;
        y += dy;
        if x < 0
            || y < 0
            || x >= i32::from(state.config.width)
            || y >= i32::from(state.config.height)
        {
            break;
        }
        let cell = Coord {
            x: x as u16,
            y: y as u16,
        };
        if blocked.contains(&cell) {
            break;
        }
        free += 1;
    }
    free
}

fn longest_direction(state: &GameState, player: usize, rng: &mut impl Rng) -> Direction {
    let current = state.snakes[player].direction;
    let scored: Vec<(Direction, usize)> = ALL_DIRECTIONS
        .into_iter()
        .filter(|&dir| dir != opposite(current))
        .map(|dir| (dir, line_of_sight(state, player, dir)))
        .collect();
    let best = scored.iter().map(|&(_, score)| score).max().unwrap_or(0);
    let candidates: Vec<Direction> = scored
        .iter()
        .filter(|&&(_, score)| score == best)
        .map(|&(dir, _)| dir)
        .collect();
    candidates.choose(rng).copied().unwrap_or(current)
}

fn random_target(state: &GameState, player: usize, rng: &mut impl Rng) -> Option<Coord> {
    let mut open = Vec::new();
    for y in 0..state.config.height {
        for x in 0..state.config.width {
            let cell = Coord { x, y };
            if !state.shots_fired[player].contains(&cell) {
                open.push(cell);
            }
        }
    }
    open.choose(rng).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use battlesnake_core::{apply_turn, Config};
    use rand::rngs::StdRng;
    use rand::SeedableRng;

    #[test]
    fn prefers_longest_line_of_sight() {
        let mut state = GameState::new(Config::default(), 1).unwrap();
        // Heading up from (8,5): up has 5 free cells, left 8, right 1.
        state.snakes[0].segments = vec![
            Coord { x: 8, y: 5 },
            Coord { x: 8, y: 6 },
            Coord { x: 8, y: 7 },
        ];
        state.snakes[0].direction = Direction::Up;
        let mut rng = StdRng::seed_from_u64(7);
        let turn = Bot::default().choose_turn(&state, 0, &mut rng);
        assert_eq!(turn.dir, Direction::Left);
    }

    #[test]
    fn plays_only_valid_turns() {
        for seed in 0..50 {
            let mut state = GameState::new(Config::default(), seed).unwrap();
            let mut rng = StdRng::seed_from_u64(seed);
            let bot = Bot::default();
            for _ in 0..40 {
                if state.winner.is_some() {
                    break;
                }
                let player = state.current_player;
                let turn = bot.choose_turn(&state, player, &mut rng);
                assert!(apply_turn(&mut state, turn).is_ok(), "seed {seed}");
            }
        }
    }

    #[test]
    fn only_shoots_unshot_cells() {
        let mut state = GameState::new(Config::default(), 3).unwrap();
        let mut rng = StdRng::seed_from_u64(3);
        for y in 0..10 {
            for x in 0..10 {
                if (x, y) != (4, 4) {
                    state.shots_fired[1].insert(Coord { x, y });
                }
            }
        }
        assert_eq!(
            random_target(&state, 1, &mut rng),
            Some(Coord { x: 4, y: 4 })
        );
        state.shots_fired[1].insert(Coord { x: 4, y: 4 });
        assert_eq!(random_target(&state, 1, &mut rng), None);
    }
}
