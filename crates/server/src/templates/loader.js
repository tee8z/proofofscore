// Initialize WASM modules, then load the bundled app JS
import initNostr, { NostrClientWrapper, SignerType, encryptNsecWithPassword, decryptNsecWithPassword } from '/ui/pkg/nostr_signer/nostr_signer.js';
import initGameEngine, { GameEngine, InputRecorder } from '/ui/pkg/game_engine/game_engine.js';

async function initApp() {
    console.log('Loading WASM modules...');

    // Initialize both WASM modules in parallel
    await Promise.all([
        initNostr(),
        initGameEngine(),
    ]);
    console.log('WASM modules loaded');

    // Make WASM classes available globally
    window.NostrClientWrapper = NostrClientWrapper;
    window.SignerType = SignerType;
    window.GameEngine = GameEngine;
    window.InputRecorder = InputRecorder;
    window.encryptNsecWithPassword = encryptNsecWithPassword;
    window.decryptNsecWithPassword = decryptNsecWithPassword;

    // Set API_BASE from body data attribute
    const apiBase = document.body.getAttribute('data-api-base') || '';
    window.API_BASE = apiBase;

    // Default relays for NIP-07 extension auth (configurable via server config)
    const relaysAttr = document.body.getAttribute('data-default-relays') || '';
    window.DEFAULT_RELAYS = relaysAttr ? relaysAttr.split(',').filter(Boolean) : [];

    // Load the bundled app JS and initialize replay viewer once it's ready
    const script = document.createElement('script');
    script.src = window.APP_JS_PATH || '/static/app.min.js';
    script.onload = function() {
        // app.min.js has executed — WASM + DOM are both ready.
        // Kick off the replay viewer if we're on a page with a replay canvas.
        if (typeof initReplayViewer === 'function') {
            initReplayViewer();
        }
    };
    document.body.appendChild(script);
}

initApp().catch(console.error);

// Register service worker for offline practice mode
if ('serviceWorker' in navigator) {
    navigator.serviceWorker.register('/sw.js', { scope: '/' })
        .then(() => console.log('Service worker registered'))
        .catch((err) => console.warn('SW registration failed:', err));
}
