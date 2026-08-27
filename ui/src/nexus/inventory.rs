use gloo_timers::callback::Interval;
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
    guest_type: String,
    status: String,
    remote: String,
    node: String,
    tags: String,
    cpu: String,
    memory: String,
    uptime: String,
}

#[derive(Clone, PartialEq)]
struct ActionMenuState {
    guest: GuestRowDetails,
    x: i32,
    y: i32,
}

fn cell_text(cells: &web_sys::HtmlCollection, index: u32) -> String {
    cells
        .item(index)
        .and_then(|cell| cell.text_content())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn guest_type_from_row(row: &web_sys::Element) -> String {
    let Some(name_cell) = row.children().item(0) else {
        return String::from("Guest");
    };

    if name_cell.query_selector(".fa-cube").ok().flatten().is_some() {
        String::from("Linux Container")
    } else if name_cell
        .query_selector(".fa-desktop")
        .ok()
        .flatten()
        .is_some()
    {
        String::from("Virtual Machine")
    } else {
        String::from("Guest")
    }
}

fn details_from_row(row: &web_sys::Element) -> Option<GuestRowDetails> {
    let cells = row.children();
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
        guest_type: guest_type_from_row(row),
        status: cell_text(&cells, 2),
        remote,
        node,
        tags,
        cpu,
        memory,
        uptime,
    })
}

fn row_from_element(mut element: web_sys::Element) -> Option<web_sys::Element> {
    loop {
        if element.tag_name() == "TR" {
            return Some(element);
        }
        element = element.parent_element()?;
    }
}

fn ancestor_with_attribute(
    mut element: web_sys::Element,
    attribute: &str,
) -> Option<web_sys::Element> {
    loop {
        if element.has_attribute(attribute) {
            return Some(element);
        }
        if element.tag_name() == "TR" {
            return None;
        }
        element = element.parent_element()?;
    }
}

fn find_guest_row(id: &str) -> Option<web_sys::Element> {
    let rows = gloo_utils::document().get_elements_by_tag_name("tr");
    for index in 0..rows.length() {
        let Some(row) = rows.item(index) else {
            continue;
        };
        if !row
            .matches(".nexus-inventory-table-shell tbody tr")
            .unwrap_or(false)
        {
            continue;
        }
        let cells = row.children();
        if cells.length() >= 9 && cell_text(&cells, 1) == id {
            return Some(row);
        }
    }
    None
}

fn mark_selected_row(id: Option<&str>) {
    let rows = gloo_utils::document().get_elements_by_tag_name("tr");
    for index in 0..rows.length() {
        let Some(row) = rows.item(index) else {
            continue;
        };
        if !row
            .matches(".nexus-inventory-table-shell tbody tr")
            .unwrap_or(false)
        {
            continue;
        }
        row.remove_attribute("data-nexus-selected").ok();
        if let Some(id) = id {
            let cells = row.children();
            if cells.length() >= 9 && cell_text(&cells, 1) == id {
                row.set_attribute("data-nexus-selected", "true").ok();
            }
        }
    }
}

fn trigger_hidden_action(id: &str, action: &str) -> bool {
    let Some(row) = find_guest_row(id) else {
        return false;
    };
    let cells = row.children();
    let Some(action_cell) = cells.item(cells.length().saturating_sub(1)) else {
        return false;
    };
    let selector = format!("[aria-label=\"{action}\"]");
    let Ok(Some(element)) = action_cell.query_selector(&selector) else {
        return false;
    };
    let Ok(element) = element.dyn_into::<web_sys::HtmlElement>() else {
        return false;
    };
    element.click();
    true
}

fn primary_action(status: &str) -> (&'static str, &'static str, &'static str) {
    let status = status.to_lowercase();
    if status.contains("paused") || status.contains("suspended") || status.contains("prelaunch") {
        ("Resume", "fa fa-play", "resume")
    } else if status.contains("running") {
        ("Shutdown", "fa fa-power-off", "shutdown")
    } else {
        ("Start", "fa fa-play", "start")
    }
}

