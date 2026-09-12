use anyhow::Error;

use proxmox_client::{ApiPathBuilder, HttpApiClient};
use proxmox_router::{Permission, Router, SubdirMap, list_subdirs_api_method};
use proxmox_schema::api;
use proxmox_sortable_macro::sortable;
use serde::Deserialize;

use pdm_api_types::remotes::REMOTE_ID_SCHEMA;
use pdm_api_types::{NODE_SCHEMA, PRIV_RESOURCE_AUDIT, PVE_STORAGE_ID_SCHEMA};

use super::connect_to_remote_by_id;

pub const ROUTER: Router = Router::new()
    .get(&list_subdirs_api_method!(STORAGE_SUBDIR))
    .subdirs(STORAGE_SUBDIR);

#[sortable]
const STORAGE_SUBDIR: SubdirMap = &sorted!([
    ("content", &Router::new().get(&API_METHOD_GET_CONTENT)),
    ("rrddata", &super::rrddata::STORAGE_RRD_ROUTER),
    ("status", &Router::new().get(&API_METHOD_GET_STATUS)),
]);

#[api(
    input: {
        properties: {
            remote: { schema: REMOTE_ID_SCHEMA },
            node: { schema: NODE_SCHEMA, },
            storage: { schema: PVE_STORAGE_ID_SCHEMA, },
        },
    },
    returns: { type: pve_api_types::StorageStatus },
    access: {
        permission: &Permission::Privilege(&["resource", "{remote}", "storage", "{storage}"], PRIV_RESOURCE_AUDIT, false),
    },
)]
/// Get the status of a qemu VM from a remote. If a node is provided, the VM must be on that
/// node, otherwise the node is determined automatically.
pub async fn get_status(
    remote: String,
    node: String,
    storage: String,
) -> Result<pve_api_types::StorageStatus, Error> {
    let pve = connect_to_remote_by_id(&remote)?;

    Ok(pve.storage_status(&node, &storage).await?)
}

#[derive(Debug, Deserialize)]
struct RawStorageContentEntry {
    volid: String,
}

#[api(
    input: {
        properties: {
            remote: { schema: REMOTE_ID_SCHEMA },
            node: { schema: NODE_SCHEMA, },
            storage: { schema: PVE_STORAGE_ID_SCHEMA, },
            content: {
                type: String,
                optional: true,
                description: "Filter by PVE content type (vztmpl, iso, images, backup, ...).",
            },
        },
    },
    returns: {
        type: Array,
        description: "Volume IDs available on this storage.",
        items: { type: String, description: "A PVE volume ID." },
    },
    access: {
        permission: &Permission::Privilege(&["resource", "{remote}", "storage", "{storage}"], PRIV_RESOURCE_AUDIT, false),
    },
)]
/// List the volumes (templates, ISOs, backups, ...) available on a storage.
///
/// pve-api-types 8.1.12 does not implement this endpoint (see the "not
/// handled" doc comment at the top of its generated/code.rs), so this goes
/// straight to the raw HttpApiClient the same way pbs_client.rs already does
/// for PBS calls without typed bindings, instead of the typed PveClient trait.
pub async fn get_content(
    remote: String,
    node: String,
    storage: String,
    content: Option<String>,
) -> Result<Vec<String>, Error> {
    let (remotes, _) = pdm_config::remotes::config()?;
    let remote_config = super::get_remote(&remotes, &remote)?;
    let client = crate::connection::make_raw_client(remote_config)?;

    let path = ApiPathBuilder::new(format!("/api2/extjs/nodes/{node}/storage/{storage}/content"))
        .maybe_arg("content", &content)
        .build();

    let entries: Vec<RawStorageContentEntry> = client.get(&path).await?.expect_json()?.data;
    Ok(entries.into_iter().map(|entry| entry.volid).collect())
}
