use battlesnake_core::{Coord, Direction, ShotOutcome};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMessage {
    Join {
        room: String,
        name: String,
        #[serde(default)]
        vs_bot: bool,
    },
    Ready,
    Turn {
        dir: Direction,
        target: Option<Coord>,
    },
    Resign,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMessage {
    Joined {
        room: String,
        player: usize,
    },
    Start {
        your_board: BoardView,
        enemy_view: EnemyView,
    },
    YourTurn {
        deadline: u64,
    },
    TurnResult {
        shot_at: Option<Coord>,
        outcome: Option<ShotResult>,
        timed_out: bool,
        your_board: BoardView,
    },
    GameOver {
        winner: usize,
        reveal: [Vec<Coord>; 2],
    },
    Error {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoardView {
    pub width: u16,
    pub height: u16,
    pub your_segments: Vec<Coord>,
    pub incoming_shots: Vec<Coord>,
}

/// Only shot cells and the public outcome are disclosed; enemy segments are not.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnemyView {
    pub width: u16,
    pub height: u16,
    pub shots: Vec<ShotRecord>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revealed_segments: Option<Vec<Coord>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShotRecord {
    pub target: Coord,
    pub outcome: ShotResult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", content = "length", rename_all = "snake_case")]
pub enum ShotResult {
    Miss,
    Sever(usize),
}

impl From<ShotOutcome> for ShotResult {
    fn from(value: ShotOutcome) -> Self {
        match value {
            ShotOutcome::Miss => Self::Miss,
            ShotOutcome::Sever(length) => Self::Sever(length),
        }
    }
}
