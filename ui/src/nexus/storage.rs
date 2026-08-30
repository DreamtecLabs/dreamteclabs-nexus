use std::collections::BTreeMap;

use pdm_api_types::resource::{RemoteResources, Resource};
use proxmox_yew_comp::http_get;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

#[derive(Clone, PartialEq)]
struct StorageEntry {
    remote: String,
    name: String,
    location: String,
    kind: String,
    status: String,
    used: u64,
    total: u64,
}

fn format_bytes(bytes: u64) -> String {
    let gib = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    if gib >= 1024.0 {
        format!("{:.1} TiB", gib / 1024.0)
    } else {
        format!("{gib:.1} GiB")
    }
}

fn usage_percent(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
    }
}

fn normalized_status(status: &str) -> String {
    status.trim().to_ascii_lowercase()
}

fn merge_shared_storage(existing: &mut StorageEntry, used: u64, total: u64, status: &str) {
    existing.used = existing.used.max(used);
    existing.total = existing.total.max(total);

    let current_status = normalized_status(&existing.status);
    let incoming_status = normalized_status(status);
    if matches!(incoming_status.as_str(), "available" | "online")
        || !matches!(current_status.as_str(), "available" | "online")
    {
        existing.status = status.to_string();
    }
}

fn split_storage(
    remotes: &[RemoteResources],
) -> (Vec<StorageEntry>, Vec<StorageEntry>, Vec<String>) {
    let mut pve = BTreeMap::<String, StorageEntry>::new();
    let mut pbs = BTreeMap::<String, StorageEntry>::new();
    let mut errors = Vec::new();

    for remote in remotes {
        if let Some(error) = &remote.error {
            errors.push(format!("{}: {}", remote.remote, error));
        }

        for resource in &remote.resources {
            match resource {
                Resource::PveStorage(storage) => {
                    let key = if storage.shared {
                        format!("{}/shared/{}", remote.remote, storage.storage)
                    } else {
                        format!("{}/{}/{}", remote.remote, storage.node, storage.storage)
                    };

                    if storage.shared {
                        pve.entry(key)
                            .and_modify(|entry| {
                                merge_shared_storage(
                                    entry,
                                    storage.disk,
                                    storage.maxdisk,
                                    &storage.status,
                                );
                            })
                            .or_insert_with(|| StorageEntry {
                                remote: remote.remote.clone(),
                                name: storage.storage.clone(),
                                location: "Shared cluster storage".to_string(),
                                kind: "Shared".to_string(),
                                status: storage.status.clone(),
                                used: storage.disk,
                                total: storage.maxdisk,
                            });
                    } else {
                        pve.insert(
                            key,
                            StorageEntry {
                                remote: remote.remote.clone(),
                                name: storage.storage.clone(),
                                location: storage.node.clone(),
                                kind: "Local".to_string(),
                                status: storage.status.clone(),
                                used: storage.disk,
                                total: storage.maxdisk,
                            },
                        );
                    }
                }
                Resource::PbsDatastore(datastore) => {
                    let key = format!("{}/{}", remote.remote, datastore.name);
                    pbs.insert(
                        key,
                        StorageEntry {
                            remote: remote.remote.clone(),
                            name: datastore.name.clone(),
                            location: datastore
                                .backing_device
                                .clone()
                                .unwrap_or_else(|| "PBS datastore".to_string()),
                            kind: datastore
                                .backend_type
                                .as_ref()
                                .map(ToString::to_string)
                                .unwrap_or_else(|| "Datastore".to_string()),
                            status: if datastore.maintenance.is_some() {
                                "maintenance".to_string()
                            } else {
                                "online".to_string()
                            },
                            used: datastore.disk,
                            total: datastore.maxdisk,
                        },
                    );
                }
                _ => {}
            }
        }
    }

    (
        pve.into_values().collect(),
        pbs.into_values().collect(),
        errors,
    )
}

fn tier_summary(title: &str, icon: &str, entries: &[StorageEntry], tone: &str) -> Html {
    let total = entries.iter().map(|entry| entry.total).sum::<u64>();
    let used = entries.iter().map(|entry| entry.used).sum::<u64>();
    let free = total.saturating_sub(used);
    let pct = usage_percent(used, total);
    let is_pve = tone == "pve";
    let capacity_label = if is_pve {
        "Configured capacity"
    } else {
        "Capacity"
    };
    let used_label = if is_pve { "Reported used" } else { "Used" };
    let free_label = if is_pve { "Reported free" } else { "Free" };
    let footer = if is_pve {
        format!("{pct:.1}% target utilization · physical backing may overlap")
    } else {
        format!("{pct:.1}% utilized")
    };

    html! {
        <section class={classes!("nexus-storage-tier-summary", tone.to_string())}>
            <div class="nexus-storage-summary-head">
                <span class="nexus-storage-summary-icon"><i class={icon.to_string()}></i></span>
                <div><span>{title}</span><strong>{entries.len()}</strong><small>{"storage targets"}</small></div>
            </div>
            <div class="nexus-storage-summary-capacity">
                <div><span>{capacity_label}</span><strong>{format_bytes(total)}</strong></div>
                <div><span>{used_label}</span><strong>{format_bytes(used)}</strong></div>
                <div><span>{free_label}</span><strong>{format_bytes(free)}</strong></div>
            </div>
            <div class="nexus-storage-progress"><span style={format!("width:{pct:.1}%")}></span></div>
            <small class="nexus-storage-summary-foot">{footer}</small>
        </section>
    }
}

