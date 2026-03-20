use maud::{html, Markup, DOCTYPE};

use crate::templates::components::auth_modals::auth_modals;
use crate::templates::layouts::navbar::navbar;

pub struct PageConfig<'a> {
    pub title: &'a str,
    pub api_base: &'a str,
}

pub fn base(config: &PageConfig, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="en" {
            head {
                meta charset="UTF-8";
                meta name="viewport" content="width=device-width, initial-scale=1.0";
                title { (config.title) }

                link rel="stylesheet" href="https://unpkg.com/nes.css@latest/css/nes.min.css";
                link href="https://fonts.googleapis.com/css?family=Press+Start+2P" rel="stylesheet";
                link rel="stylesheet" href="/static/styles.min.css";

                script src="https://unpkg.com/htmx.org@1.9.10" {}
                script type="module" src="https://unpkg.com/bitcoin-qr@1.4.1/dist/bitcoin-qr/bitcoin-qr.esm.js" {}
                script nomodule src="https://unpkg.com/bitcoin-qr@1.4.1/dist/bitcoin-qr/bitcoin-qr.js" {}
            }
            body data-api-base=(config.api_base) {
                (navbar())

                div class="container" {
                    div id="main-content" {
                        (content)
                    }
                }

                (auth_modals())

                script type="module" src="/static/loader.js" {}
            }
        }
    }
}
