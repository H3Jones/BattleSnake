# Web client

Plain HTML/CSS/JS, no build step. The Rust server serves this directory at `/`
and the game WebSocket at `/ws`, so the page connects to `ws://<host>/ws`
on the same origin.

Run `cargo run -p battlesnake-server` and open `http://127.0.0.1:7777`
(or `http://<magicdns>:7777` over Tailscale).

Controls: WASD/arrows steer, click the enemy board to aim, Enter moves and
fires, Space moves without firing.
