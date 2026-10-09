const $ = (id) => document.getElementById(id);
const DIRS = { up: [0, -1], down: [0, 1], left: [-1, 0], right: [1, 0] };
const KEYS = {
  w: "up", arrowup: "up", s: "down", arrowdown: "down",
  a: "left", arrowleft: "left", d: "right", arrowright: "right",
};

let ws, me = null, myTurn = false, dir = "right", target = null;
let own = null, enemy = null, revealed = null;

const ARROWS = { up: "^", down: "v", left: "<", right: ">" };

// Default the selection to the snake's current heading (head minus neck).
function syncDir() {
  const [h, n] = own.your_segments;
  if (!n) return;
  const dx = h.x - n.x, dy = h.y - n.y;
  dir = Object.keys(DIRS).find((k) => DIRS[k][0] === dx && DIRS[k][1] === dy) || dir;
}

const send = (msg) => ws && ws.send(JSON.stringify(msg));
const status = (text) => ($("status").textContent = text);
const same = (a, b) => a.x === b.x && a.y === b.y;

$("join").addEventListener("submit", (e) => {
  e.preventDefault();
  ws = new WebSocket(`ws://${location.host}/ws`);
  ws.onopen = () => send({ type: "join", room: $("room").value, name: $("name").value });
  ws.onclose = () => { status("Disconnected."); myTurn = false; };
  ws.onmessage = (e) => handle(JSON.parse(e.data));
});

$("ready").onclick = () => { send({ type: "ready" }); status("Ready; waiting for opponent."); };
$("resign").onclick = () => send({ type: "resign" });

function handle(m) {
  switch (m.type) {
    case "joined":
      me = m.player;
      $("join").hidden = true;
      $("controls").hidden = false;
      status(`Room ${m.room}, you are player ${me + 1}. Press Ready.`);
      break;
    case "start":
      own = m.your_board;
      enemy = m.enemy_view;
      revealed = null;
      syncDir();
      target = { x: Math.floor(own.width / 2), y: Math.floor(own.height / 2) };
      status("Game started.");
      break;
    case "your_turn":
      myTurn = true;
      status(`Your turn (direction: ${dir}).`);
      break;
    case "turn_result":
      myTurn = false;
      own = m.your_board;
      syncDir();
      if (m.shot_at && m.outcome) {
        enemy.shots.push({ target: m.shot_at, outcome: m.outcome });
        status(m.outcome.result === "miss" ? "Miss." : `Hit! Severed ${m.outcome.length} segment(s).`);
      } else {
        status(m.timed_out ? "Turn timed out." : "Opponent moved.");
      }
      break;
    case "game_over":
      myTurn = false;
      own.your_segments = m.reveal[me];
      revealed = m.reveal[1 - me];
      status(m.winner === me ? "You win!" : "You lose.");
      break;
    case "error":
      if (m.message.startsWith("Invalid turn")) myTurn = true;
      status(m.message);
      break;
  }
  render();
}

function grid(el, w, h, cellFn, onClick) {
  el.style.gridTemplateColumns = `repeat(${w}, 32px)`;
  el.replaceChildren();
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const c = document.createElement("div");
      c.className = "cell";
      cellFn(c, { x, y });
      if (onClick) c.onclick = () => onClick({ x, y });
      el.appendChild(c);
    }
  }
}

function render() {
  if (!own) return;
  grid($("own"), own.width, own.height, (c, p) => {
    const i = own.your_segments.findIndex((s) => same(s, p));
    if (i === 0) { c.classList.add("head"); c.textContent = ARROWS[dir]; }
    else if (i > 0) c.classList.add("body");
    if (own.incoming_shots.some((s) => same(s, p))) { c.classList.add("shot"); c.textContent = "x"; }
  });
  grid($("enemy"), enemy.width, enemy.height, (c, p) => {
    const shot = enemy.shots.find((s) => same(s.target, p));
    if (shot) {
      const miss = shot.outcome.result === "miss";
      c.classList.add(miss ? "miss" : "sever");
      c.textContent = miss ? "-" : shot.outcome.length;
    }
    if (revealed) {
      const i = revealed.findIndex((s) => same(s, p));
      if (i >= 0) c.classList.add(i === 0 ? "head" : "body");
    }
    if (target && same(target, p)) c.classList.add("target");
  }, (p) => { target = p; render(); });
}

document.addEventListener("keydown", (e) => {
  if (e.target.tagName === "INPUT") return;
  const k = e.key.toLowerCase();
  if (KEYS[k]) {
    dir = KEYS[k];
    if (myTurn) status(`Your turn (direction: ${dir}).`);
    render();
    e.preventDefault();
  } else if (myTurn && (e.key === "Enter" || e.key === " ")) {
    myTurn = false;
    send({ type: "turn", dir, target: e.key === "Enter" ? target : null });
    e.preventDefault();
  }
});
