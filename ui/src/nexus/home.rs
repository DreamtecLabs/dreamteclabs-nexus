use yew::prelude::*;

use crate::dashboard::view::View;

/// Nexus-owned home entry point.
///
/// The first iteration deliberately delegates rendering and data loading to
/// the upstream PDM dashboard. This gives Nexus a stable frontend seam before
/// we replace the presentation with Nexus-owned components.
#[function_component(NexusHome)]
pub fn nexus_home() -> Html {
    View::new(None).into()
}
