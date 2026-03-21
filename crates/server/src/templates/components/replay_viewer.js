// Replay viewer — plays back top games on the home page canvas
// Decodes input logs and feeds them through the WASM GameEngine to render replays

class ReplayViewer {
    constructor(canvasId) {
        this.canvas = document.getElementById(canvasId);
        if (!this.canvas) return;
        this.ctx = this.canvas.getContext("2d");
        this.replays = [];
        this.currentIndex = 0;
        this.engine = null;
        this.frameIndex = 0;
        this.inputs = [];
        this.animationId = null;
        this.lastFrameTime = 0;
        this.FRAME_MS = 1000 / 60;
        this.paused = false;
    }

    async loadReplays() {
        if (!this.canvas) return;
        try {
            const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";
            const response = await fetch(`${apiBase}/api/v1/game/replays/top`);
            if (!response.ok) return;
            this.replays = await response.json();
            if (this.replays.length > 0) {
                this.canvas.style.display = "block";
                const label = document.getElementById("replayLabel");
                if (label) label.style.display = "block";
                this.startReplay(0);
            }
        } catch (e) {
            console.error("Failed to load replays:", e);
        }
    }

    startReplay(index) {
        if (this.animationId) {
            cancelAnimationFrame(this.animationId);
            this.animationId = null;
        }

        this.currentIndex = index;
        const replay = this.replays[index];
        if (!replay) return;

        // Decode input log from base64
        const inputBytes = this.base64ToBytes(replay.inputLogBase64);
        this.inputs = this.decodeInputs(inputBytes, replay.frames);
        this.frameIndex = 0;

        // Parse seed into high/low u32
        const seed = replay.seed;
        const seedHigh = parseInt(seed.substring(0, 8), 16) >>> 0;
        const seedLow = parseInt(seed.substring(8, 16), 16) >>> 0;

        // Create engine
        try {
            this.engine = new window.GameEngine(seedHigh, seedLow, replay.engineConfig);
        } catch (e) {
            console.error("Failed to create replay engine:", e);
            return;
        }

        // Update overlay
        this.updateOverlay(replay);

        // Start the replay loop
        this.lastFrameTime = performance.now();
        this.tick();
    }

    tick() {
        if (this.paused) {
            this.animationId = requestAnimationFrame(() => this.tick());
            return;
        }

        const now = performance.now();
        const elapsed = now - this.lastFrameTime;

        if (elapsed >= this.FRAME_MS) {
            this.lastFrameTime = now - (elapsed % this.FRAME_MS);

            if (this.frameIndex < this.inputs.length && !this.engine.is_game_over()) {
                const input = this.inputs[this.frameIndex];
                this.engine.tick(input.thrust, input.left, input.right, input.shoot);
                this.frameIndex++;

                const stateJson = this.engine.get_state_json();
                const state = JSON.parse(stateJson);
                this.render(state);
                this.renderOverlay(state);
            } else {
                // Replay finished — show final frame briefly, then move to next
                this.engine = null;
                setTimeout(() => {
                    const next = (this.currentIndex + 1) % this.replays.length;
                    this.startReplay(next);
                }, 3000);
                return;
            }
        }

        this.animationId = requestAnimationFrame(() => this.tick());
    }

    updateOverlay(replay) {
        const label = document.getElementById("replayLabel");
        if (label) {
            label.textContent = `Replay ${this.currentIndex + 1}/${this.replays.length}: ${replay.username} — Score: ${replay.score} (Level ${replay.level})`;
        }
    }

    renderOverlay(state) {
        const ctx = this.ctx;
        // Score/level overlay at top
        ctx.font = "12px 'Press Start 2P', monospace";
        ctx.fillStyle = "#ffffff88";
        ctx.textAlign = "left";
        ctx.fillText(`SCORE: ${state.score}`, 10, 20);
        ctx.fillText(`LEVEL: ${state.level}`, 10, 38);
        ctx.textAlign = "right";
        ctx.fillText(`LIVES: ${state.lives}`, this.canvas.width - 10, 20);
        ctx.textAlign = "left";
    }

