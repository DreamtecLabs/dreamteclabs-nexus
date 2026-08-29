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
    row_key: String,
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
    start: Option<bool>,
    resume: Option<bool>,
    shutdown: Option<bool>,
    snapshots: Option<bool>,
    migrate: Option<bool>,
    open_pve: Option<bool>,
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

fn table_from_row(row: &web_sys::Element) -> Option<web_sys::Element> {
    let mut element = row.clone();
    loop {
        if element.tag_name() == "TABLE" {
            return Some(element);
        }
        element = element.parent_element()?;
    }
}

fn header_index(row: &web_sys::Element, label: &str) -> Option<u32> {
    let table = table_from_row(row)?;
    let headers = table.get_elements_by_tag_name("th");
    for index in 0..headers.length() {
        let text = headers
            .item(index)
            .and_then(|header| header.text_content())
            .unwrap_or_default();
        if text.trim().contains(label) {
            return Some(index);
        }
    }
    None
}

fn row_field(row: &web_sys::Element, label: &str, fallback: u32) -> String {
    let cells = row.children();
    header_index(row, label)
        .filter(|index| *index < cells.length())
        .map(|index| cell_text(&cells, index))
        .unwrap_or_else(|| cell_text(&cells, fallback))
}

fn tree_remote(row: &web_sys::Element) -> String {
    let mut previous = row.previous_element_sibling();
    while let Some(candidate) = previous {
        let cells = candidate.children();
        if cells.length() > 1 && cell_text(&cells, 1).is_empty() {
            let name = cell_text(&cells, 0);
            if !name.is_empty() {
                return name
                    .rsplit_once(" (")
                    .map(|(remote, _)| remote.to_string())
                    .unwrap_or(name);
            }
        }
        previous = candidate.previous_element_sibling();
    }
    String::from("Grouped view")
}

fn remote_from_row(row: &web_sys::Element) -> String {
    if let Some(index) = header_index(row, "Remote") {
        return cell_text(&row.children(), index);
    }
    tree_remote(row)
}

