use std::collections::BTreeMap;

use proxmox_yew_comp::{http_get, http_post};
use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;

fn bool_value(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn status_badge(ok: bool, label: &str) -> Html {
    html! {
        <span class={classes!("nexus-domain-badge", if ok { "ok" } else { "muted" })}>
            <i class={if ok { "fa fa-check-circle" } else { "fa fa-circle-o" }}></i>
            {label}
        </span>
    }
}

fn check_badge(result: Option<&Value>, key: &str, label: &str) -> Html {
    let ok = result
        .and_then(|value| value.get("checks"))
        .and_then(|checks| checks.get(key))
        .and_then(|check| check.get("ok"))
        .and_then(Value::as_bool);

    html! {
        <span class={classes!("nexus-domain-check", match ok { Some(true) => "ok", Some(false) => "bad", None => "idle" })}>
            <i class={match ok { Some(true) => "fa fa-check-circle", Some(false) => "fa fa-times-circle", None => "fa fa-circle-o" }}></i>
            {label}
        </span>
    }
}

#[function_component(NexusDomains)]
pub fn nexus_domains() -> Html {
    let inventory = use_state(|| None::<Result<Value, String>>);
    let validations = use_state(BTreeMap::<String, Value>::new);
    let busy_domain = use_state(|| None::<String>);
    let onboard_domain = use_state(String::new);
    let hestia_user = use_state(|| "admin".to_string());
    let onboarding = use_state(|| None::<Result<Value, String>>);

    {
        let inventory = inventory.clone();
        use_effect_with((), move |_| {
            spawn_local(async move {
                let result: Result<Value, _> = http_get("/domains", None).await;
                inventory.set(Some(result.map_err(|err| err.to_string())));
            });
            || ()
        });
    }

    let validate_domain = {
        let validations = validations.clone();
        let busy_domain = busy_domain.clone();
        Callback::from(move |domain: String| {
            let validations = validations.clone();
            let busy_domain = busy_domain.clone();
            busy_domain.set(Some(domain.clone()));
            spawn_local(async move {
                let result: Result<Value, _> = http_post(
                    "/domains/validate",
                    Some(json!({"domain": domain.clone()})),
                )
                .await;
                let mut next = (*validations).clone();
                match result {
                    Ok(value) => {
                        next.insert(domain.clone(), value);
                    }
                    Err(err) => {
                        next.insert(domain.clone(), json!({"healthy":false,"error":err.to_string()}));
                    }
                }
                validations.set(next);
                busy_domain.set(None);
            });
        })
    };

    let run_onboarding = {
        let onboard_domain = onboard_domain.clone();
        let hestia_user = hestia_user.clone();
        let onboarding = onboarding.clone();
        Callback::from(move |_| {
            let domain = (*onboard_domain).trim().to_ascii_lowercase();
            if domain.is_empty() {
                onboarding.set(Some(Err("Enter a domain first.".to_string())));
                return;
            }
            let user = (*hestia_user).trim().to_string();
            onboarding.set(None);
            let onboarding = onboarding.clone();
            spawn_local(async move {
                let result: Result<Value, _> = http_post(
                    "/domains/onboard",
                    Some(json!({"domain":domain,"hestia-user":user})),
                )
                .await;
                onboarding.set(Some(result.map_err(|err| err.to_string())));
            });
        })
    };

    let domain_input = {
        let onboard_domain = onboard_domain.clone();
        Callback::from(move |event: InputEvent| {
            let input = event.target().and_then(|target| target.dyn_into::<HtmlInputElement>().ok());
            if let Some(input) = input {
                onboard_domain.set(input.value());
            }
        })
    };

    let user_input = {
        let hestia_user = hestia_user.clone();
        Callback::from(move |event: InputEvent| {
            let input = event.target().and_then(|target| target.dyn_into::<HtmlInputElement>().ok());
            if let Some(input) = input {
                hestia_user.set(input.value());
            }
        })
    };

    html! {
        <div class="nexus-domains">
            <style>{DOMAINS_CSS}</style>
            <header class="nexus-domains-head">
                <div>
                    <span class="nexus-domains-eyebrow">{"DreamtecLabs operations"}</span>
                    <h1>{"Domains & Hosting"}</h1>
                    <p>{"One operational view for Cloudflare DNS/Tunnel, central DDNS, Hestia mail/webmail and delivery health."}</p>
                </div>
                <div class="nexus-domains-policy">
                    <span><i class="fa fa-shield"></i>{" mail.* / smtp.* always DNS only"}</span>
                    <span><i class="fa fa-random"></i>{" webmail.* always Tunnel-managed"}</span>
                </div>
            </header>

            <section class="nexus-domain-onboarding">
                <div class="nexus-domain-onboarding-copy">
                    <span class="nexus-domains-eyebrow">{"Idempotent workflow"}</span>
                    <h2>{"Onboard a domain"}</h2>
                    <p>{"DDNS mail record → Tunnel HTTP/80 → Hestia mail + DKIM + Roundcube → Let's Encrypt → MX/SPF/DKIM/DMARC → Tunnel HTTPS/443 with No TLS Verify → health validation."}</p>
                </div>
                <div class="nexus-domain-onboarding-form">
                    <label>{"Domain"}<input type="text" placeholder="example.com" value={(*onboard_domain).clone()} oninput={domain_input}/></label>
                    <label>{"Hestia owner"}<input type="text" value={(*hestia_user).clone()} oninput={user_input}/></label>
                    <button onclick={run_onboarding}><i class="fa fa-magic"></i>{" Run onboarding"}</button>
                </div>
                {match onboarding.as_ref() {
                    Some(Ok(value)) => html! {
                        <div class="nexus-domain-result ok">
                            <strong><i class="fa fa-check-circle"></i>{" Onboarding completed"}</strong>
                            <span>{format!("{} steps executed; final health: {}", value["result"]["steps"].as_array().map(Vec::len).unwrap_or(0), if value["validation"]["healthy"].as_bool().unwrap_or(false) { "healthy" } else { "needs attention" })}</span>
                        </div>
                    },
                    Some(Err(err)) => html! { <div class="nexus-domain-result bad"><strong><i class="fa fa-exclamation-triangle"></i>{" Onboarding failed"}</strong><span>{err}</span></div> },
                    None => Html::default(),
                }}
            </section>

            {match inventory.as_ref() {
                None => html! { <div class="nexus-domains-loading"><i class="fa fa-refresh fa-spin"></i>{" Loading domain inventory…"}</div> },
                Some(Err(err)) => html! { <div class="nexus-domain-result bad"><strong>{"Inventory unavailable"}</strong><span>{err}</span></div> },
                Some(Ok(data)) => {
                    let domains = data.get("domains").and_then(Value::as_array).cloned().unwrap_or_default();
                    html! {
                        <>
                            <div class="nexus-domain-kpis">
                                <div><span>{"Managed domains"}</span><strong>{domains.len()}</strong><small>{"Nexus source of truth"}</small></div>
                                <div><span>{"Mail relay"}</span><strong>{data["defaults"]["relay_hostname"].as_str().unwrap_or("—")}</strong><small>{data["defaults"]["relay_ipv4"].as_str().unwrap_or("—")}</small></div>
                                <div><span>{"Hestia"}</span><strong>{data["defaults"]["hestia_host"].as_str().unwrap_or("—")}</strong><small>{"Exim · Dovecot · Roundcube"}</small></div>
                                <div><span>{"Webmail origin"}</span><strong>{"HTTPS :443"}</strong><small>{"Tunnel · No TLS Verify"}</small></div>
                            </div>
                            <section class="nexus-domain-inventory">
                                <div class="nexus-domain-section-title"><div><h2>{"Domain inventory"}</h2><p>{"Live validation is read-only and never exposes credentials or provider tokens."}</p></div></div>
                                <div class="nexus-domain-table">
                                    <div class="nexus-domain-row header"><span>{"Domain"}</span><span>{"Capabilities"}</span><span>{"Health checks"}</span><span>{"Action"}</span></div>
                                    {for domains.into_iter().map(|domain| {
                                        let name = domain.get("name").and_then(Value::as_str).unwrap_or("unknown").to_string();
                                        let result = validations.get(&name);
                                        let is_busy = busy_domain.as_ref().as_ref() == Some(&name);
                                        let callback = {
                                            let validate_domain = validate_domain.clone();
                                            let name = name.clone();
                                            Callback::from(move |_| validate_domain.emit(name.clone()))
                                        };
                                        html! {
                                            <div class="nexus-domain-row">
                                                <span class="nexus-domain-name"><strong>{name.clone()}</strong><small>{if result.and_then(|r| r.get("healthy")).and_then(Value::as_bool).unwrap_or(false) { "Healthy" } else if result.is_some() { "Needs attention" } else { "Not checked" }}</small></span>
                                                <span class="nexus-domain-capabilities">
                                                    {status_badge(bool_value(&domain, "mail"), "Mail")}
                                                    {status_badge(bool_value(&domain, "webmail"), "Webmail")}
                                                    {status_badge(bool_value(&domain, "ddns"), "DDNS")}
                                                    {status_badge(bool_value(&domain, "tunnel"), "Tunnel")}
                                                </span>
                                                <span class="nexus-domain-health">
                                                    {check_badge(result, "mail_a", "A")}
                                                    {check_badge(result, "mx", "MX")}
                                                    {check_badge(result, "spf", "SPF")}
                                                    {check_badge(result, "dkim", "DKIM")}
                                                    {check_badge(result, "dmarc", "DMARC")}
                                                    {check_badge(result, "smtp_submission", "SMTP")}
                                                    {check_badge(result, "webmail_tls", "TLS")}
                                                </span>
                                                <span><button class="nexus-domain-validate" onclick={callback} disabled={is_busy}>{if is_busy { html!{<><i class="fa fa-refresh fa-spin"></i>{" Checking"}</>} } else { html!{<><i class="fa fa-heartbeat"></i>{" Validate"}</>} }}</button></span>
                                            </div>
                                        }
                                    })}
                                </div>
                            </section>
                        </>
                    }
                },
            }}
        </div>
    }
}

const DOMAINS_CSS: &str = r#"
.nexus-domains{--nx-text:#0f172a;--nx-muted:#64748b;--nx-border:#dbe3ee;--nx-blue:#2563eb;width:100%;height:100%;overflow:auto;background:linear-gradient(180deg,#f8faff,#f5f7fb 240px);color:var(--nx-text);padding:26px 30px 40px;font-family:"Roboto Flex",Roboto,Arial,sans-serif}.nexus-domains *{box-sizing:border-box}.nexus-domains-head{display:flex;justify-content:space-between;gap:28px;align-items:flex-start;margin-bottom:18px}.nexus-domains-eyebrow{display:block;color:#2563eb;font-size:10px;font-weight:780;text-transform:uppercase;letter-spacing:.08em}.nexus-domains-head h1{font-size:25px;margin:4px 0 5px;font-weight:800}.nexus-domains-head p,.nexus-domain-onboarding p,.nexus-domain-section-title p{margin:0;color:#64748b;font-size:10px;line-height:1.55}.nexus-domains-policy{display:flex;flex-direction:column;gap:6px;background:#fff;border:1px solid var(--nx-border);padding:10px 12px;border-radius:10px;box-shadow:0 2px 8px rgba(15,23,42,.04);font-size:9px;color:#334155;white-space:nowrap}.nexus-domains-policy i{color:#2563eb;width:17px}.nexus-domain-onboarding{background:#fff;border:1px solid var(--nx-border);border-radius:13px;box-shadow:0 3px 14px rgba(15,23,42,.05);padding:16px;display:grid;grid-template-columns:minmax(300px,1fr) minmax(360px,1fr);gap:16px;margin-bottom:14px}.nexus-domain-onboarding h2,.nexus-domain-section-title h2{font-size:14px;margin:3px 0 5px}.nexus-domain-onboarding-form{display:grid;grid-template-columns:1.4fr 1fr auto;gap:8px;align-items:end}.nexus-domain-onboarding-form label{font-size:8px;font-weight:700;color:#475569}.nexus-domain-onboarding-form input{display:block;width:100%;height:34px;margin-top:4px;border:1px solid #d8e0eb;border-radius:7px;padding:0 9px;background:#fff;color:#0f172a;font-size:10px}.nexus-domain-onboarding-form button,.nexus-domain-validate{height:34px;border:1px solid #1d4ed8;background:#2563eb;color:#fff;border-radius:7px;padding:0 12px;font-size:9px;font-weight:700;cursor:pointer;white-space:nowrap}.nexus-domain-onboarding-form button i,.nexus-domain-validate i{margin-right:6px}.nexus-domain-result{grid-column:1/-1;border-radius:8px;padding:9px 11px;display:flex;gap:12px;align-items:center;font-size:9px}.nexus-domain-result strong{white-space:nowrap}.nexus-domain-result.ok{background:#ecfdf3;border:1px solid #bbf7d0;color:#166534}.nexus-domain-result.bad{background:#fff7ed;border:1px solid #fed7aa;color:#9a3412}.nexus-domain-kpis{display:grid;grid-template-columns:repeat(4,1fr);gap:10px;margin-bottom:14px}.nexus-domain-kpis>div{background:#fff;border:1px solid var(--nx-border);border-radius:10px;padding:12px 14px;box-shadow:0 2px 8px rgba(15,23,42,.04);display:flex;flex-direction:column}.nexus-domain-kpis span{font-size:8px;color:#64748b;font-weight:700}.nexus-domain-kpis strong{font-size:15px;margin:3px 0;color:#0f172a}.nexus-domain-kpis small{font-size:8px;color:#94a3b8}.nexus-domain-inventory{background:#fff;border:1px solid var(--nx-border);border-radius:13px;box-shadow:0 3px 14px rgba(15,23,42,.05);overflow:hidden}.nexus-domain-section-title{padding:14px 16px;border-bottom:1px solid #e8edf4}.nexus-domain-table{width:100%}.nexus-domain-row{display:grid;grid-template-columns:minmax(150px,.8fr) minmax(250px,1.15fr) minmax(430px,2fr) 90px;align-items:center;gap:12px;min-height:56px;padding:8px 14px;border-bottom:1px solid #eef2f7;font-size:9px}.nexus-domain-row:last-child{border-bottom:0}.nexus-domain-row.header{min-height:34px;background:#f8fafc;color:#64748b;font-size:8px;font-weight:760;text-transform:uppercase;letter-spacing:.04em}.nexus-domain-name{display:flex;flex-direction:column}.nexus-domain-name strong{font-size:10px}.nexus-domain-name small{margin-top:3px;color:#94a3b8;font-size:8px}.nexus-domain-capabilities,.nexus-domain-health{display:flex;gap:5px;flex-wrap:wrap}.nexus-domain-badge,.nexus-domain-check{display:inline-flex;align-items:center;gap:4px;border-radius:999px;padding:4px 7px;border:1px solid #e2e8f0;background:#f8fafc;color:#64748b;font-size:8px}.nexus-domain-badge.ok{background:#eef4ff;border-color:#d8e5ff;color:#1d4ed8}.nexus-domain-check.ok{background:#ecfdf3;border-color:#bbf7d0;color:#15803d}.nexus-domain-check.bad{background:#fef2f2;border-color:#fecaca;color:#b91c1c}.nexus-domain-validate{background:#fff;color:#2563eb;border-color:#bfd3ff}.nexus-domain-validate:hover{background:#f5f8ff}.nexus-domain-validate:disabled{opacity:.55;cursor:wait}.nexus-domains-loading{padding:20px;background:#fff;border:1px solid var(--nx-border);border-radius:10px;color:#64748b;font-size:10px}.nexus-domains-loading i{margin-right:8px}@media(max-width:1200px){.nexus-domain-row{grid-template-columns:150px 220px 1fr 90px}.nexus-domain-onboarding{grid-template-columns:1fr}.nexus-domain-result{grid-column:auto}.nexus-domain-kpis{grid-template-columns:repeat(2,1fr)}}@media(max-width:850px){.nexus-domains{padding:18px 14px 30px}.nexus-domains-head{flex-direction:column}.nexus-domains-policy{white-space:normal}.nexus-domain-onboarding-form{grid-template-columns:1fr}.nexus-domain-row{grid-template-columns:1fr;gap:7px;padding:12px 14px}.nexus-domain-row.header{display:none}.nexus-domain-kpis{grid-template-columns:1fr}}
"#;
