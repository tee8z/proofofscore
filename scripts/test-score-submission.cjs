const { readFileSync } = require("node:fs");
const vm = require("node:vm");
const assert = require("node:assert/strict");
const { test } = require("node:test");

const source = readFileSync("crates/server/src/templates/components/game_canvas.js", "utf8");
const submission = source.slice(source.indexOf("async function postScoreWithRetry"), source.indexOf("// Keyboard input"));

function client(statuses, { retryAfter = "1", random = 0.5 } = {}) {
    const calls = [], delays = [];
    const elements = {
        scoreSubmissionStatus: { textContent: "" },
        "retry-score-button": { style: {} },
    };
    const context = vm.createContext({
        sessionId: "original-session",
        window: { gameAuth: {
            isLoggedIn: () => true,
            post: async (url, payload) => {
                calls.push({ url, payload: JSON.parse(JSON.stringify(payload)) });
                const status = statuses.shift() ?? 200;
                if (status instanceof Error) throw status;
                return { status, ok: status === 200, headers: { get: () => retryAfter }, text: async () => "busy" };
            },
        } },
        document: { body: { getAttribute: () => "" }, getElementById: id => elements[id] },
        console: { warn() {}, error() {} },
        setTimeout: (callback, delay) => { delays.push(delay); callback(); },
    });
    vm.runInContext(`Math.random = () => ${random};`, context);
    vm.runInContext(submission, context);
    return { context, calls, delays, elements };
}

test("a busy response retries the same score and eventually reports success", async () => {
    const c = client([503, 503, 200]);
    await c.context.submitScore(42, 2, 30, "input", "hash", 1800, null);
    assert.equal(c.calls.length, 3);
    assert.deepEqual(c.calls[0], c.calls[2]);
    assert.deepEqual(c.delays, [1000, 1500]);
    assert.equal(c.elements.scoreSubmissionStatus.textContent, "Score saved.");
});

test("busy retries back off within an eleven second budget and manual retry keeps the session", async () => {
    const c = client(Array(6).fill(503));
    await c.context.submitScore(42, 2, 30, "input", "hash", 1800, null);
    assert.deepEqual(c.delays, [1000, 1500, 3000, 3000, 2500]);
    assert.equal(c.calls.length, 6);
    const retry = c.elements["retry-score-button"];
    assert.equal(retry.style.display, "inline-block");
    c.context.sessionId = "later-session";
    await retry.onclick();
    assert.equal(c.calls[6].payload.session_id, "original-session");
    assert.equal(retry.style.display, "none");
});

test("jitter spreads retries and Retry-After sets the shortest wait", async () => {
    const cases = [
        [{ random: 0 }, [503, 503, 200], [1000, 1000]],
        [{ random: 0.999 }, [503, 503, 200], [1000, 1999]],
        [{ retryAfter: "3" }, [503, 200], [3000]],
        [{ retryAfter: null }, [503, 200], [750]],
    ];
    for (const [options, statuses, delays] of cases) {
        const c = client(statuses, options);
        await c.context.submitScore(42, 2, 30, "input", "hash", 1800, null);
        assert.deepEqual(c.delays, delays, JSON.stringify(options));
        assert.equal(c.elements.scoreSubmissionStatus.textContent, "Score saved.");
    }
});

test("validation failures and ambiguous failures are not automatically resubmitted", async () => {
    for (const status of [400, 422, 500, new Error("connection lost")]) {
        const c = client([status]);
        await c.context.submitScore(42, 2, 30, "input", "hash", 1800, null);
        assert.equal(c.calls.length, 1);
        assert.equal(c.elements["retry-score-button"].style.display, "none");
    }
});
