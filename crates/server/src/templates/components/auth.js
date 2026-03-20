// Auth client for Nostr-based authentication
// Depends on WASM NostrClientWrapper and SignerType being available on window

class AuthClient {
    constructor(apiBase) {
        this.apiBase = apiBase;
        this.nostrClient = null;
        this.sessionId = null;
        this.username = null;
    }

    async initialize() {
        try {
            this.nostrClient = new window.NostrClientWrapper();
            this.restoreSession();
            this.setupEventListeners();
            console.log("Auth client initialized");
        } catch (error) {
            console.error("Failed to initialize auth client:", error);
        }
    }

    setupEventListeners() {
        // Login related elements
        const loginBtn = document.getElementById("loginBtn");
        if (loginBtn) loginBtn.addEventListener("click", () => this.showLoginModal());

        const closeLoginModal = document.getElementById("closeLoginModal");
        if (closeLoginModal) closeLoginModal.addEventListener("click", () => this.hideLoginModal());

        const loginButton = document.getElementById("loginButton");
        if (loginButton) loginButton.addEventListener("click", () => this.handlePrivateKeyLogin());

        const extensionLoginButton = document.getElementById("extensionLoginButton");
        if (extensionLoginButton) extensionLoginButton.addEventListener("click", () => this.handleExtensionLogin());

        const showRegisterModal = document.getElementById("showRegisterModal");
        if (showRegisterModal) {
            showRegisterModal.addEventListener("click", (e) => {
                e.preventDefault();
                this.hideLoginModal();
                this.showRegisterModal();
            });
        }

        // Registration related elements
        const registerBtn = document.getElementById("registerBtn");
        if (registerBtn) registerBtn.addEventListener("click", () => this.showRegisterModal());

        const closeRegisterModal = document.getElementById("closeRegisterModal");
        if (closeRegisterModal) closeRegisterModal.addEventListener("click", () => this.hideRegisterModal());

        const registerStep1Button = document.getElementById("registerStep1Button");
        if (registerStep1Button) registerStep1Button.addEventListener("click", () => this.handleRegistrationComplete());

        const extensionRegisterButton = document.getElementById("extensionRegisterButton");
        if (extensionRegisterButton) extensionRegisterButton.addEventListener("click", () => this.handleExtensionRegistration());

        const copyPrivateKey = document.getElementById("copyPrivateKey");
        if (copyPrivateKey) copyPrivateKey.addEventListener("click", () => this.handleCopyPrivateKey());

        const privateKeySavedCheckbox = document.getElementById("privateKeySavedCheckbox");
        if (privateKeySavedCheckbox) {
            privateKeySavedCheckbox.addEventListener("change", (e) => {
                const step1Btn = document.getElementById("registerStep1Button");
                if (step1Btn) step1Btn.disabled = !e.target.checked;
            });
        }

        const showLoginModal = document.getElementById("showLoginModal");
        if (showLoginModal) {
            showLoginModal.addEventListener("click", (e) => {
                e.preventDefault();
                this.hideRegisterModal();
                this.showLoginModal();
            });
        }

        // Logout
        const logoutBtn = document.getElementById("logoutBtn");
        if (logoutBtn) logoutBtn.addEventListener("click", () => this.handleLogout());

        // Start screen buttons
        const startLoginBtn = document.getElementById("startLoginBtn");
        if (startLoginBtn) startLoginBtn.addEventListener("click", () => this.showLoginModal());

        const startRegisterBtn = document.getElementById("startRegisterBtn");
        if (startRegisterBtn) startRegisterBtn.addEventListener("click", () => this.showRegisterModal());

        // Tab switching
        document.querySelectorAll(".tab").forEach((tab) => {
            tab.addEventListener("click", () => {
                const parent = tab.parentElement;
                const modal = parent.closest(".modal");
                if (!modal) return;

                parent.querySelectorAll(".tab").forEach((t) => t.classList.remove("is-active"));
                tab.classList.add("is-active");

                modal.querySelectorAll(".tab-content").forEach((content) => content.classList.remove("is-active"));

                const targetId = tab.dataset.target;
                const target = document.getElementById(targetId);
                if (target) target.classList.add("is-active");
            });
        });
    }

    showLoginModal() {
        console.log("Showing login modal");
        const modal = document.getElementById("loginModal");
        if (modal) modal.classList.add("is-active");
    }

