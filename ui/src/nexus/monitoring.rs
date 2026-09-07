use std::collections::BTreeMap;

use proxmox_yew_comp::{http_get, http_post};
use serde_json::{Value, json};
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;

fn load_monitoring(state: UseStateHandle<Option<Result<Value, String>>>) {
    spawn_local(async move {
        let result: Result<Value, _> = http_get("/monitoring", None).await;
        state.set(Some(result.map_err(|err| err.to_string())));
    });
}

fn state_badge(state: &str) -> Html {
    let (class, icon, label) = match state {
        "enabled" => ("enabled", "fa fa-circle", "Monitored"),
        "maintenance" => ("maintenance", "fa fa-wrench", "Maintenance"),
        _ => ("disabled", "fa fa-pause-circle", "Disabled"),
    };
    html! {
        <span class={classes!("nexus-monitor-state", class)}>
            <i class={icon}></i>{label}
        </span>
    }
}

fn update_device_state(
    monitoring: UseStateHandle<Option<Result<Value, String>>>,
    busy: UseStateHandle<Option<String>>,
    message: UseStateHandle<Option<Result<String, String>>>,
    device: Value,
    state: &'static str,
) -> Callback<MouseEvent> {
    Callback::from(move |_| {
        let monitoring = monitoring.clone();
        let busy = busy.clone();
        let message = message.clone();
        let name = device
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let address = device
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let kind = device
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("device")
            .to_string();
        let site = device
            .get("site")
            .and_then(Value::as_str)
            .unwrap_or("home")
            .to_string();
        busy.set(Some(format!("state:{name}")));
        spawn_local(async move {
            let result: Result<Value, _> = http_post(
                "/monitoring/device",
                Some(json!({
                    "name": name,
                    "address": address,
                    "kind": kind,
                    "site": site,
                    "state": state
                })),
            )
            .await;
            match result {
                Ok(value) => {
                    let downtime_error = value
                        .pointer("/signoz_maintenance/downtime_error")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    message.set(Some(match downtime_error {
                        Some(err) => Err(format!(
                            "Device monitoring set to {state}, but SigNoz maintenance sync failed: {err}"
                        )),
                        None => Ok(format!("Device monitoring set to {state}.")),
                    }));
                    load_monitoring(monitoring);
                }
                Err(err) => message.set(Some(Err(err.to_string()))),
            }
            busy.set(None);
        });
    })
}

