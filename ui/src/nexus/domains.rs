use std::collections::BTreeMap;

use proxmox_yew_comp::{http_get, http_post};
use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use web_sys::HtmlInputElement;
use yew::prelude::*;

const REQUIRED_CHECKS: [(&str, &str); 9] = [
    ("mail_a", "A"),
    ("mx", "MX"),
    ("spf", "SPF"),
    ("dkim", "DKIM"),
    ("dmarc", "DMARC"),
    ("smtp_submission", "SMTP"),
    ("imap_tls", "IMAP"),
    ("webmail_tunnel_dns", "Webmail DNS"),
    ("webmail_tls", "Webmail TLS"),
];

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

fn check_ok(result: Option<&Value>, key: &str) -> Option<bool> {
    result
        .and_then(|value| value.get("checks"))
        .and_then(|checks| checks.get(key))
        .and_then(|check| check.get("ok"))
        .and_then(Value::as_bool)
}

fn check_badge(result: Option<&Value>, key: &str, label: &str) -> Html {
    let ok = check_ok(result, key);
    html! {
        <span class={classes!("nexus-domain-check", match ok { Some(true) => "ok", Some(false) => "bad", None => "idle" })}>
            <i class={match ok { Some(true) => "fa fa-check-circle", Some(false) => "fa fa-times-circle", None => "fa fa-circle-o" }}></i>
            {label}
        </span>
    }
}

fn configured_count(result: Option<&Value>) -> usize {
    REQUIRED_CHECKS
        .iter()
        .filter(|(key, _)| check_ok(result, key) == Some(true))
        .count()
}

