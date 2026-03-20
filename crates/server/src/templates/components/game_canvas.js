// Game canvas rendering, input handling, and game loop
// This handles the actual Asteroids game on the canvas element

let gameState = {
    score: 0,
    level: 1,
    startTime: Date.now(),
    gameTime: 0,
};

// Sound effects
const sounds = {
    shoot: new Audio(),
    explosion: new Audio(),
    levelUp: new Audio(),
};

// Game config
let gameConfig = null;
let sessionId = null;
let lastConfigUpdate = 0;
let pendingGameStart = false;

// Game entities
let ship = {};
let asteroids = [];
let bullets = [];

// Game objects
let canvas;
let ctx;
let scoreElement;
let levelElement;
let timeElement;
let gameOverDialog;
let finalScoreElement;
let restartButton;

// Try to load sounds but handle errors gracefully
try {
    sounds.shoot.src = "https://www.soundjay.com/mechanical/sounds/laser-gun-19.mp3";
    sounds.explosion.src = "https://www.soundjay.com/mechanical/sounds/explosion-01.mp3";
    sounds.levelUp.src = "https://www.soundjay.com/mechanical/sounds/beep-07.mp3";
    Object.values(sounds).forEach((sound) => { sound.volume = 0.3; });
} catch (e) {
    console.warn("Error loading sounds:", e);
}

function playSound(sound) {
    if (sound) {
        sound.currentTime = 0;
        sound.play().catch((e) => { console.warn("Could not play audio:", e); });
    }
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
    gameOverDialog = document.getElementById("game-over-dialog");
    finalScoreElement = document.getElementById("final-score");
    restartButton = document.getElementById("restart-button");

    if (restartButton) {
        restartButton.addEventListener("click", function () {
            if (gameOverDialog) gameOverDialog.style.display = "none";
            startGame();
        });
    }

    console.log("Game elements initialized successfully");
    return true;
}

function startGame() {
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
    if (gameContainer) gameContainer.style.display = "block";

    sessionId = sessionData.config.session_id;
    gameConfig = sessionData.config;

    initGame();
    pendingGameStart = false;
}

async function fetchGameConfig() {
    try {
        if (!window.gameAuth || !window.gameAuth.isLoggedIn()) throw new Error("Not logged in");

        const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";
        let url = `${apiBase}/api/v1/game/config`;
        if (sessionId) url += `?session_id=${sessionId}`;

        const response = await window.gameAuth.get(url);
        if (!response.ok) throw new Error(`Error fetching game config: ${response.statusText}`);

        return await response.json();
    } catch (error) {
        console.error("Failed to fetch game config:", error);
    }
}

async function startNewSession() {
    try {
        if (!window.gameAuth || !window.gameAuth.isLoggedIn()) throw new Error("Not logged in");

        const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";
        const response = await window.gameAuth.post(`${apiBase}/api/v1/game/session`);

        if (!response.ok) throw new Error(`Error starting new session: ${response.statusText}`);

        const data = await response.json();
        if (data.config && data.config.sessionId) {
            sessionId = data.config.sessionId;
        }
        return data.config;
    } catch (error) {
        console.error("Failed to start new session:", error);
        return null;
    }
}

async function submitScore(score, level, gameTime) {
    if (!window.gameAuth || !window.gameAuth.isLoggedIn() || !sessionId) {
        console.warn("No session ID available, cannot submit score");
        return;
    }

    try {
        const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";
        const response = await window.gameAuth.post(`${apiBase}/api/v1/game/score`, {
            score: score,
            level: level,
            play_time: gameTime,
            session_id: sessionId,
        });

        if (!response.ok) throw new Error(`Error submitting score: ${response.statusText}`);
        console.log("Score submitted successfully");
    } catch (error) {
        console.error("Failed to submit score:", error);
    }
}

