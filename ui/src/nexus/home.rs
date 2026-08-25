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

fn live_signal(alert: bool) -> Html {
    html! {
        <span class={classes!("nexus-live-signal", alert.then_some("alert"))} aria-hidden="true">
            <i></i><i></i><i></i><i></i><i></i>
        </span>
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
                <span class="nexus-kpi-title">{title}</span>
            </div>
            <div class="nexus-kpi-value-row">
                <div>
                    <div class="nexus-kpi-value">{value}</div>
                    <div class="nexus-kpi-subtitle">{subtitle}</div>
                </div>
                {live_signal(false)}
            </div>
            <div class={classes!("nexus-kpi-footer", (!healthy).then_some("warning"))}>
                <i class={if healthy { "fa fa-check" } else { "fa fa-exclamation-triangle" }}></i>
                <span>{footer}</span>
            </div>
        </section>
    }
}

fn alert_card(attention: u64, failed_remotes: u64) -> Html {
    html! {
        <section class="nexus-kpi-card nexus-alert-card">
            <div class="nexus-kpi-top">
                <span class="nexus-icon alert"><i class="fa fa-shield"></i></span>
                <span class="nexus-kpi-title">{"Active Alerts"}</span>
            </div>
            <div class="nexus-kpi-value-row">
                <div>
                    <div class="nexus-kpi-value">{attention}</div>
                    <div class="nexus-kpi-subtitle alert-text">
                        {format!("{} failed remotes · {} infrastructure issues", failed_remotes, attention.saturating_sub(failed_remotes))}
                    </div>
                </div>
                {live_signal(true)}
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
    let style = format!("background:conic-gradient({color} {safe:.1}%, #e9eef5 {safe:.1}% 100%);");
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
                        {capacity_bar("CPU", cpu_percent, "#22c55e")}
                        {capacity_bar("Memory", memory_percent, "#2563eb")}
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
.nexus-home{--nx-text:#0b1220;--nx-muted:#475569;--nx-soft:#64748b;--nx-blue:#2563eb;--nx-green:#16a34a;--nx-orange:#f97316;width:100%;height:100%;overflow:auto;background:#f6f8fc;color:var(--nx-text);font-family:"Roboto Flex",Roboto,Arial,Helvetica,sans-serif;font-weight:430}.nexus-home *{box-sizing:border-box}.nexus-navigation{background:#fff!important;color:#111827!important;border-right:1px solid #dfe5ee!important}.nexus-navigation a,.nexus-navigation button{color:#111827!important}.nexus-navigation a:hover,.nexus-navigation button:hover{background:#f3f6fb!important}.nexus-page-header{display:flex;justify-content:space-between;gap:24px;align-items:flex-start;padding:25px 30px 19px}.nexus-eyebrow{font-size:10px;font-weight:800;letter-spacing:.13em;color:#2563eb}.nexus-title-row{display:flex;align-items:center;gap:9px;margin-top:5px}.nexus-title-row h1{font-size:25px;margin:0;font-weight:750;letter-spacing:-.025em;color:#070d18}.nexus-page-header p{margin:6px 0 0;color:#475569;font-size:12px;font-weight:450}.nexus-sync-dot{width:7px;height:7px;border-radius:50%;background:#22c55e;box-shadow:0 0 0 3px #dcfce7}.nexus-muted{color:#64748b;font-size:11px}.nexus-live-pill{display:flex;align-items:center;gap:7px;background:#fff;border:1px solid #dbe3ee;border-radius:999px;padding:7px 12px;font-size:10px;font-weight:600;color:#172033;box-shadow:0 1px 2px rgba(15,23,42,.04)}.nexus-live-pill span{width:7px;height:7px;border-radius:50%;background:#22c55e;box-shadow:0 0 0 3px #dcfce7}.nexus-kpi-grid{display:grid;grid-template-columns:repeat(6,minmax(145px,1fr));gap:11px;padding:0 30px 13px}.nexus-kpi-card,.nexus-panel{background:#fff;border:1px solid #dce4ef;border-radius:11px;box-shadow:0 2px 7px rgba(15,23,42,.055)}.nexus-kpi-card{padding:14px 15px;min-height:137px;position:relative;overflow:hidden;transition:transform .15s ease,box-shadow .15s ease,border-color .15s ease}.nexus-kpi-card:hover{transform:translateY(-1px);border-color:#cbd7e8;box-shadow:0 5px 14px rgba(15,23,42,.08)}.nexus-kpi-card:after{content:"";position:absolute;left:0;right:0;bottom:0;height:2px;background:linear-gradient(90deg,#2563eb33,#2563eb,#60a5fa33);opacity:.72}.nexus-alert-card:after{background:linear-gradient(90deg,#ef444433,#ef4444,#fb718533)}.nexus-kpi-top{display:flex;align-items:center;gap:8px;font-size:11px;font-weight:700;color:#111827}.nexus-kpi-title{line-height:1.15}.nexus-icon{width:30px;height:30px;border-radius:8px;background:#edf3ff;color:#2563eb;display:inline-flex;align-items:center;justify-content:center;font-size:14px;box-shadow:inset 0 0 0 1px #dce8ff}.nexus-icon.alert{background:#fff0ed;color:#dc2626;box-shadow:inset 0 0 0 1px #ffe0da}.nexus-kpi-value-row{display:flex;align-items:flex-end;justify-content:space-between;gap:10px;margin-top:10px}.nexus-kpi-value{font-size:29px;font-weight:780;letter-spacing:-.035em;line-height:1;color:#050a13}.nexus-kpi-subtitle{font-size:10px;color:#475569;margin-top:5px;font-weight:480}.nexus-kpi-footer{font-size:10px;font-weight:560;color:#159447;margin-top:10px;display:flex;gap:5px;align-items:center}.nexus-kpi-footer.warning,.nexus-issue-count.warning,.nexus-activity-state.warning{color:#dc2626}.alert-text{color:#dc2626}.nexus-live-signal{height:26px;width:56px;display:flex;align-items:flex-end;justify-content:flex-end;gap:3px;opacity:.9}.nexus-live-signal i{display:block;width:4px;border-radius:4px 4px 1px 1px;background:#4f7fff}.nexus-live-signal i:nth-child(1){height:7px}.nexus-live-signal i:nth-child(2){height:11px}.nexus-live-signal i:nth-child(3){height:9px}.nexus-live-signal i:nth-child(4){height:17px}.nexus-live-signal i:nth-child(5){height:14px}.nexus-live-signal.alert i{background:#ef4444}.nexus-two-column{display:grid;grid-template-columns:1.08fr .92fr;gap:13px;padding:0 30px 13px}.nexus-three-column{display:grid;grid-template-columns:repeat(3,1fr);gap:13px;padding:0 30px 13px}.nexus-primary-row .nexus-panel{min-height:250px}.nexus-panel{padding:17px}.nexus-panel-header{display:flex;justify-content:space-between;gap:18px;align-items:flex-start}.nexus-panel-header h2{font-size:13.5px;margin:0;color:#080d16;font-weight:750}.nexus-panel-header span{display:block;color:#526071;font-size:10px;margin-top:4px;font-weight:450}.nexus-panel-header.compact{align-items:center}.nexus-panel-header.compact h2{display:flex;align-items:center;gap:7px}.nexus-link{color:#2563eb!important;font-weight:700!important;cursor:pointer}.nexus-time-chip{border:1px solid #dbe3ee;border-radius:999px;padding:5px 8px;color:#334155!important;margin:0!important;background:#fbfcfe;font-weight:600!important}.nexus-gauges{display:grid;grid-template-columns:repeat(3,1fr);align-items:center;gap:24px;padding:25px 10px 5px}.nexus-gauge-item{text-align:center}.nexus-gauge{width:101px;height:101px;border-radius:50%;padding:10px;margin:0 auto;box-sizing:border-box;box-shadow:0 2px 8px rgba(15,23,42,.05)}.nexus-gauge-inner{height:100%;width:100%;border-radius:50%;background:#fff;display:flex;align-items:center;justify-content:center;box-shadow:inset 0 0 0 1px #edf1f6}.nexus-gauge-inner strong{font-size:21px;color:#070d18;font-weight:760}.nexus-gauge-label{font-size:11px;font-weight:750;margin-top:10px;color:#111827}.nexus-gauge-detail{font-size:10px;color:#526071;margin-top:3px;font-weight:450}.nexus-health-badge{display:flex!important;align-items:center;gap:5px!important;color:#15803d!important;background:#effcf3;padding:5px 9px;border-radius:999px;margin:0!important;font-weight:700!important}.nexus-health-badge.warning{background:#fff4ed;color:#c4320a!important}.nexus-health-dot,.nexus-status-dot{width:7px;height:7px;border-radius:50%;background:#22c55e;display:inline-block}.nexus-health-row{display:grid;grid-template-columns:1.25fr .85fr .75fr .75fr;gap:10px;align-items:center;padding:13px 0;border-bottom:1px solid #e8edf4;font-size:10px;color:#172033}.nexus-health-row:last-child{border-bottom:0}.nexus-health-name{display:flex;align-items:center;gap:8px}.nexus-health-name strong{font-weight:700;color:#0b1220}.nexus-row-icon{color:#334155;width:25px;height:25px;border:1px solid #dbe3ee;border-radius:6px;display:flex;align-items:center;justify-content:center;background:#fbfcfe}.nexus-status-text{margin-left:5px;color:#15803d;font-weight:650}.nexus-issue-count{color:#475569}.nexus-workload-panel{min-height:173px}.nexus-workload-body{display:flex;align-items:center;gap:24px;padding:16px 3px 3px}.nexus-donut{width:94px;height:94px;border-radius:50%;padding:10px;box-sizing:border-box;box-shadow:0 2px 8px rgba(15,23,42,.045)}.nexus-donut>div{height:100%;width:100%;border-radius:50%;background:#fff;display:flex;flex-direction:column;align-items:center;justify-content:center;box-shadow:inset 0 0 0 1px #edf1f6}.nexus-donut strong{font-size:20px;color:#070d18;font-weight:760}.nexus-donut span{font-size:9px;color:#526071}.nexus-legend{flex:1;min-width:0}.nexus-legend>div{display:grid;grid-template-columns:10px 1fr 28px 38px;align-items:center;gap:7px;font-size:10px;margin:9px 0;color:#172033}.nexus-legend strong{text-align:right;color:#0b1220}.nexus-legend em{font-style:normal;color:#526071;text-align:right}.legend-dot{width:7px;height:7px;border-radius:50%;display:inline-block}.legend-dot.green{background:#22c55e}.legend-dot.gray{background:#94a3b8}.legend-dot.orange{background:#f59e0b}.nexus-bottom-row{padding-bottom:30px}.nexus-capacity-list{padding-top:16px}.nexus-capacity-row{margin:13px 0}.nexus-capacity-meta{display:flex;justify-content:space-between;font-size:10px;margin-bottom:6px}.nexus-capacity-meta strong{color:#0b1220}.nexus-capacity-meta span{color:#334155;font-weight:650}.nexus-progress{height:8px;background:#e9eef5;border-radius:999px;overflow:hidden;box-shadow:inset 0 1px 2px rgba(15,23,42,.04)}.nexus-progress span{display:block;height:100%;border-radius:999px}.nexus-activity-list{padding-top:8px}.nexus-activity-row{display:grid;grid-template-columns:30px 1fr auto;align-items:center;gap:9px;padding:10px 0;border-bottom:1px solid #e8edf4}.nexus-activity-row:last-child{border-bottom:0}.nexus-activity-icon{width:28px;height:28px;border-radius:7px;background:#eaf8ef;color:#16a34a;display:flex;align-items:center;justify-content:center}.nexus-activity-icon.warning{background:#fff4e8;color:#f79009}.nexus-activity-row strong{font-size:10px;display:block;color:#0b1220;font-weight:700}.nexus-activity-row div span{font-size:9px;color:#526071;display:block;margin-top:2px}.nexus-activity-state{font-size:9px;color:#16803c;font-weight:750}.nexus-loading,.nexus-empty-state{min-height:420px;display:flex;align-items:center;justify-content:center;flex-direction:column;gap:10px;color:#475569}.nexus-empty-state h3{margin:0;color:#0b1220}.nexus-empty-state p{max-width:520px;text-align:center}.nexus-empty-icon{font-size:32px;color:#f79009}@media(max-width:1250px){.nexus-kpi-grid{grid-template-columns:repeat(3,1fr)}.nexus-two-column{grid-template-columns:1fr}.nexus-three-column{grid-template-columns:1fr 1fr}}@media(max-width:850px){.nexus-kpi-grid,.nexus-three-column{grid-template-columns:1fr 1fr}.nexus-gauges{grid-template-columns:1fr}.nexus-health-row{grid-template-columns:1fr 1fr}.nexus-page-header,.nexus-kpi-grid,.nexus-two-column,.nexus-three-column{padding-left:16px;padding-right:16px}}@media(max-width:620px){.nexus-kpi-grid,.nexus-three-column{grid-template-columns:1fr}.nexus-page-header{flex-direction:column}.nexus-health-row{grid-template-columns:1fr}.nexus-workload-body{align-items:flex-start}}
"#;