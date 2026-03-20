use maud::{html, Markup};

pub fn auth_modals() -> Markup {
    html! {
        // Login Modal
        div id="loginModal" class="modal" {
            div class="modal-content" {
                span class="modal-close" id="closeLoginModal" { "\u{00d7}" }
                h2 class="nes-text is-primary" { "Login" }

                div class="tabs" {
                    div class="tab is-active" data-target="privateKeyLogin" { "Private Key" }
                    div class="tab" data-target="extensionLogin" { "Browser Extension" }
                }

                div id="privateKeyLogin" class="tab-content is-active" {
                    div class="nes-field" {
                        label for="loginPrivateKey" { "Private Key:" }
                        input type="password" id="loginPrivateKey" class="nes-input";
                        p id="privateKeyError" class="help-text" {}
                    }
                    button id="loginButton" class="nes-btn is-primary" { "Login" }
                }

                div id="extensionLogin" class="tab-content" {
                    p { "Login using your Nostr browser extension." }
                    button id="extensionLoginButton" class="nes-btn is-primary" {
                        "Connect with Extension"
                    }
                    p id="extensionLoginError" class="help-text" {}
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
                    div class="tab is-active" data-target="registerPrivateKey" { "Private Key" }
                    div class="tab" data-target="registerExtension" { "Browser Extension" }
                }

                div id="registerPrivateKey" class="tab-content is-active" {
                    div id="registerStep1" {
                        p {
                            "Copy and put this private key in a safe place. "
                            "Without it, you will not be able to access your account."
                        }
                        div class="nes-field" {
                            input type="text" id="privateKeyDisplay" class="nes-input" readonly;
                        }
                        button id="copyPrivateKey" class="nes-btn is-warning" {
                            "Copy to clipboard"
                        }

                        div class="nes-field" style="margin-top: 15px;" {
                            label {
                                input type="checkbox" id="privateKeySavedCheckbox" class="nes-checkbox";
                                span { "I have saved my private key" }
                            }
                        }

                        button id="registerStep1Button" class="nes-btn is-success" disabled {
                            "Complete Registration"
                        }
                    }

                    div id="registerStep2" class="is-hidden" {
                        h3 class="nes-text is-success" { "Registration Complete!" }
                        p { "Your account has been created successfully." }
                    }
                }

                div id="registerExtension" class="tab-content" {
                    p { "Register using your Nostr browser extension." }
                    button id="extensionRegisterButton" class="nes-btn is-success" {
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
