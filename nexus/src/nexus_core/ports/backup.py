from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class BackupSnapshot:
    remote: str
    datastore: str
    guest_type: str  # "ct" | "vm", PBS's own vocabulary
    vmid: str
    comment: str
    timestamp_epoch: int
    size_bytes: int | None
    protected: bool


@dataclass(frozen=True, slots=True)
class GuestBackupStatus:
    resource_id: str
    name: str
    vmid: int
    remote: str
    type: str  # "pve-lxc" | "pve-qemu"
    last_backup: BackupSnapshot | None
    backup_count: int
    stale: bool


@dataclass(frozen=True, slots=True)
class OrphanBackup:
    """A snapshot whose guest no longer exists in the current inventory --
    either destroyed without decommission cleaning up its backups, or backed
    up from a remote/vmid Nexus doesn't otherwise know about."""

    remote: str
    datastore: str
    guest_type: str
    vmid: str
    last_backup: BackupSnapshot
    backup_count: int


@dataclass(frozen=True, slots=True)
class BackupOverview:
    guests: tuple[GuestBackupStatus, ...]
    orphans: tuple[OrphanBackup, ...]
    datastore_errors: tuple[str, ...]
    stale_after_hours: int
