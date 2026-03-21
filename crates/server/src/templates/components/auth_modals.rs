use maud::{html, Markup};

pub fn auth_modals() -> Markup {
    html! {
        // Login Modal
        div id="loginModal" class="modal" {
            div class="modal-content" {
                span class="modal-close" id="closeLoginModal" { "\u{00d7}" }
                h2 class="nes-text is-primary" { "Login" }

                div class="tabs" {
                    div class="tab is-active" data-target="usernameLogin" { "Username" }
                    div class="tab" data-target="extensionLogin" { "Extension" }
                    div class="tab" data-target="recoveryLogin" { "Recovery Key" }
                }

                div id="usernameLogin" class="tab-content is-active" {
                    div class="nes-field" {
                        label for="loginUsername" { "Username:" }
                        input type="text" id="loginUsername" class="nes-input" autocomplete="username";
                    }
                    div class="nes-field" style="margin-top: 10px;" {
                        label for="loginPassword" { "Password:" }
                        input type="password" id="loginPassword" class="nes-input" autocomplete="current-password";
                    }
                    p id="usernameLoginError" class="help-text" {}
                    button id="usernameLoginButton" class="nes-btn is-primary" style="margin-top: 10px;" { "Login" }
                }

                div id="extensionLogin" class="tab-content" {
                    p { "Login using your Nostr browser extension." }
                    div class="nes-field" style="margin-top: 10px;" {
                        label for="relayInput" { "Relays (comma-separated):" }
                        input type="text" id="relayInput" class="nes-input"
                            placeholder="wss://relay.damus.io, wss://relay.primal.net";
                        p class="nes-text" style="font-size: 0.6em; margin-top: 4px;" {
                            "Leave blank to use defaults"
                        }
                    }
                    button id="extensionLoginButton" class="nes-btn is-primary" style="margin-top: 10px;" {
                        "Connect with Extension"
                    }
                    p id="extensionLoginError" class="help-text" {}
                }

                div id="recoveryLogin" class="tab-content" {
                    div class="nes-field" {
                        label for="loginPrivateKey" { "Recovery Key (nsec):" }
                        input type="password" id="loginPrivateKey" class="nes-input";
                        p id="privateKeyError" class="help-text" {}
                    }
                    button id="loginButton" class="nes-btn is-primary" { "Login" }
                }

                p class="nes-text" style="margin-top: 20px;" {
                    "Don't have an account? "
                    a href="#" id="showRegisterModal" class="nes-text is-primary" { "Sign up" }
                }
            }
        }

        // Registration Modal
        div id="registerModal" class="modal" {
            div class="modal-content" {
                span class="modal-close" id="closeRegisterModal" { "\u{00d7}" }
                h2 class="nes-text is-success" { "Create Account" }

                div class="tabs" {
                    div class="tab is-active" data-target="registerUsername" { "Username" }
                    div class="tab" data-target="registerExtension" { "Extension" }
                }

                // Username/password registration
                div id="registerUsername" class="tab-content is-active" {
                    div id="usernameRegisterStep1" {
                        div class="nes-field" {
                            label for="registerUsernameInput" { "Username:" }
                            input type="text" id="registerUsernameInput" class="nes-input"
                                placeholder="3-32 chars, starts with letter" autocomplete="username";
                        }
                        div class="nes-field" style="margin-top: 10px;" {
                            label for="registerPasswordInput" { "Password:" }
                            input type="password" id="registerPasswordInput" class="nes-input" autocomplete="new-password";
                        }
                        div class="nes-field" style="margin-top: 10px;" {
                            label for="registerPasswordConfirm" { "Confirm Password:" }
                            input type="password" id="registerPasswordConfirm" class="nes-input" autocomplete="new-password";
                        }
                        p class="nes-text" style="font-size: 0.7em; margin-top: 8px;" {
                            "Min 8 characters"
                        }
                        p id="usernameRegisterError" class="help-text" {}
                        button id="usernameRegisterButton" class="nes-btn is-success" style="margin-top: 10px;" {
                            "Create Account"
                        }
                    }

                    div id="usernameRegisterStep2" class="is-hidden" {
                        h3 class="nes-text is-warning" { "Save Your Recovery Key!" }
                        p {
                            "This is the only way to recover your account. "
                            "Copy it and store it somewhere safe."
                        }
                        div class="nes-field" style="margin-top: 10px;" {
                            input type="text" id="recoveryKeyDisplay" class="nes-input" readonly;
                        }
                        button id="copyRecoveryKey" class="nes-btn is-warning" style="margin-top: 10px;" {
                            "Copy to clipboard"
                        }

                        // Lightning address suggestion
                        div class="nes-container is-dark" style="margin-top: 16px;" {
                            h4 class="nes-text is-success" style="font-size: 0.85em;" {
                                "Set Up Lightning Address (recommended)"
                            }
                            p style="font-size: 0.7em; margin-bottom: 8px;" {
                                "Add a lightning address so you can receive prize winnings "
                                "automatically. Works with CashApp ($cashtag), Wallet of Satoshi, "
                                "Strike, or any lightning wallet."
                            }
                            div class="nes-field" {
                                input type="text" id="registerLightningAddress" class="nes-input"
                                    placeholder="$cashtag or you@wallet.com"
                                    autocomplete="off";
                            }
                            p style="font-size: 0.65em; margin-top: 4px; opacity: 0.7;" {
                                "You can always set this later in your Profile"
                            }
                        }

                        button type="button" id="usernameRegisterComplete" class="nes-btn is-warning" style="margin-top: 15px; width: 100%;" {
                            "I have saved my recovery key"
                        }
                    }
                }

                div id="registerExtension" class="tab-content" {
                    p { "Register using your Nostr browser extension." }
                    div class="nes-field" style="margin-top: 10px;" {
                        label for="relayInputRegister" { "Relays (comma-separated):" }
                        input type="text" id="relayInputRegister" class="nes-input"
                            placeholder="wss://relay.damus.io, wss://relay.primal.net";
                        p class="nes-text" style="font-size: 0.6em; margin-top: 4px;" {
                            "Leave blank to use defaults"
                        }
                    }
                    button id="extensionRegisterButton" class="nes-btn is-success" style="margin-top: 10px;" {
                        "Register with Extension"
                    }
                    p id="extensionRegisterError" class="help-text" {}
                }

                p class="nes-text" style="margin-top: 20px;" {
                    "Already have an account? "
                    a href="#" id="showLoginModal" class="nes-text is-primary" { "Login" }
                }
            }
        }
    }
}
