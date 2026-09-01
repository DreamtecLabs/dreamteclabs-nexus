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
                let result: Result<Value, _> =
                    http_post("/domains/validate", Some(json!({"domain": domain.clone()}))).await;
                let mut next = (*validations).clone();
                match result {
                    Ok(value) => {
                        next.insert(domain.clone(), value);
                    }
                    Err(err) => {
                        next.insert(
                            domain.clone(),
                            json!({"healthy":false,"error":err.to_string()}),
                        );
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
            let input = event
                .target()
                .and_then(|target| target.dyn_into::<HtmlInputElement>().ok());
            if let Some(input) = input {
                onboard_domain.set(input.value());
            }
        })
    };

    let user_input = {
        let hestia_user = hestia_user.clone();
        Callback::from(move |event: InputEvent| {
            let input = event
                .target()
                .and_then(|target| target.dyn_into::<HtmlInputElement>().ok());
            if let Some(input) = input {
                hestia_user.set(input.value());
            }
        })
    };

    html! {
        <div class="nexus-domains">
            <section class="nexus-domains-hero">
                <div>
                    <span class="nexus-eyebrow">{"Nexus · Domains & Hosting"}</span>
                    <h1>{"One control plane for DNS, mail and webmail."}</h1>
                    <p>{"Nexus keeps mail records DDNS-owned and DNS-only, while webmail remains Cloudflare Tunnel-owned. Live checks make the operational state explicit before changes are made."}</p>
                </div>
                <div class="nexus-domain-policy-card">
                    <span>{"Ownership policy"}</span>
                    <strong>{"Fail closed"}</strong>
                    <small>{"Conflicting MX/TXT records or DDNS-managed webmail stop onboarding instead of being overwritten."}</small>
                </div>
            </section>

            {
                match inventory.as_ref() {
                    None => html! { <div class="nexus-domain-state"><i class="fa fa-circle-o-notch fa-spin"></i>{"Loading domain inventory…"}</div> },
                    Some(Err(err)) => html! { <div class="nexus-domain-state error"><i class="fa fa-exclamation-triangle"></i>{format!("Unable to load inventory: {err}")}</div> },
                    Some(Ok(data)) => {
                        let domains = data.get("domains").and_then(Value::as_array).cloned().unwrap_or_default();
                        html! {
                            <>
                            <div class="nexus-domain-summary-grid">
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
                                        let is_busy = (*busy_domain).as_ref() == Some(&name);
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
                                                <span class="nexus-domain-checks">
                                                    {check_badge(result, "mail-a", "A")}
                                                    {check_badge(result, "mx", "MX")}
                                                    {check_badge(result, "spf", "SPF")}
                                                    {check_badge(result, "dkim", "DKIM")}
                                                    {check_badge(result, "dmarc", "DMARC")}
                                                    {check_badge(result, "smtp-starttls", "SMTP")}
                                                    {check_badge(result, "imap-tls", "IMAP")}
                                                    {check_badge(result, "webmail-tls", "Webmail")}
                                                </span>
                                                <span><button class="nexus-domain-action" onclick={callback} disabled={is_busy}>{if is_busy { "Checking…" } else { "Validate" }}</button></span>
                                            </div>
                                        }
                                    })}
                                </div>
                            </section>
                            </>
                        }
                    }
                }
            }

            <section class="nexus-domain-ownership">
                <div class="nexus-domain-section-title"><div><h2>{"Ownership model"}</h2><p>{"Automation is explicit about which system owns each hostname."}</p></div></div>
                <div class="nexus-domain-ownership-grid">
                    <article><i class="fa fa-refresh"></i><span>{"Central DDNS"}</span><strong>{"mail.domain"}</strong><p>{"Dynamic public A record. Always DNS-only. Nexus never writes this A record through Cloudflare API."}</p></article>
                    <article><i class="fa fa-cloud"></i><span>{"Cloudflare Tunnel"}</span><strong>{"webmail.domain"}</strong><p>{"Proxied Tunnel route. Never placed in the DDNS records file."}</p></article>
                    <article><i class="fa fa-envelope"></i><span>{"Hestia"}</span><strong>{"Mail services"}</strong><p>{"Exim, Dovecot, Roundcube, DKIM and certificate lifecycle stay on the Hestia host."}</p></article>
                </div>
            </section>

            <section class="nexus-domain-onboarding">
                <div class="nexus-domain-section-title"><div><h2>{"Onboard mail domain"}</h2><p>{"Privileged and idempotent. Existing conflicting ownership is refused instead of silently replaced."}</p></div></div>
                <div class="nexus-domain-onboarding-form">
                    <label><span>{"Domain"}</span><input value={(*onboard_domain).clone()} oninput={domain_input} placeholder="example.com" /></label>
                    <label><span>{"Hestia owner"}</span><input value={(*hestia_user).clone()} oninput={user_input} placeholder="admin" /></label>
                    <button onclick={run_onboarding}><i class="fa fa-magic"></i>{"Run onboarding"}</button>
                </div>
                {
                    match onboarding.as_ref() {
                        None => html! {},
                        Some(Ok(value)) => html! { <div class="nexus-domain-result ok"><strong>{"Onboarding completed."}</strong><span>{value["message"].as_str().unwrap_or("Providers updated and validation executed.")}</span></div> },
                        Some(Err(err)) => html! { <div class="nexus-domain-result error"><strong>{"Onboarding stopped safely."}</strong><span>{err}</span></div> },
                    }
                }
            </section>
        </div>
    }
}
