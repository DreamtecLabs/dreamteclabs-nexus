use anyhow::{Error, bail};
use proxmox_client::HttpApiClient;
use proxmox_router::{Permission, Router, SubdirMap, list_subdirs_api_method};
use proxmox_schema::api;
use proxmox_sortable_macro::sortable;
use serde::Serialize;

use pdm_api_types::remotes::REMOTE_ID_SCHEMA;
use pdm_api_types::{NODE_SCHEMA, PRIV_RESOURCE_MANAGE, RemoteUpid, VMID_SCHEMA};

use super::{get_remote, new_remote_upid};

pub const ROUTER: Router = Router::new()
    .get(&list_subdirs_api_method!(SUBDIRS))
    .subdirs(SUBDIRS);

#[sortable]
const SUBDIRS: SubdirMap = &sorted!([
    ("lxc", &Router::new().post(&API_METHOD_CREATE_LXC)),
    ("qemu", &Router::new().post(&API_METHOD_CREATE_QEMU)),
]);

#[derive(Serialize)]
struct CreateQemuParams {
    vmid: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    cores: u64,
    memory: u64,
    scsihw: &'static str,
    scsi0: String,
    net0: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    ide2: Option<String>,
    start: bool,
}

#[derive(Serialize)]
struct CreateLxcParams {
    vmid: u32,
    hostname: String,
    ostemplate: String,
    cores: u64,
    memory: u64,
    rootfs: String,
    net0: String,
    unprivileged: bool,
    start: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<String>,
}

fn validate_common(cores: u64, memory: u64, disk_gb: u64, storage: &str, bridge: &str) -> Result<(), Error> {
    if cores == 0 {
        bail!("cores must be greater than zero");
    }
    if memory < 64 {
        bail!("memory must be at least 64 MiB");
    }
    if disk_gb == 0 {
        bail!("disk size must be greater than zero");
    }
    if storage.trim().is_empty() {
        bail!("storage is required");
    }
    if bridge.trim().is_empty() {
        bail!("network bridge is required");
    }
    Ok(())
}

#[api(
    input: {
        properties: {
            remote: { schema: REMOTE_ID_SCHEMA },
            node: { schema: NODE_SCHEMA },
            vmid: { schema: VMID_SCHEMA },
            name: {
                type: String,
                optional: true,
                description: "Virtual machine name.",
            },
            cores: {
                type: Integer,
                minimum: 1,
                default: 2,
                description: "Number of virtual CPU cores.",
            },
            memory: {
                type: Integer,
                minimum: 64,
                default: 2048,
                description: "Memory in MiB.",
            },
            storage: {
                type: String,
                description: "PVE storage ID for the primary disk.",
            },
            "disk-gb": {
                type: Integer,
                minimum: 1,
                default: 32,
                description: "Primary disk size in GiB.",
            },
            bridge: {
                type: String,
                default: "vmbr0",
                description: "PVE bridge for the first network interface.",
            },
            iso: {
                type: String,
                optional: true,
                description: "Optional PVE volume ID for installation media.",
            },
            start: {
                type: Boolean,
                default: false,
                description: "Start the VM after creation.",
            },
        },
    },
    returns: { type: RemoteUpid },
    access: {
        permission: &Permission::Privilege(&["resource", "{remote}"], PRIV_RESOURCE_MANAGE, false),
    },
)]
/// Create a QEMU virtual machine on a PVE remote through the PDM backend.
#[allow(clippy::too_many_arguments)]
pub async fn create_qemu(
    remote: String,
    node: String,
    vmid: u32,
    name: Option<String>,
    cores: u64,
    memory: u64,
    storage: String,
    disk_gb: u64,
    bridge: String,
    iso: Option<String>,
    start: bool,
) -> Result<RemoteUpid, Error> {
    validate_common(cores, memory, disk_gb, &storage, &bridge)?;

    let (remotes, _) = pdm_config::remotes::config()?;
    let remote_config = get_remote(&remotes, &remote)?;
    let client = crate::connection::make_raw_client(remote_config)?;

    let params = CreateQemuParams {
        vmid,
        name: name.filter(|value| !value.trim().is_empty()),
        cores,
        memory,
        scsihw: "virtio-scsi-pci",
        scsi0: format!("{}:{}", storage.trim(), disk_gb),
        net0: format!("virtio,bridge={}", bridge.trim()),
        ide2: iso
            .filter(|value| !value.trim().is_empty())
            .map(|value| format!("{},media=cdrom", value.trim())),
        start,
    };

    let path = format!("/api2/extjs/nodes/{node}/qemu");
    let upid: pve_api_types::PveUpid = client.post(&path, &params).await?.expect_json()?.data;
    new_remote_upid(remote, upid).await
}

#[api(
    input: {
        properties: {
            remote: { schema: REMOTE_ID_SCHEMA },
            node: { schema: NODE_SCHEMA },
            vmid: { schema: VMID_SCHEMA },
            hostname: {
                type: String,
                description: "Container hostname.",
            },
            ostemplate: {
                type: String,
                description: "PVE volume ID of the LXC template.",
            },
            cores: {
                type: Integer,
                minimum: 1,
                default: 2,
                description: "Number of CPU cores.",
            },
            memory: {
                type: Integer,
                minimum: 64,
                default: 1024,
                description: "Memory in MiB.",
            },
            storage: {
                type: String,
                description: "PVE storage ID for the root filesystem.",
            },
            "disk-gb": {
                type: Integer,
                minimum: 1,
                default: 8,
                description: "Root filesystem size in GiB.",
            },
            bridge: {
                type: String,
                default: "vmbr0",
                description: "PVE bridge for eth0.",
            },
            password: {
                type: String,
                optional: true,
                description: "Optional root password.",
            },
            start: {
                type: Boolean,
                default: false,
                description: "Start the container after creation.",
            },
        },
    },
    returns: { type: RemoteUpid },
    access: {
        permission: &Permission::Privilege(&["resource", "{remote}"], PRIV_RESOURCE_MANAGE, false),
    },
)]
/// Create an LXC container on a PVE remote through the PDM backend.
#[allow(clippy::too_many_arguments)]
pub async fn create_lxc(
    remote: String,
    node: String,
    vmid: u32,
    hostname: String,
    ostemplate: String,
    cores: u64,
    memory: u64,
    storage: String,
    disk_gb: u64,
    bridge: String,
    password: Option<String>,
    start: bool,
) -> Result<RemoteUpid, Error> {
    validate_common(cores, memory, disk_gb, &storage, &bridge)?;
    if hostname.trim().is_empty() {
        bail!("hostname is required");
    }
    if ostemplate.trim().is_empty() {
        bail!("OS template is required");
    }

    let (remotes, _) = pdm_config::remotes::config()?;
    let remote_config = get_remote(&remotes, &remote)?;
    let client = crate::connection::make_raw_client(remote_config)?;

    let params = CreateLxcParams {
        vmid,
        hostname: hostname.trim().to_string(),
        ostemplate: ostemplate.trim().to_string(),
        cores,
        memory,
        rootfs: format!("{}:{}", storage.trim(), disk_gb),
        net0: format!("name=eth0,bridge={},ip=dhcp", bridge.trim()),
        unprivileged: true,
        start,
        password: password.filter(|value| !value.is_empty()),
    };

    let path = format!("/api2/extjs/nodes/{node}/lxc");
    let upid: pve_api_types::PveUpid = client.post(&path, &params).await?.expect_json()?.data;
    new_remote_upid(remote, upid).await
}