    render(state) {
        const ctx = this.ctx;
        const canvas = this.canvas;
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

        // Thrust flame
        if (ship.thrusting) {
            ctx.beginPath();
            ctx.moveTo(x2, y2);
            ctx.lineTo(ship.x - ship.radius * 1.5 * Math.cos(ship.angle), ship.y + ship.radius * 1.5 * Math.sin(ship.angle));
            ctx.lineTo(x3, y3);
            ctx.strokeStyle = "orange";
            ctx.stroke();
        }

        // Bullets
        ctx.fillStyle = "white";
        for (const bullet of state.bullets) {
            ctx.beginPath();
            ctx.arc(bullet.x, bullet.y, bullet.radius, 0, Math.PI * 2);
            ctx.fill();
        }

        // Power-ups
        for (const pu of (state.power_ups || [])) {
            const colors = { RapidFire: "#ffff00", Shield: "#00ffff", SpreadShot: "#ff00ff", SpeedBoost: "#ff8800" };
            ctx.fillStyle = colors[pu.power_type] || "#ffffff";
            ctx.beginPath();
            ctx.arc(pu.x, pu.y, pu.radius, 0, Math.PI * 2);
            ctx.fill();
            ctx.strokeStyle = ctx.fillStyle;
            ctx.lineWidth = 1;
            ctx.beginPath();
            ctx.arc(pu.x, pu.y, pu.radius * 1.5 * (0.8 + 0.2 * Math.sin(Date.now() / 200)), 0, Math.PI * 2);
            ctx.stroke();
        }

        // Enemies
        for (const enemy of (state.enemies || [])) {
            const isBoss = enemy.enemy_type === "Boss";
            ctx.strokeStyle = enemy.enemy_type === "Drone" ? "#00ff00"
                : enemy.enemy_type === "Fighter" ? "#ff4444"
                : isBoss ? "#ff00ff"
                : "#ffaa00";
            ctx.lineWidth = 2;
            ctx.beginPath();
            const er = enemy.radius;
            ctx.moveTo(enemy.x + er * Math.cos(enemy.angle), enemy.y - er * Math.sin(enemy.angle));
            ctx.lineTo(enemy.x + er * 0.6 * Math.cos(enemy.angle + 1.5), enemy.y - er * 0.6 * Math.sin(enemy.angle + 1.5));
            ctx.lineTo(enemy.x - er * Math.cos(enemy.angle), enemy.y + er * Math.sin(enemy.angle));
            ctx.lineTo(enemy.x + er * 0.6 * Math.cos(enemy.angle - 1.5), enemy.y - er * 0.6 * Math.sin(enemy.angle - 1.5));
            ctx.closePath();
            ctx.stroke();
            if (isBoss) {
                const barWidth = enemy.radius * 2;
                const barHeight = 3;
                const barX = enemy.x - barWidth / 2;
                const barY = enemy.y - enemy.radius - 8;
                ctx.fillStyle = "#333";
                ctx.fillRect(barX, barY, barWidth, barHeight);
                ctx.fillStyle = "#ff00ff";
                ctx.fillRect(barX, barY, barWidth * Math.min(enemy.hp / 10, 1), barHeight);
            }
        }

        // Enemy bullets
        ctx.fillStyle = "#ff4444";
        for (const eb of (state.enemy_bullets || [])) {
            ctx.beginPath();
            ctx.arc(eb.x, eb.y, eb.radius, 0, Math.PI * 2);
            ctx.fill();
        }

        // Asteroids
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

    // Decode base64 to Uint8Array
    base64ToBytes(b64) {
        const binary = atob(b64);
        const bytes = new Uint8Array(binary.length);
        for (let i = 0; i < binary.length; i++) {
            bytes[i] = binary.charCodeAt(i);
        }
        return bytes;
    }

    // Decode packed input bytes to frame inputs
    // 4 bits per frame, 2 frames per byte
    // bit0=thrust, bit1=left, bit2=right, bit3=shoot
    decodeInputs(bytes, frameCount) {
        const inputs = [];
        for (let i = 0; i < bytes.length && inputs.length < frameCount; i++) {
            const byte = bytes[i];
            // Low nibble = even frame
            if (inputs.length < frameCount) {
                inputs.push({
                    thrust: (byte & 1) !== 0,
                    left: (byte & 2) !== 0,
                    right: (byte & 4) !== 0,
                    shoot: (byte & 8) !== 0,
                });
            }
            // High nibble = odd frame
            if (inputs.length < frameCount) {
                const hi = byte >> 4;
                inputs.push({
                    thrust: (hi & 1) !== 0,
                    left: (hi & 2) !== 0,
                    right: (hi & 4) !== 0,
                    shoot: (hi & 8) !== 0,
                });
            }
        }
        return inputs;
    }

    // Load and play a single replay by score ID (for leaderboard watch buttons)
    async loadSingleReplay(scoreId) {
        if (!this.canvas) return;
        try {
            const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";
            const response = await fetch(`${apiBase}/api/v1/game/replay/${scoreId}`);
            if (!response.ok) {
                console.error("Failed to fetch replay:", response.status);
                return;
            }
            const replay = await response.json();
            this.replays = [replay];
            this.canvas.style.display = "block";
            const label = document.getElementById("replayLabel");
            if (label) {
                label.style.display = "block";
                label.textContent = `Replay: ${replay.username} — Score: ${replay.score} (Level ${replay.level})`;
            }
            this.startReplay(0);
            this.canvas.scrollIntoView({ behavior: "smooth", block: "center" });
        } catch (e) {
            console.error("Failed to load single replay:", e);
        }
    }

    stop() {
        if (this.animationId) {
            cancelAnimationFrame(this.animationId);
            this.animationId = null;
        }
        this.engine = null;
    }
}

// Auto-initialize on home page
window.replayViewer = null;

function initReplayViewer() {
    const canvas = document.getElementById("replayCanvas");
    if (canvas && window.GameEngine) {
        if (window.replayViewer) {
            window.replayViewer.stop();
        }
        window.replayViewer = new ReplayViewer("replayCanvas");
        // Only auto-load top replays on home page (not on leaderboard)
        const isLeaderboard = !!document.querySelector(".leaderboard-container");
        if (!isLeaderboard) {
            window.replayViewer.loadReplays();
        }
        initReplayButtons();
    }
}

// Set up click handlers for leaderboard replay buttons
function initReplayButtons() {
    document.querySelectorAll(".replay-btn").forEach(function(btn) {
        btn.addEventListener("click", function(e) {
            e.preventDefault();
            const scoreId = this.getAttribute("data-score-id");
            if (!scoreId) return;
            const canvas = document.getElementById("replayCanvas");
            if (!canvas) return;
            if (!window.replayViewer || !window.GameEngine) {
                window.replayViewer = new ReplayViewer("replayCanvas");
            }
            window.replayViewer.loadSingleReplay(scoreId);
        });
    });
}

// Primary initialization is triggered by loader.js after app.min.js loads
// (see loader.js script.onload). This ensures WASM + DOM are both ready.

// Re-initialize after HTMX page swaps (navigating back to home page etc.)
document.body.addEventListener("htmx:afterSwap", function() {
    // Short delay to let the new DOM settle
    setTimeout(function() {
        if (window.GameEngine) {
            initReplayViewer();
            initReplayButtons();
        }
    }, 100);
});

// Stop replay when navigating away from home page
window.addEventListener("auth:login", function() {
    if (window.replayViewer) {
        window.replayViewer.stop();
    }
});
