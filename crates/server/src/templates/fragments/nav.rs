use maud::Markup;

use crate::templates::layouts::navbar::navbar;

/// Returns the navbar fragment for HTMX swap after auth state change.
pub fn nav_fragment() -> Markup {
    navbar()
}
