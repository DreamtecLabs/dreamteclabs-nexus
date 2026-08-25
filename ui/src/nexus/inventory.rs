use pdm_api_types::resource::ResourcesStatus;
use proxmox_yew_comp::http_get;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

use crate::guests::GuestPanel;

fn kpi(icon: &str, label: &str, value: u64, detail: String, tone: &str) -> Html {
    html! {
        <section class={classes!("nexus-inventory-kpi", tone)}>
            <span class="nexus-inventory-kpi-icon"><i class={icon.to_string()}></i></span>
            <div class="nexus-inventory-kpi-copy">
                <span>{label}</span>
                <strong>{value}</strong>
                <small>{detail}</small>
            </div>
        </section>
    }
}

fn summary(status: &ResourcesStatus) -> Html {
    let vm_total =
        status.qemu.running + status.qemu.stopped + status.qemu.template + status.qemu.unknown;
    let lxc_total =
        status.lxc.running + status.lxc.stopped + status.lxc.template + status.lxc.unknown;
    let running = status.qemu.running + status.lxc.running;
    let stopped = status.qemu.stopped + status.lxc.stopped;
    let total = vm_total + lxc_total;

    html! {
        <div class="nexus-inventory-kpis">
            {kpi("fa fa-cubes", "Total Guests", total, format!("{} workloads managed", total), "blue")}
            {kpi("fa fa-play", "Running", running, format!("{}% of guests", if total == 0 { 0 } else { running * 100 / total }), "green")}
            {kpi("fa fa-stop", "Stopped", stopped, format!("{} currently offline", stopped), "slate")}
            {kpi("fa fa-desktop", "Virtual Machines", vm_total, format!("{} running", status.qemu.running), "blue")}
            {kpi("fa fa-cube", "Linux Containers", lxc_total, format!("{} running", status.lxc.running), "orange")}
        </div>
    }
}

#[function_component(NexusInventory)]
pub fn nexus_inventory() -> Html {
    let status = use_state(|| None::<Result<ResourcesStatus, String>>);
    {
        let status = status.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let result: Result<ResourcesStatus, _> = http_get("/resources/status", None).await;
                status.set(Some(result.map_err(|err| err.to_string())));
            });
            || ()
        });
    }

    html! {
        <div class="nexus-inventory">
            <style>{INVENTORY_CSS}</style>
            <header class="nexus-inventory-header">
                <div>
                    <div class="nexus-inventory-eyebrow">{"WORKLOADS"}</div>
                    <div class="nexus-inventory-title-row">
                        <h1>{"Inventory"}</h1>
                        <span class="nexus-inventory-live-dot"></span>
                        <span class="nexus-inventory-live-label">{"Live from PDM"}</span>
                    </div>
                    <p>{"Manage and monitor all virtual machines and containers across your infrastructure."}</p>
                </div>
                <div class="nexus-inventory-source"><span></span>{"Real-time PDM inventory"}</div>
            </header>

            {
                match status.as_ref() {
                    Some(Ok(data)) => summary(data),
                    Some(Err(err)) => html! { <div class="nexus-inventory-status-error"><i class="fa fa-exclamation-triangle"></i>{format!(" Inventory summary unavailable: {err}")}</div> },
                    None => html! { <div class="nexus-inventory-kpis loading"><span>{"Loading inventory summary…"}</span></div> },
                }
            }

            <div class="nexus-inventory-filter-guide">
                <span><i class="fa fa-filter"></i>{" Smart filters"}</span>
                <code>{"type:qemu"}</code>
                <code>{"type:lxc"}</code>
                <code>{"status:running"}</code>
                <code>{"node:pve-01"}</code>
                <code>{"remote:homelab"}</code>
                <code>{"tag:..."}</code>
                <em>{"Use these qualifiers in the guest search; they can be combined."}</em>
            </div>

            <section class="nexus-inventory-table-shell">
                {GuestPanel::new()}
            </section>
        </div>
    }
}

