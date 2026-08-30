use std::collections::{BTreeMap, BTreeSet};

use futures::future::join_all;
use pdm_api_types::RemoteUpid;
use pdm_api_types::resource::{RemoteResources, Resource};
use proxmox_yew_comp::{http_get, http_post};
use serde_json::{Value, json};
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlInputElement, HtmlSelectElement};
use yew::prelude::*;

#[derive(Clone, PartialEq, Eq)]
struct PveTarget {
    remote: String,
    node: String,
}

#[derive(Clone, PartialEq, Eq)]
struct StorageTarget {
    remote: String,
    node: String,
    storage: String,
    shared: bool,
}

#[derive(Clone, PartialEq, Eq)]
struct DeploymentTask {
    remote: String,
    node: String,
    kind: String,
    id: String,
    status: String,
    upid: String,
}

fn discover_targets(resources: &[RemoteResources]) -> (Vec<PveTarget>, Vec<StorageTarget>) {
    let mut targets = BTreeSet::new();
    let mut storages = BTreeMap::new();

    for remote in resources {
        for resource in &remote.resources {
            match resource {
                Resource::PveNode(node) => {
                    targets.insert((remote.remote.clone(), node.node.clone()));
                }
                Resource::PveStorage(storage)
                    if storage.status.eq_ignore_ascii_case("available") =>
                {
                    let key = (
                        remote.remote.clone(),
                        storage.node.clone(),
                        storage.storage.clone(),
                    );
                    storages.entry(key).or_insert_with(|| StorageTarget {
                        remote: remote.remote.clone(),
                        node: storage.node.clone(),
                        storage: storage.storage.clone(),
                        shared: storage.shared,
                    });
                }
                _ => {}
            }
        }
    }

    (
        targets
            .into_iter()
            .map(|(remote, node)| PveTarget { remote, node })
            .collect(),
        storages.into_values().collect(),
    )
}

fn input_value(event: InputEvent) -> String {
    event.target_unchecked_into::<HtmlInputElement>().value()
}

fn select_value(event: Event) -> String {
    event.target_unchecked_into::<HtmlSelectElement>().value()
}

fn selected_or_first(selected: &str, values: impl Iterator<Item = String>) -> String {
    if !selected.is_empty() {
        return selected.to_string();
    }
    values.into_iter().next().unwrap_or_default()
}

fn json_display(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(value) if !value.is_null() => value.to_string(),
        _ => "—".to_string(),
    }
}

fn task_from_value(remote: &str, value: &Value) -> Option<DeploymentTask> {
    let kind = value.get("type")?.as_str()?.to_string();
    if !matches!(
        kind.as_str(),
        "qmcreate" | "vzcreate" | "qmclone" | "vzclone"
    ) {
        return None;
    }

    Some(DeploymentTask {
        remote: remote.to_string(),
        node: value
            .get("node")
            .and_then(Value::as_str)
            .unwrap_or("—")
            .to_string(),
        kind,
        id: json_display(value.get("id")),
        status: value
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("Running")
            .to_string(),
        upid: value
            .get("upid")
            .and_then(Value::as_str)
            .unwrap_or("—")
            .to_string(),
    })
}

async fn load_deployment_tasks(
    resources: Vec<RemoteResources>,
) -> Result<Vec<DeploymentTask>, String> {
    let remotes: BTreeSet<String> = resources
        .iter()
        .filter(|remote| {
            remote
                .resources
                .iter()
                .any(|resource| matches!(resource, Resource::PveNode(_)))
        })
        .map(|remote| remote.remote.clone())
        .collect();

    let results = join_all(remotes.into_iter().map(|remote| async move {
        let url = format!("/pve/remotes/{remote}/tasks");
        let result: Result<Vec<Value>, _> = http_get(&url, None).await;
        (remote, result)
    }))
    .await;

    let mut tasks = Vec::new();
    let mut errors = Vec::new();
    for (remote, result) in results {
        match result {
            Ok(values) => tasks.extend(
                values
                    .iter()
                    .filter_map(|value| task_from_value(&remote, value)),
            ),
            Err(err) => errors.push(format!("{remote}: {err}")),
        }
    }

    if tasks.is_empty() && !errors.is_empty() {
        return Err(errors.join(" · "));
    }

    tasks.truncate(30);
    Ok(tasks)
}

