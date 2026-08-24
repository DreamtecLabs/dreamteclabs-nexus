use pdm_api_types::resource::ResourcesStatus;
use proxmox_yew_comp::http_get;
use wasm_bindgen_futures::spawn_local;
use yew::prelude::*;

fn percentage(used: f64, total: f64) -> f64 {
    if total <= 0.0 {
        0.0
    } else {
        (used / total * 100.0).clamp(0.0, 100.0)
    }
}

fn percentage_u64(used: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        (used as f64 / total as f64 * 100.0).clamp(0.0, 100.0)
    }
}

fn format_bytes(bytes: u64) -> String {
    let gib = bytes as f64 / 1024.0 / 1024.0 / 1024.0;
    if gib >= 1024.0 {
        format!("{:.1} TiB", gib / 1024.0)
    } else {
        format!("{gib:.1} GiB")
    }
}

fn kpi_card(
    icon: &str,
    title: &str,
    value: u64,
    subtitle: String,
    healthy: bool,
    footer: &str,
) -> Html {
    html! {
        <section class="nexus-kpi-card">
            <div class="nexus-kpi-top">
                <span class="nexus-icon"><i class={icon.to_string()}></i></span>
                <span>{title}</span>
            </div>
            <div class="nexus-kpi-value">{value}</div>
            <div class="nexus-kpi-subtitle">{subtitle}</div>
            <div class={classes!("nexus-kpi-footer", (!healthy).then_some("warning"))}>
                <i class={if healthy { "fa fa-check" } else { "fa fa-exclamation-triangle" }}></i>
                <span>{footer}</span>
            </div>
        </section>
    }
}

fn alert_card(attention: u64, failed_remotes: u64) -> Html {
    html! {
        <section class="nexus-kpi-card">
            <div class="nexus-kpi-top">
                <span class="nexus-icon alert"><i class="fa fa-shield"></i></span>
                <span>{"Active Alerts"}</span>
            </div>
            <div class="nexus-kpi-value">{attention}</div>
            <div class="nexus-kpi-subtitle alert-text">
                {format!("{} failed remotes · {} infrastructure issues", failed_remotes, attention.saturating_sub(failed_remotes))}
            </div>
            <div class={classes!("nexus-kpi-footer", (attention > 0).then_some("warning"))}>
                <i class={if attention == 0 { "fa fa-check" } else { "fa fa-exclamation-circle" }}></i>
                <span>{if attention == 0 { "No active infrastructure alerts" } else { "Review infrastructure health" }}</span>
            </div>
        </section>
    }
}

fn gauge(label: &str, value: f64, detail: String, color: &str) -> Html {
    let safe = value.clamp(0.0, 100.0);
    let style = format!("background:conic-gradient({color} {safe:.1}%, #edf1f6 {safe:.1}% 100%);");
    html! {
        <div class="nexus-gauge-item">
            <div class="nexus-gauge" style={style}>
                <div class="nexus-gauge-inner"><strong>{format!("{safe:.0}%")}</strong></div>
            </div>
            <div class="nexus-gauge-label">{label}</div>
            <div class="nexus-gauge-detail">{detail}</div>
        </div>
    }
}

fn health_row(label: &str, online: u64, issues: u64, detail: String) -> Html {
    html! {
        <div class="nexus-health-row">
            <div class="nexus-health-name">
                <span class="nexus-row-icon"><i class="fa fa-server"></i></span>
                <strong>{label}</strong>
            </div>
            <div><span class="nexus-status-dot"></span><span class="nexus-status-text">{format!("{} online", online)}</span></div>
            <div class={classes!("nexus-issue-count", (issues > 0).then_some("warning"))}>
                {if issues == 0 { "No issues".to_string() } else { format!("{} issues", issues) }}
            </div>
            <div class="nexus-muted">{detail}</div>
        </div>
    }
}

