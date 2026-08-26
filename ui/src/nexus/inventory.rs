use pdm_api_types::resource::ResourcesStatus;
use proxmox_yew_comp::http_get;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew::virtual_dom::VNode;

use crate::guests::GuestPanel;

fn kpi(
    icon: &'static str,
    label: &'static str,
    value: u64,
    detail: String,
    tone: &'static str,
) -> Html {
    html! {
        <section class={classes!("nexus-inventory-kpi", tone)}>
            <span class="nexus-inventory-kpi-icon"><i class={icon}></i></span>
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
                <div class="nexus-inventory-source">
                    <span></span>
                    {"Real-time PDM inventory"}
                    <i class="fa fa-heartbeat"></i>
                </div>
            </header>

            {
                match status.as_ref() {
                    Some(Ok(data)) => summary(data),
                    Some(Err(err)) => html! { <div class="nexus-inventory-status-error"><i class="fa fa-exclamation-triangle"></i>{format!(" Inventory summary unavailable: {err}")}</div> },
                    None => html! { <div class="nexus-inventory-kpis loading"><span>{"Loading inventory summary…"}</span></div> },
                }
            }

            <section class="nexus-inventory-workspace">
                <div class="nexus-inventory-filter-guide">
                    <span class="nexus-filter-heading"><i class="fa fa-filter"></i>{" Smart filters"}</span>
                    <code>{"type:qemu"}</code>
                    <code>{"type:lxc"}</code>
                    <code>{"status:running"}</code>
                    <code>{"node:pve-01"}</code>
                    <code>{"remote:homelab"}</code>
                    <code class="nexus-filter-more">{"+ More"}</code>
                    <span class="nexus-filter-hint">{"Combine qualifiers in the guest filter"}</span>
                </div>

                <div class="nexus-inventory-table-shell">
                    {VNode::from(GuestPanel::new())}
                </div>
            </section>
        </div>
    }
}