fn storage_table(title: &str, subtitle: &str, icon: &str, entries: &[StorageEntry]) -> Html {
    html! {
        <section class="nexus-storage-panel">
            <header class="nexus-storage-panel-head">
                <div><h2><i class={icon.to_string()}></i>{title}</h2><p>{subtitle}</p></div>
                <span class="nexus-storage-count">{format!("{} targets", entries.len())}</span>
            </header>
            <div class="nexus-storage-table-wrap">
                <table class="nexus-storage-table">
                    <thead><tr><th>{"Storage"}</th><th>{"Remote"}</th><th>{"Location"}</th><th>{"Type"}</th><th>{"Status"}</th><th>{"Capacity"}</th><th>{"Used"}</th><th>{"Free"}</th><th>{"Usage"}</th></tr></thead>
                    <tbody>
                    {for entries.iter().map(|entry| {
                        let pct = usage_percent(entry.used, entry.total);
                        let normalized = normalized_status(&entry.status);
                        let status_class = if matches!(normalized.as_str(), "online" | "available") { "ok" } else { "warning" };
                        html! {
                            <tr>
                                <td><strong>{entry.name.clone()}</strong></td>
                                <td>{entry.remote.clone()}</td>
                                <td>{entry.location.clone()}</td>
                                <td><span class="nexus-storage-type">{entry.kind.clone()}</span></td>
                                <td><span class={classes!("nexus-storage-status", status_class)}><i></i>{entry.status.clone()}</span></td>
                                <td>{format_bytes(entry.total)}</td>
                                <td>{format_bytes(entry.used)}</td>
                                <td>{format_bytes(entry.total.saturating_sub(entry.used))}</td>
                                <td>
                                    <div class="nexus-storage-usage-cell"><span>{format!("{pct:.1}%")}</span><div><i style={format!("width:{pct:.1}%")}></i></div></div>
                                </td>
                            </tr>
                        }
                    })}
                    {if entries.is_empty() { html! { <tr><td colspan="9" class="nexus-storage-empty">{"No storage targets reported in this tier."}</td></tr> } } else { Html::default() }}
                    </tbody>
                </table>
            </div>
        </section>
    }
}

#[function_component(NexusStorage)]
pub fn nexus_storage() -> Html {
    let resources = use_state(|| None::<Result<Vec<RemoteResources>, String>>);

    {
        let resources = resources.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let result: Result<Vec<RemoteResources>, _> =
                    http_get("/resources/list", None).await;
                resources.set(Some(result.map_err(|err| err.to_string())));
            });
            || ()
        });
    }

    let content = match resources.as_ref() {
        Some(Ok(data)) => {
            let (pve, pbs, errors) = split_storage(data);
            html! {
                <>
                    <div class="nexus-storage-kpis">
                        {tier_summary("PVE Storage Tier", "fa fa-server", &pve, "pve")}
                        {tier_summary("PBS Backup Tier", "fa fa-database", &pbs, "pbs")}
                    </div>
                    {if errors.is_empty() { Html::default() } else { html! {
                        <div class="nexus-storage-warning"><i class="fa fa-exclamation-triangle"></i><span>{format!("Some remotes could not report storage: {}", errors.join(" · "))}</span></div>
                    } }}
                    {storage_table("PVE Storage", "Configured storage targets exposed by Proxmox VE. Shared storage is consolidated once per remote and storage ID; target totals are not presented as unique physical capacity.", "fa fa-server", &pve)}
                    {storage_table("PBS Datastores", "Backup capacity exposed by Proxmox Backup Server remotes.", "fa fa-database", &pbs)}
                </>
            }
        }
        Some(Err(error)) => {
            html! { <div class="nexus-storage-error"><i class="fa fa-exclamation-triangle"></i>{format!(" Storage inventory unavailable: {error}")}</div> }
        }
        None => {
            html! { <div class="nexus-storage-loading"><i class="fa fa-refresh fa-spin"></i>{" Loading storage inventory…"}</div> }
        }
    };

    html! {
        <div class="nexus-storage-page">
            <header class="nexus-storage-header">
                <div><div class="nexus-storage-eyebrow">{"CAPACITY & DATA PROTECTION"}</div><h1>{"Storage"}</h1><p>{"Unified capacity view across Proxmox VE and Proxmox Backup Server."}</p></div>
                <span class="nexus-storage-live"><i></i>{"Live from PDM"}</span>
            </header>
            {content}
        </div>
    }
}
