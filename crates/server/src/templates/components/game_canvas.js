// Game canvas rendering, input handling, and game loop
// All game state is managed by the deterministic WASM engine.
// This JS layer handles: input capture, rendering, and input recording.

let engine = null;
let recorder = null;
let sessionId = null;
let gameInterval = null;
let gameSeed = null;
let pendingGameStart = false;

const FRAME_MS = 1000 / 60; // Fixed 60fps for determinism
const TIMING_SAMPLE_INTERVAL = 60; // Sample timing every 60 frames (once per second)

// Input state — tracks which keys are currently held
const keys = { thrust: false, left: false, right: false, shoot: false };

// Game timing
let gameStartTime = 0;
let frameCounter = 0;
let lastTimingSample = 0;
let timingSamples = [];

// DOM elements
let canvas, ctx, scoreElement, levelElement, timeElement, livesElement;
let gameOverDialog, finalScoreElement, restartButton;

// Sound effects — synthesized via Web Audio API (no external files needed)
let audioCtx = null;
function getAudioCtx() {
    if (!audioCtx) {
        try { audioCtx = new (window.AudioContext || window.webkitAudioContext)(); }
        catch (e) { /* audio not available */ }
    }
    return audioCtx;
}

const sounds = {
    explosion: function() {
        const ctx = getAudioCtx();
        if (!ctx) return;
        // White noise burst with low-pass filter for an explosion effect
        const duration = 0.4;
        const buf = ctx.createBuffer(1, ctx.sampleRate * duration, ctx.sampleRate);
        const data = buf.getChannelData(0);
        for (let i = 0; i < data.length; i++) {
            data[i] = (Math.random() * 2 - 1) * (1 - i / data.length);
        }
        const src = ctx.createBufferSource();
        src.buffer = buf;
        const filter = ctx.createBiquadFilter();
        filter.type = "lowpass";
        filter.frequency.setValueAtTime(1000, ctx.currentTime);
        filter.frequency.exponentialRampToValueAtTime(200, ctx.currentTime + duration);
        const gain = ctx.createGain();
        gain.gain.setValueAtTime(0.3, ctx.currentTime);
        gain.gain.exponentialRampToValueAtTime(0.01, ctx.currentTime + duration);
        src.connect(filter);
        filter.connect(gain);
        gain.connect(ctx.destination);
        src.start();
    },
};

function playSound(sound) {
    if (typeof sound === "function") { try { sound(); } catch (e) { /* optional */ } }
}

function initializeElements() {
    console.log("Initializing game elements");
    canvas = document.getElementById("gameCanvas");
    if (!canvas) {
        console.warn("Game canvas element not found - may not be on game page");
        return false;
    }
    ctx = canvas.getContext("2d");
    scoreElement = document.getElementById("score");
    levelElement = document.getElementById("level");
    timeElement = document.getElementById("time");
    livesElement = document.getElementById("lives");
    gameOverDialog = document.getElementById("game-over-dialog");
    finalScoreElement = document.getElementById("final-score");
    restartButton = document.getElementById("restart-button");

    if (restartButton) {
        restartButton.addEventListener("click", function () {
            if (gameOverDialog) gameOverDialog.style.display = "none";
            if (practiceMode) {
                startPracticeMode();
            } else {
                startGame();
            }
        });
    }

    var playForRealBtn = document.getElementById("play-for-real-button");
    if (playForRealBtn) {
        playForRealBtn.addEventListener("click", function () {
            if (gameOverDialog) gameOverDialog.style.display = "none";
            practiceMode = false;
            startGame();
        });
    }

    console.log("Game elements initialized successfully");
    return true;
}

// Practice mode — play without login, payment, or score submission
let practiceMode = false;

function startPracticeMode() {
    console.log("Starting practice mode...");
    practiceMode = true;

    // Generate a random seed locally
    const seedHex = Array.from(crypto.getRandomValues(new Uint8Array(8)))
        .map(b => b.toString(16).padStart(2, "0")).join("");

    // Use default engine config (embedded by server in page)
    const engineConfig = window.DEFAULT_ENGINE_CONFIG || "{}";

    startGameWithConfig({
        config: {
            sessionId: "practice_" + Date.now(),
            seed: seedHex,
            engineConfig: engineConfig,
        },
        plays_remaining: 999,
    });
}