#[function_component(NexusInfrastructure)]
pub fn nexus_infrastructure() -> Html {
    let resources = use_state(|| None::<Result<Vec<RemoteResources>, String>>);
    let workload = use_state(|| "qemu".to_string());
    let remote = use_state(String::new);
    let node = use_state(String::new);
    let storage = use_state(String::new);
    let vmid = use_state(String::new);
    let name = use_state(String::new);
    let cores = use_state(|| "2".to_string());
    let memory = use_state(|| "2048".to_string());
    let disk = use_state(|| "32".to_string());
    let bridge = use_state(|| "vmbr0".to_string());
    let source = use_state(String::new);
    let password = use_state(String::new);
    let start = use_state(|| true);
    let busy = use_state(|| false);
    let result = use_state(|| None::<Result<String, String>>);

    {
        let resources = resources.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let value: Result<Vec<RemoteResources>, _> =
                    http_get("/resources/list", None).await;
                resources.set(Some(value.map_err(|err| err.to_string())));
            });
            || ()
        });
    }

    let (targets, storages) = resources
        .as_ref()
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .map(|resources| discover_targets(resources))
        .unwrap_or_default();

    let selected_remote =
        selected_or_first(&remote, targets.iter().map(|target| target.remote.clone()));
    let selected_node = selected_or_first(
        &node,
        targets
            .iter()
            .filter(|target| target.remote == selected_remote)
            .map(|target| target.node.clone()),
    );
    let selected_storage = selected_or_first(
        &storage,
        storages
            .iter()
            .filter(|target| {
                target.remote == selected_remote && (target.shared || target.node == selected_node)
            })
            .map(|target| target.storage.clone()),
    );

    let use_next_id = {
        let vmid = vmid.clone();
        let result = result.clone();
        let selected_remote = selected_remote.clone();
        Callback::from(move |_| {
            if selected_remote.is_empty() {
                result.set(Some(Err("Select a PVE remote first.".to_string())));
                return;
            }
            let vmid = vmid.clone();
            let result = result.clone();
            let url = format!("/pve/remotes/{selected_remote}/cluster-nextid");
            spawn_local(async move {
                let next: Result<u32, _> = http_get(&url, None).await;
                match next {
                    Ok(next) => vmid.set(next.to_string()),
                    Err(err) => {
                        result.set(Some(Err(format!("Unable to reserve the next VMID: {err}"))))
                    }
                }
            });
        })
    };

    let deploy = {
        let workload = (*workload).clone();
        let selected_remote = selected_remote.clone();
        let selected_node = selected_node.clone();
        let selected_storage = selected_storage.clone();
        let vmid = (*vmid).clone();
        let name = (*name).clone();
        let cores = (*cores).clone();
        let memory = (*memory).clone();
        let disk = (*disk).clone();
        let bridge = (*bridge).clone();
        let source = (*source).clone();
        let password = (*password).clone();
        let start_value = *start;
        let busy = busy.clone();
        let result = result.clone();

        Callback::from(move |_| {
            let parsed = || -> Result<(u32, u64, u64, u64), String> {
                let vmid = vmid
                    .parse::<u32>()
                    .map_err(|_| "VMID must be a number.".to_string())?;
                let cores = cores
                    .parse::<u64>()
                    .map_err(|_| "Cores must be a number.".to_string())?;
                let memory = memory
                    .parse::<u64>()
                    .map_err(|_| "Memory must be a number.".to_string())?;
                let disk = disk
                    .parse::<u64>()
                    .map_err(|_| "Disk size must be a number.".to_string())?;
                Ok((vmid, cores, memory, disk))
            }();

            let (vmid, cores, memory, disk) = match parsed {
                Ok(value) => value,
                Err(err) => {
                    result.set(Some(Err(err)));
                    return;
                }
            };

            if selected_remote.is_empty() || selected_node.is_empty() || selected_storage.is_empty()
            {
                result.set(Some(Err(
                    "Remote, node and storage are required.".to_string()
                )));
                return;
            }
            if name.trim().is_empty() {
                result.set(Some(Err(if workload == "lxc" {
                    "Container hostname is required.".to_string()
                } else {
                    "Virtual machine name is required.".to_string()
                })));
                return;
            }
            if workload == "lxc" && source.trim().is_empty() {
                result.set(Some(Err(
                    "An LXC OS template volume is required.".to_string()
                )));
                return;
            }

            busy.set(true);
            result.set(None);
            let busy = busy.clone();
            let result = result.clone();
            let remote = selected_remote.clone();
            let node = selected_node.clone();
            let storage = selected_storage.clone();
            let workload = workload.clone();
            let name = name.clone();
            let bridge = bridge.clone();
            let source = source.clone();
            let password = password.clone();

            spawn_local(async move {
                let (url, payload) = if workload == "lxc" {
                    (
                        format!("/pve/remotes/{remote}/deploy/lxc"),
                        json!({
                            "node": node,
                            "vmid": vmid,
                            "hostname": name,
                            "ostemplate": source,
                            "cores": cores,
                            "memory": memory,
                            "storage": storage,
                            "disk-gb": disk,
                            "bridge": bridge,
                            "password": if password.is_empty() { Value::Null } else { Value::String(password) },
                            "start": start_value,
                        }),
                    )
                } else {
                    (
                        format!("/pve/remotes/{remote}/deploy/qemu"),
                        json!({
                            "node": node,
                            "vmid": vmid,
                            "name": name,
                            "cores": cores,
                            "memory": memory,
                            "storage": storage,
                            "disk-gb": disk,
                            "bridge": bridge,
                            "iso": if source.is_empty() { Value::Null } else { Value::String(source) },
                            "start": start_value,
                        }),
                    )
                };

                let response: Result<RemoteUpid, _> = http_post(&url, Some(payload)).await;
                busy.set(false);
                result.set(Some(
                    response
                        .map(|upid| format!("Deployment submitted to PDM: {upid:?}"))
                        .map_err(|err| err.to_string()),
                ));
            });
        })
    };

    let remote_onchange = {
        let remote = remote.clone();
        let node = node.clone();
        let storage = storage.clone();
        Callback::from(move |event: Event| {
            remote.set(select_value(event));
            node.set(String::new());
            storage.set(String::new());
        })
    };
    let node_onchange = {
        let node = node.clone();
        let storage = storage.clone();
        Callback::from(move |event: Event| {
            node.set(select_value(event));
            storage.set(String::new());
        })
    };

    html! {
        <div class="nexus-infra-page">
            <header class="nexus-infra-header">
                <div><div class="nexus-infra-eyebrow">{"PROVISIONING & LIFECYCLE"}</div><h1>{"Infrastructure"}</h1><p>{"Deploy compute workloads through the PDM control plane. No browser-to-PVE calls."}</p></div>
                <span class="nexus-infra-live"><i></i>{"PDM native"}</span>
            </header>

            <div class="nexus-infra-workload-cards">
                <button class={classes!("nexus-infra-workload", (*workload == "qemu").then_some("active"))} onclick={{ let workload = workload.clone(); Callback::from(move |_| workload.set("qemu".to_string())) }}>
                    <i class="fa fa-desktop"></i><strong>{"Virtual Machine"}</strong><span>{"QEMU/KVM workload with configurable compute, storage and install media."}</span>
                </button>
                <button class={classes!("nexus-infra-workload", (*workload == "lxc").then_some("active"))} onclick={{ let workload = workload.clone(); Callback::from(move |_| workload.set("lxc".to_string())) }}>
                    <i class="fa fa-cube"></i><strong>{"Linux Container"}</strong><span>{"Lightweight LXC workload from an existing PVE OS template."}</span>
                </button>
            </div>

            <section class="nexus-infra-panel">
                <header><div><h2>{if *workload == "lxc" { "Deploy Linux Container" } else { "Deploy Virtual Machine" }}</h2><p>{"The request is validated and executed by the PDM backend against the selected PVE remote."}</p></div></header>
                <div class="nexus-infra-form-grid">
                    <label><span>{"PVE remote"}</span><select onchange={remote_onchange} value={selected_remote.clone()}>{for targets.iter().map(|target| target.remote.clone()).collect::<BTreeSet<_>>().into_iter().map(|value| html!{<option value={value.clone()}>{value}</option>})}</select></label>
                    <label><span>{"Node"}</span><select onchange={node_onchange} value={selected_node.clone()}>{for targets.iter().filter(|target| target.remote == selected_remote).map(|target| html!{<option value={target.node.clone()}>{target.node.clone()}</option>})}</select></label>
                    <label><span>{"Storage"}</span><select onchange={{ let storage = storage.clone(); Callback::from(move |event: Event| storage.set(select_value(event))) }} value={selected_storage.clone()}>{for storages.iter().filter(|target| target.remote == selected_remote && (target.shared || target.node == selected_node)).map(|target| html!{<option value={target.storage.clone()}>{format!("{}{}", target.storage, if target.shared { " · shared" } else { "" })}</option>})}</select></label>
                    <label><span>{"VMID"}</span><div class="nexus-infra-inline"><input value={(*vmid).clone()} oninput={{ let vmid = vmid.clone(); Callback::from(move |event: InputEvent| vmid.set(input_value(event))) }} placeholder="Next available ID"/><button type="button" onclick={use_next_id}>{"Next ID"}</button></div></label>
                    <label><span>{if *workload == "lxc" { "Hostname" } else { "VM name" }}</span><input value={(*name).clone()} oninput={{ let name = name.clone(); Callback::from(move |event: InputEvent| name.set(input_value(event))) }} placeholder={if *workload == "lxc" { "app-01" } else { "server-01" }}/></label>
                    <label><span>{"CPU cores"}</span><input type="number" min="1" value={(*cores).clone()} oninput={{ let cores = cores.clone(); Callback::from(move |event: InputEvent| cores.set(input_value(event))) }}/></label>
                    <label><span>{"Memory (MiB)"}</span><input type="number" min="64" value={(*memory).clone()} oninput={{ let memory = memory.clone(); Callback::from(move |event: InputEvent| memory.set(input_value(event))) }}/></label>
                    <label><span>{"Disk (GiB)"}</span><input type="number" min="1" value={(*disk).clone()} oninput={{ let disk = disk.clone(); Callback::from(move |event: InputEvent| disk.set(input_value(event))) }}/></label>
                    <label><span>{"Network bridge"}</span><input value={(*bridge).clone()} oninput={{ let bridge = bridge.clone(); Callback::from(move |event: InputEvent| bridge.set(input_value(event))) }} placeholder="vmbr0"/></label>
                    <label class="nexus-infra-wide"><span>{if *workload == "lxc" { "OS template volume" } else { "Installation ISO volume (optional)" }}</span><input value={(*source).clone()} oninput={{ let source = source.clone(); Callback::from(move |event: InputEvent| source.set(input_value(event))) }} placeholder={if *workload == "lxc" { "local:vztmpl/debian-13-standard_13.1-2_amd64.tar.zst" } else { "local:iso/debian.iso" }}/></label>
                    {if *workload == "lxc" { html!{<label class="nexus-infra-wide"><span>{"Root password (optional)"}</span><input type="password" value={(*password).clone()} oninput={{ let password = password.clone(); Callback::from(move |event: InputEvent| password.set(input_value(event))) }}/></label>} } else { Html::default() }}
                </div>
                <div class="nexus-infra-actions">
                    <label class="nexus-infra-check"><input type="checkbox" checked={*start} onchange={{ let start = start.clone(); Callback::from(move |event: Event| start.set(event.target_unchecked_into::<HtmlInputElement>().checked())) }}/><span>{"Start after deployment"}</span></label>
                    <button class="nexus-infra-deploy" disabled={*busy || targets.is_empty()} onclick={deploy}>{if *busy { "Submitting to PDM…" } else { "Deploy workload" }}</button>
                </div>
                {match result.as_ref() {
                    Some(Ok(message)) => html!{<div class="nexus-infra-result ok"><i class="fa fa-check-circle"></i>{message.clone()}</div>},
                    Some(Err(message)) => html!{<div class="nexus-infra-result error"><i class="fa fa-exclamation-triangle"></i>{message.clone()}</div>},
                    None => Html::default(),
                }}
            </section>

            <div class="nexus-infra-note"><i class="fa fa-shield"></i><div><strong>{"Control-plane boundary"}</strong><span>{"Creation is performed by PDM server-side APIs using the configured PVE remote. Credentials and PVE endpoints are never exposed to this workflow."}</span></div></div>
        </div>
    }
}