fn workload_panel(title: &str, icon: &str, total: u64, running: u64, stopped: u64) -> Html {
    let running_pct = if total == 0 {
        0.0
    } else {
        running as f64 / total as f64 * 100.0
    };
    let stopped_pct = if total == 0 {
        0.0
    } else {
        stopped as f64 / total as f64 * 100.0
    };

    html! {
        <section class="nexus-panel nexus-workload-panel">
            <div class="nexus-panel-header compact">
                <h2><i class={icon.to_string()}></i>{title}</h2>
                <span class="nexus-link">{"View all →"}</span>
            </div>
            <div class="nexus-workload-body">
                <div class="nexus-donut" style={format!("background:conic-gradient(#22c55e {running_pct:.1}%, #e5e7eb {running_pct:.1}% 100%);")}>
                    <div><strong>{total}</strong><span>{"Total"}</span></div>
                </div>
                <div class="nexus-legend">
                    <div><span class="legend-dot green"></span><span>{"Running"}</span><strong>{running}</strong><em>{format!("{running_pct:.0}%")}</em></div>
                    <div><span class="legend-dot gray"></span><span>{"Stopped"}</span><strong>{stopped}</strong><em>{format!("{stopped_pct:.0}%")}</em></div>
                </div>
            </div>
        </section>
    }
}

fn backup_panel(status: &ResourcesStatus) -> Html {
    let online = status.pbs_datastores.online;
    let warning = status.pbs_datastores.under_maintenance.unwrap_or_default()
        + status.pbs_datastores.high_usage.unwrap_or_default();
    let unknown = status.pbs_datastores.unknown.unwrap_or_default();
    let total = online + status.pbs_datastores.under_maintenance.unwrap_or_default() + unknown;
    let ok_pct = if total == 0 {
        0.0
    } else {
        online as f64 / total as f64 * 100.0
    };

    html! {
        <section class="nexus-panel nexus-workload-panel">
            <div class="nexus-panel-header compact">
                <h2><i class="fa fa-database"></i>{"Backup Status"}</h2>
                <span class="nexus-link">{"View all →"}</span>
            </div>
            <div class="nexus-workload-body">
                <div class="nexus-donut" style={format!("background:conic-gradient(#22c55e {ok_pct:.1}%, #e5e7eb {ok_pct:.1}% 100%);")}>
                    <div><strong>{total}</strong><span>{"Datastores"}</span></div>
                </div>
                <div class="nexus-legend">
                    <div><span class="legend-dot green"></span><span>{"Online"}</span><strong>{online}</strong><em>{format!("{ok_pct:.0}%")}</em></div>
                    <div><span class="legend-dot orange"></span><span>{"Warnings"}</span><strong>{warning}</strong><em>{""}</em></div>
                    <div><span class="legend-dot gray"></span><span>{"Unknown"}</span><strong>{unknown}</strong><em>{""}</em></div>
                </div>
            </div>
        </section>
    }
}

fn capacity_bar(label: &str, value: f64, color: &str) -> Html {
    let safe = value.clamp(0.0, 100.0);
    html! {
        <div class="nexus-capacity-row">
            <div class="nexus-capacity-meta"><strong>{label}</strong><span>{format!("{safe:.0}%")}</span></div>
            <div class="nexus-progress"><span style={format!("width:{safe:.1}%;background:{color};")}></span></div>
        </div>
    }
}

fn activity(icon: &str, title: &str, detail: String, healthy: bool) -> Html {
    html! {
        <div class="nexus-activity-row">
            <span class={classes!("nexus-activity-icon", (!healthy).then_some("warning"))}><i class={icon.to_string()}></i></span>
            <div><strong>{title}</strong><span>{detail}</span></div>
            <span class={classes!("nexus-activity-state", (!healthy).then_some("warning"))}>{if healthy { "OK" } else { "Attention" }}</span>
        </div>
    }
}