#[function_component(NexusMonitoring)]
pub fn nexus_monitoring() -> Html {
    let monitoring = use_state(|| None::<Result<Value, String>>);
    let signoz = use_state(|| None::<Result<Value, String>>);
    let busy = use_state(|| None::<String>);
    let message = use_state(|| None::<Result<String, String>>);
    let probes = use_state(BTreeMap::<String, Value>::new);
    let show_add = use_state(|| false);
    let search = use_state(String::new);
    let type_filter = use_state(String::new);
    let site_filter = use_state(String::new);
    let state_filter = use_state(String::new);
    let name = use_state(String::new);
    let address = use_state(String::new);
    let kind = use_state(|| "device".to_string());
    let site = use_state(|| "home".to_string());

    {
        let monitoring = monitoring.clone();
        let signoz = signoz.clone();
        use_effect_with((), move |_| {
            load_monitoring(monitoring);
            spawn_local(async move {
                let result: Result<Value, _> = http_get("/monitoring/signoz", None).await;
                signoz.set(Some(result.map_err(|err| err.to_string())));
            });
            || ()
        });
    }

    let add_device = {
        let monitoring = monitoring.clone();
        let busy = busy.clone();
        let message = message.clone();
        let name_state = name.clone();
        let address_state = address.clone();
        let kind_state = kind.clone();
        let site_state = site.clone();
        let show_add = show_add.clone();
        Callback::from(move |_| {
            let name_value = (*name_state).trim().to_string();
            let address_value = (*address_state).trim().to_string();
            let kind_value = (*kind_state).trim().to_string();
            let site_value = (*site_state).trim().to_string();
            if name_value.is_empty() || address_value.is_empty() || kind_value.is_empty() {
                message.set(Some(Err(
                    "Name, IP/hostname and type are required.".to_string()
                )));
                return;
            }
            let monitoring = monitoring.clone();
            let busy = busy.clone();
            let message = message.clone();
            let name_state = name_state.clone();
            let address_state = address_state.clone();
            let show_add = show_add.clone();
            busy.set(Some("add".to_string()));
            spawn_local(async move {
                let result: Result<Value, _> = http_post(
                    "/monitoring/device",
                    Some(json!({
                        "name": name_value,
                        "address": address_value,
                        "kind": kind_value,
                        "site": site_value,
                        "state": "enabled"
                    })),
                )
                .await;
                match result {
                    Ok(value) => {
                        let reconcile_error = value
                            .pointer("/reconcile/error")
                            .and_then(Value::as_str)
                            .map(str::to_string);
                        message.set(Some(match reconcile_error {
                            Some(err) => Err(format!(
                                "Device saved, but probe reconciliation failed: {err}"
                            )),
                            None => Ok("Device saved and monitoring reconciled.".to_string()),
                        }));
                        name_state.set(String::new());
                        address_state.set(String::new());
                        show_add.set(false);
                        load_monitoring(monitoring);
                    }
                    Err(err) => message.set(Some(Err(err.to_string()))),
                }
                busy.set(None);
            });
        })
    };

    let reconcile = {
        let monitoring = monitoring.clone();
        let busy = busy.clone();
        let message = message.clone();
        Callback::from(move |_| {
            let monitoring = monitoring.clone();
            let busy = busy.clone();
            let message = message.clone();
            busy.set(Some("reconcile".to_string()));
            spawn_local(async move {
                let result: Result<Value, _> =
                    http_post("/monitoring/reconcile", Some(json!({}))).await;
                match result {
                    Ok(_) => {
                        message.set(Some(Ok(
                            "ICMP collector reconciled successfully.".to_string()
                        )));
                        load_monitoring(monitoring);
                    }
                    Err(err) => message.set(Some(Err(err.to_string()))),
                }
                busy.set(None);
            });
        })
    };

    let inventory = monitoring
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|value| value.get("inventory"));
    let devices = inventory
        .and_then(|value| value.get("devices"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let enabled = devices
        .iter()
        .filter(|device| device.get("state").and_then(Value::as_str) == Some("enabled"))
        .count();
    let maintenance = devices
        .iter()
        .filter(|device| device.get("state").and_then(Value::as_str) == Some("maintenance"))
        .count();
    let disabled = devices.len().saturating_sub(enabled + maintenance);
    let down = probes
        .values()
        .filter(|probe| {
            !probe
                .get("reachable")
                .and_then(Value::as_bool)
                .unwrap_or(false)
        })
        .count();

    let query = search.trim().to_lowercase();
    let type_query = type_filter.trim().to_lowercase();
    let site_query = site_filter.trim().to_lowercase();
    let state_query = state_filter.trim().to_lowercase();
    let filtered_devices: Vec<Value> = devices
        .iter()
        .filter(|device| {
            let id = device
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase();
            let name = device
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase();
            let address = device
                .get("address")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_lowercase();
            let kind = device
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("device")
                .to_lowercase();
            let site = device
                .get("site")
                .and_then(Value::as_str)
                .unwrap_or("home")
                .to_lowercase();
            let state = device
                .get("state")
                .and_then(Value::as_str)
                .unwrap_or("disabled")
                .to_lowercase();
            (query.is_empty()
                || id.contains(&query)
                || name.contains(&query)
                || address.contains(&query))
                && (type_query.is_empty() || kind.contains(&type_query))
                && (site_query.is_empty() || site.contains(&site_query))
                && (state_query.is_empty() || state.contains(&state_query))
        })
        .cloned()
        .collect();

    let engine = monitoring
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|value| value.get("probe_engine"));
    let engine_active = engine
        .and_then(|value| value.get("service_active"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let collector_installed = engine
        .and_then(|value| value.get("collector_installed"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let signoz_value = signoz.as_ref().and_then(|result| result.as_ref().ok());
    let signoz_connected = signoz_value
        .and_then(|value| value.get("connected"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let signoz_url = signoz_value
        .and_then(|value| value.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("http://192.168.0.47:8080");
    let rule_count = signoz_value
        .and_then(|value| value.get("rule_count"))
        .and_then(Value::as_u64);

    html! {
        <div class="nexus-monitoring">
            <style>{MONITORING_CSS}</style>
            <header class="nexus-monitor-header">
                <div class="nexus-monitor-title">
                    <h1>{"Monitoring"}</h1>
                    <p>{"Agentless health, maintenance and observability from Nexus."}</p>
                </div>
                <div class="nexus-monitor-header-actions">
                    <span class={classes!("nexus-collector-status", if engine_active { "ok" } else { "bad" })}>
                        <i class="fa fa-circle"></i>{format!("Collector: {} · {} devices", if engine_active { "Running" } else { "Stopped" }, enabled)}
                    </span>
                    <button class="nexus-monitor-secondary" onclick={reconcile} disabled={busy.is_some()}>
                        <i class="fa fa-refresh"></i>{" Reconcile"}
                    </button>
                    <button class="nexus-monitor-primary" onclick={{ let show_add = show_add.clone(); Callback::from(move |_| show_add.set(!*show_add)) }} disabled={busy.is_some()}>
                        <i class="fa fa-plus"></i>{" Add device"}
                    </button>
                </div>
            </header>

            <div class="nexus-monitor-kpis">
                <section><small>{"Monitored"}</small><strong>{enabled}</strong><span>{"ICMP targets active"}</span></section>
                <section class={if down > 0 { "danger" } else { "" }}><small>{"Down"}</small><strong>{down}</strong><span>{"Latest manual probes"}</span></section>
                <section><small>{"Maintenance"}</small><strong>{maintenance}</strong><span>{"Intentionally suppressed"}</span></section>
                <section><small>{"Disabled"}</small><strong>{disabled}</strong><span>{"Retained in inventory"}</span></section>
                <section class={if signoz_connected { "success" } else { "warning" }}><small>{"SigNoz"}</small><strong>{if signoz_connected { "Connected" } else { "Attention" }}</strong><span>{signoz_url}</span></section>
            </div>

            {if let Some(result) = message.as_ref() {
                match result {
                    Ok(text) => html! { <div class="nexus-monitor-message ok"><i class="fa fa-check-circle"></i>{text}</div> },
                    Err(text) => html! { <div class="nexus-monitor-message error"><i class="fa fa-exclamation-triangle"></i>{text}</div> },
                }
            } else {
                Html::default()
            }}

            <section class="nexus-monitor-card nexus-device-card">
                <div class="nexus-monitor-card-head">
                    <div><h2>{"Network Devices"}</h2><p>{"Manage ICMP monitoring, maintenance and probe state."}</p></div>
                    <span>{format!("{} of {} devices", filtered_devices.len(), devices.len())}</span>
                </div>

                {if *show_add {
                    html! {
                        <div class="nexus-monitor-add">
                            <label><span>{"Name"}</span><input value={(*name).clone()} placeholder="omada-switch-01" oninput={{ let name = name.clone(); Callback::from(move |event: InputEvent| name.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                            <label><span>{"IP / Hostname"}</span><input value={(*address).clone()} placeholder="192.168.0.10" oninput={{ let address = address.clone(); Callback::from(move |event: InputEvent| address.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                            <label><span>{"Type"}</span><input value={(*kind).clone()} placeholder="switch" oninput={{ let kind = kind.clone(); Callback::from(move |event: InputEvent| kind.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                            <label><span>{"Site"}</span><input value={(*site).clone()} placeholder="home" oninput={{ let site = site.clone(); Callback::from(move |event: InputEvent| site.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                            <button class="nexus-monitor-primary" onclick={add_device} disabled={busy.is_some()}>{"Save device"}</button>
                        </div>
                    }
                } else {
                    Html::default()
                }}

                <div class="nexus-monitor-toolbar">
                    <label class="search"><i class="fa fa-search"></i><input value={(*search).clone()} placeholder="Search name, ID or address" oninput={{ let search = search.clone(); Callback::from(move |event: InputEvent| search.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                    <input value={(*type_filter).clone()} placeholder="Type" oninput={{ let type_filter = type_filter.clone(); Callback::from(move |event: InputEvent| type_filter.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/>
                    <input value={(*site_filter).clone()} placeholder="Site" oninput={{ let site_filter = site_filter.clone(); Callback::from(move |event: InputEvent| site_filter.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/>
                    <input value={(*state_filter).clone()} placeholder="State" oninput={{ let state_filter = state_filter.clone(); Callback::from(move |event: InputEvent| state_filter.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/>
                </div>

                {match monitoring.as_ref() {
                    None => html! { <div class="nexus-monitor-empty">{"Loading monitoring inventory…"}</div> },
                    Some(Err(err)) => html! { <div class="nexus-monitor-empty error">{format!("Monitoring inventory unavailable: {err}")}</div> },
                    Some(Ok(_)) if devices.is_empty() => html! { <div class="nexus-monitor-empty">{"No devices yet. Use Add device to create the first target."}</div> },
                    Some(Ok(_)) if filtered_devices.is_empty() => html! { <div class="nexus-monitor-empty">{"No devices match the current filters."}</div> },
                    Some(Ok(_)) => html! {
                        <div class="nexus-monitor-table-wrap">
                            <table class="nexus-monitor-table">
                                <thead><tr><th>{"Name"}</th><th>{"IP / Hostname"}</th><th>{"Type"}</th><th>{"Site"}</th><th>{"State"}</th><th>{"Last probe"}</th><th>{"Latency"}</th><th>{"Actions"}</th></tr></thead>
                                <tbody>{for filtered_devices.iter().map(|device| {
                                    let id = device.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                                    let probe = probes.get(&id).cloned();
                                    let probe_click = {
                                        let id = id.clone();
                                        let probes = probes.clone();
                                        let busy = busy.clone();
                                        let message = message.clone();
                                        Callback::from(move |_| {
                                            let id = id.clone();
                                            let probes = probes.clone();
                                            let busy = busy.clone();
                                            let message = message.clone();
                                            busy.set(Some(format!("probe:{id}")));
                                            spawn_local(async move {
                                                let result: Result<Value, _> = http_post(
                                                    "/monitoring/device-probe",
                                                    Some(json!({ "id": id.clone() })),
                                                )
                                                .await;
                                                match result {
                                                    Ok(value) => {
                                                        let mut next = (*probes).clone();
                                                        next.insert(id, value);
                                                        probes.set(next);
                                                    }
                                                    Err(err) => message.set(Some(Err(err.to_string()))),
                                                }
                                                busy.set(None);
                                            });
                                        })
                                    };
                                    let delete_click = {
                                        let id = id.clone();
                                        let monitoring = monitoring.clone();
                                        let busy = busy.clone();
                                        let message = message.clone();
                                        Callback::from(move |_| {
                                            let id = id.clone();
                                            let monitoring = monitoring.clone();
                                            let busy = busy.clone();
                                            let message = message.clone();
                                            busy.set(Some(format!("delete:{id}")));
                                            spawn_local(async move {
                                                let result: Result<Value, _> = http_post(
                                                    "/monitoring/device-delete",
                                                    Some(json!({ "id": id })),
                                                )
                                                .await;
                                                match result {
                                                    Ok(_) => {
                                                        message.set(Some(Ok("Device removed from monitoring.".to_string())));
                                                        load_monitoring(monitoring);
                                                    }
                                                    Err(err) => message.set(Some(Err(err.to_string()))),
                                                }
                                                busy.set(None);
                                            });
                                        })
                                    };
                                    let enable_click = update_device_state(monitoring.clone(), busy.clone(), message.clone(), device.clone(), "enabled");
                                    let maintenance_click = update_device_state(monitoring.clone(), busy.clone(), message.clone(), device.clone(), "maintenance");
                                    let disable_click = update_device_state(monitoring.clone(), busy.clone(), message.clone(), device.clone(), "disabled");
                                    let name = device.get("name").and_then(Value::as_str).unwrap_or("Unnamed");
                                    let address = device.get("address").and_then(Value::as_str).unwrap_or("");
                                    let kind = device.get("kind").and_then(Value::as_str).unwrap_or("device");
                                    let site = device.get("site").and_then(Value::as_str).unwrap_or("home");
                                    let state = device.get("state").and_then(Value::as_str).unwrap_or("disabled");
                                    let reachable = probe.as_ref().and_then(|p| p.get("reachable")).and_then(Value::as_bool);
                                    let latency = probe.as_ref().and_then(|p| p.get("latency_ms")).and_then(Value::as_f64);
                                    html! {
                                        <tr>
                                            <td><div class="nexus-device-name"><span class={classes!("nexus-health-dot", match reachable { Some(true) => "ok", Some(false) => "bad", None => "idle" })}></span><div><strong>{name}</strong><small>{id}</small></div></div></td>
                                            <td><code>{address}</code></td>
                                            <td><span class="nexus-chip">{kind}</span></td>
                                            <td><span class="nexus-chip">{site}</span></td>
                                            <td>{state_badge(state)}</td>
                                            <td>{match reachable {
                                                Some(true) => html! { <span class="nexus-probe ok">{"Reachable now"}</span> },
                                                Some(false) => html! { <span class="nexus-probe bad">{"Unreachable now"}</span> },
                                                None => html! { <span class="nexus-probe idle">{"Not tested"}</span> },
                                            }}</td>
                                            <td>{latency.map(|value| format!("{value:.1} ms")).unwrap_or_else(|| "—".to_string())}</td>
                                            <td><div class="nexus-monitor-actions">
                                                <button title="Probe now" onclick={probe_click}><i class="fa fa-bolt"></i></button>
                                                <button title="Enable" onclick={enable_click}><i class="fa fa-play"></i></button>
                                                <button title="Maintenance" onclick={maintenance_click}><i class="fa fa-wrench"></i></button>
                                                <button title="Disable" onclick={disable_click}><i class="fa fa-pause"></i></button>
                                                <button class="danger" title="Delete" onclick={delete_click}><i class="fa fa-trash"></i></button>
                                            </div></td>
                                        </tr>
                                    }
                                })}</tbody>
                            </table>
                        </div>
                    },
                }}
                <div class="nexus-monitor-table-footer"><span>{format!("{} items", filtered_devices.len())}</span><span>{"Rows per page: 25"}</span></div>
            </section>

            <section class="nexus-monitor-overview">
                <div class="nexus-monitor-card">
                    <h2>{"SigNoz"}</h2>
                    <dl>
                        <div><dt>{"API"}</dt><dd>{if signoz_connected { "Connected" } else { "Unavailable" }}</dd></div>
                        <div><dt>{"Alert rules"}</dt><dd>{rule_count.map(|value| value.to_string()).unwrap_or_else(|| "—".to_string())}</dd></div>
                        <div><dt>{"Credentials"}</dt><dd>{if signoz_value.and_then(|value| value.get("configured")).and_then(Value::as_bool).unwrap_or(false) { "Configured server-side" } else { "API key required" }}</dd></div>
                    </dl>
                    {if let Some(error) = signoz_value.and_then(|value| value.get("error")).and_then(Value::as_str) {
                        html! { <p class="nexus-monitor-inline-error">{error}</p> }
                    } else { Html::default() }}
                </div>
                <div class="nexus-monitor-card">
                    <h2>{"ICMP Probe Engine"}</h2>
                    <dl>
                        <div><dt>{"Collector"}</dt><dd>{if collector_installed { "otelcol-contrib detected" } else { "Not installed" }}</dd></div>
                        <div><dt>{"Service"}</dt><dd>{if engine_active { "Running" } else { "Stopped" }}</dd></div>
                        <div><dt>{"Telemetry"}</dt><dd>{"OTLP → SigNoz"}</dd></div>
                    </dl>
                </div>
            </section>
        </div>
    }
}

const MONITORING_CSS: &str = r#"
.nexus-monitoring{min-height:100%;overflow:auto;background:#f7f9fc;color:#172033;padding:20px 24px 28px;font-family:"Roboto Flex",Roboto,Arial,sans-serif;font-size:14px}.nexus-monitoring *{box-sizing:border-box}.nexus-monitor-header{display:flex;justify-content:space-between;align-items:center;gap:18px;margin-bottom:16px}.nexus-monitor-title h1{font-size:30px;line-height:1.1;margin:0 0 4px}.nexus-monitor-title p,.nexus-monitor-card-head p{margin:0;color:#69758a;font-size:14px}.nexus-monitor-header-actions{display:flex;align-items:center;gap:10px}.nexus-collector-status{font-size:13px;color:#64748b;white-space:nowrap}.nexus-collector-status i{font-size:8px;margin-right:6px}.nexus-collector-status.ok i{color:#16a34a}.nexus-collector-status.bad i{color:#dc2626}.nexus-monitor-primary,.nexus-monitor-secondary,.nexus-monitor-actions button{border:1px solid #cfdae8;border-radius:8px;background:#fff;color:#334155;cursor:pointer}.nexus-monitor-primary{background:#2563eb;border-color:#2563eb;color:#fff;height:40px;padding:0 14px;font-weight:700;font-size:13px}.nexus-monitor-secondary{height:40px;padding:0 14px;font-weight:700;font-size:13px}.nexus-monitor-kpis{display:grid;grid-template-columns:repeat(5,minmax(140px,1fr));gap:12px;margin-bottom:14px}.nexus-monitor-kpis section,.nexus-monitor-card{background:#fff;border:1px solid #dce4ef;border-radius:12px}.nexus-monitor-kpis section{padding:14px 16px;display:flex;flex-direction:column;min-height:108px}.nexus-monitor-kpis section.warning{background:#fffaf2}.nexus-monitor-kpis section.success{background:#f4fbf7}.nexus-monitor-kpis section.danger{background:#fff5f5}.nexus-monitor-kpis small{font-size:13px;color:#64748b}.nexus-monitor-kpis strong{font-size:28px;line-height:1.1;margin:5px 0}.nexus-monitor-kpis span{font-size:12px;color:#7b8798}.nexus-monitor-message{padding:10px 12px;border-radius:8px;border:1px solid;margin-bottom:12px;font-size:13px}.nexus-monitor-message i{margin-right:6px}.nexus-monitor-message.ok{background:#f0fdf4;border-color:#bbf7d0;color:#166534}.nexus-monitor-message.error,.nexus-monitor-inline-error{color:#9a3412}.nexus-monitor-message.error{background:#fff7ed;border-color:#fed7aa}.nexus-device-card{margin-bottom:14px}.nexus-monitor-card h2{font-size:18px;margin:0}.nexus-monitor-card>h2{padding:16px 18px;border-bottom:1px solid #edf1f6}.nexus-monitor-card-head{padding:16px 18px;display:flex;justify-content:space-between;gap:12px;align-items:center;border-bottom:1px solid #edf1f6}.nexus-monitor-card-head span{font-size:12px;color:#64748b}.nexus-monitor-add{padding:12px 16px;background:#fbfcff;border-bottom:1px solid #edf1f6;display:grid;grid-template-columns:1.15fr 1.15fr .75fr .75fr auto;gap:10px;align-items:end}.nexus-monitor-add label{display:flex;flex-direction:column;gap:5px;font-size:12px;color:#64748b;font-weight:700}.nexus-monitor-add input,.nexus-monitor-toolbar input{height:38px;border:1px solid #d7e0eb;border-radius:8px;padding:0 11px;font-size:13px;background:#fff}.nexus-monitor-toolbar{padding:12px 16px;display:grid;grid-template-columns:minmax(260px,1fr) 150px 150px 150px;gap:10px;border-bottom:1px solid #edf1f6;background:#fff}.nexus-monitor-toolbar .search{position:relative}.nexus-monitor-toolbar .search i{position:absolute;left:12px;top:12px;color:#94a3b8}.nexus-monitor-toolbar .search input{width:100%;padding-left:32px}.nexus-monitor-table-wrap{overflow-x:auto}.nexus-monitor-table{width:100%;border-collapse:collapse;font-size:13px}.nexus-monitor-table th{text-align:left;height:40px;background:#f8fafc;color:#64748b;font-size:12px;padding:0 12px}.nexus-monitor-table td{height:58px;border-top:1px solid #eef2f7;padding:8px 12px;white-space:nowrap}.nexus-device-name{display:flex;align-items:center;gap:9px}.nexus-device-name strong,.nexus-device-name small{display:block}.nexus-device-name small{font-size:11px;color:#94a3b8;margin-top:2px}.nexus-health-dot{width:9px;height:9px;border-radius:50%;background:#cbd5e1;flex:0 0 auto}.nexus-health-dot.ok{background:#16a34a}.nexus-health-dot.bad{background:#dc2626}.nexus-monitor-table code{font-size:12px}.nexus-chip{display:inline-block;padding:3px 8px;background:#f1f5f9;border-radius:999px;color:#475569;font-size:12px}.nexus-monitor-state,.nexus-probe{font-size:12px}.nexus-monitor-state i{font-size:7px;margin-right:6px}.nexus-monitor-state.enabled,.nexus-probe.ok{color:#15803d}.nexus-monitor-state.maintenance{color:#b45309}.nexus-monitor-state.disabled,.nexus-probe.idle{color:#94a3b8}.nexus-probe.bad{color:#b91c1c}.nexus-monitor-actions{display:flex;gap:5px}.nexus-monitor-actions button{width:32px;height:32px}.nexus-monitor-actions button.danger{color:#b91c1c}.nexus-monitor-table-footer{height:42px;padding:0 16px;display:flex;align-items:center;justify-content:space-between;border-top:1px solid #eef2f7;color:#64748b;font-size:12px}.nexus-monitor-empty{padding:50px 16px;text-align:center;color:#94a3b8;font-size:13px}.nexus-monitor-empty.error{color:#b45309}.nexus-monitor-overview{display:grid;grid-template-columns:1fr 1fr;gap:14px}.nexus-monitor-card dl{margin:0;padding:14px 18px}.nexus-monitor-card dl div{display:grid;grid-template-columns:120px 1fr;gap:10px;margin:8px 0;font-size:13px}.nexus-monitor-card dt{color:#64748b}.nexus-monitor-card dd{margin:0;font-weight:600}.nexus-monitor-inline-error{padding:0 18px 14px;font-size:12px}@media(max-width:1180px){.nexus-monitor-kpis{grid-template-columns:repeat(3,1fr)}.nexus-monitor-toolbar{grid-template-columns:1fr 1fr}.nexus-monitor-add{grid-template-columns:1fr 1fr}.nexus-monitor-overview{grid-template-columns:1fr}}@media(max-width:760px){.nexus-monitoring{padding:16px}.nexus-monitor-header{align-items:flex-start;flex-direction:column}.nexus-monitor-header-actions{width:100%;flex-wrap:wrap}.nexus-monitor-kpis{grid-template-columns:1fr 1fr}.nexus-monitor-toolbar,.nexus-monitor-add{grid-template-columns:1fr}}@media(max-width:520px){.nexus-monitor-kpis{grid-template-columns:1fr}}
"#;