function startGame() {
    if (!window.gameAuth || !window.gameAuth.isLoggedIn()) {
        const loginModal = document.getElementById("loginModal");
        if (loginModal) loginModal.classList.add("is-active");
        return;
    }

    practiceMode = false;
    console.log("Starting game...");
    const startGameBtn = document.getElementById("startGameBtn");
    if (startGameBtn) {
        startGameBtn.disabled = true;
        startGameBtn.textContent = "Loading...";
    }

    window.initializePaymentHandler()
        .then((paymentHandler) => {
            console.log("Payment handler ready, requesting game session");
            return paymentHandler.requestGameSession();
        })
        .then((result) => {
            if (startGameBtn) {
                startGameBtn.disabled = false;
                startGameBtn.textContent = "Start Game";
            }
            if (result && result.success) {
                startGameWithConfig(result.data);
            } else if (result && result.requiresPayment) {
                console.log("Waiting for payment to complete...");
                pendingGameStart = true;
            } else {
                console.error("Failed to start game:", result ? result.error : "Unknown error");
                alert("Failed to start game. Please try again.");
            }
        })
        .catch((error) => {
            console.error("Error starting game:", error);
            if (startGameBtn) {
                startGameBtn.disabled = false;
                startGameBtn.textContent = "Start Game";
            }
        });
}

function startGameWithConfig(sessionData) {
    console.log("Starting game with config:", sessionData);

    const startScreen = document.getElementById("start-screen");
    if (startScreen) startScreen.style.display = "none";
    const gameContainer = document.querySelector(".game-container");
    if (gameContainer) gameContainer.style.removeProperty("display");
    document.body.classList.add("game-active");

    sessionId = sessionData.config.sessionId || sessionData.config.session_id;

    // Parse seed from hex string into two u32 halves for WASM
    const seedHex = sessionData.config.seed || "0000000000000000";
    gameSeed = seedHex;
    const seedHigh = parseInt(seedHex.substring(0, 8), 16) >>> 0;
    const seedLow = parseInt(seedHex.substring(8, 16), 16) >>> 0;

    // Get engine config (snake_case JSON from server)
    const engineConfig = sessionData.config.engineConfig || sessionData.config.engine_config;
    const configJson = typeof engineConfig === "string" ? engineConfig : JSON.stringify(engineConfig);

    console.log("Creating WASM engine with seed:", seedHex);

    try {
        engine = new window.GameEngine(seedHigh, seedLow, configJson);
        recorder = new window.InputRecorder();
    } catch (e) {
        console.error("Failed to create WASM game engine:", e);
        alert("Failed to initialize game engine.");
        return;
    }

    if (!canvas || !ctx) {
        if (!initializeElements()) {
            console.error("Cannot start game: Canvas element not found");
            return;
        }
    }

    gameStartTime = Date.now();
    frameCounter = 0;
    lastTimingSample = 0;
    timingSamples = [];
    pendingGameStart = false;

    // Show/hide practice mode indicator
    const practiceIndicator = document.getElementById("practiceModeIndicator");
    if (practiceIndicator) {
        practiceIndicator.style.display = practiceMode ? "block" : "none";
    }

    // Start the fixed-timestep game loop
    if (gameInterval) clearInterval(gameInterval);
    gameInterval = setInterval(gameTick, FRAME_MS);
}