async function initGame() {
    if (!canvas || !ctx) {
        if (!initializeElements()) {
            console.error("Cannot initialize game: Canvas element not found");
            return;
        }
    }

    if (window.gameAuth && window.gameAuth.isLoggedIn()) {
        const newSessionConfig = await startNewSession();
        gameConfig = newSessionConfig || (await fetchGameConfig());
    } else {
        gameConfig = await fetchGameConfig();
    }

    if (!gameConfig) {
        console.error("Cannot start game without config");
        return;
    }

    lastConfigUpdate = Date.now();

    gameState.score = 0;
    gameState.level = 1;
    gameState.startTime = Date.now();
    gameState.gameTime = 0;

    ship = {
        x: canvas.width / 2,
        y: canvas.height / 2,
        radius: gameConfig.ship.radius,
        angle: 0,
        rotation: 0,
        thrusting: false,
        thrust: { x: 0, y: 0 },
        invulnerable: true,
        invulnerableTime: Date.now() + gameConfig.ship.invulnerabilityTime,
        draw: function () {
            ctx.strokeStyle =
                this.invulnerable && Math.floor(Date.now() / 100) % 2 === 0 ? "gray" : "white";
            ctx.lineWidth = 2;
            ctx.beginPath();

            const x1 = this.x + this.radius * Math.cos(this.angle);
            const y1 = this.y - this.radius * Math.sin(this.angle);
            const x2 = this.x - this.radius * (Math.cos(this.angle) + Math.sin(this.angle));
            const y2 = this.y + this.radius * (Math.sin(this.angle) - Math.cos(this.angle));
            const x3 = this.x - this.radius * (Math.cos(this.angle) - Math.sin(this.angle));
            const y3 = this.y + this.radius * (Math.sin(this.angle) + Math.cos(this.angle));

            ctx.moveTo(x1, y1);
            ctx.lineTo(x2, y2);
            ctx.lineTo(x3, y3);
            ctx.closePath();
            ctx.stroke();

            if (this.thrusting) {
                ctx.beginPath();
                ctx.moveTo(x2, y2);
                const tx1 = this.x - this.radius * 1.5 * Math.cos(this.angle);
                const ty1 = this.y + this.radius * 1.5 * Math.sin(this.angle);
                const tx2 = this.x - this.radius * (Math.cos(this.angle) - Math.sin(this.angle));
                const ty2 = this.y + this.radius * (Math.sin(this.angle) + Math.cos(this.angle));
                ctx.lineTo(tx1, ty1);
                ctx.lineTo(tx2, ty2);
                ctx.strokeStyle = "orange";
                ctx.stroke();
            }
        },
    };

    asteroids.length = 0;
    bullets.length = 0;

    if (scoreElement) scoreElement.textContent = gameState.score;
    if (levelElement) levelElement.textContent = gameState.level;
    if (timeElement) timeElement.textContent = gameState.gameTime;

    createAsteroids();
    requestAnimationFrame(update);
}

function createAsteroids() {
    if (!gameConfig || !gameConfig.asteroids) {
        console.error("Cannot create asteroids - no game config available");
        return;
    }

    const count = Math.floor(gameConfig.asteroids.initialCount * Math.sqrt(gameState.level));

    for (let i = 0; i < count; i++) {
        let x, y;
        do {
            x = Math.random() * canvas.width;
            y = Math.random() * canvas.height;
        } while (Math.sqrt(Math.pow(ship.x - x, 2) + Math.pow(ship.y - y, 2)) < 100);

        asteroids.push({
            x: x,
            y: y,
            xv: (Math.random() * 2 - 1) * gameConfig.asteroids.speed * (1 + 0.1 * (gameState.level - 1)),
            yv: (Math.random() * 2 - 1) * gameConfig.asteroids.speed * (1 + 0.1 * (gameState.level - 1)),
            radius: gameConfig.asteroids.size,
            angle: Math.random() * Math.PI * 2,
            vertices: Math.floor(
                Math.random() * (gameConfig.asteroids.vertices.max - gameConfig.asteroids.vertices.min + 1)
            ) + gameConfig.asteroids.vertices.min,
            offsets: Array(gameConfig.asteroids.vertices.max).fill(0).map(() => Math.random() * 0.4 + 0.8),
        });
    }
}

