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

fn kpi(icon: &str, label: &str, value: String, detail: &str, tone: &str) -> Html {
    html! {
        <section class={classes!("nexus-monitor-kpi", tone.to_string())}>
            <span class="nexus-monitor-kpi-icon"><i class={icon.to_string()}></i></span>
            <div>
                <small>{label}</small>
                <strong>{value}</strong>
                <span>{detail}</span>
            </div>
        </section>
    }
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
                message.set(Some(Err("Name, IP/hostname and type are required.".to_string())));
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
                            Some(err) => Err(format!("Device saved, but probe reconciliation failed: {err}")),
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

    let run_reconcile = {
        let monitoring = monitoring.clone();
        let busy = busy.clone();
        let message = message.clone();
        Callback::from(move |_| {
            let monitoring = monitoring.clone();
            let busy = busy.clone();
            let message = message.clone();
            busy.set(Some("reconcile".to_string()));
            spawn_local(async move {
                let result: Result<Value, _> = http_post("/monitoring/reconcile", Some(json!({}))).await;
                match result {
                    Ok(_) => {
                        message.set(Some(Ok("ICMP collector reconciled successfully.".to_string())));
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
    let engine_active = monitoring
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|value| value.pointer("/probe_engine/service_active"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let collector_installed = monitoring
        .as_ref()
        .and_then(|result| result.as_ref().ok())
        .and_then(|value| value.pointer("/probe_engine/collector_installed"))
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
                    <p>{"Manage SigNoz observability and agentless network-device monitoring from Nexus."}</p>
                </div>
                <button class="nexus-monitor-secondary" onclick={run_reconcile} disabled={busy.is_some()}>
                    <i class="fa fa-refresh"></i>{" Reconcile probes"}
                </button>
            </header>

            <div class="nexus-monitor-kpis">
                {kpi("fa fa-heartbeat", "SigNoz API", if signoz_connected { "Connected".into() } else { "Attention".into() }, signoz_url, if signoz_connected { "green" } else { "orange" })}
                {kpi("fa fa-wifi", "Monitored devices", enabled.to_string(), "ICMP targets active", "blue")}
                {kpi("fa fa-wrench", "Maintenance", maintenance.to_string(), "Alerts intentionally suppressed", "orange")}
                {kpi("fa fa-pause-circle", "Disabled", disabled.to_string(), "Not currently probed", "slate")}
            </div>

            {if let Some(result) = message.as_ref() {
                match result {
                    Ok(text) => html! { <div class="nexus-monitor-message ok"><i class="fa fa-check-circle"></i>{text}</div> },
                    Err(text) => html! { <div class="nexus-monitor-message error"><i class="fa fa-exclamation-triangle"></i>{text}</div> },
                }
            } else { Html::default() }}

            <section class="nexus-monitor-grid">
                <div class="nexus-monitor-card nexus-monitor-signoz">
                    <div class="nexus-monitor-card-head">
                        <div>
                            <span class="nexus-monitor-section-icon"><i class="fa fa-heartbeat"></i></span>
                            <div><h2>{"SigNoz"}</h2><p>{"Metrics, alerts and incident visibility"}</p></div>
                        </div>
                        <span class={classes!("nexus-monitor-health", if signoz_connected { "ok" } else { "bad" })}>
                            <i class="fa fa-circle"></i>{if signoz_connected { "API connected" } else { "API unavailable" }}
                        </span>
                    </div>
                    <dl class="nexus-monitor-detail-list">
                        <div><dt>{"Endpoint"}</dt><dd>{signoz_url}</dd></div>
                        <div><dt>{"Alert rules"}</dt><dd>{rule_count.map(|v| v.to_string()).unwrap_or_else(|| "—".to_string())}</dd></div>
                        <div><dt>{"API auth"}</dt><dd>{if signoz_value.and_then(|v| v.get("configured")).and_then(Value::as_bool).unwrap_or(false) { "Service account configured" } else { "API key required" }}</dd></div>
                    </dl>
                    {if let Some(error) = signoz_value.and_then(|value| value.get("error")).and_then(Value::as_str) {
                        html! { <p class="nexus-monitor-inline-error">{error}</p> }
                    } else { Html::default() }}
                </div>

                <div class="nexus-monitor-card nexus-monitor-engine">
                    <div class="nexus-monitor-card-head">
                        <div>
                            <span class="nexus-monitor-section-icon"><i class="fa fa-bullseye"></i></span>
                            <div><h2>{"ICMP Probe Engine"}</h2><p>{"Dedicated OpenTelemetry collector"}</p></div>
                        </div>
                        <span class={classes!("nexus-monitor-health", if engine_active { "ok" } else { "idle" })}>
                            <i class="fa fa-circle"></i>{if engine_active { "Running" } else { "Stopped" }}
                        </span>
                    </div>
                    <dl class="nexus-monitor-detail-list">
                        <div><dt>{"Collector"}</dt><dd>{if collector_installed { "otelcol-contrib detected" } else { "Not installed on Nexus host" }}</dd></div>
                        <div><dt>{"Active targets"}</dt><dd>{enabled}</dd></div>
                        <div><dt>{"Telemetry"}</dt><dd>{"OTLP → SigNoz"}</dd></div>
                    </dl>
                </div>
            </section>

            <section class="nexus-monitor-card nexus-monitor-devices">
                <div class="nexus-monitor-card-head">
                    <div>
                        <span class="nexus-monitor-section-icon"><i class="fa fa-wifi"></i></span>
                        <div><h2>{"Network Devices"}</h2><p>{"Nexus is the source of truth; enabled devices are automatically rendered into the ICMP collector."}</p></div>
                    </div>
                    <span class="nexus-monitor-count">{format!("{} devices", devices.len())}</span>
                </div>

                <div class="nexus-monitor-add">
                    <label><span>{"Name"}</span><input value={(*name).clone()} placeholder="omada-switch-01" oninput={{ let name = name.clone(); Callback::from(move |event: InputEvent| name.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                    <label><span>{"IP / Hostname"}</span><input value={(*address).clone()} placeholder="192.168.0.10" oninput={{ let address = address.clone(); Callback::from(move |event: InputEvent| address.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                    <label><span>{"Type"}</span><input value={(*kind).clone()} placeholder="switch" oninput={{ let kind = kind.clone(); Callback::from(move |event: InputEvent| kind.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                    <label><span>{"Site"}</span><input value={(*site).clone()} placeholder="home" oninput={{ let site = site.clone(); Callback::from(move |event: InputEvent| site.set(event.target_unchecked_into::<HtmlInputElement>().value())) }}/></label>
                    <button class="nexus-monitor-primary" onclick={add_device} disabled={busy.is_some()}><i class="fa fa-plus"></i>{" Add device"}</button>
                </div>

                {match monitoring.as_ref() {
                    Some(Err(err)) => html! { <div class="nexus-monitor-empty error">{format!("Monitoring inventory unavailable: {err}")}</div> },
                    None => html! { <div class="nexus-monitor-empty">{"Loading monitoring inventory…"}</div> },
                    Some(Ok(_)) if devices.is_empty() => html! { <div class="nexus-monitor-empty"><i class="fa fa-wifi"></i><strong>{"No network devices yet"}</strong><span>{"Add the first device above. No terminal configuration is required."}</span></div> },
                    Some(Ok(_)) => html! {
                        <div class="nexus-monitor-table-wrap"><table class="nexus-monitor-table">
                            <thead><tr><th>{"Device"}</th><th>{"Address"}</th><th>{"Type"}</th><th>{"Site"}</th><th>{"Profile"}</th><th>{"State"}</th><th>{"Last probe"}</th><th>{"Actions"}</th></tr></thead>
                            <tbody>{for devices.iter().map(|device| {
                                let id = device.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                                let device_name = device.get("name").and_then(Value::as_str).unwrap_or("Unnamed").to_string();
                                let device_address = device.get("address").and_then(Value::as_str).unwrap_or("").to_string();
                                let device_kind = device.get("kind").and_then(Value::as_str).unwrap_or("device").to_string();
                                let device_site = device.get("site").and_then(Value::as_str).unwrap_or("home").to_string();
                                let device_state = device.get("state").and_then(Value::as_str).unwrap_or("disabled").to_string();
                                let probe = probes.get(&id).cloned();

                                let probe_click = {
                                    let id = id.clone(); let probes = probes.clone(); let busy = busy.clone(); let message = message.clone();
                                    Callback::from(move |_| { let id = id.clone(); let probes = probes.clone(); let busy = busy.clone(); let message = message.clone(); busy.set(Some(format!("probe:{id}"))); spawn_local(async move { let result: Result<Value,_> = http_post("/monitoring/device-probe", Some(json!({"id":id.clone()}))).await; match result { Ok(value) => { let mut next = (*probes).clone(); next.insert(id.clone(), value); probes.set(next); }, Err(err) => message.set(Some(Err(err.to_string()))) } busy.set(None); }); })
                                };
                                let state_click = {
                                    let name = device_name.clone(); let address = device_address.clone(); let kind = device_kind.clone(); let site = device_site.clone(); let current = device_state.clone(); let monitoring = monitoring.clone(); let busy = busy.clone(); let message = message.clone();
                                    Callback::from(move |_| { let name = name.clone(); let address = address.clone(); let kind = kind.clone(); let site = site.clone(); let next_state = if current == "enabled" { "maintenance" } else { "enabled" }; let monitoring = monitoring.clone(); let busy = busy.clone(); let message = message.clone(); busy.set(Some(format!("state:{name}"))); spawn_local(async move { let result: Result<Value,_> = http_post("/monitoring/device", Some(json!({"name":name,"address":address,"kind":kind,"site":site,"state":next_state}))).await; match result { Ok(_) => { message.set(Some(Ok("Monitoring state updated.".to_string()))); load_monitoring(monitoring); }, Err(err) => message.set(Some(Err(err.to_string()))) } busy.set(None); }); })
                                };
                                let delete_click = {
                                    let id = id.clone(); let monitoring = monitoring.clone(); let busy = busy.clone(); let message = message.clone();
                                    Callback::from(move |_| { let id = id.clone(); let monitoring = monitoring.clone(); let busy = busy.clone(); let message = message.clone(); busy.set(Some(format!("delete:{id}"))); spawn_local(async move { let result: Result<Value,_> = http_post("/monitoring/device-delete", Some(json!({"id":id}))).await; match result { Ok(_) => { message.set(Some(Ok("Device removed from monitoring.".to_string()))); load_monitoring(monitoring); }, Err(err) => message.set(Some(Err(err.to_string()))) } busy.set(None); }); })
                                };
                                html! { <tr>
                                    <td><strong>{device_name}</strong><small>{id.clone()}</small></td>
                                    <td><code>{device_address}</code></td>
                                    <td>{device_kind}</td><td>{device_site}</td><td><span class="nexus-monitor-profile">{"ICMP"}</span></td>
                                    <td>{state_badge(&device_state)}</td>
                                    <td>{if let Some(probe) = probe { if probe.get("reachable").and_then(Value::as_bool).unwrap_or(false) { html! { <span class="nexus-probe-result ok"><i class="fa fa-check-circle"></i>{probe.get("latency_ms").and_then(Value::as_f64).map(|v| format!("{v:.1} ms")).unwrap_or_else(|| "reachable".to_string())}</span> } } else { html! { <span class="nexus-probe-result bad"><i class="fa fa-times-circle"></i>{"unreachable"}</span> } } } else { html! { <span class="nexus-probe-result idle">{"Not tested"}</span> } }}</td>
                                    <td><div class="nexus-monitor-actions"><button title="Probe now" onclick={probe_click}><i class="fa fa-bolt"></i></button><button title={if device_state == "enabled" { "Maintenance" } else { "Enable monitoring" }} onclick={state_click}><i class={if device_state == "enabled" { "fa fa-wrench" } else { "fa fa-play" }}></i></button><button class="danger" title="Delete" onclick={delete_click}><i class="fa fa-trash"></i></button></div></td>
                                </tr> }
                            })}</tbody>
                        </table></div>
                    },
                }}
            </section>
        </div>
    }
}

const MONITORING_CSS: &str = r#"
.nexus-monitoring{--nx-text:#0f172a;--nx-muted:#64748b;--nx-border:#dce4ef;--nx-blue:#2563eb;--nx-green:#16a34a;min-height:100%;overflow:auto;background:linear-gradient(180deg,#f8faff 0,#f6f8fc 220px);color:var(--nx-text);padding:24px 30px 32px;font-family:"Roboto Flex",Roboto,Arial,sans-serif}.nexus-monitoring *{box-sizing:border-box}.nexus-monitor-header{display:flex;justify-content:space-between;align-items:flex-end;gap:18px;margin-bottom:18px}.nexus-monitor-eyebrow{font-size:9px;letter-spacing:.12em;color:#2563eb;font-weight:800}.nexus-monitor-header h1{font-size:25px;line-height:1;margin:6px 0 7px;font-weight:800}.nexus-monitor-header p{margin:0;color:var(--nx-muted);font-size:10px}.nexus-monitor-primary,.nexus-monitor-secondary{min-height:34px;border-radius:8px;border:1px solid #cbd7e6;padding:0 12px;font-size:10px;font-weight:700;cursor:pointer}.nexus-monitor-primary{background:#2563eb;border-color:#2563eb;color:#fff}.nexus-monitor-secondary{background:#fff;color:#334155}.nexus-monitor-primary:disabled,.nexus-monitor-secondary:disabled{opacity:.55;cursor:wait}.nexus-monitor-kpis{display:grid;grid-template-columns:repeat(4,minmax(160px,1fr));gap:10px;margin-bottom:12px}.nexus-monitor-kpi{background:#fff;border:1px solid var(--nx-border);border-radius:11px;padding:12px 13px;display:flex;gap:11px;align-items:center;min-height:80px;position:relative;overflow:hidden}.nexus-monitor-kpi:after{content:"";position:absolute;bottom:0;left:0;right:0;height:2px;background:#2563eb}.nexus-monitor-kpi.green:after{background:#22c55e}.nexus-monitor-kpi.orange:after{background:#f59e0b}.nexus-monitor-kpi.slate:after{background:#94a3b8}.nexus-monitor-kpi-icon,.nexus-monitor-section-icon{width:34px;height:34px;border-radius:9px;display:inline-flex;align-items:center;justify-content:center;background:#eef4ff;color:#2563eb;flex:none}.nexus-monitor-kpi>div{display:flex;min-width:0;flex-direction:column}.nexus-monitor-kpi small{color:#475569;font-size:9px;font-weight:700}.nexus-monitor-kpi strong{font-size:18px;line-height:1.2;margin:2px 0;font-weight:800;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.nexus-monitor-kpi span:last-child{font-size:8px;color:#94a3b8;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}.nexus-monitor-message{padding:9px 11px;border-radius:8px;border:1px solid;margin-bottom:12px;font-size:10px;display:flex;gap:7px;align-items:center}.nexus-monitor-message.ok{background:#f0fdf4;border-color:#bbf7d0;color:#166534}.nexus-monitor-message.error{background:#fff7ed;border-color:#fed7aa;color:#9a3412}.nexus-monitor-grid{display:grid;grid-template-columns:1.2fr .8fr;gap:12px;margin-bottom:12px}.nexus-monitor-card{background:#fff;border:1px solid var(--nx-border);border-radius:12px;box-shadow:0 3px 13px rgba(15,23,42,.045)}.nexus-monitor-card-head{padding:14px 15px;display:flex;justify-content:space-between;gap:12px;align-items:center;border-bottom:1px solid #edf1f6}.nexus-monitor-card-head>div{display:flex;align-items:center;gap:10px;min-width:0}.nexus-monitor-card-head h2{font-size:12px;margin:0;font-weight:780}.nexus-monitor-card-head p{font-size:8.5px;color:#64748b;margin:3px 0 0}.nexus-monitor-health{font-size:8.5px;border-radius:999px;padding:5px 8px;background:#f1f5f9;color:#64748b;white-space:nowrap}.nexus-monitor-health i{font-size:6px;margin-right:5px}.nexus-monitor-health.ok{background:#ecfdf3;color:#15803d}.nexus-monitor-health.bad{background:#fff7ed;color:#c2410c}.nexus-monitor-detail-list{margin:0;padding:12px 15px;display:grid;gap:9px}.nexus-monitor-detail-list>div{display:grid;grid-template-columns:100px 1fr;gap:10px}.nexus-monitor-detail-list dt{font-size:9px;color:#64748b}.nexus-monitor-detail-list dd{margin:0;font-size:9px;font-weight:600;overflow:hidden;text-overflow:ellipsis}.nexus-monitor-inline-error{margin:0 15px 12px;color:#b45309;font-size:8.5px}.nexus-monitor-count{font-size:9px;color:#64748b}.nexus-monitor-add{padding:11px 15px;background:#fbfcff;border-bottom:1px solid #edf1f6;display:grid;grid-template-columns:1.1fr 1.1fr .8fr .7fr auto;gap:8px;align-items:end}.nexus-monitor-add label{display:flex;flex-direction:column;gap:4px;font-size:8px;color:#64748b;font-weight:700}.nexus-monitor-add input{height:32px;border:1px solid #d7e0eb;border-radius:7px;background:#fff;color:#0f172a;padding:0 9px;font-size:10px;outline:none}.nexus-monitor-add input:focus{border-color:#8db4ff;box-shadow:0 0 0 3px rgba(37,99,235,.08)}.nexus-monitor-table-wrap{overflow-x:auto}.nexus-monitor-table{width:100%;border-collapse:collapse;font-size:9.5px}.nexus-monitor-table th{text-align:left;height:36px;background:#f8fafc;color:#64748b;font-size:8.5px;font-weight:750;border-bottom:1px solid #e2e8f0;padding:0 10px;white-space:nowrap}.nexus-monitor-table td{height:45px;border-bottom:1px solid #eef2f7;padding:6px 10px;white-space:nowrap;color:#334155}.nexus-monitor-table tbody tr:hover td{background:#f8fbff}.nexus-monitor-table td:first-child strong{display:block;color:#111827;font-size:9.5px}.nexus-monitor-table td:first-child small{display:block;color:#94a3b8;font-size:7.5px;margin-top:2px}.nexus-monitor-table code{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:8.5px;color:#334155}.nexus-monitor-profile{background:#eef4ff;color:#1d4ed8;border-radius:999px;padding:4px 7px;font-weight:700;font-size:8px}.nexus-monitor-state,.nexus-probe-result{display:inline-flex;gap:5px;align-items:center;font-size:8.5px}.nexus-monitor-state i{font-size:6px}.nexus-monitor-state.enabled,.nexus-probe-result.ok{color:#15803d}.nexus-monitor-state.maintenance{color:#b45309}.nexus-monitor-state.disabled,.nexus-probe-result.idle{color:#94a3b8}.nexus-probe-result.bad{color:#b91c1c}.nexus-monitor-actions{display:flex;gap:5px;justify-content:flex-end}.nexus-monitor-actions button{width:27px;height:27px;border:1px solid #dce4ef;border-radius:7px;background:#fff;color:#475569;cursor:pointer}.nexus-monitor-actions button:hover{background:#f5f8ff;border-color:#bfd0e9}.nexus-monitor-actions button.danger{color:#b91c1c}.nexus-monitor-empty{min-height:150px;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:7px;color:#94a3b8;font-size:9px}.nexus-monitor-empty>i{font-size:24px;color:#cbd5e1}.nexus-monitor-empty strong{font-size:11px;color:#475569}.nexus-monitor-empty.error{color:#b45309}@media(max-width:1050px){.nexus-monitor-kpis{grid-template-columns:repeat(2,1fr)}.nexus-monitor-grid{grid-template-columns:1fr}.nexus-monitor-add{grid-template-columns:1fr 1fr}}@media(max-width:650px){.nexus-monitoring{padding:16px}.nexus-monitor-header{align-items:flex-start;flex-direction:column}.nexus-monitor-kpis{grid-template-columns:1fr}.nexus-monitor-add{grid-template-columns:1fr}.nexus-monitor-secondary{width:100%}}
"#;