const INVENTORY_CSS: &str = r#"
.nexus-inventory{--nx-text:#0b1220;--nx-muted:#475569;--nx-blue:#2563eb;--nx-green:#16a34a;--nx-orange:#f97316;width:100%;height:100%;overflow:auto;background:#f6f8fc;color:var(--nx-text);font-family:"Roboto Flex",Roboto,Arial,Helvetica,sans-serif;font-weight:430;padding-bottom:24px}.nexus-inventory *{box-sizing:border-box}.nexus-inventory-header{display:flex;justify-content:space-between;align-items:flex-start;gap:24px;padding:25px 30px 18px}.nexus-inventory-eyebrow{font-size:10px;font-weight:800;letter-spacing:.13em;color:var(--nx-blue)}.nexus-inventory-title-row{display:flex;align-items:center;gap:9px;margin-top:5px}.nexus-inventory-title-row h1{font-size:25px;margin:0;font-weight:750;letter-spacing:-.025em;color:#070d18}.nexus-inventory-header p{margin:6px 0 0;color:#475569;font-size:12px}.nexus-inventory-live-dot{width:7px;height:7px;border-radius:50%;background:#22c55e;box-shadow:0 0 0 3px #dcfce7}.nexus-inventory-live-label{font-size:11px;color:#64748b}.nexus-inventory-source{display:flex;align-items:center;gap:7px;background:#fff;border:1px solid #dbe3ee;border-radius:999px;padding:7px 12px;font-size:10px;font-weight:650;color:#172033;box-shadow:0 1px 2px rgba(15,23,42,.04)}.nexus-inventory-source span{width:7px;height:7px;border-radius:50%;background:#22c55e}.nexus-inventory-kpis{display:grid;grid-template-columns:repeat(5,minmax(145px,1fr));gap:11px;padding:0 30px 13px}.nexus-inventory-kpi{background:#fff;border:1px solid #dce4ef;border-radius:10px;box-shadow:0 2px 7px rgba(15,23,42,.05);padding:12px 14px;display:flex;align-items:center;gap:12px;min-height:92px;position:relative;overflow:hidden}.nexus-inventory-kpi:after{content:"";position:absolute;left:0;bottom:0;right:0;height:2px;background:#2563eb}.nexus-inventory-kpi.green:after{background:#22c55e}.nexus-inventory-kpi.orange:after{background:#f97316}.nexus-inventory-kpi.slate:after{background:#94a3b8}.nexus-inventory-kpi-icon{width:34px;height:34px;border-radius:9px;background:#edf3ff;color:#2563eb;display:flex;align-items:center;justify-content:center;font-size:14px;flex:none}.nexus-inventory-kpi.green .nexus-inventory-kpi-icon{background:#ecfdf3;color:#15803d}.nexus-inventory-kpi.orange .nexus-inventory-kpi-icon{background:#fff7ed;color:#ea580c}.nexus-inventory-kpi.slate .nexus-inventory-kpi-icon{background:#f1f5f9;color:#475569}.nexus-inventory-kpi-copy{display:flex;flex-direction:column;min-width:0}.nexus-inventory-kpi-copy>span{font-size:10px;font-weight:700;color:#334155}.nexus-inventory-kpi-copy strong{font-size:24px;line-height:1.1;margin-top:2px;color:#050a13;font-weight:780}.nexus-inventory-kpi-copy small{font-size:9px;color:#64748b;margin-top:3px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.nexus-inventory-kpis.loading{display:flex;min-height:92px;align-items:center;color:#64748b;font-size:11px}.nexus-inventory-status-error{margin:0 30px 13px;padding:10px 12px;border:1px solid #fed7aa;background:#fff7ed;border-radius:8px;color:#9a3412;font-size:10px}.nexus-inventory-filter-guide{margin:0 30px 10px;display:flex;align-items:center;gap:7px;flex-wrap:wrap;color:#334155;font-size:10px}.nexus-inventory-filter-guide>span{font-weight:750;color:#111827}.nexus-inventory-filter-guide code{background:#fff;border:1px solid #dbe3ee;border-radius:999px;padding:4px 8px;color:#1d4ed8;font-family:inherit;font-weight:650}.nexus-inventory-filter-guide em{font-style:normal;color:#64748b}.nexus-inventory-table-shell{margin:0 30px;background:#fff;border:1px solid #dce4ef;border-radius:11px;box-shadow:0 2px 7px rgba(15,23,42,.055);overflow:hidden;min-height:420px}.nexus-inventory-table-shell input{color:#0b1220!important}.nexus-inventory-table-shell table{color:#111827!important;font-size:11px}.nexus-inventory-table-shell th{color:#334155!important;font-weight:750!important;background:#f8fafc!important}.nexus-inventory-table-shell td{color:#111827!important}.nexus-inventory-table-shell tr:hover td{background:#f8fbff!important}@media(max-width:1200px){.nexus-inventory-kpis{grid-template-columns:repeat(3,1fr)}}@media(max-width:850px){.nexus-inventory-header{flex-direction:column}.nexus-inventory-kpis{grid-template-columns:1fr 1fr}.nexus-inventory-header,.nexus-inventory-kpis{padding-left:16px;padding-right:16px}.nexus-inventory-table-shell,.nexus-inventory-filter-guide,.nexus-inventory-status-error{margin-left:16px;margin-right:16px}}@media(max-width:620px){.nexus-inventory-kpis{grid-template-columns:1fr}}
"#;
