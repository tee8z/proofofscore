const { readFileSync } = require("node:fs");
const vm = require("node:vm");
const assert = require("node:assert/strict");
const { test } = require("node:test");

const source = readFileSync("crates/server/src/templates/components/game_canvas.js", "utf8");
const submission = source.slice(source.indexOf("async function postScoreWithRetry"), source.indexOf("// Keyboard input"));

function client(statuses) {
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
                return { status, ok: status === 200, headers: { get: () => "1" }, text: async () => "busy" };
            },
        } },
        document: { body: { getAttribute: () => "" }, getElementById: id => elements[id] },
        console: { warn() {}, error() {} },
        setTimeout: (callback, delay) => { delays.push(delay); callback(); },
    });
    vm.runInContext(submission, context);
    return { context, calls, delays, elements };
}

test("a busy response retries the same score and eventually reports success", async () => {
    const c = client([503, 503, 200]);
    await c.context.submitScore(42, 2, 30, "input", "hash", 1800, null);
    assert.equal(c.calls.length, 3);
    assert.deepEqual(c.calls[0], c.calls[2]);
    assert.deepEqual(c.delays, [1000, 1000]);
    assert.equal(c.elements.scoreSubmissionStatus.textContent, "Score saved.");
});

test("busy retries are bounded and manual retry preserves the original session", async () => {
    const c = client(Array(12).fill(503));
    await c.context.submitScore(42, 2, 30, "input", "hash", 1800, null);
    assert.equal(c.calls.length, 12);
    assert.equal(c.delays.length, 11);
    const retry = c.elements["retry-score-button"];
    assert.equal(retry.style.display, "inline-block");
    c.context.sessionId = "later-session";
    await retry.onclick();
    assert.equal(c.calls[12].payload.session_id, "original-session");
    assert.equal(retry.style.display, "none");
});

test("validation failures and ambiguous failures are not automatically resubmitted", async () => {
    for (const status of [400, 422, 500, new Error("connection lost")]) {
        const c = client([status]);
        await c.context.submitScore(42, 2, 30, "input", "hash", 1800, null);
        assert.equal(c.calls.length, 1);
        assert.equal(c.elements["retry-score-button"].style.display, "none");
    }
});
