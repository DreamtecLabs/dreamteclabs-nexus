use pdm_api_types::resource::{RemoteStatus, ResourcesStatus};
use proxmox_yew_comp::http_get;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

fn pct(used: f64, total: f64) -> f64 {
    if total <= 0.0 {
        0.0
    } else {
        ((used / total) * 100.0).clamp(0.0, 100.0)
    }
}

fn format_bytes(value: u64) -> String {
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    const TIB: f64 = GIB * 1024.0;
    let value = value as f64;
    if value >= TIB {
        format!("{:.1} TiB", value / TIB)
    } else {
        format!("{:.1} GiB", value / GIB)
    }
}

fn metric_card(label: &str, value: String, detail: String, accent: &str) -> Html {
    html! {
        <div style="background:#fff;border:1px solid #e5e7eb;border-radius:14px;padding:18px;box-shadow:0 1px 2px rgba(15,23,42,.04);">
            <div style="display:flex;align-items:center;justify-content:space-between;gap:12px;">
                <span style="font-size:11px;font-weight:700;letter-spacing:.06em;text-transform:uppercase;color:#64748b;">{label}</span>
                <span style={format!("display:inline-block;width:9px;height:9px;border-radius:999px;background:{accent};")}></span>
            </div>
            <div style="font-size:27px;font-weight:750;line-height:1.1;margin-top:12px;color:#0f172a;">{value}</div>
            <div style="font-size:12px;color:#64748b;margin-top:7px;">{detail}</div>
        </div>
    }
}

fn usage_card(title: &str, percentage: f64, detail: String) -> Html {
    html! {
        <div style="background:#fff;border:1px solid #e5e7eb;border-radius:14px;padding:18px;box-shadow:0 1px 2px rgba(15,23,42,.04);">
            <div style="display:flex;align-items:start;justify-content:space-between;gap:16px;">
                <div>
                    <div style="font-size:13px;font-weight:700;color:#0f172a;">{title}</div>
                    <div style="font-size:12px;color:#64748b;margin-top:4px;">{detail}</div>
                </div>
                <div style="font-size:22px;font-weight:750;color:#0f172a;">{format!("{percentage:.0}%")}</div>
            </div>
            <div style="height:8px;background:#e2e8f0;border-radius:999px;overflow:hidden;margin-top:18px;">
                <div style={format!("height:100%;width:{percentage:.1}%;background:#2563eb;border-radius:999px;")}></div>
            </div>
        </div>
    }
}