fn decorate_action_cells() {
    let document = gloo_utils::document();
    let rows = document.get_elements_by_tag_name("tr");

    for index in 0..rows.length() {
        let Some(row) = rows.item(index) else {
            continue;
        };
        if !row
            .matches(".nexus-inventory-table-shell tbody tr")
            .unwrap_or(false)
        {
            continue;
        }

        let cells = row.children();
        if cells.length() < 9 || cell_text(&cells, 1).is_empty() {
            continue;
        }
        let Some(action_cell) = cells.item(cells.length() - 1) else {
            continue;
        };

        let status = cell_text(&cells, 2);
        let (action, icon, tone) = primary_action(&status);

        if let Ok(Some(existing)) = action_cell.query_selector(".nexus-row-actions") {
            if let Ok(Some(primary)) = existing.query_selector(".nexus-primary-action") {
                primary.set_attribute("data-nexus-action", action).ok();
                primary
                    .set_attribute("aria-label", &format!("{action} guest"))
                    .ok();
                primary.set_class_name(&format!("nexus-primary-action {tone}"));
                primary.set_inner_html(&format!("<i class=\"{icon}\"></i>"));
            }
            continue;
        }

        let Ok(wrapper) = document.create_element("div") else {
            continue;
        };
        wrapper.set_class_name("nexus-row-actions");

        let Ok(primary) = document.create_element("button") else {
            continue;
        };
        primary
            .set_attribute("type", "button")
            .and_then(|_| primary.set_attribute("data-nexus-action", action))
            .and_then(|_| primary.set_attribute("aria-label", &format!("{action} guest")))
            .ok();
        primary.set_class_name(&format!("nexus-primary-action {tone}"));
        primary.set_inner_html(&format!("<i class=\"{icon}\"></i>"));

        let Ok(overflow) = document.create_element("button") else {
            continue;
        };
        overflow
            .set_attribute("type", "button")
            .and_then(|_| overflow.set_attribute("data-nexus-overflow", "true"))
            .and_then(|_| overflow.set_attribute("aria-label", "More guest actions"))
            .ok();
        overflow.set_class_name("nexus-overflow-action");
        overflow.set_inner_html("<i class=\"fa fa-ellipsis-v\"></i>");

        wrapper.append_child(&primary).ok();
        wrapper.append_child(&overflow).ok();
        action_cell.append_child(&wrapper).ok();
    }
}

fn parse_human_size(value: &str) -> Option<f64> {
    let mut parts = value.split_whitespace();
    let number = parts.next()?.parse::<f64>().ok()?;
    let unit = parts.next().unwrap_or("B");
    let multiplier = match unit {
        "KiB" => 1024.0,
        "MiB" => 1024.0 * 1024.0,
        "GiB" => 1024.0 * 1024.0 * 1024.0,
        "TiB" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => 1.0,
    };
    Some(number * multiplier)
}

fn memory_percent(value: &str) -> u32 {
    let Some((used, total)) = value.split_once(" of ") else {
        return 0;
    };
    let Some(used) = parse_human_size(used) else {
        return 0;
    };
    let Some(total) = parse_human_size(total) else {
        return 0;
    };
    if total <= 0.0 {
        0
    } else {
        ((used / total) * 100.0).round().clamp(0.0, 100.0) as u32
    }
}