    hideLoginModal() {
        const modal = document.getElementById("loginModal");
        if (modal) modal.classList.remove("is-active");
        const keyInput = document.getElementById("loginPrivateKey");
        if (keyInput) keyInput.value = "";
        const keyError = document.getElementById("privateKeyError");
        if (keyError) keyError.textContent = "";
        const extError = document.getElementById("extensionLoginError");
        if (extError) extError.textContent = "";
    }

    showRegisterModal() {
        console.log("Showing register modal");
        this.handleRegisterInit();
        const modal = document.getElementById("registerModal");
        if (modal) modal.classList.add("is-active");
    }

    hideRegisterModal() {
        const modal = document.getElementById("registerModal");
        if (modal) modal.classList.remove("is-active");
        const extError = document.getElementById("extensionRegisterError");
        if (extError) extError.textContent = "";
    }

    async handleRegisterInit() {
        try {
            await this.nostrClient.initialize(window.SignerType.PrivateKey, null);
            const privateKeyDisplay = document.getElementById("privateKeyDisplay");
            if (privateKeyDisplay) {
                privateKeyDisplay.value = await this.nostrClient.getPrivateKey();
            }

            const step1 = document.getElementById("registerStep1");
            if (step1) step1.classList.remove("is-hidden");
            const step2 = document.getElementById("registerStep2");
            if (step2) step2.classList.add("is-hidden");
            const step1Btn = document.getElementById("registerStep1Button");
            if (step1Btn) step1Btn.disabled = true;
            const checkbox = document.getElementById("privateKeySavedCheckbox");
            if (checkbox) checkbox.checked = false;
        } catch (error) {
            console.error("Failed to generate private key:", error);
        }
    }

    async handleCopyPrivateKey() {
        const privateKeyDisplay = document.getElementById("privateKeyDisplay");
        if (!privateKeyDisplay) return;
        const privateKey = privateKeyDisplay.value;
        await navigator.clipboard.writeText(privateKey);

        const copyBtn = document.getElementById("copyPrivateKey");
        if (copyBtn) {
            const originalText = copyBtn.textContent;
            copyBtn.textContent = "Copied!";
            setTimeout(() => { copyBtn.textContent = originalText; }, 2000);
        }
    }

    async handlePrivateKeyLogin() {
        const errorElement = document.getElementById("privateKeyError");
        if (errorElement) errorElement.textContent = "";

        const keyInput = document.getElementById("loginPrivateKey");
        const privateKey = keyInput ? keyInput.value : "";
        if (!privateKey) {
            if (errorElement) errorElement.textContent = "Please enter your private key";
            return;
        }

        try {
            await this.nostrClient.initialize(window.SignerType.PrivateKey, privateKey);
            await this.login();
        } catch (error) {
            console.error("Private key login failed:", error);
            if (errorElement) errorElement.textContent = "Login failed. Please check your private key.";
        }
    }

    async handleExtensionLogin() {
        const errorElement = document.getElementById("extensionLoginError");
        if (errorElement) errorElement.textContent = "";

        try {
            await this.nostrClient.initialize(window.SignerType.NIP07, null);
            await this.login();
        } catch (error) {
            console.error("Extension login failed:", error);
            if (errorElement) {
                if (error.toString().includes("No NIP-07")) {
                    errorElement.textContent = "No Nostr extension found. Please install a compatible extension.";
                } else {
                    errorElement.textContent = "Login failed. Please try again.";
                }
            }
        }
    }

    async handleExtensionRegistration() {
        const errorElement = document.getElementById("extensionRegisterError");
        if (errorElement) errorElement.textContent = "";

        try {
            await this.nostrClient.initialize(window.SignerType.NIP07, null);
            await this.register();
            await this.login();
        } catch (error) {
            console.error("Extension registration failed:", error);
            if (errorElement) {
                if (error.toString().includes("No NIP-07")) {
                    errorElement.textContent = "No Nostr extension found. Please install a compatible extension.";
                } else {
                    errorElement.textContent = "Registration failed. Please try again.";
                }
            }
        }
    }

    async handleRegistrationComplete() {
        try {
            await this.register();
            this.showRegistrationSuccess();
            await this.login();
        } catch (error) {
            console.error("Registration failed:", error);
        }
    }

    showRegistrationSuccess() {
        const step1 = document.getElementById("registerStep1");
        if (step1) step1.classList.add("is-hidden");
        const step2 = document.getElementById("registerStep2");
        if (step2) step2.classList.remove("is-hidden");

        setTimeout(() => { this.hideRegisterModal(); }, 2000);
    }

