const AUTH_REQUIRED_ROUTES = ["/play", "/fragments/score", "/api/v1/game", "/api/v1/prizes"];

function requiresAuth(url) {
    return AUTH_REQUIRED_ROUTES.some((route) => url.includes(route));
}

function isLoggedIn() {
    return window.gameAuth && window.gameAuth.isLoggedIn();
}

async function generateAuthHeader(method, url) {
    if (!isLoggedIn()) return null;

    try {
        const fullUrl = new URL(url, window.location.origin).href;
        return await window.gameAuth.createAuthHeader(fullUrl, method, null);
    } catch (error) {
        console.error("Failed to generate auth header:", error);
        return null;
    }
}

function showAuthError(message) {
    const notification = document.createElement("div");
    notification.className = "nes-container is-dark";
    notification.style.cssText =
        "position: fixed; top: 20px; right: 20px; z-index: 9999; max-width: 400px; border-color: #ff6b6b;";
    notification.innerHTML = `
        <p class="nes-text is-error">${message}</p>
    `;
    document.body.appendChild(notification);
    setTimeout(() => notification.remove(), 5000);
}

function setupHtmxAuth() {
    // Use htmx:confirm for async auth header generation
    document.body.addEventListener("htmx:confirm", async (event) => {
        const { verb, path } = event.detail;

        // If route doesn't require auth, let HTMX proceed normally
        if (!requiresAuth(path)) return;

        // If user is not logged in, show login modal instead
        if (!isLoggedIn()) {
            event.preventDefault();
            const loginModal = document.getElementById("loginModal");
            if (loginModal) {
                loginModal.classList.add("is-active");
            }
            return;
        }

        // User is logged in, generate auth header
        event.preventDefault();

        try {
            const authHeader = await generateAuthHeader(verb, path);

            if (authHeader) {
                event.detail.elt._pendingAuthHeader = authHeader;
                event.detail.issueRequest();
            } else {
                console.error("HTMX auth: Failed to generate auth header for", path);
                showAuthError("Failed to authenticate request. Please try logging in again.");
            }
        } catch (error) {
            console.error("HTMX auth: Exception during auth header generation:", error);
            showAuthError("Authentication error: " + error.message);
        }
    });

    // Synchronously apply the pre-generated header
    document.body.addEventListener("htmx:configRequest", (event) => {
        const elt = event.detail.elt;
        if (elt._pendingAuthHeader) {
            event.detail.headers["Authorization"] = elt._pendingAuthHeader;
            delete elt._pendingAuthHeader;
        }
    });

    // Handle auth errors from server responses
    document.body.addEventListener("htmx:responseError", (event) => {
        if (event.detail.xhr.status === 401) {
            showAuthError("Session expired. Please log in again.");
        }
    });
}

window.requiresAuth = requiresAuth;
window.isLoggedIn = isLoggedIn;
window.generateAuthHeader = generateAuthHeader;
window.setupHtmxAuth = setupHtmxAuth;