const INVENTORY_CSS: &str = r#"
.nexus-inventory{--nx-text:#0b1220;--nx-muted:#64748b;--nx-blue:#2563eb;--nx-green:#16a34a;--nx-orange:#f97316;--nx-border:#dce4ef;--nx-bg:#f6f8fc;width:100%;height:100%;overflow:auto;background:linear-gradient(180deg,#f8faff 0,#f6f8fc 220px);color:var(--nx-text);font-family:"Roboto Flex",Roboto,Arial,Helvetica,sans-serif;font-weight:430;padding-bottom:28px}.nexus-inventory *{box-sizing:border-box}.nexus-inventory-header{display:flex;justify-content:space-between;align-items:flex-start;gap:24px;padding:26px 30px 19px}.nexus-inventory-eyebrow{font-size:10px;font-weight:800;letter-spacing:.14em;color:var(--nx-blue)}.nexus-inventory-title-row{display:flex;align-items:center;gap:9px;margin-top:5px}.nexus-inventory-title-row h1{font-size:26px;margin:0;font-weight:760;letter-spacing:-.03em;color:#070d18}.nexus-inventory-header p{margin:7px 0 0;color:#526076;font-size:12px}.nexus-inventory-live-dot{width:7px;height:7px;border-radius:50%;background:#22c55e;box-shadow:0 0 0 3px #dcfce7}.nexus-inventory-live-label{font-size:11px;color:#64748b}.nexus-inventory-source{display:flex;align-items:center;gap:7px;background:rgba(255,255,255,.9);border:1px solid #dbe3ee;border-radius:999px;padding:8px 13px;font-size:10px;font-weight:680;color:#172033;box-shadow:0 2px 8px rgba(15,23,42,.04)}.nexus-inventory-source span{width:7px;height:7px;border-radius:50%;background:#22c55e}.nexus-inventory-source i{color:#22c55e;margin-left:2px}.nexus-inventory-kpis{display:grid;grid-template-columns:repeat(5,minmax(145px,1fr));gap:12px;padding:0 30px 18px}.nexus-inventory-kpi{background:#fff;border:1px solid var(--nx-border);border-radius:12px;box-shadow:0 3px 12px rgba(15,23,42,.055);padding:13px 15px;display:flex;align-items:center;gap:12px;min-height:92px;position:relative;overflow:hidden;transition:transform .15s ease,box-shadow .15s ease}.nexus-inventory-kpi:hover{transform:translateY(-1px);box-shadow:0 6px 18px rgba(15,23,42,.075)}.nexus-inventory-kpi:after{content:"";position:absolute;left:0;bottom:0;right:0;height:2px;background:#2563eb}.nexus-inventory-kpi.green:after{background:#22c55e}.nexus-inventory-kpi.orange:after{background:#f97316}.nexus-inventory-kpi.slate:after{background:#94a3b8}.nexus-inventory-kpi-icon{width:38px;height:38px;border-radius:11px;background:#edf3ff;color:#2563eb;display:flex;align-items:center;justify-content:center;font-size:15px;flex:none}.nexus-inventory-kpi.green .nexus-inventory-kpi-icon{background:#ecfdf3;color:#15803d}.nexus-inventory-kpi.orange .nexus-inventory-kpi-icon{background:#fff7ed;color:#ea580c}.nexus-inventory-kpi.slate .nexus-inventory-kpi-icon{background:#f1f5f9;color:#475569}.nexus-inventory-kpi-copy{display:flex;flex-direction:column;min-width:0}.nexus-inventory-kpi-copy>span{font-size:10px;font-weight:720;color:#334155}.nexus-inventory-kpi-copy strong{font-size:24px;line-height:1.08;margin-top:2px;color:#050a13;font-weight:790}.nexus-inventory-kpi-copy small{font-size:9px;color:#64748b;margin-top:4px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.nexus-inventory-kpis.loading{display:flex;min-height:92px;align-items:center;color:#64748b;font-size:11px}.nexus-inventory-status-error{margin:0 30px 18px;padding:10px 12px;border:1px solid #fed7aa;background:#fff7ed;border-radius:9px;color:#9a3412;font-size:10px}.nexus-inventory-workspace{margin:0 30px;background:#fff;border:1px solid var(--nx-border);border-radius:13px;box-shadow:0 4px 16px rgba(15,23,42,.055);overflow:hidden}.nexus-inventory-filter-guide{min-height:56px;padding:12px 14px;display:flex;align-items:center;gap:7px;flex-wrap:wrap;color:#334155;font-size:10px;border-bottom:1px solid #e7edf5;background:linear-gradient(180deg,#fff,#fbfcff)}.nexus-filter-heading{font-weight:760;color:#111827;margin-right:4px}.nexus-inventory-filter-guide code{background:#f5f7ff;border:1px solid #dbe4ff;border-radius:999px;padding:5px 10px;color:#1d4ed8;font-family:inherit;font-weight:680;box-shadow:0 1px 2px rgba(37,99,235,.03)}.nexus-inventory-filter-guide code.nexus-filter-more{background:#fff;color:#334155;border-color:#dbe3ee}.nexus-filter-hint{font-size:9px;color:#94a3b8;margin-left:auto}.nexus-inventory-table-shell{background:#fff;overflow:hidden;min-height:420px}.nexus-inventory-table-shell>div{border:0!important}.nexus-inventory-table-shell input{color:#0b1220!important;background:#fff!important;border-color:#d7e0eb!important;border-radius:8px!important;min-height:36px!important;font-size:11px!important;padding-left:11px!important}.nexus-inventory-table-shell input:focus{border-color:#8db4ff!important;box-shadow:0 0 0 3px rgba(37,99,235,.08)!important}.nexus-inventory-table-shell table{color:#111827!important;font-size:11px;border-collapse:separate!important;border-spacing:0!important}.nexus-inventory-table-shell thead th{color:#506078!important;font-weight:720!important;background:#f8fafc!important;border-bottom:1px solid #dfe6ef!important;height:42px!important}.nexus-inventory-table-shell tbody td{color:#111827!important;border-bottom:1px solid #eef2f7!important;height:45px!important;background:#fff!important}.nexus-inventory-table-shell tbody tr:nth-child(even) td{background:#fcfdff!important}.nexus-inventory-table-shell tbody tr:hover td{background:#f4f8ff!important}.nexus-inventory-table-shell tbody tr:focus-within td{background:#eef5ff!important}.nexus-inventory-table-shell button{border-radius:7px!important}.nexus-inventory-table-shell button[aria-pressed="true"]{box-shadow:0 2px 6px rgba(37,99,235,.15)!important}.nexus-inventory-table-shell [class*="toolbar"]{background:#fff!important;border-bottom:1px solid #e7edf5!important;padding:10px 12px!important;min-height:56px!important}.nexus-inventory-table-shell [class*="segmented"]{border-radius:8px!important;overflow:hidden!important}.nexus-inventory-table-shell [role="button"]{transition:background-color .12s ease,transform .12s ease}.nexus-inventory-table-shell [role="button"]:hover{transform:translateY(-1px)}.nexus-inventory-table-shell td:last-child{white-space:nowrap}.nexus-inventory-table-shell td:last-child [role="button"]{width:28px!important;height:28px!important;min-width:28px!important;border:1px solid #dce4ef!important;border-radius:7px!important;background:#fff!important;margin-left:2px!important}.nexus-inventory-table-shell td:last-child [role="button"]:hover{background:#f5f8ff!important;border-color:#bfd0e9!important}.nexus-inventory-table-shell td:last-child [aria-disabled="true"]{opacity:.38!important}.nexus-inventory-table-shell .pwt-loading-icon{color:#2563eb!important}.nexus-inventory-table-shell [class*="primary"]{border-radius:7px!important}.nexus-inventory-table-shell ::-webkit-scrollbar{height:10px;width:10px}.nexus-inventory-table-shell ::-webkit-scrollbar-thumb{background:#ccd6e3;border:2px solid #fff;border-radius:999px}.nexus-inventory-table-shell ::-webkit-scrollbar-track{background:#fff}@media(max-width:1200px){.nexus-inventory-kpis{grid-template-columns:repeat(3,1fr)}.nexus-filter-hint{display:none}}@media(max-width:850px){.nexus-inventory-header{flex-direction:column}.nexus-inventory-kpis{grid-template-columns:1fr 1fr}.nexus-inventory-header,.nexus-inventory-kpis{padding-left:16px;padding-right:16px}.nexus-inventory-workspace,.nexus-inventory-status-error{margin-left:16px;margin-right:16px}}@media(max-width:620px){.nexus-inventory-kpis{grid-template-columns:1fr}.nexus-inventory-header{padding-top:18px}.nexus-inventory-source{display:none}.nexus-inventory-filter-guide{align-items:flex-start}.nexus-inventory-workspace{border-radius:10px}}
"#;
