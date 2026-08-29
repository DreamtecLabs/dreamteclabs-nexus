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
                    pve.entry(key).or_insert_with(|| StorageEntry {
                        remote: remote.remote.clone(),
                        name: storage.storage.clone(),
                        location: if storage.shared {
                            "Shared cluster storage".to_string()
                        } else {
                            storage.node.clone()
                        },
                        kind: if storage.shared { "Shared" } else { "Local" }.to_string(),
                        status: storage.status.clone(),
                        used: storage.disk,
                        total: storage.maxdisk,
                    });
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

    html! {
        <section class={classes!("nexus-storage-tier-summary", tone.to_string())}>
            <div class="nexus-storage-summary-head">
                <span class="nexus-storage-summary-icon"><i class={icon.to_string()}></i></span>
                <div><span>{title}</span><strong>{entries.len()}</strong><small>{"storage targets"}</small></div>
            </div>
            <div class="nexus-storage-summary-capacity">
                <div><span>{"Capacity"}</span><strong>{format_bytes(total)}</strong></div>
                <div><span>{"Used"}</span><strong>{format_bytes(used)}</strong></div>
                <div><span>{"Free"}</span><strong>{format_bytes(free)}</strong></div>
            </div>
            <div class="nexus-storage-progress"><span style={format!("width:{pct:.1}%")}></span></div>
            <small class="nexus-storage-summary-foot">{format!("{pct:.1}% utilized")}</small>
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
                        let status_class = if entry.status == "online" || entry.status == "available" { "ok" } else { "warning" };
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
                    {storage_table("PVE Storage", "Live storage capacity exposed by Proxmox VE remotes. Shared storage is counted once per remote.", "fa fa-server", &pve)}
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
            <style>{STORAGE_CSS}</style>
            <header class="nexus-storage-header">
                <div><div class="nexus-storage-eyebrow">{"CAPACITY & DATA PROTECTION"}</div><h1>{"Storage"}</h1><p>{"Unified capacity view across Proxmox VE and Proxmox Backup Server."}</p></div>
                <span class="nexus-storage-live"><i></i>{"Live from PDM"}</span>
            </header>
            {content}
        </div>
    }
}

