use yew::prelude::*;

use crate::dashboard::view::View;

/// Nexus-owned home entry point.
///
/// The first Nexus dashboard iteration keeps the upstream PDM dashboard as the
/// live data and interaction surface, but establishes a full-width Nexus shell
/// around it. This lets us evolve the information architecture independently
/// while keeping the PDM engine and dashboard internals easy to synchronize.
#[function_component(NexusHome)]
pub fn nexus_home() -> Html {
    let upstream_dashboard: Html = View::new(None).into();

    html! {
        <div style="display:flex; flex-direction:column; width:100%; min-width:0; min-height:100%; background:var(--pwt-color-surface, transparent);">
            <div style="padding:20px 24px 14px 24px; border-bottom:1px solid rgba(127,127,127,0.18);">
                <div style="display:flex; align-items:flex-start; justify-content:space-between; gap:24px; flex-wrap:wrap;">
                    <div style="min-width:260px;">
                        <div style="font-size:12px; font-weight:700; letter-spacing:0.09em; text-transform:uppercase; opacity:0.58;">
                            {"DreamtecLabs"}
                        </div>
                        <div style="font-size:28px; font-weight:700; line-height:1.1; margin-top:3px;">
                            {"Nexus"}
                        </div>
                        <div style="margin-top:7px; opacity:0.68; font-size:13px; max-width:650px; line-height:1.45;">
                            {"Unified operations for Proxmox VE and Backup Server. Live infrastructure data is provided by the Proxmox Datacenter Manager engine."}
                        </div>
                    </div>

                    <div style="display:flex; gap:8px; flex-wrap:wrap; align-items:center; padding-top:4px;">
                        <span style="padding:6px 10px; border:1px solid rgba(127,127,127,0.24); border-radius:999px; font-size:12px; font-weight:600;">
                            {"PVE + PBS"}
                        </span>
                        <span style="padding:6px 10px; border:1px solid rgba(127,127,127,0.24); border-radius:999px; font-size:12px; font-weight:600;">
                            {"Live inventory"}
                        </span>
                        <span style="padding:6px 10px; border:1px solid rgba(127,127,127,0.24); border-radius:999px; font-size:12px; font-weight:600;">
                            {"Frontend-only extension"}
                        </span>
                    </div>
                </div>

                <div style="display:flex; gap:22px; flex-wrap:wrap; margin-top:18px; font-size:13px;">
                    <div style="font-weight:650; border-bottom:2px solid currentColor; padding-bottom:8px;">
                        {"Overview"}
                    </div>
                    <div style="opacity:0.52; padding-bottom:8px;">
                        {"Infrastructure"}
                    </div>
                    <div style="opacity:0.52; padding-bottom:8px;">
                        {"Backups"}
                    </div>
                    <div style="opacity:0.52; padding-bottom:8px;">
                        {"Activity"}
                    </div>
                </div>
            </div>

            <div style="display:flex; flex-direction:column; flex:1 1 auto; min-width:0; width:100%;">
                {upstream_dashboard}
            </div>
        </div>
    }
}