fn dashboard(status: &ResourcesStatus) -> Html {
    let pve_nodes = status.pve_nodes.online + status.pve_nodes.offline + status.pve_nodes.unknown;
    let qemu_total =
        status.qemu.running + status.qemu.stopped + status.qemu.template + status.qemu.unknown;
    let lxc_total =
        status.lxc.running + status.lxc.stopped + status.lxc.template + status.lxc.unknown;
    let pbs_nodes = status.pbs_nodes.online + status.pbs_nodes.offline + status.pbs_nodes.unknown;
    let datastores = status.pbs_datastores.online
        + status.pbs_datastores.under_maintenance.unwrap_or_default()
        + status.pbs_datastores.unknown.unwrap_or_default();
    let attention = status.failed_remotes
        + status.pve_nodes.offline
        + status.pve_nodes.unknown
        + status.pbs_nodes.offline
        + status.pbs_nodes.unknown
        + status.pbs_datastores.under_maintenance.unwrap_or_default()
        + status.pbs_datastores.high_usage.unwrap_or_default()
        + status.pbs_datastores.unknown.unwrap_or_default();

    let cpu_percent = percentage(status.pve_cpu_stats.used, status.pve_cpu_stats.max);
    let memory_percent =
        percentage_u64(status.pve_memory_stats.used, status.pve_memory_stats.total);
    let storage_used = status.pve_storage_stats.used + status.pbs_storage_stats.used;
    let storage_total = status.pve_storage_stats.total + status.pbs_storage_stats.total;
    let storage_percent = percentage_u64(storage_used, storage_total);

    html! {
        <>
            <div class="nexus-page-header">
                <div>
                    <div class="nexus-eyebrow">{"OVERVIEW"}</div>
                    <div class="nexus-title-row"><h1>{"Dashboard"}</h1><span class="nexus-sync-dot"></span><span class="nexus-muted">{"Live from PDM"}</span></div>
                    <p>{"Unified view of your Proxmox VE and Backup Server infrastructure."}</p>
                </div>
                <div class="nexus-live-pill"><span></span>{"All systems connected"}</div>
            </div>

            <div class="nexus-kpi-grid">
                {kpi_card("fa fa-server", "Virtual Environment Nodes", pve_nodes, format!("{} online", status.pve_nodes.online), status.pve_nodes.offline == 0, "All nodes healthy")}
                {kpi_card("fa fa-desktop", "Virtual Machines", qemu_total, format!("{} running · {} stopped", status.qemu.running, status.qemu.stopped), true, "Managed by PDM")}
                {kpi_card("fa fa-cube", "Linux Containers", lxc_total, format!("{} running · {} stopped", status.lxc.running, status.lxc.stopped), true, "Managed by PDM")}
                {kpi_card("fa fa-hdd-o", "Backup Server Nodes", pbs_nodes, format!("{} online", status.pbs_nodes.online), status.pbs_nodes.offline == 0, "Backup infrastructure")}
                {kpi_card("fa fa-database", "Backup Datastores", datastores, format!("{} online", status.pbs_datastores.online), status.pbs_datastores.under_maintenance.unwrap_or_default() == 0, "Protected storage")}
                {alert_card(attention, status.failed_remotes)}
            </div>

            <div class="nexus-two-column nexus-primary-row">
                <section class="nexus-panel">
                    <div class="nexus-panel-header"><div><h2>{"Resource Utilization"}</h2><span>{"Aggregated PVE/PBS capacity"}</span></div><span class="nexus-time-chip"><i class="fa fa-clock-o"></i>{" Live"}</span></div>
                    <div class="nexus-gauges">
                        {gauge("CPU", cpu_percent, format!("{:.1} of {:.0} cores", status.pve_cpu_stats.used, status.pve_cpu_stats.max), "#22c55e")}
                        {gauge("Memory", memory_percent, format!("{} of {}", format_bytes(status.pve_memory_stats.used), format_bytes(status.pve_memory_stats.total)), "#2563eb")}
                        {gauge("Storage", storage_percent, format!("{} of {}", format_bytes(storage_used), format_bytes(storage_total)), "#f97316")}
                    </div>
                </section>

                <section class="nexus-panel">
                    <div class="nexus-panel-header"><div><h2>{"Infrastructure Health"}</h2><span>{"Live availability across connected remotes"}</span></div><span class={classes!("nexus-health-badge", (attention > 0).then_some("warning"))}><span class="nexus-health-dot"></span>{if attention == 0 { "Healthy" } else { "Attention" }}</span></div>
                    <div class="nexus-health-table">
                        {health_row("PVE Nodes", status.pve_nodes.online, status.pve_nodes.offline + status.pve_nodes.unknown, format!("{} total", pve_nodes))}
                        {health_row("PBS Nodes", status.pbs_nodes.online, status.pbs_nodes.offline + status.pbs_nodes.unknown, format!("{} total", pbs_nodes))}
                        {health_row("Remotes", status.remotes.saturating_sub(status.failed_remotes), status.failed_remotes, format!("{} connected", status.remotes))}
                        {health_row("Datastores", status.pbs_datastores.online, status.pbs_datastores.under_maintenance.unwrap_or_default() + status.pbs_datastores.unknown.unwrap_or_default(), format!("{} total", datastores))}
                    </div>
                </section>
            </div>

            <div class="nexus-three-column">
                {workload_panel("Virtual Machines", "fa fa-desktop", qemu_total, status.qemu.running, status.qemu.stopped)}
                {workload_panel("Linux Containers", "fa fa-cube", lxc_total, status.lxc.running, status.lxc.stopped)}
                {backup_panel(status)}
            </div>

            <div class="nexus-two-column nexus-bottom-row">
                <section class="nexus-panel">
                    <div class="nexus-panel-header"><div><h2>{"Capacity Overview"}</h2><span>{"Current utilization against available capacity"}</span></div></div>
                    <div class="nexus-capacity-list">
                        {capacity_bar("CPU", cpu_percent, "#2563eb")}
                        {capacity_bar("Memory", memory_percent, "#22c55e")}
                        {capacity_bar("Storage", storage_percent, "#f97316")}
                    </div>
                </section>
                <section class="nexus-panel">
                    <div class="nexus-panel-header"><div><h2>{"Operational Status"}</h2><span>{"Current state reported by the PDM engine"}</span></div></div>
                    <div class="nexus-activity-list">
                        {activity("fa fa-check", "All configured remotes", format!("{} available · {} failed", status.remotes.saturating_sub(status.failed_remotes), status.failed_remotes), status.failed_remotes == 0)}
                        {activity("fa fa-server", "PVE node availability", format!("{} online · {} unavailable", status.pve_nodes.online, status.pve_nodes.offline + status.pve_nodes.unknown), status.pve_nodes.offline + status.pve_nodes.unknown == 0)}
                        {activity("fa fa-database", "Backup datastore health", format!("{} online · {} require attention", status.pbs_datastores.online, status.pbs_datastores.under_maintenance.unwrap_or_default() + status.pbs_datastores.high_usage.unwrap_or_default() + status.pbs_datastores.unknown.unwrap_or_default()), status.pbs_datastores.under_maintenance.unwrap_or_default() + status.pbs_datastores.high_usage.unwrap_or_default() + status.pbs_datastores.unknown.unwrap_or_default() == 0)}
                    </div>
                </section>
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
        <div class="nexus-home">
            <style>{NEXUS_CSS}</style>
            {
                match status.as_ref() {
                    None => html! { <div class="nexus-loading"><i class="fa fa-circle-o-notch fa-spin"></i><span>{"Loading infrastructure overview…"}</span></div> },
                    Some(Ok(data)) => dashboard(data),
                    Some(Err(err)) => html! { <div class="nexus-empty-state"><div class="nexus-empty-icon"><i class="fa fa-exclamation-triangle"></i></div><h3>{"Unable to load infrastructure status"}</h3><p>{err}</p></div> },
                }
            }
        </div>
    }
}