fn cpu_percent(value: &str) -> u32 {
    value
        .trim_end_matches('%')
        .trim()
        .parse::<f64>()
        .map(|value| value.round().clamp(0.0, 100.0) as u32)
        .unwrap_or(0)
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

fn detail_drawer(
    details: &GuestRowDetails,
    close: Callback<MouseEvent>,
    open_pve: Callback<MouseEvent>,
) -> Html {
    let running = details.status.to_lowercase().contains("running");
    let cpu = cpu_percent(&details.cpu);
    let memory = memory_percent(&details.memory);

    html! {
        <aside class="nexus-guest-drawer" role="complementary" aria-label="Guest details">
            <div class="nexus-guest-drawer-head">
                <div class="nexus-guest-drawer-title">
                    <h2>{format!("{} ({})", details.name, details.id)}</h2>
                    <div class="nexus-guest-drawer-meta">
                        <span class={classes!("nexus-guest-status", running.then_some("running"))}><i class="fa fa-circle"></i>{details.status.clone()}</span>
                        <span>{details.guest_type.clone()}</span>
                    </div>
                </div>
                <button class="nexus-guest-drawer-close" aria-label="Close guest details" onclick={close}><i class="fa fa-times"></i></button>
            </div>
            <div class="nexus-guest-tabs" role="tablist" aria-label="Guest detail sections">
                <span class="active" role="tab" aria-selected="true">{"Overview"}</span>
                <span role="tab">{"Compute"}</span>
                <span role="tab">{"Network"}</span>
                <span role="tab">{"Storage"}</span>
                <span role="tab">{"Snapshots"}</span>
                <span role="tab">{"Tasks"}</span>
            </div>
            <div class="nexus-guest-drawer-body">
                <section class="nexus-guest-card">
                    <h3>{"General"}</h3>
                    <dl>
                        <div><dt>{"ID"}</dt><dd>{details.id.clone()}</dd></div>
                        <div><dt>{"Name"}</dt><dd>{details.name.clone()}</dd></div>
                        <div><dt>{"Type"}</dt><dd>{details.guest_type.clone()}</dd></div>
                        <div><dt>{"Status"}</dt><dd><span class={classes!("nexus-guest-inline-status", running.then_some("running"))}><i class="fa fa-circle"></i>{details.status.clone()}</span></dd></div>
                        <div><dt>{"Remote"}</dt><dd>{details.remote.clone()}</dd></div>
                        <div><dt>{"Node"}</dt><dd>{details.node.clone()}</dd></div>
                        <div><dt>{"Uptime"}</dt><dd>{details.uptime.clone()}</dd></div>
                        <div><dt>{"PVE Web UI"}</dt><dd><button class="nexus-link-button" onclick={open_pve}>{"Open in Proxmox "}<i class="fa fa-external-link"></i></button></dd></div>
                    </dl>
                </section>
                <section class="nexus-guest-card">
                    <h3>{"Resources"}</h3>
                    <div class="nexus-resource-row">
                        <div><span>{"CPU Usage"}</span><strong>{details.cpu.clone()}</strong></div>
                        <div class="nexus-resource-track"><span style={format!("width:{cpu}%")}></span></div>
                    </div>
                    <div class="nexus-resource-row">
                        <div><span>{"Memory Usage"}</span><strong>{format!("{} ({}%)", details.memory, memory)}</strong></div>
                        <div class="nexus-resource-track"><span style={format!("width:{memory}%")}></span></div>
                    </div>
                    <p class="nexus-resource-note">{"Storage and swap telemetry remain available through the PVE deep link."}</p>
                </section>
                <section class="nexus-guest-card">
                    <h3>{"Tags"}</h3>
                    <div class="nexus-guest-tags">
                        {if details.tags.is_empty() {
                            html! {<span class="empty">{"No tags"}</span>}
                        } else {
                            html! {for details.tags.split_whitespace().map(|tag| html! {<span>{tag}</span>})}
                        }}
                    </div>
                </section>
            </div>
        </aside>
    }
}

#[function_component(NexusInventory)]
pub fn nexus_inventory() -> Html {
    let status = use_state(|| None::<Result<ResourcesStatus, String>>);
    let selected = use_state(|| None::<GuestRowDetails>);
    let action_menu = use_state(|| None::<ActionMenuState>);

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

    use_effect_with((), move |_| {
        decorate_action_cells();
        let interval = Interval::new(750, decorate_action_cells);
        move || drop(interval)
    });

    let on_table_click = {
        let selected = selected.clone();
        let action_menu = action_menu.clone();
        Callback::from(move |event: MouseEvent| {
            let Some(target) = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };

            if let Some(button) = ancestor_with_attribute(target.clone(), "data-nexus-action") {
                event.stop_propagation();
                let Some(row) = row_from_element(button.clone()) else {
                    return;
                };
                let Some(details) = details_from_row(&row) else {
                    return;
                };
                if let Some(action) = button.get_attribute("data-nexus-action") {
                    trigger_hidden_action(&details.id, &action);
                }
                action_menu.set(None);
                return;
            }

            if let Some(button) = ancestor_with_attribute(target.clone(), "data-nexus-overflow") {
                event.stop_propagation();
                let Some(row) = row_from_element(button) else {
                    return;
                };
                let Some(details) = details_from_row(&row) else {
                    return;
                };
                mark_selected_row(Some(&details.id));
                selected.set(Some(details.clone()));
                action_menu.set(Some(ActionMenuState {
                    guest: details,
                    x: event.client_x(),
                    y: event.client_y(),
                }));
                return;
            }

            let Some(row) = row_from_element(target) else {
                return;
            };
            let Some(details) = details_from_row(&row) else {
                return;
            };
            mark_selected_row(Some(&details.id));
            selected.set(Some(details));
            action_menu.set(None);
        })
    };

    let close_drawer = {
        let selected = selected.clone();
        let action_menu = action_menu.clone();
        Callback::from(move |_| {
            selected.set(None);
            action_menu.set(None);
            mark_selected_row(None);
        })
    };

    let close_menu = {
        let action_menu = action_menu.clone();
        Callback::from(move |_| action_menu.set(None))
    };

    let open_pve = {
        let selected = selected.clone();
        Callback::from(move |event: MouseEvent| {
            event.stop_propagation();
            if let Some(details) = selected.as_ref() {
                trigger_hidden_action(&details.id, "Open in PVE UI");
            }
        })
    };

    let root_class = classes!(
        "nexus-inventory",
        selected.is_some().then_some("drawer-open")
    );

    html! {
        <div class={root_class}>
            <style>{INVENTORY_CSS}</style>
            {match status.as_ref() {
                Some(Ok(data)) => summary(data),
                Some(Err(err)) => html! {<div class="nexus-inventory-status-error"><i class="fa fa-exclamation-triangle"></i>{format!(" Inventory summary unavailable: {err}")}</div>},
                None => html! {<div class="nexus-inventory-kpis loading"><span>{"Loading inventory summary…"}</span></div>},
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
                <div class="nexus-inventory-table-shell" onclick={on_table_click}>{VNode::from(GuestPanel::new())}</div>
            </section>
            {selected.as_ref().map(|details| detail_drawer(details, close_drawer.clone(), open_pve.clone()))}
            {action_menu.as_ref().map(|menu| {
                let id = menu.guest.id.clone();
                let running = menu.guest.status.to_lowercase().contains("running");
                let paused = menu.guest.status.to_lowercase().contains("paused")
                    || menu.guest.status.to_lowercase().contains("suspended")
                    || menu.guest.status.to_lowercase().contains("prelaunch");
                let snapshots = {
                    let id = id.clone();
                    let action_menu = action_menu.clone();
                    Callback::from(move |event: MouseEvent| {
                        event.stop_propagation();
                        trigger_hidden_action(&id, "Snapshots");
                        action_menu.set(None);
                    })
                };
                let migrate = {
                    let id = id.clone();
                    let action_menu = action_menu.clone();
                    Callback::from(move |event: MouseEvent| {
                        event.stop_propagation();
                        trigger_hidden_action(&id, "Migrate");
                        action_menu.set(None);
                    })
                };
                let open = {
                    let id = id.clone();
                    let action_menu = action_menu.clone();
                    Callback::from(move |event: MouseEvent| {
                        event.stop_propagation();
                        trigger_hidden_action(&id, "Open in PVE UI");
                        action_menu.set(None);
                    })
                };
                let shutdown = {
                    let id = id.clone();
                    let action_menu = action_menu.clone();
                    Callback::from(move |event: MouseEvent| {
                        event.stop_propagation();
                        trigger_hidden_action(&id, "Shutdown");
                        action_menu.set(None);
                    })
                };
                let start_or_resume = {
                    let id = id.clone();
                    let action_menu = action_menu.clone();
                    let action = if paused { "Resume" } else { "Start" };
                    Callback::from(move |event: MouseEvent| {
                        event.stop_propagation();
                        trigger_hidden_action(&id, action);
                        action_menu.set(None);
                    })
                };
                html! {
                    <div class="nexus-action-menu-backdrop" onclick={close_menu.clone()}>
                        <div
                            class="nexus-action-menu"
                            style={format!("left:{}px;top:{}px", (menu.x - 150).max(10), (menu.y + 8).max(10))}
                            onclick={Callback::from(|event: MouseEvent| event.stop_propagation())}
                            role="menu"
                            aria-label={format!("Actions for {}", menu.guest.name)}
                        >
                            <button role="menuitem" onclick={snapshots}><i class="fa fa-camera"></i><span>{"Snapshots"}</span></button>
                            <button role="menuitem" onclick={migrate}><i class="fa fa-exchange"></i><span>{"Migrate"}</span></button>
                            <button role="menuitem" onclick={open}><i class="fa fa-external-link"></i><span>{"Open in PVE"}</span></button>
                            <div class="separator"></div>
                            <button class="danger" role="menuitem" disabled={!running} onclick={shutdown}><i class="fa fa-power-off"></i><span>{"Shutdown"}</span></button>
                            <button class="success" role="menuitem" disabled={running && !paused} onclick={start_or_resume}><i class="fa fa-play"></i><span>{if paused { "Resume" } else { "Start" }}</span></button>
                        </div>
                    </div>
                }
            })}
        </div>
    }
}

const INVENTORY_CSS: &str = r#"
.nexus-inventory{--nx-text:#0b1220;--nx-muted:#64748b;--nx-blue:#2563eb;--nx-green:#16a34a;--nx-orange:#f97316;--nx-border:#dce4ef;width:100%;height:100%;overflow:auto;background:linear-gradient(180deg,#f8faff 0,#f6f8fc 180px);color:var(--nx-text);font-family:"Roboto Flex",Roboto,Arial,Helvetica,sans-serif;font-weight:430;padding:14px 0 20px}.nexus-inventory *{box-sizing:border-box}.nexus-inventory-live-dot{display:inline-block;width:7px;height:7px;border-radius:50%;background:#22c55e;box-shadow:0 0 0 3px #dcfce7;margin-right:7px}.nexus-inventory-kpis{display:grid;grid-template-columns:repeat(5,minmax(145px,1fr));gap:10px;padding:0 30px 12px;transition:padding-right .18s ease}.nexus-inventory-kpi{background:#fff;border:1px solid var(--nx-border);border-radius:11px;box-shadow:0 2px 9px rgba(15,23,42,.05);padding:10px 13px;display:flex;align-items:center;gap:10px;min-height:76px;position:relative;overflow:hidden}.nexus-inventory-kpi:after{content:"";position:absolute;left:0;bottom:0;right:0;height:2px;background:#2563eb}.nexus-inventory-kpi.green:after{background:#22c55e}.nexus-inventory-kpi.orange:after{background:#f97316}.nexus-inventory-kpi.slate:after{background:#94a3b8}.nexus-inventory-kpi-icon{width:34px;height:34px;border-radius:9px;background:#edf3ff;color:#2563eb;display:flex;align-items:center;justify-content:center;font-size:14px;flex:none}.nexus-inventory-kpi.green .nexus-inventory-kpi-icon{background:#ecfdf3;color:#15803d}.nexus-inventory-kpi.orange .nexus-inventory-kpi-icon{background:#fff7ed;color:#ea580c}.nexus-inventory-kpi.slate .nexus-inventory-kpi-icon{background:#f1f5f9;color:#475569}.nexus-inventory-kpi-copy{display:flex;flex-direction:column;min-width:0}.nexus-inventory-kpi-copy>span{font-size:9px;font-weight:720;color:#334155}.nexus-inventory-kpi-copy strong{font-size:21px;line-height:1.05;margin-top:1px;color:#050a13;font-weight:790}.nexus-inventory-kpi-copy small{font-size:8px;color:#64748b;margin-top:2px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.nexus-inventory-kpis.loading{display:flex;min-height:76px;align-items:center;color:#64748b;font-size:11px}.nexus-inventory-status-error{margin:0 30px 12px;padding:9px 11px;border:1px solid #fed7aa;background:#fff7ed;border-radius:8px;color:#9a3412;font-size:10px}.nexus-inventory-workspace{margin:0 30px;background:#fff;border:1px solid var(--nx-border);border-radius:12px;box-shadow:0 3px 13px rgba(15,23,42,.05);overflow:hidden;transition:margin-right .18s ease}.nexus-inventory-filter-guide{min-height:44px;padding:8px 12px;display:flex;align-items:center;gap:6px;flex-wrap:wrap;color:#334155;font-size:9px;border-bottom:1px solid #e7edf5;background:linear-gradient(180deg,#fff,#fbfcff)}.nexus-filter-heading{font-weight:760;color:#111827;margin-right:3px}.nexus-inventory-filter-guide code{background:#f5f7ff;border:1px solid #dbe4ff;border-radius:999px;padding:4px 8px;color:#1d4ed8;font-family:inherit;font-weight:680}.nexus-inventory-filter-guide code.nexus-filter-more{background:#fff;color:#334155;border-color:#dbe3ee}.nexus-filter-hint{font-size:9px;color:#64748b;margin-left:auto;display:flex;align-items:center;white-space:nowrap}.nexus-inventory-table-shell{background:#fff;overflow:hidden;min-height:420px}.nexus-inventory-table-shell>div{border:0!important}.nexus-inventory-table-shell input{color:#0b1220!important;background:#fff!important;border-color:#d7e0eb!important;border-radius:7px!important;min-height:32px!important;font-size:10px!important;padding-left:10px!important}.nexus-inventory-table-shell input:focus{border-color:#8db4ff!important;box-shadow:0 0 0 3px rgba(37,99,235,.08)!important}.nexus-inventory-table-shell table{color:#111827!important;font-size:10px;border-collapse:separate!important;border-spacing:0!important}.nexus-inventory-table-shell thead th{color:#506078!important;font-weight:720!important;background:#f8fafc!important;border-bottom:1px solid #dfe6ef!important;height:38px!important}.nexus-inventory-table-shell tbody td{color:#111827!important;border-bottom:1px solid #eef2f7!important;height:41px!important;background:#fff!important}.nexus-inventory-table-shell tbody tr:nth-child(even) td{background:#fcfdff!important}.nexus-inventory-table-shell tbody tr:hover td{background:#f4f8ff!important;cursor:pointer}.nexus-inventory-table-shell tbody tr[data-nexus-selected="true"] td{background:#eef5ff!important}.nexus-inventory-table-shell button{border-radius:7px!important}.nexus-inventory-table-shell button[aria-pressed="true"]{box-shadow:0 2px 6px rgba(37,99,235,.15)!important}.nexus-inventory-table-shell [class*="toolbar"]{background:#fff!important;border-bottom:1px solid #e7edf5!important;padding:7px 10px!important;min-height:46px!important}.nexus-inventory-table-shell [class*="segmented"]{border-radius:8px!important;overflow:hidden!important}.nexus-inventory-table-shell th:last-child{width:92px!important;max-width:92px!important}.nexus-inventory-table-shell td:last-child{white-space:nowrap;width:92px!important;max-width:92px!important;position:relative}.nexus-inventory-table-shell td:last-child>div:not(.nexus-row-actions){display:none!important}.nexus-row-actions{display:flex!important;align-items:center;justify-content:flex-start;gap:6px;padding:0 4px}.nexus-row-actions button{width:28px!important;height:28px!important;min-width:28px!important;border:1px solid #dce4ef!important;border-radius:7px!important;background:#fff!important;color:#64748b!important;display:inline-flex!important;align-items:center!important;justify-content:center!important;cursor:pointer!important}.nexus-row-actions button:hover{background:#f5f8ff!important;border-color:#bfd0e9!important;transform:translateY(-1px)}.nexus-row-actions .nexus-primary-action.shutdown{color:#2563eb!important;background:#f8fbff!important}.nexus-row-actions .nexus-primary-action.start{color:#16a34a!important;background:#f7fff9!important}.nexus-row-actions .nexus-primary-action.resume{color:#d97706!important;background:#fffbeb!important}.nexus-overflow-action{font-size:11px!important}.nexus-inventory-table-shell .pwt-loading-icon{color:#2563eb!important}.nexus-action-menu-backdrop{position:fixed;inset:0;z-index:220;background:transparent}.nexus-action-menu{position:fixed;width:168px;background:#fff;border:1px solid #dce4ef;border-radius:9px;box-shadow:0 12px 32px rgba(15,23,42,.18);padding:5px;z-index:221}.nexus-action-menu button{width:100%;height:34px;border:0;background:#fff;border-radius:6px;display:flex;align-items:center;gap:9px;padding:0 9px;text-align:left;color:#1f2937;font-size:10px;cursor:pointer}.nexus-action-menu button i{width:16px;text-align:center;color:#475569}.nexus-action-menu button:hover:not(:disabled){background:#f4f7fb}.nexus-action-menu button:disabled{opacity:.35;cursor:not-allowed}.nexus-action-menu button.danger{color:#dc2626}.nexus-action-menu button.danger i{color:#dc2626}.nexus-action-menu button.success{color:#15803d}.nexus-action-menu button.success i{color:#15803d}.nexus-action-menu .separator{height:1px;background:#e7edf5;margin:5px 2px}.nexus-guest-drawer{position:fixed;top:69px;right:0;bottom:0;width:400px;background:#fff;border-left:1px solid #dce4ef;box-shadow:-8px 0 30px rgba(15,23,42,.08);z-index:180;display:flex;flex-direction:column;animation:nexusDrawerIn .18s ease-out}.nexus-guest-drawer-head{min-height:92px;padding:18px 18px 12px;display:flex;justify-content:space-between;gap:14px;border-bottom:1px solid #edf1f6}.nexus-guest-drawer-title h2{margin:0;color:#0f172a;font-size:17px;line-height:1.2;font-weight:780}.nexus-guest-drawer-meta{display:flex;align-items:center;gap:12px;margin-top:12px;font-size:10px;color:#475569}.nexus-guest-status,.nexus-guest-inline-status{display:inline-flex;align-items:center;gap:6px}.nexus-guest-status i,.nexus-guest-inline-status i{font-size:7px;color:#94a3b8}.nexus-guest-status.running i,.nexus-guest-inline-status.running i{color:#16a34a}.nexus-guest-drawer-close{width:30px;height:30px;border:0;background:#fff;color:#111827;font-size:14px;cursor:pointer;border-radius:7px}.nexus-guest-drawer-close:hover{background:#f1f5f9}.nexus-guest-tabs{height:45px;display:flex;align-items:flex-end;gap:19px;padding:0 18px;border-bottom:1px solid #e7edf5;overflow-x:auto}.nexus-guest-tabs span{height:45px;display:flex;align-items:center;white-space:nowrap;font-size:9px;color:#475569;border-bottom:2px solid transparent}.nexus-guest-tabs span.active{color:#2563eb;border-bottom-color:#2563eb;font-weight:700}.nexus-guest-drawer-body{padding:14px;overflow:auto;display:flex;flex-direction:column;gap:12px}.nexus-guest-card{border:1px solid #dce4ef;border-radius:9px;padding:14px;background:#fff}.nexus-guest-card h3{font-size:11px;margin:0 0 13px;color:#111827;font-weight:760}.nexus-guest-card dl{margin:0;display:grid;gap:10px}.nexus-guest-card dl>div{display:grid;grid-template-columns:110px 1fr;gap:12px;align-items:center}.nexus-guest-card dt{font-size:9px;color:#475569}.nexus-guest-card dd{margin:0;font-size:9px;color:#0f172a;font-weight:520;min-width:0;overflow:hidden;text-overflow:ellipsis}.nexus-link-button{border:0;background:transparent;color:#2563eb;padding:0;font-size:9px;cursor:pointer}.nexus-link-button:hover{text-decoration:underline}.nexus-resource-row{margin-bottom:15px}.nexus-resource-row>div:first-child{display:flex;align-items:center;justify-content:space-between;gap:12px;font-size:9px;color:#475569;margin-bottom:6px}.nexus-resource-row strong{font-weight:600;color:#0f172a}.nexus-resource-track{height:5px;border-radius:999px;background:#e8edf4;overflow:hidden}.nexus-resource-track span{display:block;height:100%;border-radius:999px;background:#2563eb}.nexus-resource-note{margin:4px 0 0;color:#94a3b8;font-size:8px;line-height:1.45}.nexus-guest-tags{display:flex;gap:6px;flex-wrap:wrap}.nexus-guest-tags span{background:#eef4ff;border:1px solid #dbe7ff;border-radius:999px;padding:5px 8px;color:#2563eb;font-size:8px}.nexus-guest-tags span.empty{background:#f8fafc;border-color:#e2e8f0;color:#94a3b8}.nexus-inventory.drawer-open .nexus-inventory-workspace{margin-right:430px}.nexus-inventory.drawer-open .nexus-inventory-kpis{padding-right:430px}@keyframes nexusDrawerIn{from{transform:translateX(16px);opacity:.5}to{transform:translateX(0);opacity:1}}@media(max-width:1200px){.nexus-inventory-kpis{grid-template-columns:repeat(3,1fr)}.nexus-filter-hint{display:none}.nexus-inventory.drawer-open .nexus-inventory-kpis{padding-right:30px}.nexus-inventory.drawer-open .nexus-inventory-workspace{margin-right:30px}.nexus-guest-drawer{width:380px;box-shadow:-14px 0 40px rgba(15,23,42,.16)}}@media(max-width:850px){.nexus-inventory-kpis{grid-template-columns:1fr 1fr;padding-left:16px;padding-right:16px}.nexus-inventory-workspace,.nexus-inventory-status-error{margin-left:16px;margin-right:16px}.nexus-guest-drawer{width:min(92vw,400px)}}@media(max-width:620px){.nexus-inventory{padding-top:10px}.nexus-inventory-kpis{grid-template-columns:1fr}.nexus-inventory-filter-guide{align-items:flex-start}.nexus-inventory-workspace{border-radius:10px}}
"#;