#[function_component(NexusDeployments)]
pub fn nexus_deployments() -> Html {
    let tasks = use_state(|| None::<Result<Vec<DeploymentTask>, String>>);
    let refresh_token = use_state(|| 0u64);

    {
        let tasks = tasks.clone();
        let refresh = *refresh_token;
        use_effect_with(refresh, move |_| {
            spawn_local(async move {
                let resources: Result<Vec<RemoteResources>, _> =
                    http_get("/resources/list", None).await;
                let result = match resources {
                    Ok(resources) => load_deployment_tasks(resources).await,
                    Err(err) => Err(err.to_string()),
                };
                tasks.set(Some(result));
            });
            || ()
        });
    }

    html! {
        <div class="nexus-infra-page">
            <header class="nexus-infra-header">
                <div><div class="nexus-infra-eyebrow">{"PROVISIONING HISTORY"}</div><h1>{"Deployments"}</h1><p>{"Recent VM and LXC creation tasks reported directly by PVE through PDM."}</p></div>
                <button class="nexus-infra-refresh" onclick={{ let refresh_token = refresh_token.clone(); Callback::from(move |_| refresh_token.set(*refresh_token + 1)) }}><i class="fa fa-refresh"></i>{"Refresh"}</button>
            </header>
            <section class="nexus-infra-panel">
                <div class="nexus-infra-table-wrap"><table class="nexus-infra-table">
                    <thead><tr><th>{"Remote"}</th><th>{"Node"}</th><th>{"Operation"}</th><th>{"Guest"}</th><th>{"Status"}</th><th>{"PVE task"}</th></tr></thead>
                    <tbody>
                    {match tasks.as_ref() {
                        Some(Ok(values)) if values.is_empty() => html!{<tr><td colspan="6" class="nexus-infra-empty">{"No recent VM/LXC deployment tasks were reported."}</td></tr>},
                        Some(Ok(values)) => html!{for values.iter().map(|task| {
                            let status_class = if task.status.eq_ignore_ascii_case("ok") { "ok" } else if task.status.eq_ignore_ascii_case("running") { "running" } else { "error" };
                            html!{<tr><td><strong>{task.remote.clone()}</strong></td><td>{task.node.clone()}</td><td>{match task.kind.as_str() { "qmcreate" => "Create VM", "vzcreate" => "Create LXC", "qmclone" => "Clone VM", "vzclone" => "Clone LXC", _ => task.kind.as_str() }}</td><td>{task.id.clone()}</td><td><span class={classes!("nexus-infra-status", status_class)}>{task.status.clone()}</span></td><td class="nexus-infra-upid">{task.upid.clone()}</td></tr>}
                        })},
                        Some(Err(error)) => html!{<tr><td colspan="6" class="nexus-infra-empty error">{format!("Deployment history unavailable: {error}")}</td></tr>},
                        None => html!{<tr><td colspan="6" class="nexus-infra-empty"><i class="fa fa-refresh fa-spin"></i>{" Loading deployment tasks…"}</td></tr>},
                    }}
                    </tbody>
                </table></div>
            </section>
        </div>
    }
}