const NEXUS_CSS: &str = r#"
.nexus-navigation{background:#fff!important;color:#344054!important;border-right:1px solid #e4e7ec!important}.nexus-navigation a,.nexus-navigation button{color:#344054!important}.nexus-navigation a:hover,.nexus-navigation button:hover{background:#f2f4f7!important}.nexus-home{width:100%;height:100%;overflow:auto;background:#f7f9fc;color:#182230;font-family:Arial,Helvetica,sans-serif}.nexus-page-header{display:flex;justify-content:space-between;gap:24px;align-items:flex-start;padding:24px 28px 18px}.nexus-eyebrow{font-size:10px;font-weight:700;letter-spacing:.12em;color:#8a94a6}.nexus-title-row{display:flex;align-items:center;gap:9px;margin-top:4px}.nexus-title-row h1{font-size:24px;margin:0;font-weight:700;letter-spacing:-.02em}.nexus-page-header p{margin:5px 0 0;color:#697386;font-size:12px}.nexus-sync-dot{width:7px;height:7px;border-radius:50%;background:#22c55e}.nexus-muted{color:#8a94a6;font-size:11px}.nexus-live-pill{display:flex;align-items:center;gap:7px;background:#fff;border:1px solid #e4e7ec;border-radius:999px;padding:7px 11px;font-size:10px;color:#475467}.nexus-live-pill span{width:7px;height:7px;border-radius:50%;background:#22c55e}.nexus-kpi-grid{display:grid;grid-template-columns:repeat(6,minmax(145px,1fr));gap:10px;padding:0 28px 12px}.nexus-kpi-card,.nexus-panel{background:#fff;border:1px solid #e3e8ef;border-radius:10px;box-shadow:0 1px 2px rgba(16,24,40,.035)}.nexus-kpi-card{padding:13px 15px;min-height:132px}.nexus-kpi-top{display:flex;align-items:center;gap:8px;font-size:11px;font-weight:650;color:#344054}.nexus-icon{width:29px;height:29px;border-radius:7px;background:#eef3ff;color:#315bea;display:inline-flex;align-items:center;justify-content:center;font-size:14px}.nexus-icon.alert{background:#fff1eb;color:#f04438}.nexus-kpi-value{font-size:28px;font-weight:700;margin-top:11px;letter-spacing:-.03em}.nexus-kpi-subtitle{font-size:10px;color:#667085;margin-top:2px}.nexus-kpi-footer{font-size:10px;color:#16a34a;margin-top:11px;display:flex;gap:5px;align-items:center}.nexus-kpi-footer.warning,.nexus-issue-count.warning,.nexus-activity-state.warning{color:#f04438}.alert-text{color:#f04438}.nexus-two-column{display:grid;grid-template-columns:1.08fr .92fr;gap:12px;padding:0 28px 12px}.nexus-three-column{display:grid;grid-template-columns:repeat(3,1fr);gap:12px;padding:0 28px 12px}.nexus-primary-row .nexus-panel{min-height:245px}.nexus-panel{padding:16px}.nexus-panel-header{display:flex;justify-content:space-between;gap:18px;align-items:flex-start}.nexus-panel-header h2{font-size:13px;margin:0;color:#182230}.nexus-panel-header span{display:block;color:#98a2b3;font-size:10px;margin-top:3px}.nexus-panel-header.compact{align-items:center}.nexus-panel-header.compact h2{display:flex;align-items:center;gap:7px}.nexus-link{color:#315bea!important;font-weight:600;cursor:pointer}.nexus-time-chip{border:1px solid #e4e7ec;border-radius:999px;padding:5px 8px;color:#667085!important;margin:0!important}.nexus-gauges{display:grid;grid-template-columns:repeat(3,1fr);align-items:center;gap:24px;padding:24px 10px 4px}.nexus-gauge-item{text-align:center}.nexus-gauge{width:98px;height:98px;border-radius:50%;padding:10px;margin:0 auto;box-sizing:border-box}.nexus-gauge-inner{height:100%;width:100%;border-radius:50%;background:#fff;display:flex;align-items:center;justify-content:center}.nexus-gauge-inner strong{font-size:20px}.nexus-gauge-label{font-size:11px;font-weight:700;margin-top:9px}.nexus-gauge-detail{font-size:10px;color:#98a2b3;margin-top:2px}.nexus-health-badge{display:flex!important;align-items:center;gap:5px!important;color:#15803d!important;background:#effcf3;padding:5px 8px;border-radius:999px;margin:0!important}.nexus-health-badge.warning{background:#fff4ed;color:#c4320a!important}.nexus-health-dot,.nexus-status-dot{width:7px;height:7px;border-radius:50%;background:#22c55e;display:inline-block}.nexus-health-row{display:grid;grid-template-columns:1.25fr .85fr .75fr .75fr;gap:10px;align-items:center;padding:13px 0;border-bottom:1px solid #eef1f5;font-size:10px}.nexus-health-row:last-child{border-bottom:0}.nexus-health-name{display:flex;align-items:center;gap:8px}.nexus-row-icon{color:#667085;width:24px;height:24px;border:1px solid #e4e7ec;border-radius:6px;display:flex;align-items:center;justify-content:center}.nexus-status-text{margin-left:5px;color:#16803c}.nexus-issue-count{color:#667085}.nexus-workload-panel{min-height:170px}.nexus-workload-body{display:flex;align-items:center;gap:24px;padding:15px 3px 3px}.nexus-donut{width:92px;height:92px;border-radius:50%;padding:10px;box-sizing:border-box}.nexus-donut>div{height:100%;width:100%;border-radius:50%;background:#fff;display:flex;flex-direction:column;align-items:center;justify-content:center}.nexus-donut strong{font-size:19px}.nexus-donut span{font-size:9px;color:#98a2b3}.nexus-legend{flex:1;min-width:0}.nexus-legend>div{display:grid;grid-template-columns:10px 1fr 28px 38px;align-items:center;gap:7px;font-size:10px;margin:9px 0}.nexus-legend strong{text-align:right}.nexus-legend em{font-style:normal;color:#98a2b3;text-align:right}.legend-dot{width:7px;height:7px;border-radius:50%;display:inline-block}.legend-dot.green{background:#22c55e}.legend-dot.gray{background:#98a2b3}.legend-dot.orange{background:#f59e0b}.nexus-bottom-row{padding-bottom:28px}.nexus-capacity-list{padding-top:16px}.nexus-capacity-row{margin:12px 0}.nexus-capacity-meta{display:flex;justify-content:space-between;font-size:10px;margin-bottom:6px}.nexus-capacity-meta span{color:#667085}.nexus-progress{height:7px;background:#edf1f6;border-radius:999px;overflow:hidden}.nexus-progress span{display:block;height:100%;border-radius:999px}.nexus-activity-list{padding-top:8px}.nexus-activity-row{display:grid;grid-template-columns:30px 1fr auto;align-items:center;gap:9px;padding:10px 0;border-bottom:1px solid #eef1f5}.nexus-activity-row:last-child{border-bottom:0}.nexus-activity-icon{width:28px;height:28px;border-radius:50%;background:#eaf8ef;color:#16a34a;display:flex;align-items:center;justify-content:center}.nexus-activity-icon.warning{background:#fff4e8;color:#f79009}.nexus-activity-row strong{font-size:10px;display:block}.nexus-activity-row div span{font-size:9px;color:#98a2b3;display:block;margin-top:2px}.nexus-activity-state{font-size:9px;color:#16803c;font-weight:700}.nexus-loading,.nexus-empty-state{min-height:420px;display:flex;align-items:center;justify-content:center;flex-direction:column;gap:10px;color:#667085}.nexus-empty-state h3{margin:0;color:#182230}.nexus-empty-state p{max-width:520px;text-align:center}.nexus-empty-icon{font-size:32px;color:#f79009}@media(max-width:1250px){.nexus-kpi-grid{grid-template-columns:repeat(3,1fr)}.nexus-two-column{grid-template-columns:1fr}.nexus-three-column{grid-template-columns:1fr 1fr}}@media(max-width:850px){.nexus-kpi-grid,.nexus-three-column{grid-template-columns:1fr 1fr}.nexus-gauges{grid-template-columns:1fr}.nexus-health-row{grid-template-columns:1fr 1fr}.nexus-page-header,.nexus-kpi-grid,.nexus-two-column,.nexus-three-column{padding-left:16px;padding-right:16px}}@media(max-width:620px){.nexus-kpi-grid,.nexus-three-column{grid-template-columns:1fr}.nexus-page-header{flex-direction:column}.nexus-health-row{grid-template-columns:1fr}.nexus-workload-body{align-items:flex-start}}
"#;