function shootBullet() {
    if (!gameConfig || bullets.length >= gameConfig.bullets.maxCount) return;

    bullets.push({
        x: ship.x + ship.radius * Math.cos(ship.angle),
        y: ship.y - ship.radius * Math.sin(ship.angle),
        xv: gameConfig.bullets.speed * Math.cos(ship.angle),
        yv: -gameConfig.bullets.speed * Math.sin(ship.angle),
        radius: gameConfig.bullets.radius,
        lifeTime: gameConfig.bullets.lifeTime,
    });

    playSound(sounds.shoot);
}

function checkCollisions() {
    if (ship.invulnerable && Date.now() > ship.invulnerableTime) {
        ship.invulnerable = false;
    }

    for (let i = asteroids.length - 1; i >= 0; i--) {
        for (let j = bullets.length - 1; j >= 0; j--) {
            const dx = asteroids[i].x - bullets[j].x;
            const dy = asteroids[i].y - bullets[j].y;
            const distance = Math.sqrt(dx * dx + dy * dy);

            if (distance < asteroids[i].radius + bullets[j].radius) {
                asteroids.splice(i, 1);
                bullets.splice(j, 1);
                playSound(sounds.explosion);

                gameState.score += gameConfig.scoring.pointsPerAsteroid * gameState.level;
                if (scoreElement) scoreElement.textContent = gameState.score;

                if (asteroids.length === 0) {
                    gameState.level++;
                    if (levelElement) levelElement.textContent = gameState.level;
                    playSound(sounds.levelUp);
                    createAsteroids();
                }
                break;
            }
        }
    }

    if (!ship.invulnerable) {
        for (let i = 0; i < asteroids.length; i++) {
            const dx = ship.x - asteroids[i].x;
            const dy = ship.y - asteroids[i].y;
            const distance = Math.sqrt(dx * dx + dy * dy);

            if (distance < ship.radius + asteroids[i].radius) {
                gameOver();
                return;
            }
        }
    }
}

