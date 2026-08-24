use yew::prelude::*;

use crate::dashboard::view::View;

/// Nexus-owned home entry point.
///
/// Keep the upstream PDM dashboard as the data/interaction surface while Nexus
/// establishes its own presentation layer around it. This is intentionally a
/// very small seam so upstream dashboard changes remain easy to absorb.
#[function_component(NexusHome)]
pub fn nexus_home() -> Html {
    let upstream_dashboard: Html = View::new(None).into();

    html! {
        <>
            <div style="padding: 18px 24px 4px 24px;">
                <div style="font-size: 20px; font-weight: 650; line-height: 1.25;">
                    {"DreamtecLabs Nexus"}
                </div>
                <div style="margin-top: 4px; opacity: 0.68; font-size: 13px;">
                    {"Unified infrastructure control plane · powered by Proxmox Datacenter Manager"}
                </div>
            </div>
            {upstream_dashboard}
        </>
    }
}