const STORAGE_CSS: &str = r#"
.nexus-storage-page{--text:#0f172a;--muted:#64748b;--border:#dce4ef;--blue:#2563eb;--green:#16a34a;--orange:#f97316;width:100%;height:100%;overflow:auto;padding:22px 30px 30px;background:linear-gradient(180deg,#f8faff 0,#f5f7fb 240px);color:var(--text);font-family:"Roboto Flex",Roboto,Arial,sans-serif;box-sizing:border-box}.nexus-storage-page *{box-sizing:border-box}.nexus-storage-header{display:flex;align-items:flex-start;justify-content:space-between;gap:20px;margin-bottom:16px}.nexus-storage-eyebrow{font-size:9px;font-weight:800;letter-spacing:.13em;color:#64748b}.nexus-storage-header h1{font-size:25px;margin:4px 0 3px}.nexus-storage-header p{margin:0;color:var(--muted);font-size:11px}.nexus-storage-live{display:flex;align-items:center;gap:8px;background:#fff;border:1px solid var(--border);border-radius:999px;padding:7px 11px;font-size:9px;color:#475569;box-shadow:0 2px 8px rgba(15,23,42,.04)}.nexus-storage-live i{width:7px;height:7px;border-radius:50%;background:#22c55e;box-shadow:0 0 0 3px #dcfce7}.nexus-storage-kpis{display:grid;grid-template-columns:1fr 1fr;gap:12px;margin-bottom:14px}.nexus-storage-tier-summary{background:#fff;border:1px solid var(--border);border-radius:12px;padding:14px 16px;box-shadow:0 3px 13px rgba(15,23,42,.05)}.nexus-storage-summary-head{display:flex;gap:11px;align-items:center}.nexus-storage-summary-icon{width:38px;height:38px;border-radius:10px;background:#eef4ff;color:#2563eb;display:flex;align-items:center;justify-content:center}.nexus-storage-tier-summary.pbs .nexus-storage-summary-icon{background:#fff7ed;color:#ea580c}.nexus-storage-summary-head>div{display:grid;grid-template-columns:auto auto;column-gap:7px;align-items:baseline}.nexus-storage-summary-head span{font-size:10px;font-weight:750}.nexus-storage-summary-head strong{font-size:20px}.nexus-storage-summary-head small{grid-column:1/-1;color:var(--muted);font-size:8px}.nexus-storage-summary-capacity{display:grid;grid-template-columns:repeat(3,1fr);gap:12px;margin-top:13px}.nexus-storage-summary-capacity div{display:flex;flex-direction:column}.nexus-storage-summary-capacity span{font-size:8px;color:var(--muted)}.nexus-storage-summary-capacity strong{font-size:13px;margin-top:2px}.nexus-storage-progress{height:5px;background:#e8edf4;border-radius:999px;overflow:hidden;margin-top:12px}.nexus-storage-progress span{display:block;height:100%;background:#2563eb;border-radius:999px}.nexus-storage-tier-summary.pbs .nexus-storage-progress span{background:#f97316}.nexus-storage-summary-foot{display:block;text-align:right;color:var(--muted);font-size:8px;margin-top:5px}.nexus-storage-panel{background:#fff;border:1px solid var(--border);border-radius:12px;box-shadow:0 3px 13px rgba(15,23,42,.05);overflow:hidden;margin-top:12px}.nexus-storage-panel-head{display:flex;align-items:center;justify-content:space-between;padding:13px 15px;border-bottom:1px solid #e8edf4}.nexus-storage-panel-head h2{font-size:12px;margin:0;display:flex;gap:8px;align-items:center}.nexus-storage-panel-head h2 i{color:#2563eb}.nexus-storage-panel-head p{font-size:8px;color:var(--muted);margin:3px 0 0}.nexus-storage-count{font-size:8px;color:#475569;background:#f8fafc;border:1px solid #e2e8f0;padding:5px 8px;border-radius:999px}.nexus-storage-table-wrap{overflow:auto}.nexus-storage-table{width:100%;border-collapse:collapse;font-size:9px;min-width:930px}.nexus-storage-table th{text-align:left;background:#f8fafc;color:#526176;font-size:8px;font-weight:760;padding:9px 11px;border-bottom:1px solid #dfe6ef;white-space:nowrap}.nexus-storage-table td{padding:10px 11px;border-bottom:1px solid #eef2f7;color:#263244;white-space:nowrap}.nexus-storage-table tbody tr:last-child td{border-bottom:0}.nexus-storage-table tbody tr:hover td{background:#f7faff}.nexus-storage-table td strong{color:#0f172a}.nexus-storage-type{display:inline-flex;padding:3px 6px;border-radius:999px;background:#f1f5f9;color:#475569}.nexus-storage-status{display:inline-flex;align-items:center;gap:5px;text-transform:capitalize}.nexus-storage-status i{width:6px;height:6px;border-radius:50%;background:#94a3b8}.nexus-storage-status.ok i{background:#22c55e}.nexus-storage-status.warning i{background:#f59e0b}.nexus-storage-usage-cell{display:flex;align-items:center;gap:7px;min-width:115px}.nexus-storage-usage-cell>span{width:34px;text-align:right}.nexus-storage-usage-cell>div{height:5px;flex:1;background:#e8edf4;border-radius:999px;overflow:hidden}.nexus-storage-usage-cell i{display:block;height:100%;background:#2563eb;border-radius:999px}.nexus-storage-empty{text-align:center!important;color:#94a3b8!important;padding:25px!important}.nexus-storage-warning,.nexus-storage-error,.nexus-storage-loading{display:flex;align-items:center;gap:8px;border-radius:9px;padding:10px 12px;font-size:9px;margin-bottom:12px}.nexus-storage-warning{background:#fff7ed;border:1px solid #fed7aa;color:#9a3412}.nexus-storage-error{background:#fef2f2;border:1px solid #fecaca;color:#991b1b}.nexus-storage-loading{background:#fff;border:1px solid var(--border);color:#64748b}.nexus-storage-loading i{color:#2563eb}@media(max-width:900px){.nexus-storage-page{padding:16px}.nexus-storage-kpis{grid-template-columns:1fr}.nexus-storage-header{flex-direction:column}.nexus-storage-live{align-self:flex-start}}
"#;