function gameTick() {
    if (!engine) return;

    // Record input and advance engine
    recorder.record(keys.thrust, keys.left, keys.right, keys.shoot);
    engine.tick(keys.thrust, keys.left, keys.right, keys.shoot);

    // Sample frame timing every TIMING_SAMPLE_INTERVAL frames
    frameCounter++;
    if (frameCounter % TIMING_SAMPLE_INTERVAL === 0) {
        const now = performance.now();
        if (lastTimingSample > 0) {
            // Expected: TIMING_SAMPLE_INTERVAL * FRAME_MS ms (~1000ms)
            const expected = TIMING_SAMPLE_INTERVAL * FRAME_MS;
            const delta = now - lastTimingSample;
            // Offset from expected in microseconds
            const offsetUs = Math.round((delta - expected) * 1000);
            timingSamples.push(Math.max(-32768, Math.min(32767, offsetUs)));
        }
        lastTimingSample = now;
    }

    // Get state and render
    const stateJson = engine.get_state_json();
    const state = JSON.parse(stateJson);

    render(state);

    // Update HUD
    if (scoreElement) scoreElement.textContent = state.score;
    if (levelElement) levelElement.textContent = state.level;
    if (timeElement) timeElement.textContent = Math.floor((Date.now() - gameStartTime) / 1000);
    if (livesElement) livesElement.textContent = "♦ ".repeat(state.lives).trim();

    // Draw phase name at top center
    if (state.phase) {
        ctx.fillStyle = "#888";
        ctx.font = "12px monospace";
        ctx.textAlign = "center";
        ctx.fillText(state.phase, canvas.width / 2, 15);
        ctx.textAlign = "start";
    }

    // Draw active power-up indicator
    if (state.active_power_up) {
        const ap = state.active_power_up;
        const colors = { RapidFire: "#ffff00", Shield: "#00ffff", SpreadShot: "#ff00ff", SpeedBoost: "#ff8800" };
        ctx.fillStyle = colors[ap.power_type] || "#fff";
        ctx.font = "11px monospace";
        const label = ap.power_type === "Shield" ? "SHIELD" : `${ap.power_type.toUpperCase()} ${ap.remaining_secs.toFixed(1)}s`;
        ctx.fillText(label, canvas.width - 150, 15);
    }

    // Draw time bonus flash
    if (state.last_time_bonus > 0 && state.frame % 60 < 30) {
        ctx.fillStyle = "#ffff00";
        ctx.font = "16px monospace";
        ctx.textAlign = "center";
        ctx.fillText(`TIME BONUS +${state.last_time_bonus}`, canvas.width / 2, 35);
        ctx.textAlign = "start";
    }

    // Draw shield circle around ship if active
    if (state.active_power_up && state.active_power_up.power_type === "Shield") {
        ctx.strokeStyle = "#00ffff";
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.arc(state.ship.x, state.ship.y, state.ship.radius * 2, 0, Math.PI * 2);
        ctx.stroke();
    }

    // Check game over
    if (state.game_over) {
        clearInterval(gameInterval);
        gameInterval = null;
        handleGameOver(state);
    }
}