fn dashboard(status: &ResourcesStatus) -> Html {
    let pve_nodes = status.pve_nodes.online + status.pve_nodes.offline + status.pve_nodes.unknown;
    let vm_total =
        status.qemu.running + status.qemu.stopped + status.qemu.template + status.qemu.unknown;
    let lxc_total =
        status.lxc.running + status.lxc.stopped + status.lxc.template + status.lxc.unknown;
    let healthy = status
        .remote_list
        .iter()
        .filter(|r| matches!(r.status.clone(), RemoteStatus::Good))
        .count();
    let warnings = status
        .remote_list
        .iter()
        .filter(|r| {
            matches!(
                r.status.clone(),
                RemoteStatus::Warning | RemoteStatus::Error
            )
        })
        .count()
        + status.failed_remotes as usize;

    let cpu = pct(status.pve_cpu_stats.used, status.pve_cpu_stats.max);
    let memory = pct(
        status.pve_memory_stats.used as f64,
        status.pve_memory_stats.total as f64,
    );
    let storage = pct(
        status.pve_storage_stats.used as f64,
        status.pve_storage_stats.total as f64,
    );
    let backup_storage = pct(
        status.pbs_storage_stats.used as f64,
        status.pbs_storage_stats.total as f64,
    );

    html! {
        <>
            <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(180px,1fr));gap:14px;">
                {metric_card("Platform health", if warnings == 0 { "Healthy".into() } else { format!("{warnings} issue(s)") }, format!("{healthy} / {} remotes healthy", status.remotes), if warnings == 0 { "#10b981" } else { "#f59e0b" })}
                {metric_card("PVE nodes", status.pve_nodes.online.to_string(), format!("{pve_nodes} total · {} offline", status.pve_nodes.offline), "#2563eb")}
                {metric_card("Virtual machines", status.qemu.running.to_string(), format!("{vm_total} total · {} stopped", status.qemu.stopped), "#8b5cf6")}
                {metric_card("Containers", status.lxc.running.to_string(), format!("{lxc_total} total · {} stopped", status.lxc.stopped), "#0ea5e9")}
                {metric_card("Backup server", status.pbs_nodes.online.to_string(), format!("{} datastore(s) online", status.pbs_datastores.online), "#14b8a6")}
            </div>

            <div style="margin-top:22px;font-size:13px;font-weight:700;color:#334155;">{"Resource utilization"}</div>
            <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(240px,1fr));gap:14px;margin-top:10px;">
                {usage_card("CPU", cpu, format!("{:.1} of {:.0} cores in use", status.pve_cpu_stats.used, status.pve_cpu_stats.max))}
                {usage_card("Memory", memory, format!("{} of {}", format_bytes(status.pve_memory_stats.used), format_bytes(status.pve_memory_stats.total)))}
                {usage_card("PVE storage", storage, format!("{} of {}", format_bytes(status.pve_storage_stats.used), format_bytes(status.pve_storage_stats.total)))}
                {usage_card("Backup storage", backup_storage, format!("{} of {}", format_bytes(status.pbs_storage_stats.used), format_bytes(status.pbs_storage_stats.total)))}
            </div>

            <div style="display:grid;grid-template-columns:minmax(0,1.35fr) minmax(280px,.65fr);gap:14px;margin-top:22px;">
                <div style="background:#fff;border:1px solid #e5e7eb;border-radius:14px;padding:18px;">
                    <div style="font-size:13px;font-weight:700;color:#0f172a;">{"Infrastructure overview"}</div>
                    <div style="display:grid;grid-template-columns:repeat(auto-fit,minmax(150px,1fr));gap:12px;margin-top:16px;">
                        <div style="padding:14px;border-radius:10px;background:#f8fafc;"><div style="font-size:11px;color:#64748b;">{"REMOTES"}</div><div style="font-size:22px;font-weight:700;margin-top:5px;">{status.remotes}</div></div>
                        <div style="padding:14px;border-radius:10px;background:#f8fafc;"><div style="font-size:11px;color:#64748b;">{"PVE NODES"}</div><div style="font-size:22px;font-weight:700;margin-top:5px;">{pve_nodes}</div></div>
                        <div style="padding:14px;border-radius:10px;background:#f8fafc;"><div style="font-size:11px;color:#64748b;">{"WORKLOADS"}</div><div style="font-size:22px;font-weight:700;margin-top:5px;">{vm_total + lxc_total}</div></div>
                        <div style="padding:14px;border-radius:10px;background:#f8fafc;"><div style="font-size:11px;color:#64748b;">{"DATASTORES"}</div><div style="font-size:22px;font-weight:700;margin-top:5px;">{status.pbs_datastores.online}</div></div>
                    </div>
                </div>
                <div style="background:#fff;border:1px solid #e5e7eb;border-radius:14px;padding:18px;">
                    <div style="font-size:13px;font-weight:700;color:#0f172a;">{"Attention"}</div>
                    <div style="margin-top:14px;font-size:13px;color:#475569;line-height:1.6;">
                        if warnings == 0 {
                            <div style="display:flex;gap:9px;align-items:center;"><span style="color:#10b981;font-size:18px;">{"●"}</span><span>{"No infrastructure issues detected."}</span></div>
                        } else {
                            <div style="display:flex;gap:9px;align-items:center;"><span style="color:#f59e0b;font-size:18px;">{"●"}</span><span>{format!("{warnings} remote or connectivity issue(s) require attention.")}</span></div>
                        }
                        <div style="margin-top:10px;display:flex;gap:9px;align-items:center;"><span style="color:#2563eb;font-size:18px;">{"●"}</span><span>{format!("{} VM(s) and {} container(s) currently stopped.", status.qemu.stopped, status.lxc.stopped)}</span></div>
                    </div>
                </div>
            </div>
        </>
    }
}

#[function_component(NexusHome)]
pub fn nexus_home() -> Html {
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
        <div style="width:100%;min-height:100%;background:#f8fafc;color:#0f172a;overflow:auto;">
            <div style="max-width:1600px;margin:0 auto;padding:24px 28px 32px;">
                <div style="display:flex;align-items:flex-start;justify-content:space-between;gap:18px;flex-wrap:wrap;">
                    <div>
                        <div style="font-size:12px;font-weight:700;letter-spacing:.08em;text-transform:uppercase;color:#64748b;">{"DreamtecLabs Nexus"}</div>
                        <h1 style="font-size:26px;line-height:1.15;margin:5px 0 0;font-weight:760;">{"Infrastructure Overview"}</h1>
                        <div style="font-size:13px;color:#64748b;margin-top:7px;">{"Unified operations across Proxmox VE and Proxmox Backup Server"}</div>
                    </div>
                    <div style="display:flex;gap:8px;align-items:center;">
                        <span style="font-size:12px;padding:7px 10px;border:1px solid #dbe2ea;border-radius:999px;background:#fff;color:#475569;">{"Live inventory"}</span>
                        <span style="font-size:12px;padding:7px 10px;border:1px solid #dbe2ea;border-radius:999px;background:#fff;color:#475569;">{"PDM engine"}</span>
                    </div>
                </div>

                <div style="margin-top:22px;">
                    {
                        match status.as_ref() {
                            None => html! { <div style="background:#fff;border:1px solid #e5e7eb;border-radius:14px;padding:24px;color:#64748b;">{"Loading infrastructure status…"}</div> },
                            Some(Ok(data)) => dashboard(data),
                            Some(Err(err)) => html! { <div style="background:#fff;border:1px solid #fecaca;border-radius:14px;padding:20px;color:#b91c1c;">{format!("Unable to load infrastructure status: {err}")}</div> },
                        }
                    }
                </div>
            </div>
        </div>
    }
}
