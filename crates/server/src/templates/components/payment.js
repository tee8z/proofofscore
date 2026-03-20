// Payment handler for Lightning invoice display and polling

class PaymentHandler {
    constructor() {
        this.currentPaymentId = null;
        this.paymentCheckInterval = null;
        this.paymentData = null;
        this.initialized = false;
    }

    init() {
        this.paymentModal = document.getElementById("paymentModal");
        this.paymentRequest = document.getElementById("paymentRequest");
        this.copyFeedback = document.getElementById("copyFeedback");
        this.qrContainer = document.getElementById("qrContainer");
        this.paymentStatus = document.getElementById("paymentStatus");
        this.copyInvoiceBtn = document.getElementById("copyInvoiceBtn");
        this.checkPaymentBtn = document.getElementById("checkPaymentBtn");
        this.cancelPaymentBtn = document.getElementById("cancelPaymentBtn");

        if (this.checkElements()) {
            this.setupEventListeners();
            this.initialized = true;
            console.log("Payment handler initialized");
            return true;
        }
        return false;
    }

    checkElements() {
        return !!(
            this.paymentModal &&
            this.paymentRequest &&
            this.qrContainer &&
            this.paymentStatus
        );
    }

    setupEventListeners() {
        if (this.copyInvoiceBtn) {
            this.copyInvoiceBtn.addEventListener("click", () => this.copyInvoiceToClipboard());
        }
        if (this.checkPaymentBtn) {
            this.checkPaymentBtn.addEventListener("click", () => this.checkPaymentStatus());
        }
        if (this.cancelPaymentBtn) {
            this.cancelPaymentBtn.addEventListener("click", () => this.hidePaymentModal());
        }
    }

    async requestGameSession() {
        try {
            console.log("Requesting new game session...");
            const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";
            const response = await window.gameAuth.post(`${apiBase}/api/v1/game/session`);

            if (response.status === 201 || response.status === 200) {
                console.log("Game session created successfully");
                const data = await response.json();
                return { success: true, data };
            } else if (response.status === 402) {
                console.log("Payment required to start game");
                const data = await response.json();
                if (data.payment_required) {
                    this.showPaymentModal(data);
                    return { success: false, requiresPayment: true, data };
                }
            } else {
                console.error("Unexpected response:", response.status);
                const errorText = await response.text();
                throw new Error(`Failed to create game session: ${errorText}`);
            }
        } catch (error) {
            console.error("Error requesting game session:", error);
            return { success: false, error: error.message };
        }
    }

    showPaymentModal(paymentData) {
        console.log("Showing payment modal with data:", paymentData);
        this.paymentData = paymentData;
        this.currentPaymentId = paymentData.payment_id;

        if (this.qrContainer) this.qrContainer.innerHTML = "";
        if (this.paymentRequest) this.paymentRequest.value = paymentData.invoice;

        // Create Bitcoin QR code element
        const qrElement = document.createElement("bitcoin-qr");
        qrElement.setAttribute("lightning", paymentData.invoice);
        qrElement.setAttribute("width", 250);
        qrElement.setAttribute("height", 250);
        qrElement.setAttribute("dots-type", "rounded");
        qrElement.setAttribute("corners-square-type", "extra-rounded");
        qrElement.setAttribute("background-color", "#ffffff");
        qrElement.setAttribute("dots-color", "#000000");
        if (this.qrContainer) this.qrContainer.appendChild(qrElement);

        if (this.paymentStatus) {
            this.paymentStatus.innerHTML = '<p>Waiting for payment...</p><p class="nes-text is-primary">Amount: 500 sats</p>';
        }

        if (this.paymentModal) this.paymentModal.style.display = "block";

        this.startPaymentCheck();
    }

    hidePaymentModal() {
        if (this.paymentModal) this.paymentModal.style.display = "none";
        this.stopPaymentCheck();
    }

