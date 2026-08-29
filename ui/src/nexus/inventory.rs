use pdm_api_types::resource::ResourcesStatus;
use proxmox_yew_comp::http_get;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew::virtual_dom::VNode;

use super::super::guests::GuestPanel;

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
            {match status.as_ref() {
                Some(Ok(data)) => summary(data),
                Some(Err(err)) => html! {
                    <div class="nexus-inventory-status-error">
                        <i class="fa fa-exclamation-triangle"></i>
                        {format!(" Inventory summary unavailable: {err}")}
                    </div>
                },
                None => html! {
                    <div class="nexus-inventory-kpis loading">
                        <span>{"Loading inventory summary…"}</span>
                    </div>
                },
            }}
            <section class="nexus-inventory-workspace">
                <div class="nexus-inventory-filter-guide">
                    <span class="nexus-filter-heading"><i class="fa fa-filter"></i>{" Smart filters"}</span>
                    <code>{"type:qemu"}</code>
                    <code>{"type:lxc"}</code>
                    <code>{"status:running"}</code>
                    <code>{"node:pve-01"}</code>
                    <code>{"remote:homelab"}</code>
                    <code class="nexus-filter-more">{"+ More"}</code>
                    <span class="nexus-filter-hint"><span class="nexus-inventory-live-dot"></span>{"Live from PDM"}</span>
                </div>
                <div class="nexus-inventory-table-shell">{VNode::from(GuestPanel::nexus())}</div>
            </section>
        </div>
    }
}

