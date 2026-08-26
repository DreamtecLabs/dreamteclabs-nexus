use pdm_api_types::resource::ResourcesStatus;
use proxmox_yew_comp::http_get;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;
use yew::virtual_dom::VNode;

use crate::guests::GuestPanel;

#[derive(Clone, PartialEq)]
struct GuestRowDetails {
    name: String,
    id: String,
    status: String,
    remote: String,
    node: String,
    tags: String,
    cpu: String,
    memory: String,
    uptime: String,
}

fn cell_text(cells: &web_sys::HtmlCollection, index: u32) -> String {
    cells
        .item(index)
        .and_then(|cell| cell.text_content())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn details_from_click(event: &MouseEvent) -> Option<GuestRowDetails> {
    let mut element = event.target()?.dyn_into::<web_sys::Element>().ok()?;

    loop {
        let tag = element.tag_name();
        if tag == "BUTTON" || element.get_attribute("role").as_deref() == Some("button") {
            return None;
        }
        if tag == "TR" {
            break;
        }
        element = element.parent_element()?;
    }

    let cells = element.children();
    let count = cells.length();
    if count < 9 {
        return None;
    }

    let id = cell_text(&cells, 1);
    if id.is_empty() {
        return None;
    }

    let (remote, node, tags, cpu, memory, uptime) = if count >= 10 {
        (
            cell_text(&cells, 3),
            cell_text(&cells, 4),
            cell_text(&cells, 5),
            cell_text(&cells, 6),
            cell_text(&cells, 7),
            cell_text(&cells, 8),
        )
    } else {
        (
            String::from("Grouped view"),
            cell_text(&cells, 3),
            cell_text(&cells, 4),
            cell_text(&cells, 5),
            cell_text(&cells, 6),
            cell_text(&cells, 7),
        )
    };

    Some(GuestRowDetails {
        name: cell_text(&cells, 0),
        id,
        status: cell_text(&cells, 2),
        remote,
        node,
        tags,
        cpu,
        memory,
        uptime,
    })
}

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
            <div class="nexus-inventory-kpi-copy"><span>{label}</span><strong>{value}</strong><small>{detail}</small></div>
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

fn detail_drawer(details: &GuestRowDetails, close: Callback<MouseEvent>) -> Html {
    let running = details.status.to_lowercase().contains("running");
    html! {
        <aside class="nexus-guest-drawer" role="complementary" aria-label="Guest details">
            <div class="nexus-guest-drawer-head">
                <div>
                    <span class="nexus-guest-drawer-eyebrow">{"GUEST DETAILS"}</span>
                    <h2>{details.name.clone()}</h2>
                    <div class="nexus-guest-drawer-meta">
                        <span>{format!("ID {}", details.id)}</span>
                        <span class={classes!("nexus-guest-status", running.then_some("running"))}>{details.status.clone()}</span>
                    </div>
                </div>
                <button class="nexus-guest-drawer-close" aria-label="Close guest details" onclick={close}><i class="fa fa-times"></i></button>
            </div>
            <div class="nexus-guest-tabs"><span class="active">{"Overview"}</span><span>{"Runtime"}</span></div>
            <div class="nexus-guest-drawer-body">
                <section>
                    <h3>{"Placement"}</h3>
                    <dl>
                        <div><dt>{"Remote"}</dt><dd>{details.remote.clone()}</dd></div>
                        <div><dt>{"Node"}</dt><dd>{details.node.clone()}</dd></div>
                    </dl>
                </section>
                <section>
                    <h3>{"Runtime"}</h3>
                    <dl>
                        <div><dt>{"CPU usage"}</dt><dd>{details.cpu.clone()}</dd></div>
                        <div><dt>{"Memory"}</dt><dd>{details.memory.clone()}</dd></div>
                        <div><dt>{"Uptime"}</dt><dd>{details.uptime.clone()}</dd></div>
                    </dl>
                </section>
                <section>
                    <h3>{"Tags"}</h3>
                    <p class="nexus-guest-tags">{if details.tags.is_empty() { "No tags".to_string() } else { details.tags.clone() }}</p>
                </section>
                <p class="nexus-guest-drawer-note"><i class="fa fa-info-circle"></i>{" Use the row action tray for lifecycle, snapshots, migration and PVE deep-link operations."}</p>
            </div>
        </aside>
    }
}

#[function_component(NexusInventory)]
pub fn nexus_inventory() -> Html {
    let status = use_state(|| None::<Result<ResourcesStatus, String>>);
    let selected = use_state(|| None::<GuestRowDetails>);

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

    let on_table_click = {
        let selected = selected.clone();
        Callback::from(move |event: MouseEvent| {
            if let Some(details) = details_from_click(&event) {
                selected.set(Some(details));
            }
        })
    };
    let close_drawer = {
        let selected = selected.clone();
        Callback::from(move |_| selected.set(None))
    };

    html! {
        <div class="nexus-inventory">
            <style>{INVENTORY_CSS}</style>
            {match status.as_ref() {
                Some(Ok(data)) => summary(data),
                Some(Err(err)) => html! { <div class="nexus-inventory-status-error"><i class="fa fa-exclamation-triangle"></i>{format!(" Inventory summary unavailable: {err}")}</div> },
                None => html! { <div class="nexus-inventory-kpis loading"><span>{"Loading inventory summary…"}</span></div> },
            }}
            <section class="nexus-inventory-workspace">
                <div class="nexus-inventory-filter-guide">
                    <span class="nexus-filter-heading"><i class="fa fa-filter"></i>{" Smart filters"}</span>
                    <code>{"type:qemu"}</code><code>{"type:lxc"}</code><code>{"status:running"}</code><code>{"node:pve-01"}</code><code>{"remote:homelab"}</code><code class="nexus-filter-more">{"+ More"}</code>
                    <span class="nexus-filter-hint"><span class="nexus-inventory-live-dot"></span>{"Live from PDM"}</span>
                </div>
                <div class="nexus-inventory-table-shell" onclick={on_table_click}>{VNode::from(GuestPanel::new())}</div>
            </section>
            {selected.as_ref().map(|details| detail_drawer(details, close_drawer))}
        </div>
    }
}

const INVENTORY_CSS: &str = r#"
.nexus-inventory{--nx-text:#0b1220;--nx-muted:#64748b;--nx-blue:#2563eb;--nx-green:#16a34a;--nx-orange:#f97316;--nx-border:#dce4ef;width:100%;height:100%;overflow:auto;background:linear-gradient(180deg,#f8faff 0,#f6f8fc 180px);color:var(--nx-text);font-family:"Roboto Flex",Roboto,Arial,Helvetica,sans-serif;font-weight:430;padding:14px 0 20px}.nexus-inventory *{box-sizing:border-box}.nexus-inventory-live-dot{display:inline-block;width:7px;height:7px;border-radius:50%;background:#22c55e;box-shadow:0 0 0 3px #dcfce7;margin-right:7px}.nexus-inventory-kpis{display:grid;grid-template-columns:repeat(5,minmax(145px,1fr));gap:10px;padding:0 30px 12px}.nexus-inventory-kpi{background:#fff;border:1px solid var(--nx-border);border-radius:11px;box-shadow:0 2px 9px rgba(15,23,42,.05);padding:10px 13px;display:flex;align-items:center;gap:10px;min-height:76px;position:relative;overflow:hidden}.nexus-inventory-kpi:after{content:"";position:absolute;left:0;bottom:0;right:0;height:2px;background:#2563eb}.nexus-inventory-kpi.green:after{background:#22c55e}.nexus-inventory-kpi.orange:after{background:#f97316}.nexus-inventory-kpi.slate:after{background:#94a3b8}.nexus-inventory-kpi-icon{width:34px;height:34px;border-radius:9px;background:#edf3ff;color:#2563eb;display:flex;align-items:center;justify-content:center;font-size:14px;flex:none}.nexus-inventory-kpi.green .nexus-inventory-kpi-icon{background:#ecfdf3;color:#15803d}.nexus-inventory-kpi.orange .nexus-inventory-kpi-icon{background:#fff7ed;color:#ea580c}.nexus-inventory-kpi.slate .nexus-inventory-kpi-icon{background:#f1f5f9;color:#475569}.nexus-inventory-kpi-copy{display:flex;flex-direction:column;min-width:0}.nexus-inventory-kpi-copy>span{font-size:9px;font-weight:720;color:#334155}.nexus-inventory-kpi-copy strong{font-size:21px;line-height:1.05;margin-top:1px;color:#050a13;font-weight:790}.nexus-inventory-kpi-copy small{font-size:8px;color:#64748b;margin-top:2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.nexus-inventory-kpis.loading{display:flex;min-height:76px;align-items:center;color:#64748b;font-size:11px}.nexus-inventory-status-error{margin:0 30px 12px;padding:9px 11px;border:1px solid #fed7aa;background:#fff7ed;border-radius:8px;color:#9a3412;font-size:10px}.nexus-inventory-workspace{margin:0 30px;background:#fff;border:1px solid var(--nx-border);border-radius:12px;box-shadow:0 3px 13px rgba(15,23,42,.05);overflow:hidden}.nexus-inventory-filter-guide{min-height:44px;padding:8px 12px;display:flex;align-items:center;gap:6px;flex-wrap:wrap;color:#334155;font-size:9px;border-bottom:1px solid #e7edf5;background:linear-gradient(180deg,#fff,#fbfcff)}.nexus-filter-heading{font-weight:760;color:#111827;margin-right:3px}.nexus-inventory-filter-guide code{background:#f5f7ff;border:1px solid #dbe4ff;border-radius:999px;padding:4px 8px;color:#1d4ed8;font-family:inherit;font-weight:680}.nexus-inventory-filter-guide code.nexus-filter-more{background:#fff;color:#334155;border-color:#dbe3ee}.nexus-filter-hint{font-size:9px;color:#64748b;margin-left:auto;display:flex;align-items:center;white-space:nowrap}.nexus-inventory-table-shell{background:#fff;overflow:hidden;min-height:420px}.nexus-inventory-table-shell>div{border:0!important}.nexus-inventory-table-shell input{color:#0b1220!important;background:#fff!important;border-color:#d7e0eb!important;border-radius:7px!important;min-height:32px!important;font-size:10px!important}.nexus-inventory-table-shell table{color:#111827!important;font-size:10px;border-collapse:separate!important;border-spacing:0!important}.nexus-inventory-table-shell thead th{color:#506078!important;font-weight:720!important;background:#f8fafc!important;border-bottom:1px solid #dfe6ef!important;height:38px!important}.nexus-inventory-table-shell tbody td{color:#111827!important;border-bottom:1px solid #eef2f7!important;height:41px!important;background:#fff!important}.nexus-inventory-table-shell tbody tr:nth-child(even) td{background:#fcfdff!important}.nexus-inventory-table-shell tbody tr:hover td{background:#f4f8ff!important;cursor:pointer}.nexus-inventory-table-shell button{border-radius:7px!important}.nexus-inventory-table-shell [class*="toolbar"]{background:#fff!important;border-bottom:1px solid #e7edf5!important;padding:7px 10px!important;min-height:46px!important}.nexus-inventory-table-shell th:last-child{width:112px!important;max-width:112px!important}.nexus-inventory-table-shell td:last-child{white-space:nowrap;width:112px!important;max-width:112px!important;position:relative;overflow:visible!important}.nexus-inventory-table-shell td:last-child>div{position:relative;display:flex!important;justify-content:flex-end!important;min-width:72px}.nexus-inventory-table-shell td:last-child [role="button"]{width:26px!important;height:26px!important;min-width:26px!important;border:1px solid #dce4ef!important;border-radius:7px!important;background:#fff!important;margin-left:3px!important}.nexus-inventory-table-shell td:last-child>div>[role="button"]{display:none!important}.nexus-inventory-table-shell td:last-child>div>[role="button"]:first-of-type:not([aria-disabled="true"]){display:flex!important}.nexus-inventory-table-shell td:last-child>div:has(>[role="button"]:first-of-type[aria-disabled="true"])>[role="button"]:nth-of-type(2):not([aria-disabled="true"]){display:flex!important}.nexus-inventory-table-shell td:last-child>div:after{content:"•••";letter-spacing:1px;width:28px;height:26px;margin-left:4px;border:1px solid #dce4ef;border-radius:7px;background:#fff;color:#64748b;display:flex;align-items:center;justify-content:center;font-weight:700}.nexus-inventory-table-shell td:last-child>div:hover,.nexus-inventory-table-shell td:last-child>div:focus-within{position:absolute;right:7px;top:50%;transform:translateY(-50%);z-index:20;padding:4px 5px;background:#fff;border:1px solid #dce4ef;border-radius:9px;box-shadow:0 8px 24px rgba(15,23,42,.14)}.nexus-inventory-table-shell td:last-child>div:hover>[role="button"],.nexus-inventory-table-shell td:last-child>div:focus-within>[role="button"]{display:flex!important}.nexus-inventory-table-shell td:last-child>div:hover>[aria-disabled="true"],.nexus-inventory-table-shell td:last-child>div:focus-within>[aria-disabled="true"]{display:none!important}.nexus-inventory-table-shell td:last-child>div:hover:after,.nexus-inventory-table-shell td:last-child>div:focus-within:after{display:none}.nexus-guest-drawer{position:fixed;z-index:90;top:70px;right:18px;bottom:18px;width:min(370px,calc(100vw - 36px));background:#fff;border:1px solid #dbe4ef;border-radius:14px;box-shadow:0 22px 60px rgba(15,23,42,.22);overflow:auto;animation:nexusDrawerIn .16s ease-out}.nexus-guest-drawer-head{display:flex;justify-content:space-between;gap:14px;padding:20px 20px 14px;border-bottom:1px solid #eef2f7}.nexus-guest-drawer-eyebrow{font-size:9px;font-weight:800;letter-spacing:.12em;color:#64748b}.nexus-guest-drawer h2{font-size:20px;margin:3px 0 7px;color:#0b1220}.nexus-guest-drawer-meta{display:flex;align-items:center;gap:8px;font-size:10px;color:#64748b}.nexus-guest-status{padding:3px 7px;border-radius:999px;background:#f1f5f9;color:#475569;font-weight:700}.nexus-guest-status.running{background:#dcfce7;color:#15803d}.nexus-guest-drawer-close{width:30px;height:30px;border:1px solid #dbe4ef;background:#fff;color:#64748b;border-radius:8px;cursor:pointer}.nexus-guest-tabs{display:flex;gap:18px;padding:0 20px;border-bottom:1px solid #eef2f7;font-size:10px;font-weight:700;color:#64748b}.nexus-guest-tabs span{padding:11px 0 9px}.nexus-guest-tabs .active{color:#1d4ed8;border-bottom:2px solid #2563eb}.nexus-guest-drawer-body{padding:16px 20px 20px}.nexus-guest-drawer-body section{padding:0 0 14px;margin-bottom:14px;border-bottom:1px solid #eef2f7}.nexus-guest-drawer-body h3{margin:0 0 10px;font-size:10px;text-transform:uppercase;letter-spacing:.08em;color:#64748b}.nexus-guest-drawer dl{margin:0;display:grid;gap:8px}.nexus-guest-drawer dl div{display:flex;justify-content:space-between;gap:20px}.nexus-guest-drawer dt{font-size:10px;color:#64748b}.nexus-guest-drawer dd{margin:0;font-size:10px;font-weight:680;text-align:right;color:#111827}.nexus-guest-tags{margin:0;font-size:10px;color:#334155}.nexus-guest-drawer-note{font-size:9px;line-height:1.5;color:#64748b;background:#f8fafc;border-radius:8px;padding:10px;margin:0}.nexus-guest-drawer-note i{margin-right:6px;color:#2563eb}@keyframes nexusDrawerIn{from{opacity:0;transform:translateX(16px)}to{opacity:1;transform:translateX(0)}}@media(max-width:1200px){.nexus-inventory-kpis{grid-template-columns:repeat(3,1fr)}.nexus-filter-hint{display:none}}@media(max-width:850px){.nexus-inventory-kpis{grid-template-columns:1fr 1fr;padding-left:16px;padding-right:16px}.nexus-inventory-workspace,.nexus-inventory-status-error{margin-left:16px;margin-right:16px}}@media(max-width:620px){.nexus-inventory{padding-top:10px}.nexus-inventory-kpis{grid-template-columns:1fr}.nexus-inventory-filter-guide{align-items:flex-start}.nexus-inventory-workspace{border-radius:10px}.nexus-guest-drawer{top:60px;right:8px;bottom:8px;width:calc(100vw - 16px)}}
"#;