    copyInvoiceToClipboard() {
        if (!this.paymentRequest) return;
        navigator.clipboard.writeText(this.paymentRequest.value).then(() => {
            if (this.copyFeedback) {
                this.copyFeedback.classList.add("visible");
                setTimeout(() => { this.copyFeedback.classList.remove("visible"); }, 2000);
            }
        }).catch((err) => {
            console.error("Failed to copy:", err);
        });
    }

    startPaymentCheck() {
        this.stopPaymentCheck();
        this.checkPaymentStatus();
        this.paymentCheckInterval = setInterval(() => this.checkPaymentStatus(), 5000);
    }

    stopPaymentCheck() {
        if (this.paymentCheckInterval) {
            clearInterval(this.paymentCheckInterval);
            this.paymentCheckInterval = null;
        }
    }

    async checkPaymentStatus() {
        if (!this.currentPaymentId) return;

        try {
            const apiBase = window.API_BASE || document.body.getAttribute("data-api-base") || "";

            if (this.paymentStatus) {
                this.paymentStatus.innerHTML = '<div class="spinner"></div><p>Checking payment status...</p>';
            }

            const response = await window.gameAuth.get(
                `${apiBase}/api/v1/payments/status/${this.currentPaymentId}`
            );

            if (!response.ok) throw new Error(`Failed to check payment: ${response.statusText}`);

            const data = await response.json();

            if (data.status === "paid") {
                this.handleSuccessfulPayment();
            } else if (data.status === "failed") {
                this.handleFailedPayment();
            } else {
                if (this.paymentStatus) {
                    this.paymentStatus.innerHTML = '<p>Waiting for payment...</p><p class="nes-text is-primary">Amount: 500 sats</p>';
                }
            }
        } catch (error) {
            console.error("Error checking payment status:", error);
            if (this.paymentStatus) {
                this.paymentStatus.innerHTML = `<p class="nes-text is-error">Error checking payment</p><p>${error.message}</p>`;
            }
        }
    }

    handleSuccessfulPayment() {
        console.log("Payment successful! Starting game...");
        if (this.paymentStatus) {
            this.paymentStatus.innerHTML = '<p class="nes-text is-success">Payment received!</p><p>Starting game...</p>';
        }
        this.stopPaymentCheck();

        setTimeout(() => {
            this.hidePaymentModal();
            this.startGameAfterPayment();
        }, 2000);
    }

    handleFailedPayment() {
        console.log("Payment failed");
        if (this.paymentStatus) {
            this.paymentStatus.innerHTML =
                '<p class="nes-text is-error">Payment failed</p><p>Please try again</p>';
        }
        this.stopPaymentCheck();
    }

    async startGameAfterPayment() {
        const result = await this.requestGameSession();
        if (result.success && window.startGameWithConfig) {
            window.startGameWithConfig(result.data);
        } else {
            console.error("Failed to start game after payment:", result.error);
        }
    }
}

// Initialize payment handler
window.gamePayment = new PaymentHandler();

document.addEventListener("DOMContentLoaded", function () {
    if (window.gamePayment) {
        window.gamePayment.init();
    }
});

// Re-initialize after HTMX swaps
document.body.addEventListener("htmx:afterSwap", function () {
    if (window.gamePayment && !window.gamePayment.initialized) {
        window.gamePayment.init();
    }
});

window.initializePaymentHandler = function () {
    if (window.gamePayment && window.gamePayment.initialized) {
        return Promise.resolve(window.gamePayment);
    }

    return new Promise((resolve) => {
        const handler = window.gamePayment || new PaymentHandler();
        window.gamePayment = handler;

        if (handler.init()) {
            resolve(handler);
            return;
        }

        let attempts = 0;
        const maxAttempts = 10;
        const attemptInit = () => {
            attempts++;
            if (handler.init()) {
                resolve(handler);
            } else if (attempts < maxAttempts) {
                setTimeout(attemptInit, 500);
            } else {
                console.error("Failed to initialize payment handler after max attempts");
                resolve(handler);
            }
        };
        setTimeout(attemptInit, 500);
    });
};