function render(state) {
    if (!ctx) return;

    // Clear
    ctx.fillStyle = "black";
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    // Draw ship
    const ship = state.ship;
    ctx.strokeStyle = ship.invulnerable && Math.floor(Date.now() / 100) % 2 === 0 ? "gray" : "white";
    ctx.lineWidth = 2;
    ctx.beginPath();

    const x1 = ship.x + ship.radius * Math.cos(ship.angle);
    const y1 = ship.y - ship.radius * Math.sin(ship.angle);
    const x2 = ship.x - ship.radius * (Math.cos(ship.angle) + Math.sin(ship.angle));
    const y2 = ship.y + ship.radius * (Math.sin(ship.angle) - Math.cos(ship.angle));
    const x3 = ship.x - ship.radius * (Math.cos(ship.angle) - Math.sin(ship.angle));
    const y3 = ship.y + ship.radius * (Math.sin(ship.angle) + Math.cos(ship.angle));

    ctx.moveTo(x1, y1);
    ctx.lineTo(x2, y2);
    ctx.lineTo(x3, y3);
    ctx.closePath();
    ctx.stroke();

    // Draw thrust flame
    if (ship.thrusting) {
        ctx.beginPath();
        ctx.moveTo(x2, y2);
        const tx1 = ship.x - ship.radius * 1.5 * Math.cos(ship.angle);
        const ty1 = ship.y + ship.radius * 1.5 * Math.sin(ship.angle);
        ctx.lineTo(tx1, ty1);
        ctx.lineTo(x3, y3);
        ctx.strokeStyle = "orange";
        ctx.stroke();
    }

    // Draw bullets
    ctx.fillStyle = "white";
    for (const bullet of state.bullets) {
        ctx.beginPath();
        ctx.arc(bullet.x, bullet.y, bullet.radius, 0, Math.PI * 2);
        ctx.fill();
    }

    // Draw power-ups
    for (const pu of (state.power_ups || [])) {
        const colors = { RapidFire: "#ffff00", Shield: "#00ffff", SpreadShot: "#ff00ff", SpeedBoost: "#ff8800" };
        ctx.fillStyle = colors[pu.power_type] || "#ffffff";
        ctx.beginPath();
        ctx.arc(pu.x, pu.y, pu.radius, 0, Math.PI * 2);
        ctx.fill();
        // Pulsing effect
        ctx.strokeStyle = ctx.fillStyle;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.arc(pu.x, pu.y, pu.radius * 1.5 * (0.8 + 0.2 * Math.sin(Date.now() / 200)), 0, Math.PI * 2);
        ctx.stroke();
    }

    // Draw enemies
    for (const enemy of (state.enemies || [])) {
        const isBoss = enemy.enemy_type === "Boss";
        ctx.strokeStyle = enemy.enemy_type === "Drone" ? "#00ff00"
            : enemy.enemy_type === "Fighter" ? "#ff4444"
            : isBoss ? "#ff00ff"
            : "#ffaa00"; // Bomber
        ctx.lineWidth = 2;
        ctx.beginPath();
        // Diamond shape for enemies
        const er = enemy.radius;
        ctx.moveTo(enemy.x + er * Math.cos(enemy.angle), enemy.y - er * Math.sin(enemy.angle));
        ctx.lineTo(enemy.x + er * 0.6 * Math.cos(enemy.angle + 1.5), enemy.y - er * 0.6 * Math.sin(enemy.angle + 1.5));
        ctx.lineTo(enemy.x - er * Math.cos(enemy.angle), enemy.y + er * Math.sin(enemy.angle));
        ctx.lineTo(enemy.x + er * 0.6 * Math.cos(enemy.angle - 1.5), enemy.y - er * 0.6 * Math.sin(enemy.angle - 1.5));
        ctx.closePath();
        ctx.stroke();

        // Boss HP bar
        if (isBoss) {
            const barWidth = enemy.radius * 2;
            const barHeight = 3;
            const barX = enemy.x - barWidth / 2;
            const barY = enemy.y - enemy.radius - 8;
            ctx.fillStyle = "#333";
            ctx.fillRect(barX, barY, barWidth, barHeight);
            ctx.fillStyle = "#ff00ff";
            // We don't know max HP from render state, so show proportionally
            ctx.fillRect(barX, barY, barWidth * Math.min(enemy.hp / 10, 1), barHeight);
        }
    }

    // Draw enemy bullets
    ctx.fillStyle = "#ff4444";
    for (const eb of (state.enemy_bullets || [])) {
        ctx.beginPath();
        ctx.arc(eb.x, eb.y, eb.radius, 0, Math.PI * 2);
        ctx.fill();
    }

    // Draw asteroids
    ctx.strokeStyle = "white";
    ctx.lineWidth = 2;
    for (const asteroid of state.asteroids) {
        ctx.beginPath();
        for (let j = 0; j < asteroid.vertices; j++) {
            const angle = (j * Math.PI * 2) / asteroid.vertices;
            const offset = asteroid.offsets[j] || 1;
            const ax = asteroid.x + asteroid.radius * offset * Math.cos(angle + asteroid.angle);
            const ay = asteroid.y + asteroid.radius * offset * Math.sin(angle + asteroid.angle);
            if (j === 0) ctx.moveTo(ax, ay);
            else ctx.lineTo(ax, ay);
        }
        ctx.closePath();
        ctx.stroke();
    }
}