fn guest_type_from_row(row: &web_sys::Element) -> String {
    let Some(name_cell) = row.children().item(0) else {
        return String::from("Guest");
    };

    if name_cell
        .query_selector(".fa-cube")
        .ok()
        .flatten()
        .is_some()
    {
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

fn has_icon(element: &web_sys::Element, icon_class: &str) -> bool {
    let own = element
        .get_attribute("class")
        .map(|classes| classes.split_whitespace().any(|class| class == icon_class))
        .unwrap_or(false);
    own || element
        .query_selector(&format!(".{icon_class}"))
        .ok()
        .flatten()
        .is_some()
}

fn classify_original_actions(row: &web_sys::Element) {
    let cells = row.children();
    if cells.length() == 0 {
        return;
    }
    let Some(action_cell) = cells.item(cells.length() - 1) else {
        return;
    };
    let status = row_field(row, "Status", 2).to_lowercase();
    let resume =
        status.contains("paused") || status.contains("suspended") || status.contains("prelaunch");

    let descendants = action_cell.get_elements_by_tag_name("*");
    for index in 0..descendants.length() {
        let Some(element) = descendants.item(index) else {
            continue;
        };
        let is_button = element.tag_name() == "BUTTON"
            || element.get_attribute("role").as_deref() == Some("button");
        if !is_button
            || element.has_attribute("data-nexus-primary")
            || element.has_attribute("data-nexus-overflow")
        {
            continue;
        }

        let action = if has_icon(&element, "fa-power-off") {
            Some("shutdown")
        } else if has_icon(&element, "fa-play") {
            Some(if resume { "resume" } else { "start" })
        } else if has_icon(&element, "fa-history") {
            Some("snapshots")
        } else if has_icon(&element, "fa-paper-plane-o") {
            Some("migrate")
        } else if has_icon(&element, "fa-external-link") {
            Some("open-pve")
        } else {
            None
        };

        if let Some(action) = action {
            element
                .set_attribute("data-nexus-hidden-action", action)
                .ok();
        }
    }
}

fn action_state(row: &web_sys::Element, action: &str) -> Option<bool> {
    let cells = row.children();
    let action_cell = cells.item(cells.length().checked_sub(1)?)?;
    let selector = format!("[data-nexus-hidden-action=\"{action}\"]");
    let element = action_cell.query_selector(&selector).ok().flatten()?;
    let disabled = element.get_attribute("aria-disabled").as_deref() == Some("true")
        || element.has_attribute("disabled");
    Some(!disabled)
}

fn row_signature(row: &web_sys::Element) -> String {
    format!(
        "{}|{}|{}|{}",
        remote_from_row(row),
        guest_type_from_row(row),
        row_field(row, "ID", 1),
        row_field(row, "Name", 0)
    )
}

fn details_from_row(row: &web_sys::Element) -> Option<GuestRowDetails> {
    classify_original_actions(row);

    let cells = row.children();
    if cells.length() < 8 {
        return None;
    }

    let id = row_field(row, "ID", 1);
    if id.is_empty() {
        return None;
    }

    let tags = header_index(row, "Tags")
        .filter(|index| *index < cells.length())
        .map(|index| cell_text(&cells, index))
        .unwrap_or_default();

    Some(GuestRowDetails {
        row_key: row_signature(row),
        name: row_field(row, "Name", 0),
        id,
        guest_type: guest_type_from_row(row),
        status: row_field(row, "Status", 2),
        remote: remote_from_row(row),
        node: row_field(row, "Node", 4.min(cells.length() - 1)),
        tags,
        cpu: row_field(row, "CPU Usage", 5.min(cells.length() - 1)),
        memory: row_field(row, "Memory Usage", 6.min(cells.length() - 1)),
        uptime: row_field(row, "Uptime", 7.min(cells.length() - 1)),
        start: action_state(row, "start"),
        resume: action_state(row, "resume"),
        shutdown: action_state(row, "shutdown"),
        snapshots: action_state(row, "snapshots"),
        migrate: action_state(row, "migrate"),
        open_pve: action_state(row, "open-pve"),
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

fn find_guest_row(row_key: &str) -> Option<web_sys::Element> {
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
        if row.get_attribute("data-nexus-row-key").as_deref() == Some(row_key) {
            return Some(row);
        }
    }
    None
}

fn mark_selected_row(row_key: Option<&str>) {
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
        if row_key.is_some() && row.get_attribute("data-nexus-row-key").as_deref() == row_key {
            row.set_attribute("data-nexus-selected", "true").ok();
        }
    }
}

fn trigger_hidden_action(row_key: &str, action: &str) -> bool {
    let Some(row) = find_guest_row(row_key) else {
        return false;
    };
    let cells = row.children();
    let Some(action_cell) = cells.item(cells.length().saturating_sub(1)) else {
        return false;
    };
    let selector = format!("[data-nexus-hidden-action=\"{action}\"]");
    let Ok(Some(element)) = action_cell.query_selector(&selector) else {
        return false;
    };
    let Ok(element) = element.dyn_into::<web_sys::HtmlElement>() else {
        return false;
    };
    element.click();
    true
}

fn enabled_primary(
    details: &GuestRowDetails,
) -> Option<(&'static str, &'static str, &'static str)> {
    if details.resume == Some(true) {
        Some(("resume", "fa fa-play", "resume"))
    } else if details.shutdown == Some(true) {
        Some(("shutdown", "fa fa-power-off", "shutdown"))
    } else if details.start == Some(true) {
        Some(("start", "fa fa-play", "start"))
    } else {
        None
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

        classify_original_actions(&row);
        let Some(details) = details_from_row(&row) else {
            continue;
        };
        row.set_attribute("data-nexus-row-key", &details.row_key)
            .ok();

        let cells = row.children();
        let Some(action_cell) = cells.item(cells.length() - 1) else {
            continue;
        };

        let wrapper = match action_cell.query_selector(".nexus-row-actions") {
            Ok(Some(existing)) => existing,
            _ => {
                let Ok(wrapper) = document.create_element("div") else {
                    continue;
                };
                wrapper.set_class_name("nexus-row-actions");

                let Ok(primary) = document.create_element("button") else {
                    continue;
                };
                primary
                    .set_attribute("type", "button")
                    .and_then(|_| primary.set_attribute("data-nexus-primary", "true"))
                    .ok();
                primary.set_class_name("nexus-primary-action");

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
                wrapper
            }
        };

        if let Ok(Some(primary)) = wrapper.query_selector(".nexus-primary-action") {
            if let Some((action, icon, tone)) = enabled_primary(&details) {
                primary.remove_attribute("hidden").ok();
                primary.set_attribute("data-nexus-action", action).ok();
                primary
                    .set_attribute("aria-label", &format!("{} guest", action))
                    .ok();
                primary.set_class_name(&format!("nexus-primary-action {tone}"));
                primary.set_inner_html(&format!("<i class=\"{icon}\"></i>"));
            } else {
                primary.set_attribute("hidden", "true").ok();
                primary.remove_attribute("data-nexus-action").ok();
            }
        }
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
                        {details.open_pve.map(|enabled| html! {
                            <div><dt>{"PVE Web UI"}</dt><dd><button class="nexus-link-button" disabled={!enabled} onclick={open_pve.clone()}>{"Open in Proxmox "}<i class="fa fa-external-link"></i></button></dd></div>
                        })}
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
            if event.button() != 0 {
                return;
            }

            let Some(target) = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
            else {
                return;
            };

            if let Some(button) = ancestor_with_attribute(target.clone(), "data-nexus-primary") {
                event.stop_propagation();
                let Some(row) = row_from_element(button.clone()) else {
                    return;
                };
                let Some(details) = details_from_row(&row) else {
                    return;
                };
                if let Some(action) = button.get_attribute("data-nexus-action") {
                    trigger_hidden_action(&details.row_key, &action);
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
                mark_selected_row(Some(&details.row_key));
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
            mark_selected_row(Some(&details.row_key));
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
                trigger_hidden_action(&details.row_key, "open-pve");
            }
        })
    };

    let root_class = classes!(
        "nexus-inventory",
        selected.is_some().then_some("drawer-open")
    );

    html! {
        <div class={root_class}>
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
                <div class="nexus-inventory-table-shell" onmousedown={on_table_click}>{VNode::from(GuestPanel::new())}</div>
            </section>
            {selected.as_ref().map(|details| detail_drawer(details, close_drawer.clone(), open_pve.clone()))}
            {action_menu.as_ref().map(|menu| {
                let row_key = menu.guest.row_key.clone();
                let action_button = |label: &'static str,
                                     icon: &'static str,
                                     action: &'static str,
                                     state: Option<bool>,
                                     tone: &'static str| {
                    state.map(|enabled| {
                        let row_key = row_key.clone();
                        let action_menu = action_menu.clone();
                        let callback = Callback::from(move |event: MouseEvent| {
                            event.stop_propagation();
                            trigger_hidden_action(&row_key, action);
                            action_menu.set(None);
                        });
                        html! {
                            <button class={tone} role="menuitem" disabled={!enabled} onclick={callback}>
                                <i class={icon}></i><span>{label}</span>
                            </button>
                        }
                    })
                };
                let has_lifecycle = menu.guest.shutdown.is_some()
                    || menu.guest.start.is_some()
                    || menu.guest.resume.is_some();
                html! {
                    <div class="nexus-action-menu-backdrop" onclick={close_menu.clone()}>
                        <div
                            class="nexus-action-menu"
                            style={format!("left:{}px;top:{}px", (menu.x - 150).max(10), (menu.y + 8).max(10))}
                            onclick={Callback::from(|event: MouseEvent| event.stop_propagation())}
                            role="menu"
                            aria-label={format!("Actions for {}", menu.guest.name)}
                        >
                            {action_button("Snapshots", "fa fa-camera", "snapshots", menu.guest.snapshots, "")}
                            {action_button("Migrate", "fa fa-exchange", "migrate", menu.guest.migrate, "")}
                            {action_button("Open in PVE", "fa fa-external-link", "open-pve", menu.guest.open_pve, "")}
                            {has_lifecycle.then(|| html! {<div class="separator"></div>})}
                            {action_button("Shutdown", "fa fa-power-off", "shutdown", menu.guest.shutdown, "danger")}
                            {action_button("Resume", "fa fa-play", "resume", menu.guest.resume, "success")}
                            {action_button("Start", "fa fa-play", "start", menu.guest.start, "success")}
                        </div>
                    </div>
                }
            })}
        </div>
    }
}
