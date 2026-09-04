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
                Ok(_) => {
                    message.set(Some(Ok(format!("Device monitoring set to {state}."))));
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
        Callback::from(move |_| {
            let name_value = (*name_state).trim().to_string();
            let address_value = (*address_state).trim().to_string();
            let kind_value = (*kind_state).trim().to_string();
            let site_value = (*site_state).trim().to_string();
            if name_value.is_empty() || address_value.is_empty() || kind_value.is_empty() {
                message.set(Some(Err(
                    "Name, IP/hostname and type are required.".to_string(),
                )));
                return;
            }
            let monitoring = monitoring.clone();
            let busy = busy.clone();
            let message = message.clone();
            let name_state = name_state.clone();
            let address_state = address_state.clone();
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
                            "ICMP collector reconciled successfully.".to_string(),
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
                <div>
                    <span class="nexus-monitor-eyebrow">{"OBSERVABILITY CONTROL PLANE"}</span>
                    <h1>{"Monitoring"}</h1>
                    <p>{"Manage SigNoz observability and agentless devices from Nexus."}</p>
                </div>
                <button class="nexus-monitor-secondary" onclick={reconcile} disabled={busy.is_some()}>
                    <i class="fa fa-refresh"></i>{" Reconcile probes"}
                </button>
            </header>

            <div class="nexus-monitor-kpis">
                <section><small>{"SigNoz API"}</small><strong>{if signoz_connected { "Connected" } else { "Attention" }}</strong><span>{signoz_url}</span></section>
                <section><small>{"Monitored"}</small><strong>{enabled}</strong><span>{"ICMP targets active"}</span></section>
                <section><small>{"Maintenance"}</small><strong>{maintenance}</strong><span>{"Intentionally not probed"}</span></section>
                <section><small>{"Disabled"}</small><strong>{disabled}</strong><span>{"Retained in inventory"}</span></section>
            </div>

            {if let Some(result) = message.as_ref() {
                match result {
                    Ok(text) => html! { <div class="nexus-monitor-message ok"><i class="fa fa-check-circle"></i>{text}</div> },
                    Err(text) => html! { <div class="nexus-monitor-message error"><i class="fa fa-exclamation-triangle"></i>{text}</div> },
                }
            } else {
                Html::default()
            }}

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
                    } else {
                        Html::default()
                    }}
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

            <section class="nexus-monitor-card">
                <div class="nexus-monitor-card-head">
                    <div><h2>{"Network Devices"}</h2><p>{"Add and manage ICMP monitoring without editing collector configuration."}</p></div>
                    <span>{format!("{} devices", devices.len())}</span>
                </div>
                <div class="nexus-monitor-add">
                    <label><span>{"Name"}</span><input value={(*name).clone()} placeholder="omada-switch-01" oninput={{ let name = name.clone(); Callback::from(move |event: InputEvent| name.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                    <label><span>{"IP / Hostname"}</span><input value={(*address).clone()} placeholder="192.168.0.10" oninput={{ let address = address.clone(); Callback::from(move |event: InputEvent| address.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                    <label><span>{"Type"}</span><input value={(*kind).clone()} placeholder="switch" oninput={{ let kind = kind.clone(); Callback::from(move |event: InputEvent| kind.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                    <label><span>{"Site"}</span><input value={(*site).clone()} placeholder="home" oninput={{ let site = site.clone(); Callback::from(move |event: InputEvent| site.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                    <button class="nexus-monitor-primary" onclick={add_device} disabled={busy.is_some()}><i class="fa fa-plus"></i>{" Add device"}</button>
                </div>

                {match monitoring.as_ref() {
                    None => html! { <div class="nexus-monitor-empty">{"Loading monitoring inventory…"}</div> },
                    Some(Err(err)) => html! { <div class="nexus-monitor-empty error">{format!("Monitoring inventory unavailable: {err}")}</div> },
                    Some(Ok(_)) if devices.is_empty() => html! { <div class="nexus-monitor-empty">{"No devices yet. Add the first device above."}</div> },
                    Some(Ok(_)) => html! {
                        <div class="nexus-monitor-table-wrap">
                            <table class="nexus-monitor-table">
                                <thead><tr><th>{"Device"}</th><th>{"Address"}</th><th>{"Type"}</th><th>{"Site"}</th><th>{"State"}</th><th>{"Last probe"}</th><th>{"Actions"}</th></tr></thead>
                                <tbody>{for devices.iter().map(|device| {
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
                                    html! {
                                        <tr>
                                            <td><strong>{name}</strong><small>{id}</small></td>
                                            <td><code>{address}</code></td>
                                            <td>{kind}</td>
                                            <td>{site}</td>
                                            <td>{state_badge(state)}</td>
                                            <td>{if let Some(probe) = probe {
                                                if probe.get("reachable").and_then(Value::as_bool).unwrap_or(false) {
                                                    html! { <span class="nexus-probe ok">{probe.get("latency_ms").and_then(Value::as_f64).map(|value| format!("{value:.1} ms")).unwrap_or_else(|| "reachable".to_string())}</span> }
                                                } else {
                                                    html! { <span class="nexus-probe bad">{"unreachable"}</span> }
                                                }
                                            } else {
                                                html! { <span class="nexus-probe idle">{"Not tested"}</span> }
                                            }}</td>
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
            </section>
        </div>
    }
}

const MONITORING_CSS: &str = r#"
.nexus-monitoring{min-height:100%;overflow:auto;background:#f7f9fc;color:#172033;padding:24px 30px 32px;font-family:"Roboto Flex",Roboto,Arial,sans-serif}.nexus-monitoring *{box-sizing:border-box}.nexus-monitor-header{display:flex;justify-content:space-between;align-items:flex-end;gap:16px;margin-bottom:18px}.nexus-monitor-eyebrow{font-size:9px;letter-spacing:.12em;color:#2563eb;font-weight:800}.nexus-monitor-header h1{font-size:25px;margin:5px 0}.nexus-monitor-header p,.nexus-monitor-card-head p{margin:0;color:#69758a;font-size:10px}.nexus-monitor-primary,.nexus-monitor-secondary,.nexus-monitor-actions button{border:1px solid #cfdae8;border-radius:7px;background:#fff;color:#334155;cursor:pointer}.nexus-monitor-primary{background:#2563eb;border-color:#2563eb;color:#fff;height:34px;padding:0 12px;font-weight:700}.nexus-monitor-secondary{height:34px;padding:0 12px;font-weight:700}.nexus-monitor-kpis{display:grid;grid-template-columns:repeat(4,minmax(150px,1fr));gap:10px;margin-bottom:12px}.nexus-monitor-kpis section,.nexus-monitor-card{background:#fff;border:1px solid #dce4ef;border-radius:11px}.nexus-monitor-kpis section{padding:12px;display:flex;flex-direction:column}.nexus-monitor-kpis small{font-size:9px;color:#64748b}.nexus-monitor-kpis strong{font-size:18px;margin:3px 0}.nexus-monitor-kpis span{font-size:8px;color:#94a3b8}.nexus-monitor-message{padding:9px 11px;border-radius:8px;border:1px solid;margin-bottom:12px;font-size:10px}.nexus-monitor-message i{margin-right:6px}.nexus-monitor-message.ok{background:#f0fdf4;border-color:#bbf7d0;color:#166534}.nexus-monitor-message.error,.nexus-monitor-inline-error{color:#9a3412}.nexus-monitor-message.error{background:#fff7ed;border-color:#fed7aa}.nexus-monitor-overview{display:grid;grid-template-columns:1fr 1fr;gap:12px;margin-bottom:12px}.nexus-monitor-card h2{font-size:12px;margin:0}.nexus-monitor-card>h2{padding:14px 15px;border-bottom:1px solid #edf1f6}.nexus-monitor-card dl{margin:0;padding:12px 15px}.nexus-monitor-card dl div{display:grid;grid-template-columns:100px 1fr;gap:10px;margin:7px 0;font-size:9px}.nexus-monitor-card dt{color:#64748b}.nexus-monitor-card dd{margin:0;font-weight:600}.nexus-monitor-inline-error{padding:0 15px 12px;font-size:9px}.nexus-monitor-card-head{padding:14px 15px;display:flex;justify-content:space-between;gap:12px;align-items:center;border-bottom:1px solid #edf1f6}.nexus-monitor-card-head span{font-size:9px;color:#64748b}.nexus-monitor-add{padding:11px 15px;background:#fbfcff;border-bottom:1px solid #edf1f6;display:grid;grid-template-columns:1.1fr 1.1fr .8fr .7fr auto;gap:8px;align-items:end}.nexus-monitor-add label{display:flex;flex-direction:column;gap:4px;font-size:8px;color:#64748b;font-weight:700}.nexus-monitor-add input{height:32px;border:1px solid #d7e0eb;border-radius:7px;padding:0 9px;font-size:10px}.nexus-monitor-table-wrap{overflow-x:auto}.nexus-monitor-table{width:100%;border-collapse:collapse;font-size:9.5px}.nexus-monitor-table th{text-align:left;height:36px;background:#f8fafc;color:#64748b;font-size:8.5px;padding:0 10px}.nexus-monitor-table td{height:46px;border-top:1px solid #eef2f7;padding:6px 10px;white-space:nowrap}.nexus-monitor-table td:first-child strong,.nexus-monitor-table td:first-child small{display:block}.nexus-monitor-table td:first-child small{font-size:7.5px;color:#94a3b8}.nexus-monitor-table code{font-size:8.5px}.nexus-monitor-state,.nexus-probe{font-size:8.5px}.nexus-monitor-state i{font-size:6px;margin-right:5px}.nexus-monitor-state.enabled,.nexus-probe.ok{color:#15803d}.nexus-monitor-state.maintenance{color:#b45309}.nexus-monitor-state.disabled,.nexus-probe.idle{color:#94a3b8}.nexus-probe.bad{color:#b91c1c}.nexus-monitor-actions{display:flex;gap:4px}.nexus-monitor-actions button{width:28px;height:28px}.nexus-monitor-actions button.danger{color:#b91c1c}.nexus-monitor-empty{padding:55px 15px;text-align:center;color:#94a3b8;font-size:10px}.nexus-monitor-empty.error{color:#b45309}@media(max-width:950px){.nexus-monitor-kpis{grid-template-columns:1fr 1fr}.nexus-monitor-overview{grid-template-columns:1fr}.nexus-monitor-add{grid-template-columns:1fr 1fr}}@media(max-width:620px){.nexus-monitoring{padding:16px}.nexus-monitor-header{align-items:flex-start;flex-direction:column}.nexus-monitor-kpis,.nexus-monitor-add{grid-template-columns:1fr}}
"#;