async function handleGameOver(state) {
    playSound(sounds.explosion);
    document.body.classList.remove("game-active");

    if (finalScoreElement) finalScoreElement.textContent = state.score;

    if (!practiceMode && window.gameAuth && window.gameAuth.isLoggedIn() && sessionId && recorder) {
        const gameTime = Math.floor((Date.now() - gameStartTime) / 1000);
        const inputLog = recorder.finish(); // Uint8Array
        const frameCount = recorder.frame_count();

        // Compute SHA-256 hash of input log
        const hashBuffer = await crypto.subtle.digest("SHA-256", inputLog);
        const inputHash = Array.from(new Uint8Array(hashBuffer))
            .map((b) => b.toString(16).padStart(2, "0"))
            .join("");

        // Base64 encode the input log
        const inputLogBase64 = btoa(String.fromCharCode(...inputLog));

        // Encode frame timings as Int16Array -> base64
        let frameTimingsB64 = null;
        if (timingSamples.length > 0) {
            const timingBuffer = new Int16Array(timingSamples);
            const timingBytes = new Uint8Array(timingBuffer.buffer);
            frameTimingsB64 = btoa(String.fromCharCode(...timingBytes));
        }

        await submitScore(state.score, state.level, gameTime, inputLogBase64, inputHash, frameCount, frameTimingsB64);
    }

    // In practice mode, show a note that the score wasn't saved
    const gameOverPlays = document.getElementById("gameOverPlaysRemaining");
    if (practiceMode) {
        const status = document.getElementById("scoreSubmissionStatus");
        const retry = document.getElementById("retry-score-button");
        if (status) status.textContent = "";
        if (retry) retry.style.display = "none";
    }
    if (practiceMode && gameOverPlays) {
        gameOverPlays.textContent = "Practice mode — score not submitted";
        gameOverPlays.className = "nes-text is-warning";
        gameOverPlays.style.display = "block";
    }

    // Show "Play for Real" button in practice mode
    var playForRealBtn = document.getElementById("play-for-real-button");
    if (playForRealBtn) {
        playForRealBtn.style.display = practiceMode ? "inline-block" : "none";
    }

    if (gameOverDialog) gameOverDialog.style.display = "block";
}

// Retry only an explicit busy response: network failures and other errors can
// be ambiguous after a write, so they must not automatically resubmit a score.
async function postScoreWithRetry(url, payload) {
    for (let attempt = 0; ; attempt++) {
        const response = await window.gameAuth.post(url, payload);
        if (response.status !== 503 || attempt >= 11) return response;
        const seconds = Number(response.headers.get("Retry-After"));
        const delay = Number.isFinite(seconds) && seconds > 0 ? Math.min(seconds, 5) : 1;
        await new Promise(resolve => setTimeout(resolve, delay * 1000));
    }
}

async function submitScore(score, level, gameTime, inputLog, inputHash, frames, frameTimings) {
    if (!window.gameAuth || !window.gameAuth.isLoggedIn() || !sessionId) {
        console.warn("No session ID available, cannot submit score");
        return;
    }
    // Capture the original session and log for both automatic and manual retry.
    const payload = {
        score, level, play_time: gameTime, session_id: sessionId,
        input_log: inputLog, input_hash: inputHash, frames, frame_timings: frameTimings,
    };
    const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";
    const status = document.getElementById("scoreSubmissionStatus");
    const retry = document.getElementById("retry-score-button");
    async function send() {
        if (retry) retry.style.display = "none";
        if (status) status.textContent = "Saving score…";
        try {
            const response = await postScoreWithRetry(`${apiBase}/api/v1/game/score`, payload);
            if (response.ok) {
                if (status) status.textContent = "Score saved.";
            } else {
                if (status) status.textContent = response.status === 503
                    ? "Score not saved yet. Please try saving again."
                    : "Score could not be verified.";
                if (retry && response.status === 503) {
                    retry.style.display = "inline-block";
                    retry.onclick = send;
                }
                console.error("Score submission rejected:", await response.text());
            }
        } catch (error) {
            if (status) status.textContent = "Could not confirm the save. Check the leaderboard.";
            console.error("Failed to submit score:", error);
        }
    }
    await send();
}

// Keyboard input
document.addEventListener("keydown", function (event) {
    switch (event.key) {
        case "ArrowLeft": keys.left = true; break;
        case "ArrowRight": keys.right = true; break;
        case "ArrowUp": keys.thrust = true; break;
        case " ": keys.shoot = true; event.preventDefault(); break;
    }
});