function update() {
    if (!canvas || !ctx || !gameConfig) return;

    gameState.gameTime = Math.floor((Date.now() - gameState.startTime) / 1000);
    if (timeElement) timeElement.textContent = gameState.gameTime;

    if (
        window.gameAuth &&
        window.gameAuth.isLoggedIn() &&
        gameState.gameTime > 0 &&
        gameState.gameTime % 30 === 0 &&
        Date.now() - lastConfigUpdate > 5000
    ) {
        lastConfigUpdate = Date.now();
        fetchGameConfig().then((newConfig) => {
            if (newConfig) gameConfig = newConfig;
        });
    }

    ctx.fillStyle = "black";
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    ship.angle += ship.rotation;

    if (ship.thrusting) {
        ship.thrust.x += gameConfig.ship.thrust * Math.cos(ship.angle);
        ship.thrust.y -= gameConfig.ship.thrust * Math.sin(ship.angle);
    } else {
        ship.thrust.x *= 1 - gameConfig.ship.friction;
        ship.thrust.y *= 1 - gameConfig.ship.friction;
    }

    ship.x += ship.thrust.x;
    ship.y += ship.thrust.y;

    if (ship.x < 0) ship.x = canvas.width;
    if (ship.x > canvas.width) ship.x = 0;
    if (ship.y < 0) ship.y = canvas.height;
    if (ship.y > canvas.height) ship.y = 0;

    ship.draw();

    for (let i = bullets.length - 1; i >= 0; i--) {
        bullets[i].x += bullets[i].xv;
        bullets[i].y += bullets[i].yv;

        if (bullets[i].x < 0) bullets[i].x = canvas.width;
        if (bullets[i].x > canvas.width) bullets[i].x = 0;
        if (bullets[i].y < 0) bullets[i].y = canvas.height;
        if (bullets[i].y > canvas.height) bullets[i].y = 0;

        bullets[i].lifeTime--;
        if (bullets[i].lifeTime <= 0) {
            bullets.splice(i, 1);
            continue;
        }

        ctx.fillStyle = "white";
        ctx.beginPath();
        ctx.arc(bullets[i].x, bullets[i].y, bullets[i].radius, 0, Math.PI * 2);
        ctx.fill();
    }

    for (let i = 0; i < asteroids.length; i++) {
        asteroids[i].x += asteroids[i].xv;
        asteroids[i].y += asteroids[i].yv;

        if (asteroids[i].x < -asteroids[i].radius) asteroids[i].x = canvas.width + asteroids[i].radius;
        if (asteroids[i].x > canvas.width + asteroids[i].radius) asteroids[i].x = -asteroids[i].radius;
        if (asteroids[i].y < -asteroids[i].radius) asteroids[i].y = canvas.height + asteroids[i].radius;
        if (asteroids[i].y > canvas.height + asteroids[i].radius) asteroids[i].y = -asteroids[i].radius;

        ctx.strokeStyle = "white";
        ctx.lineWidth = 2;
        ctx.beginPath();

        for (let j = 0; j < asteroids[i].vertices; j++) {
            const angle = (j * Math.PI * 2) / asteroids[i].vertices;
            const offset = asteroids[i].offsets[j] || 1;
            const x = asteroids[i].x + asteroids[i].radius * offset * Math.cos(angle + asteroids[i].angle);
            const y = asteroids[i].y + asteroids[i].radius * offset * Math.sin(angle + asteroids[i].angle);

            if (j === 0) {
                ctx.moveTo(x, y);
            } else {
                ctx.lineTo(x, y);
            }
        }

        ctx.closePath();
        ctx.stroke();
    }

    checkCollisions();

    if (!gameOverDialog || !gameOverDialog.style.display || gameOverDialog.style.display === "none") {
        requestAnimationFrame(update);
    }
}

function gameOver() {
    if (finalScoreElement) finalScoreElement.textContent = gameState.score;

    if (window.gameAuth && window.gameAuth.isLoggedIn()) {
        submitScore(gameState.score, gameState.level, gameState.gameTime);
    }

    if (gameOverDialog) gameOverDialog.style.display = "block";
    playSound(sounds.explosion);
}

// Keyboard input
document.addEventListener("keydown", function (event) {
    if (!gameConfig) return;

    switch (event.key) {
        case "ArrowLeft":
            ship.rotation = gameConfig.ship.turnSpeed;
            break;
        case "ArrowRight":
            ship.rotation = -gameConfig.ship.turnSpeed;
            break;
        case "ArrowUp":
            ship.thrusting = true;
            break;
        case " ":
            shootBullet();
            event.preventDefault();
            break;
    }
});

document.addEventListener("keyup", function (event) {
    switch (event.key) {
        case "ArrowLeft":
        case "ArrowRight":
            ship.rotation = 0;
            break;
        case "ArrowUp":
            ship.thrusting = false;
            break;
    }
});

// Auth event listeners
window.addEventListener("auth:login", function (event) {
    console.log("Authentication successful", event.detail);
    if (!sessionId) sessionId = event.detail.sessionId;
});

window.addEventListener("auth:logout", function () {
    console.log("User logged out");
    sessionId = null;
    pendingGameStart = false;
    gameConfig = null;
});

// Setup start game button
function setupStartGameButton() {
    const startGameBtn = document.getElementById("startGameBtn");
    if (startGameBtn) {
        startGameBtn.addEventListener("click", startGame);
    }
}

// Initialize on DOM load
document.addEventListener("DOMContentLoaded", function () {
    initializeElements();
    setupStartGameButton();
});

// Re-initialize after HTMX swaps
document.body.addEventListener("htmx:afterSwap", function () {
    initializeElements();
    setupStartGameButton();
});

// Export for payment handler callback
window.startGameWithConfig = startGameWithConfig;