fn result_healthy(result: Option<&Value>) -> bool {
    result
        .and_then(|value| value.get("healthy"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

#[function_component(NexusDomains)]
pub fn nexus_domains() -> Html {
    let inventory = use_state(|| None::<Result<Value, String>>);
    let validations = use_state(BTreeMap::<String, Value>::new);
    let busy_domain = use_state(|| None::<String>);
    let busy_action = use_state(|| None::<String>);
    let action_results = use_state(BTreeMap::<String, Result<String, String>>::new);
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
        let busy_action = busy_action.clone();
        Callback::from(move |domain: String| {
            let validations = validations.clone();
            let busy_domain = busy_domain.clone();
            let busy_action = busy_action.clone();
            busy_domain.set(Some(domain.clone()));
            busy_action.set(Some("validate".to_string()));
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
                busy_action.set(None);
            });
        })
    };

    let reconcile_domain = {
        let validations = validations.clone();
        let busy_domain = busy_domain.clone();
        let busy_action = busy_action.clone();
        let action_results = action_results.clone();
        let hestia_user = hestia_user.clone();
        Callback::from(move |domain: String| {
            let validations = validations.clone();
            let busy_domain = busy_domain.clone();
            let busy_action = busy_action.clone();
            let action_results = action_results.clone();
            let user = (*hestia_user).trim().to_string();
            busy_domain.set(Some(domain.clone()));
            busy_action.set(Some("reconcile".to_string()));
            spawn_local(async move {
                let result: Result<Value, _> = http_post(
                    "/domains/onboard",
                    Some(json!({"domain":domain.clone(),"hestia-user":user})),
                )
                .await;

                let mut messages = (*action_results).clone();
                match result {
                    Ok(value) => {
                        if let Some(validation) = value.get("validation") {
                            let mut next = (*validations).clone();
                            next.insert(domain.clone(), validation.clone());
                            validations.set(next);
                        }
                        let healthy = value
                            .get("validation")
                            .and_then(|v| v.get("healthy"))
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        messages.insert(
                            domain.clone(),
                            Ok(if healthy {
                                "Configuration reconciled and validated successfully.".to_string()
                            } else {
                                "Reconciliation completed, but one or more live checks still need attention.".to_string()
                            }),
                        );
                    }
                    Err(err) => {
                        messages.insert(domain.clone(), Err(err.to_string()));
                    }
                }
                action_results.set(messages);
                busy_domain.set(None);
                busy_action.set(None);
            });
        })
    };

    let run_onboarding = {
        let onboard_domain = onboard_domain.clone();
        let hestia_user = hestia_user.clone();
        let onboarding = onboarding.clone();
        let validations = validations.clone();
        Callback::from(move |_| {
            let domain = (*onboard_domain).trim().to_ascii_lowercase();
            if domain.is_empty() {
                onboarding.set(Some(Err("Enter a domain first.".to_string())));
                return;
            }
            let user = (*hestia_user).trim().to_string();
            onboarding.set(None);
            let onboarding = onboarding.clone();
            let validations = validations.clone();
            spawn_local(async move {
                let result: Result<Value, _> = http_post(
                    "/domains/onboard",
                    Some(json!({"domain":domain.clone(),"hestia-user":user})),
                )
                .await;
                if let Ok(value) = &result {
                    if let Some(validation) = value.get("validation") {
                        let mut next = (*validations).clone();
                        next.insert(domain, validation.clone());
                        validations.set(next);
                    }
                }
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
                    <p>{"Validate the current state, see exactly what is missing, and reconcile incomplete domains with one click. Nexus only creates or updates the components it owns and fails closed on ambiguous conflicts."}</p>
                </div>
                <div class="nexus-domain-policy-card">
                    <span>{"Reconciliation policy"}</span>
                    <strong>{"Idempotent · Fail closed"}</strong>
                    <small>{"Existing correct configuration is preserved. Conflicting MX/TXT records or DDNS-managed webmail stop repair instead of being overwritten."}</small>
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
                                <div><span>{"Repair"}</span><strong>{"One click"}</strong><small>{"Reconcile then validate"}</small></div>
                            </div>
                            <section class="nexus-domain-inventory">
                                <div class="nexus-domain-section-title"><div><h2>{"Domain inventory"}</h2><p>{"Validate is read-only. Fix configuration reconciles missing owned components and immediately validates the result."}</p></div></div>
                                <div class="nexus-domain-table">
                                    <div class="nexus-domain-row header"><span>{"Domain"}</span><span>{"Capabilities"}</span><span>{"Configuration"}</span><span>{"Action"}</span></div>
                                    {for domains.into_iter().map(|domain| {
                                        let name = domain.get("name").and_then(Value::as_str).unwrap_or("unknown").to_string();
                                        let result = validations.get(&name);
                                        let healthy = result_healthy(result);
                                        let configured = configured_count(result);
                                        let is_busy = (*busy_domain).as_ref() == Some(&name);
                                        let validating = is_busy && (*busy_action).as_deref() == Some("validate");
                                        let reconciling = is_busy && (*busy_action).as_deref() == Some("reconcile");
                                        let validate_callback = {
                                            let validate_domain = validate_domain.clone();
                                            let name = name.clone();
                                            Callback::from(move |_| validate_domain.emit(name.clone()))
                                        };
                                        let reconcile_callback = {
                                            let reconcile_domain = reconcile_domain.clone();
                                            let name = name.clone();
                                            Callback::from(move |_| reconcile_domain.emit(name.clone()))
                                        };
                                        let message = action_results.get(&name);
                                        html! {
                                            <div class="nexus-domain-row">
                                                <span class="nexus-domain-name">
                                                    <strong>{name.clone()}</strong>
                                                    <small>{
                                                        if result.is_none() { "Not checked".to_string() }
                                                        else if healthy { "Active · Healthy".to_string() }
                                                        else { format!("Incomplete · {configured}/{} configured", REQUIRED_CHECKS.len()) }
                                                    }</small>
                                                    {
                                                        match message {
                                                            Some(Ok(text)) => html! { <small class="ok">{text}</small> },
                                                            Some(Err(text)) => html! { <small class="error">{format!("Repair stopped safely: {text}")}</small> },
                                                            None => html! {},
                                                        }
                                                    }
                                                </span>
                                                <span class="nexus-domain-capabilities">
                                                    {status_badge(bool_value(&domain, "mail"), "Mail")}
                                                    {status_badge(bool_value(&domain, "webmail"), "Webmail")}
                                                    {status_badge(bool_value(&domain, "ddns"), "DDNS")}
                                                    {status_badge(bool_value(&domain, "tunnel"), "Tunnel")}
                                                </span>
                                                <span class="nexus-domain-checks">
                                                    {for REQUIRED_CHECKS.iter().map(|(key, label)| check_badge(result, key, label))}
                                                </span>
                                                <span>
                                                    {
                                                        if result.is_some() && !healthy {
                                                            html! { <button class="nexus-domain-action" onclick={reconcile_callback} disabled={is_busy}><i class="fa fa-magic"></i>{if reconciling { "Fixing…" } else { "Fix configuration" }}</button> }
                                                        } else {
                                                            html! { <button class="nexus-domain-action" onclick={validate_callback} disabled={is_busy}>{if validating { "Checking…" } else if healthy { "Revalidate" } else { "Validate" }}</button> }
                                                        }
                                                    }
                                                </span>
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
                <div class="nexus-domain-section-title"><div><h2>{"Ownership model"}</h2><p>{"Reconciliation changes only the systems Nexus explicitly owns."}</p></div></div>
                <div class="nexus-domain-ownership-grid">
                    <article><i class="fa fa-refresh"></i><span>{"Central DDNS"}</span><strong>{"mail.domain"}</strong><p>{"Dynamic public A record. Always DNS-only. Nexus never writes this A record through the Cloudflare API."}</p></article>
                    <article><i class="fa fa-cloud"></i><span>{"Cloudflare Tunnel"}</span><strong>{"webmail.domain"}</strong><p>{"Proxied Tunnel route. Never placed in the DDNS records file."}</p></article>
                    <article><i class="fa fa-envelope"></i><span>{"Hestia"}</span><strong>{"Mail services"}</strong><p>{"Mail domain, Roundcube, DKIM and certificate lifecycle remain on the Hestia host."}</p></article>
                </div>
            </section>

            <section class="nexus-domain-onboarding">
                <div class="nexus-domain-section-title"><div><h2>{"Add or complete a mail domain"}</h2><p>{"The same idempotent reconciler handles brand-new domains and domains that were only partially configured."}</p></div></div>
                <div class="nexus-domain-onboarding-form">
                    <label><span>{"Domain"}</span><input value={(*onboard_domain).clone()} oninput={domain_input} placeholder="example.com" /></label>
                    <label><span>{"Hestia owner"}</span><input value={(*hestia_user).clone()} oninput={user_input} placeholder="admin" /></label>
                    <button onclick={run_onboarding}><i class="fa fa-magic"></i>{"Complete setup"}</button>
                </div>
                {
                    match onboarding.as_ref() {
                        None => html! {},
                        Some(Ok(value)) => {
                            let healthy = value.get("validation").and_then(|v| v.get("healthy")).and_then(Value::as_bool).unwrap_or(false);
                            html! { <div class={classes!("nexus-domain-result", if healthy { "ok" } else { "error" })}><strong>{if healthy { "Setup completed and validated." } else { "Setup completed, but live validation still needs attention." }}</strong><span>{"The domain was reconciled using the same ownership and conflict-safety policy as one-click repair."}</span></div> }
                        },
                        Some(Err(err)) => html! { <div class="nexus-domain-result error"><strong>{"Setup stopped safely."}</strong><span>{err}</span></div> },
                    }
                }
            </section>
        </div>
    }
}