    handleLogout() {
        localStorage.removeItem("gameSession");
        localStorage.removeItem("gameUsername");

        this.nostrClient = new window.NostrClientWrapper();
        this.sessionId = null;
        this.username = null;

        this.updateAuthUI();

        window.dispatchEvent(new CustomEvent("auth:logout"));
    }

    async createAuthHeader(url, method, body) {
        return this.nostrClient.getAuthHeader(url, method, body || null);
    }

    async get(url, options) {
        options = options || {};
        const authHeader = await this.createAuthHeader(url, "GET", null);
        return fetch(url, {
            ...options,
            method: "GET",
            headers: {
                "Content-Type": "application/json",
                ...(options.headers || {}),
                Authorization: authHeader,
            },
        });
    }

    async post(url, body, options) {
        options = options || {};
        const authHeader = await this.createAuthHeader(url, "POST", body || null);
        return fetch(url, {
            ...options,
            method: "POST",
            headers: {
                "Content-Type": "application/json",
                ...(options.headers || {}),
                Authorization: authHeader,
            },
            body: body ? JSON.stringify(body) : null,
        });
    }

    async register() {
        const pubkey = await this.nostrClient.getPublicKey();
        const response = await this.post(`${this.apiBase}/api/v1/users/register`, {
            username: `player_${pubkey.substring(0, 8)}`,
        });

        if (!response.ok) {
            throw new Error(`Registration failed: ${response.status} ${response.statusText}`);
        }

        return response.json();
    }

    async login() {
        const response = await this.post(`${this.apiBase}/api/v1/users/login`);

        if (!response.ok) {
            throw new Error(`Login failed: ${response.status} ${response.statusText}`);
        }

        const data = await response.json();
        this.sessionId = data.session_id;
        this.username = data.username;

        localStorage.setItem("gameSession", this.sessionId);
        localStorage.setItem("gameUsername", this.username);

        this.updateAuthUI();

        this.hideLoginModal();
        this.hideRegisterModal();

        window.dispatchEvent(
            new CustomEvent("auth:login", {
                detail: {
                    sessionId: this.sessionId,
                    username: this.username,
                },
            })
        );

        return data;
    }

    restoreSession() {
        this.sessionId = localStorage.getItem("gameSession");
        this.username = localStorage.getItem("gameUsername");

        if (this.sessionId && this.username) {
            this.updateAuthUI();

            window.dispatchEvent(
                new CustomEvent("auth:login", {
                    detail: {
                        sessionId: this.sessionId,
                        username: this.username,
                    },
                })
            );
        }
    }

    updateAuthUI() {
        const authButtons = document.getElementById("authButtons");
        const userInfoArea = document.getElementById("userInfoArea");
        const usernameDisplay = document.getElementById("usernameDisplay");
        const playGameNav = document.getElementById("play-game-nav");
        const homeAuthCta = document.getElementById("home-auth-cta");
        const homePlayCta = document.getElementById("home-play-cta");

        if (this.sessionId && this.username) {
            if (authButtons) authButtons.classList.add("is-hidden");
            if (userInfoArea) userInfoArea.classList.remove("is-hidden");
            if (usernameDisplay) usernameDisplay.textContent = this.username;
            if (playGameNav) playGameNav.style.display = "inline-block";
            if (homeAuthCta) homeAuthCta.classList.add("is-hidden");
            if (homePlayCta) homePlayCta.classList.remove("is-hidden");
        } else {
            if (authButtons) authButtons.classList.remove("is-hidden");
            if (userInfoArea) userInfoArea.classList.add("is-hidden");
            if (playGameNav) playGameNav.style.display = "none";
            if (homeAuthCta) homeAuthCta.classList.remove("is-hidden");
            if (homePlayCta) homePlayCta.classList.add("is-hidden");
        }
    }

    isLoggedIn() {
        return !!this.sessionId;
    }

    getSessionId() {
        return this.sessionId;
    }
}

// Initialize auth client on DOMContentLoaded
document.addEventListener("DOMContentLoaded", async function () {
    const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";
    const auth = new AuthClient(apiBase);
    await auth.initialize();

    window.gameAuth = auth;
    window.gameAuth.showLoginModal = auth.showLoginModal.bind(auth);
    window.gameAuth.showRegisterModal = auth.showRegisterModal.bind(auth);

    // Setup HTMX auth if available
    if (window.setupHtmxAuth) {
        window.setupHtmxAuth();
    }

    // Re-bind event listeners after HTMX swaps (for dynamically loaded content)
    document.body.addEventListener("htmx:afterSwap", function () {
        auth.setupEventListeners();
        auth.updateAuthUI();
    });
});
