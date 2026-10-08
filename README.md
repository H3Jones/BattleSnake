# BattleSnake

BattleSnake is a small two-player game combining Battleships-style hidden shots
with snake movement. It is a Cargo workspace with a deterministic Rust rules
engine, JSON WebSocket protocol, authoritative server, and a minimal terminal
client. The `web/` directory holds a static browser client served by the
server at `/` (open `http://127.0.0.1:7777`).

## Rules and architecture

- Players take alternating turns on separate 10x10 boards.
- The default starting lengths are 3 for player 1 and 4 for player 2.
- On a turn, a player moves one cell and then may fire once at the opponent's
  board. Moving into a wall or your own body loses before a shot is resolved.
- The tail vacates during ordinary movement, so moving into its old cell is
  allowed; it is not allowed on a growth move. Reversing directly into the neck
  is prohibited.
- Every third move by that snake grows it by one segment.
- Hitting a non-head segment severs that segment and all segments behind it.
  Only the shooter learns `Miss` or `Sever(n)`; a head hit immediately ends the
  game. Each player can only fire at a given cell once.
- A 30-second turn timeout moves straight ahead without firing.
- Each client sees their own board and only the publicly disclosed results on
  the opponent board. Both full boards are revealed at game end.

`crates/core` has no IO and uses a seeded RNG for reproducible initial
placement. `crates/proto` defines the serde wire messages. `crates/server`
owns room state and exposes `/ws`; `crates/tui` is a basic interactive client.

## Build and test

```sh
cargo build
cargo test
```

## Run locally

Start the server:

```sh
cargo run -p battlesnake-server
```

It listens on `127.0.0.1:7777` by default. The WebSocket endpoint is
`ws://127.0.0.1:7777/ws`.

In two terminals, start a TUI client for the same room:

```sh
cargo run -p battlesnake-tui -- ws://127.0.0.1:7777/ws ROOM Alice
cargo run -p battlesnake-tui -- ws://127.0.0.1:7777/ws ROOM Bob
```

Use the same room code for both players (or omit it for an automatically
generated room, then share the code shown by the first client). Press `R` when
ready. WASD steers, arrow keys aim, Enter moves and fires, Space moves without
firing, and Q exits.

## Run over Tailscale

Run the server on the host with its Tailscale interface reachable. Bind it to
all interfaces with:

```sh
cargo run -p battlesnake-server -- --bind 0.0.0.0:7777
```

Connect from another tailnet device using the host's MagicDNS name and the
WebSocket path, for example:

```sh
cargo run -p battlesnake-tui -- ws://my-server:7777/ws ROOM Alice
```

Allow TCP port 7777 through any host firewall. The connection is plain
WebSocket (`ws://`); use it only on a trusted network such as your tailnet.