document.addEventListener("keyup", function (event) {
    switch (event.key) {
        case "ArrowLeft": keys.left = false; break;
        case "ArrowRight": keys.right = false; break;
        case "ArrowUp": keys.thrust = false; break;
        case " ": keys.shoot = false; break;
    }
});

// Touch input
function setupTouchControls() {
    const buttons = [
        { id: "touchLeft", key: "left" },
        { id: "touchRight", key: "right" },
        { id: "touchThrust", key: "thrust" },
        { id: "touchFire", key: "shoot" },
    ];

    for (const { id, key } of buttons) {
        const btn = document.getElementById(id);
        if (!btn) continue;

        btn.addEventListener("touchstart", function (e) {
            e.preventDefault();
            keys[key] = true;
            btn.classList.add("active");
        }, { passive: false });

        btn.addEventListener("touchend", function (e) {
            e.preventDefault();
            keys[key] = false;
            btn.classList.remove("active");
        }, { passive: false });

        btn.addEventListener("touchcancel", function () {
            keys[key] = false;
            btn.classList.remove("active");
        });
    }

    // Prevent page scrolling while game is active
    const gameSection = document.getElementById("game-section");
    if (gameSection) {
        gameSection.addEventListener("touchmove", function (e) {
            if (document.querySelector(".game-container[style*='block']")) {
                e.preventDefault();
            }
        }, { passive: false });
    }
}

// Auth event listeners
window.addEventListener("auth:login", function (event) {
    console.log("Authentication successful", event.detail);
    if (!sessionId) sessionId = event.detail.sessionId;
});

window.addEventListener("auth:logout", function () {
    console.log("User logged out");
    sessionId = null;
    pendingGameStart = false;
    if (gameInterval) { clearInterval(gameInterval); gameInterval = null; }
    engine = null;
    recorder = null;
});

// Setup start game button and practice buttons
function setupStartGameButton() {
    const startGameBtn = document.getElementById("startGameBtn");
    if (startGameBtn) {
        startGameBtn.addEventListener("click", startGame);
    }
    // Practice button on the game page
    const practiceBtn = document.getElementById("practiceBtn");
    if (practiceBtn) {
        practiceBtn.addEventListener("click", startPracticeMode);
    }
    // Practice buttons on the home page — flag practice mode before HTMX navigates to /play
    for (const id of ["homePracticeBtn", "homePracticeBtn2"]) {
        const btn = document.getElementById(id);
        if (btn) {
            btn.addEventListener("click", function() {
                window._startPracticeAfterSwap = true;
            });
        }
    }
}

// Initialize
if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", function () {
        initializeElements();
        setupStartGameButton();
        setupTouchControls();
    });
} else {
    initializeElements();
    setupStartGameButton();
    setupTouchControls();
}

// Re-initialize after HTMX swaps
document.body.addEventListener("htmx:afterSwap", function () {
    initializeElements();
    setupStartGameButton();
    setupTouchControls();

    // Auto-start practice mode if flagged from home page
    if (window._startPracticeAfterSwap && document.getElementById("practiceBtn")) {
        window._startPracticeAfterSwap = false;
        setTimeout(startPracticeMode, 100);
    }
});

// Update plays remaining display
window.updatePlaysRemaining = function(remaining) {
    const display = document.getElementById("playsRemainingDisplay");
    if (display) {
        if (remaining > 0) {
            display.textContent = `Plays remaining: ${remaining}`;
            display.style.display = "block";
        } else {
            display.style.display = "none";
        }
    }
    const gameOverDisplay = document.getElementById("gameOverPlaysRemaining");
    if (gameOverDisplay) {
        if (remaining > 0) {
            gameOverDisplay.textContent = `Plays remaining: ${remaining}`;
            gameOverDisplay.style.display = "block";
        } else {
            gameOverDisplay.textContent = "No plays remaining — payment required for next game";
            gameOverDisplay.style.display = "block";
            gameOverDisplay.className = "nes-text is-warning";
        }
    }
};

// Export for payment handler callback
window.startGameWithConfig = startGameWithConfig;
