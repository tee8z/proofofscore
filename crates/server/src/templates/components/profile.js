// Profile modal handler — stats + lightning address management

class ProfileHandler {
    constructor() {
        this.initialized = false;
    }

    init() {
        this.modal = document.getElementById("profileModal");
        this.closeBtn = document.getElementById("closeProfileModal");
        this.profileBtn = document.getElementById("profileBtn");
        this.usernameDisplay = document.getElementById("usernameDisplay");
        this.lightningInput = document.getElementById("lightningAddressInput");
        this.statusEl = document.getElementById("lightningAddressStatus");
        this.saveBtn = document.getElementById("saveLightningAddress");
        this.clearBtn = document.getElementById("clearLightningAddress");

        if (!this.modal) return false;

        this.setupEventListeners();
        this.initialized = true;
        return true;
    }

    setupEventListeners() {
        if (this.profileBtn) {
            this.profileBtn.addEventListener("click", () => this.show());
        }
        if (this.usernameDisplay) {
            this.usernameDisplay.addEventListener("click", () => this.show());
        }
        if (this.closeBtn) {
            this.closeBtn.addEventListener("click", () => this.hide());
        }
        if (this.saveBtn) {
            this.saveBtn.addEventListener("click", () => this.saveLightningAddress());
        }
        if (this.clearBtn) {
            this.clearBtn.addEventListener("click", () => this.clearLightningAddress());
        }
    }

    async show() {
        if (!this.modal) return;
        this.modal.classList.add("is-active");
        await this.loadProfile();
    }

    hide() {
        if (this.modal) this.modal.classList.remove("is-active");
        this.setStatus("", "");
    }

    setStatus(message, type) {
        if (!this.statusEl) return;
        this.statusEl.textContent = message;
        this.statusEl.className = "help-text";
        if (type === "success") this.statusEl.classList.add("nes-text", "is-success");
        else if (type === "error") this.statusEl.classList.add("nes-text", "is-error");
    }

    async loadProfile() {
        if (!window.gameAuth || !window.gameAuth.isLoggedIn()) return;

        try {
            const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";
            const response = await window.gameAuth.get(`${apiBase}/api/v1/users/profile`);

            if (!response.ok) {
                console.error("Failed to load profile:", response.status);
                return;
            }

            const data = await response.json();

            // Populate lightning address
            if (this.lightningInput) {
                this.lightningInput.value = data.lightning_address || "";
            }

            // Show current status
            if (data.lightning_address) {
                this.setStatus("Current: " + data.lightning_address + " (prizes auto-pay here)", "success");
            } else {
                this.setStatus("No lightning address set — prizes require manual invoice claim", "");
            }

            // Store it for payment flow
            localStorage.setItem("lightningAddress", data.lightning_address || "");

            // Populate stats
            const stats = data.stats || {};
            this.setStat("profileHighScore", stats.highScore || 0);
            this.setStat("profileTotalPlays", stats.totalPlays || 0);
            this.setStat("profileGamesPurchased", stats.totalGamesPurchased || 0);
            this.setStat("profileTotalSpent", (stats.totalSpentSats || 0) + " sats");
            this.setStat("profilePrizesWon", stats.prizesWon || 0);
            this.setStat("profileTotalEarned", (stats.totalEarnedSats || 0) + " sats");
        } catch (error) {
            console.error("Error loading profile:", error);
        }
    }

    setStat(id, value) {
        const el = document.getElementById(id);
        if (el) el.textContent = value;
    }

    async saveLightningAddress() {
        if (!window.gameAuth || !window.gameAuth.isLoggedIn()) return;
        const address = this.lightningInput ? this.lightningInput.value.trim() : "";

        if (!address) {
            this.setStatus("Enter a lightning address or use Clear to remove", "error");
            return;
        }

        this.setStatus("Saving...", "");
        this.saveBtn.disabled = true;

        try {
            const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";
            const response = await window.gameAuth.post(
                `${apiBase}/api/v1/users/lightning-address`,
                { lightning_address: address }
            );

            if (response.ok) {
                const data = await response.json();
                if (this.lightningInput) {
                    this.lightningInput.value = data.lightning_address || address;
                }
                localStorage.setItem("lightningAddress", data.lightning_address || address);
                this.setStatus("Lightning address saved! Prizes will be sent here automatically.", "success");
            } else {
                const text = await response.text();
                this.setStatus(text || "Failed to save", "error");
            }
        } catch (error) {
            console.error("Error saving lightning address:", error);
            this.setStatus("Network error — please try again", "error");
        } finally {
            this.saveBtn.disabled = false;
        }
    }

    async clearLightningAddress() {
        if (!window.gameAuth || !window.gameAuth.isLoggedIn()) return;

        this.setStatus("Clearing...", "");
        this.clearBtn.disabled = true;

        try {
            const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";
            const response = await window.gameAuth.post(
                `${apiBase}/api/v1/users/lightning-address`,
                { lightning_address: null }
            );

            if (response.ok) {
                if (this.lightningInput) this.lightningInput.value = "";
                localStorage.removeItem("lightningAddress");
                this.setStatus("Lightning address cleared. You'll need to submit invoices manually for prizes.", "success");
            } else {
                const text = await response.text();
                this.setStatus(text || "Failed to clear", "error");
            }
        } catch (error) {
            console.error("Error clearing lightning address:", error);
            this.setStatus("Network error — please try again", "error");
        } finally {
            this.clearBtn.disabled = false;
        }
    }
}

// Initialize
window.gameProfile = new ProfileHandler();

function initProfile() {
    if (window.gameProfile) {
        window.gameProfile.init();
    }
}

if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", initProfile);
} else {
    initProfile();
}

// Re-init after HTMX swaps
document.body.addEventListener("htmx:afterSwap", function () {
    if (window.gameProfile && !window.gameProfile.initialized) {
        window.gameProfile.init();
    }
});

// Store lightning address from login response
window.addEventListener("auth:login", function () {
    // Profile data gets loaded when modal opens — but also cache
    // the lightning address from the login response if available
    const addr = localStorage.getItem("lightningAddress");
    if (addr) {
        console.log("Lightning address cached:", addr);
    }
});
