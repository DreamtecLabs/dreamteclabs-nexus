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

fn check_adopted(result: Option<&Value>, key: &str) -> bool {
    result
        .and_then(|value| value.get("checks"))
        .and_then(|checks| checks.get(key))
        .and_then(|check| check.get("adopted"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn check_badge(result: Option<&Value>, key: &str, label: &str) -> Html {
    let ok = check_ok(result, key);
    let adopted = check_adopted(result, key);
    let title = if adopted {
        "Existing configuration kept and monitored"
    } else {
        ""
    };
    html! {
        <span title={title} class={classes!(
            "nexus-domain-check",
            match ok { Some(true) => "ok", Some(false) => "bad", None => "idle" },
            adopted.then_some("adopted")
        )}>
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

fn decision_required(result: Option<&Value>) -> bool {
    result
        .and_then(|value| value.get("decision_required"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn existing_mode(result: Option<&Value>) -> bool {
    result
        .and_then(|value| value.get("configuration_mode"))
        .and_then(Value::as_str)
        == Some("existing")
}

fn existing_record_labels(result: Option<&Value>) -> String {
    let Some(records) = result
        .and_then(|value| value.get("existing_configuration"))
        .and_then(|value| value.get("records"))
        .and_then(Value::as_object)
    else {
        return String::new();
    };
    let mut labels: Vec<String> = records.keys().map(|key| key.to_ascii_uppercase()).collect();
    labels.sort();
    labels.join(", ")
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
                    Some(json!({"domain":domain.clone(),"hestia_user":user,"replace_existing":false})),
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

    let adopt_existing = {
        let validations = validations.clone();
        let busy_domain = busy_domain.clone();
        let busy_action = busy_action.clone();
        let action_results = action_results.clone();
        Callback::from(move |domain: String| {
            let validations = validations.clone();
            let busy_domain = busy_domain.clone();
            let busy_action = busy_action.clone();
            let action_results = action_results.clone();
            busy_domain.set(Some(domain.clone()));
            busy_action.set(Some("adopt".to_string()));
            spawn_local(async move {
                let result: Result<Value, _> =
                    http_post("/domains/adopt", Some(json!({"domain":domain.clone()}))).await;
                let mut messages = (*action_results).clone();
                match result {
                    Ok(value) => {
                        if let Some(validation) = value.get("validation") {
                            let mut next = (*validations).clone();
                            next.insert(domain.clone(), validation.clone());
                            validations.set(next);
                        }
                        messages.insert(
                            domain.clone(),
                            Ok("Existing configuration kept as the monitored policy.".to_string()),
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

    let migrate_domain = {
        let inventory = inventory.clone();
        let validations = validations.clone();
        let busy_domain = busy_domain.clone();
        let busy_action = busy_action.clone();
        let action_results = action_results.clone();
        let hestia_user = hestia_user.clone();
        Callback::from(move |domain: String| {
            let inventory = inventory.clone();
            let validations = validations.clone();
            let busy_domain = busy_domain.clone();
            let busy_action = busy_action.clone();
            let action_results = action_results.clone();
            let user = (*hestia_user).trim().to_string();
            busy_domain.set(Some(domain.clone()));
            busy_action.set(Some("migrate".to_string()));
            spawn_local(async move {
                let result: Result<Value, _> = http_post(
                    "/domains/onboard",
                    Some(json!({"domain":domain.clone(),"hestia_user":user,"replace_existing":true})),
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
                        let refreshed: Result<Value, _> = http_get("/domains", None).await;
                        inventory.set(Some(refreshed.map_err(|err| err.to_string())));
                        let healthy = value
                            .get("validation")
                            .and_then(|v| v.get("healthy"))
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        messages.insert(
                            domain.clone(),
                            Ok(if healthy {
                                "Migrated to the Nexus Hestia standard and validated successfully."
                                    .to_string()
                            } else {
                                "Migration completed, but live validation still needs attention."
                                    .to_string()
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
                    Some(json!({"domain":domain.clone(),"hestia_user":user,"replace_existing":false})),
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
                    <p>{"Validate the current state, keep legitimate existing configurations, or migrate them explicitly to the Nexus standard. Nothing conflicting is replaced without a user decision."}</p>
                </div>
                <div class="nexus-domain-policy-card">
                    <span>{"Reconciliation policy"}</span>
                    <strong>{"Discover · Decide · Reconcile"}</strong>
                    <small>{"Existing configuration is preserved until you choose: keep and monitor it, or migrate it to Hestia and the Nexus DNS standard."}</small>
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
                                <div><span>{"Conflict handling"}</span><strong>{"User decision"}</strong><small>{"Keep existing or migrate"}</small></div>
                            </div>
                            <section class="nexus-domain-inventory">
                                <div class="nexus-domain-section-title"><div><h2>{"Domain inventory"}</h2><p>{"Validate is read-only. Existing configurations require an explicit keep-or-migrate decision before Nexus changes conflicting records."}</p></div></div>
                                <div class="nexus-domain-table">
                                    <div class="nexus-domain-row header"><span>{"Domain"}</span><span>{"Capabilities"}</span><span>{"Configuration"}</span><span>{"Action"}</span></div>
                                    {for domains.into_iter().map(|domain| {
                                        let name = domain.get("name").and_then(Value::as_str).unwrap_or("unknown").to_string();
                                        let result = validations.get(&name);
                                        let healthy = result_healthy(result);
                                        let configured = configured_count(result);
                                        let needs_decision = decision_required(result);
                                        let kept_existing = existing_mode(result);
                                        let existing_labels = existing_record_labels(result);
                                        let is_busy = (*busy_domain).as_ref() == Some(&name);
                                        let validating = is_busy && (*busy_action).as_deref() == Some("validate");
                                        let reconciling = is_busy && (*busy_action).as_deref() == Some("reconcile");
                                        let adopting = is_busy && (*busy_action).as_deref() == Some("adopt");
                                        let migrating = is_busy && (*busy_action).as_deref() == Some("migrate");
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
                                        let adopt_callback = {
                                            let adopt_existing = adopt_existing.clone();
                                            let name = name.clone();
                                            Callback::from(move |_| adopt_existing.emit(name.clone()))
                                        };
                                        let migrate_callback = {
                                            let migrate_domain = migrate_domain.clone();
                                            let name = name.clone();
                                            Callback::from(move |_| migrate_domain.emit(name.clone()))
                                        };
                                        let message = action_results.get(&name);
                                        html! {
                                            <div class={classes!("nexus-domain-row", needs_decision.then_some("decision"))}>
                                                <span class="nexus-domain-name">
                                                    <strong>{name.clone()}</strong>
                                                    <small>{
                                                        if result.is_none() { "Not checked".to_string() }
                                                        else if healthy && kept_existing { "Active · Healthy · Existing policy".to_string() }
                                                        else if healthy { "Active · Healthy".to_string() }
                                                        else if needs_decision { "Existing configuration · Decision required".to_string() }
                                                        else { format!("Incomplete · {configured}/{} configured", REQUIRED_CHECKS.len()) }
                                                    }</small>
                                                    {
                                                        if needs_decision {
                                                            html! { <small class="decision-note">{format!("Found existing configuration{}{}. Keep it, or migrate to the Nexus standard.", if existing_labels.is_empty() { "" } else { ": " }, existing_labels)}</small> }
                                                        } else { html! {} }
                                                    }
                                                    {
                                                        match message {
                                                            Some(Ok(text)) => html! { <small class="ok">{text}</small> },
                                                            Some(Err(text)) => html! { <small class="error">{format!("Action stopped safely: {text}")}</small> },
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
                                                        if needs_decision {
                                                            html! {
                                                                <div class="nexus-domain-choice">
                                                                    <button class="nexus-domain-action secondary" onclick={adopt_callback} disabled={is_busy}>
                                                                        <i class="fa fa-check"></i>{if adopting { "Keeping…" } else { "Keep existing" }}
                                                                    </button>
                                                                    <button class="nexus-domain-action" onclick={migrate_callback} disabled={is_busy}>
                                                                        <i class="fa fa-exchange"></i>{if migrating { "Migrating…" } else { "Use Nexus standard" }}
                                                                    </button>
                                                                </div>
                                                            }
                                                        } else if result.is_some() && !healthy {
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
                <div class="nexus-domain-section-title"><div><h2>{"Ownership model"}</h2><p>{"Reconciliation changes only the systems Nexus explicitly owns or the records you explicitly choose to migrate."}</p></div></div>
                <div class="nexus-domain-ownership-grid">
                    <article><i class="fa fa-refresh"></i><span>{"Central DDNS"}</span><strong>{"mail.domain"}</strong><p>{"Dynamic public A record. Always DNS-only. Nexus never writes this A record through the Cloudflare API."}</p></article>
                    <article><i class="fa fa-cloud"></i><span>{"Cloudflare Tunnel"}</span><strong>{"webmail.domain"}</strong><p>{"Proxied Tunnel route. Existing conflicting routes are not replaced without migration approval."}</p></article>
                    <article><i class="fa fa-envelope"></i><span>{"Mail policy"}</span><strong>{"Existing or Hestia"}</strong><p>{"Existing MX/TXT policy can be adopted and monitored, or explicitly migrated to Hestia, Roundcube and Nexus-managed DNS."}</p></article>
                </div>
            </section>

            <section class="nexus-domain-onboarding">
                <div class="nexus-domain-section-title"><div><h2>{"Add or complete a mail domain"}</h2><p>{"Brand-new and incomplete domains use the idempotent reconciler. Existing conflicts stop safely and are presented for a keep-or-migrate decision."}</p></div></div>
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