const INVENTORY_CSS: &str = r#"
.nexus-inventory{--nx-text:#0b1220;--nx-muted:#64748b;--nx-blue:#2563eb;--nx-green:#16a34a;--nx-orange:#f97316;--nx-border:#dce4ef;width:100%;height:100%;overflow:auto;background:linear-gradient(180deg,#f8faff 0,#f6f8fc 180px);color:var(--nx-text);font-family:"Roboto Flex",Roboto,Arial,Helvetica,sans-serif;font-weight:430;padding:14px 0 20px}.nexus-inventory *{box-sizing:border-box}.nexus-inventory-live-dot{display:inline-block;width:7px;height:7px;border-radius:50%;background:#22c55e;box-shadow:0 0 0 3px #dcfce7;margin-right:7px}.nexus-inventory-kpis{display:grid;grid-template-columns:repeat(5,minmax(145px,1fr));gap:10px;padding:0 30px 12px}.nexus-inventory-kpi{background:#fff;border:1px solid var(--nx-border);border-radius:11px;box-shadow:0 2px 9px rgba(15,23,42,.05);padding:10px 13px;display:flex;align-items:center;gap:10px;min-height:76px;position:relative;overflow:hidden}.nexus-inventory-kpi:after{content:"";position:absolute;left:0;bottom:0;right:0;height:2px;background:#2563eb}.nexus-inventory-kpi.green:after{background:#22c55e}.nexus-inventory-kpi.orange:after{background:#f97316}.nexus-inventory-kpi.slate:after{background:#94a3b8}.nexus-inventory-kpi-icon{width:34px;height:34px;border-radius:9px;background:#edf3ff;color:#2563eb;display:flex;align-items:center;justify-content:center;font-size:14px;flex:none}.nexus-inventory-kpi.green .nexus-inventory-kpi-icon{background:#ecfdf3;color:#15803d}.nexus-inventory-kpi.orange .nexus-inventory-kpi-icon{background:#fff7ed;color:#ea580c}.nexus-inventory-kpi.slate .nexus-inventory-kpi-icon{background:#f1f5f9;color:#475569}.nexus-inventory-kpi-copy{display:flex;flex-direction:column;min-width:0}.nexus-inventory-kpi-copy>span{font-size:9px;font-weight:720;color:#334155}.nexus-inventory-kpi-copy strong{font-size:21px;line-height:1.05;margin-top:1px;color:#050a13;font-weight:790}.nexus-inventory-kpi-copy small{font-size:8px;color:#64748b;margin-top:2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.nexus-inventory-kpis.loading{display:flex;min-height:76px;align-items:center;color:#64748b;font-size:11px}.nexus-inventory-status-error{margin:0 30px 12px;padding:9px 11px;border:1px solid #fed7aa;background:#fff7ed;border-radius:8px;color:#9a3412;font-size:10px}.nexus-inventory-workspace{margin:0 30px;background:#fff;border:1px solid var(--nx-border);border-radius:12px;box-shadow:0 3px 13px rgba(15,23,42,.05);overflow:hidden}.nexus-inventory-filter-guide{min-height:44px;padding:8px 12px;display:flex;align-items:center;gap:6px;flex-wrap:wrap;color:#334155;font-size:9px;border-bottom:1px solid #e7edf5;background:linear-gradient(180deg,#fff,#fbfcff)}.nexus-filter-heading{font-weight:760;color:#111827;margin-right:3px}.nexus-inventory-filter-guide code{background:#f5f7ff;border:1px solid #dbe4ff;border-radius:999px;padding:4px 8px;color:#1d4ed8;font-family:inherit;font-weight:680}.nexus-inventory-filter-guide code.nexus-filter-more{background:#fff;color:#334155;border-color:#dbe3ee}.nexus-filter-hint{font-size:9px;color:#64748b;margin-left:auto;display:flex;align-items:center;white-space:nowrap}.nexus-inventory-table-shell{background:#fff;overflow:hidden;min-height:420px}.nexus-inventory-table-shell>div{border:0!important}.nexus-inventory-table-shell input{color:#0b1220!important;background:#fff!important;border-color:#d7e0eb!important;border-radius:7px!important;min-height:32px!important;font-size:10px!important;padding-left:10px!important}.nexus-inventory-table-shell input:focus{border-color:#8db4ff!important;box-shadow:0 0 0 3px rgba(37,99,235,.08)!important}.nexus-inventory-table-shell table{color:#111827!important;font-size:10px;border-collapse:separate!important;border-spacing:0!important}.nexus-inventory-table-shell thead th{color:#506078!important;font-weight:720!important;background:#f8fafc!important;border-bottom:1px solid #dfe6ef!important;height:38px!important}.nexus-inventory-table-shell tbody td{color:#111827!important;border-bottom:1px solid #eef2f7!important;height:41px!important;background:#fff!important}.nexus-inventory-table-shell tbody tr:nth-child(even) td{background:#fcfdff!important}.nexus-inventory-table-shell tbody tr:hover td{background:#f4f8ff!important;cursor:pointer}.nexus-inventory-table-shell button{border-radius:7px!important}.nexus-inventory-table-shell button[aria-pressed="true"]{box-shadow:0 2px 6px rgba(37,99,235,.15)!important}.nexus-inventory-table-shell [class*="toolbar"]{background:#fff!important;border-bottom:1px solid #e7edf5!important;padding:7px 10px!important;min-height:46px!important}.nexus-inventory-table-shell [class*="segmented"]{border-radius:8px!important;overflow:hidden!important}.nexus-inventory-table-shell th:last-child{width:92px!important;max-width:92px!important}.nexus-inventory-table-shell td:last-child{white-space:nowrap;width:92px!important;max-width:92px!important}.nexus-row-actions{display:flex!important;align-items:center;justify-content:flex-end;gap:6px;padding:0 4px}.nexus-row-actions button{width:28px!important;height:28px!important;min-width:28px!important;border:1px solid #dce4ef!important;border-radius:7px!important;background:#fff!important;color:#64748b!important;display:inline-flex!important;align-items:center!important;justify-content:center!important;cursor:pointer!important}.nexus-row-actions button:hover{background:#f5f8ff!important;border-color:#bfd0e9!important;transform:translateY(-1px)}.nexus-row-actions .nexus-primary-action.shutdown{color:#2563eb!important;background:#f8fbff!important}.nexus-row-actions .nexus-primary-action.start{color:#16a34a!important;background:#f7fff9!important}.nexus-row-actions .nexus-primary-action.resume{color:#d97706!important;background:#fffbeb!important}.nexus-inventory-table-shell .pwt-loading-icon{color:#2563eb!important}.nexus-guest-drawer{position:fixed;top:69px;right:0;bottom:0;width:400px;background:#fff;border-left:1px solid #dce4ef;box-shadow:-8px 0 30px rgba(15,23,42,.08);z-index:180;display:flex;flex-direction:column;animation:nexusDrawerIn .18s ease-out}.nexus-guest-drawer-head{min-height:92px;padding:18px 18px 12px;display:flex;justify-content:space-between;gap:14px;border-bottom:1px solid #edf1f6}.nexus-guest-drawer-title h2{margin:0;color:#0f172a;font-size:17px;line-height:1.2;font-weight:780}.nexus-guest-drawer-meta{display:flex;align-items:center;gap:12px;margin-top:12px;font-size:10px;color:#475569}.nexus-guest-status,.nexus-guest-inline-status{display:inline-flex;align-items:center;gap:6px}.nexus-guest-status i,.nexus-guest-inline-status i{font-size:7px;color:#94a3b8}.nexus-guest-status.running i,.nexus-guest-inline-status.running i{color:#16a34a}.nexus-guest-drawer-close{width:30px;height:30px;border:0;background:#fff;color:#111827;font-size:14px;cursor:pointer;border-radius:7px}.nexus-guest-drawer-close:hover{background:#f1f5f9}.nexus-guest-tabs{height:45px;display:flex;align-items:flex-end;gap:19px;padding:0 18px;border-bottom:1px solid #e7edf5;overflow-x:auto}.nexus-guest-tabs span{height:45px;display:flex;align-items:center;white-space:nowrap;font-size:9px;color:#475569;border-bottom:2px solid transparent}.nexus-guest-tabs span.active{color:#2563eb;border-bottom-color:#2563eb;font-weight:700}.nexus-guest-drawer-body{padding:14px;overflow:auto;display:flex;flex-direction:column;gap:12px}.nexus-guest-card{border:1px solid #dce4ef;border-radius:9px;padding:14px;background:#fff}.nexus-guest-card h3{font-size:11px;margin:0 0 13px;color:#111827;font-weight:760}.nexus-guest-card dl{margin:0;display:grid;gap:10px}.nexus-guest-card dl>div{display:grid;grid-template-columns:110px 1fr;gap:12px;align-items:center}.nexus-guest-card dt{font-size:9px;color:#475569}.nexus-guest-card dd{margin:0;font-size:9px;color:#0f172a;font-weight:520;min-width:0;overflow:hidden;text-overflow:ellipsis}.nexus-link-button{border:0;background:transparent;color:#2563eb;padding:0;font-size:9px;cursor:pointer}.nexus-link-button:hover{text-decoration:underline}.nexus-resource-row{margin-bottom:15px}.nexus-resource-row>div:first-child{display:flex;align-items:center;justify-content:space-between;gap:12px;font-size:9px;color:#475569;margin-bottom:6px}.nexus-resource-row strong{font-weight:600;color:#0f172a}.nexus-resource-track{height:5px;border-radius:999px;background:#e8edf4;overflow:hidden}.nexus-resource-track span{display:block;height:100%;border-radius:999px;background:#2563eb}.nexus-resource-note{margin:4px 0 0;color:#94a3b8;font-size:8px;line-height:1.45}.nexus-guest-tags{display:flex;gap:6px;flex-wrap:wrap}.nexus-guest-tags span{background:#eef4ff;border:1px solid #dbe7ff;border-radius:999px;padding:5px 8px;color:#2563eb;font-size:8px}.nexus-guest-tags span.empty{background:#f8fafc;border-color:#e2e8f0;color:#94a3b8}@keyframes nexusDrawerIn{from{transform:translateX(16px);opacity:.5}to{transform:translateX(0);opacity:1}}@media(max-width:1200px){.nexus-inventory-kpis{grid-template-columns:repeat(3,1fr)}.nexus-filter-hint{display:none}.nexus-guest-drawer{width:380px;box-shadow:-14px 0 40px rgba(15,23,42,.16)}}@media(max-width:850px){.nexus-inventory-kpis{grid-template-columns:1fr 1fr;padding-left:16px;padding-right:16px}.nexus-inventory-workspace,.nexus-inventory-status-error{margin-left:16px;margin-right:16px}.nexus-guest-drawer{width:min(92vw,400px)}}@media(max-width:620px){.nexus-inventory{padding-top:10px}.nexus-inventory-kpis{grid-template-columns:1fr}.nexus-inventory-filter-guide{align-items:flex-start}.nexus-inventory-workspace{border-radius:10px}}
"#;
