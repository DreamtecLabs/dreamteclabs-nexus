from __future__ import annotations

import asyncio
import time

from nexus_core.ports.backup import BackupOverview, BackupSnapshot, GuestBackupStatus, OrphanBackup


class BackupService:
    """Read-only visibility into PBS backup coverage: per-guest last-backup
    status plus any snapshot that no longer matches a guest in inventory.

    Deliberately status-only for now -- no "run a backup now" or restore
    action lives here yet. Whether a job is *configured* in PVE isn't
    answerable without a PDM endpoint that doesn't exist yet
    (GET /cluster/backup returns 404); what actually landed in PBS is the
    stronger signal anyway, since a configured-but-failing job is worse
    than no job at all.
    """

    def __init__(self, infrastructure_service, pdm, *, stale_after_hours: int = 48) -> None:
        self._infrastructure = infrastructure_service
        self._pdm = pdm
        self._stale_after_hours = max(1, stale_after_hours)

    async def overview(self) -> BackupOverview:
        snapshot = await self._infrastructure.list_resources()
        guests = [resource for resource in snapshot.resources if resource.is_guest]
        datastores = sorted({(resource.remote, resource.name) for resource in snapshot.resources if resource.type == "pbs-datastore"})

        results = await asyncio.gather(
            *(self._pdm.pbs_snapshots(remote, datastore) for remote, datastore in datastores),
            return_exceptions=True,
        )
        backups_by_guest: dict[tuple[str, str], list[BackupSnapshot]] = {}
        errors: list[str] = []
        for (remote, datastore), result in zip(datastores, results):
            if isinstance(result, BaseException):
                errors.append(f"{remote}/{datastore}: {type(result).__name__}")
                continue
            for raw in result:
                parsed = self._parse_snapshot(remote, datastore, raw)
                if parsed is not None:
                    backups_by_guest.setdefault((parsed.guest_type, parsed.vmid), []).append(parsed)

        now = time.time()
        guest_keys: set[tuple[str, str]] = set()
        guest_statuses: list[GuestBackupStatus] = []
        for guest in guests:
            key = ("ct" if guest.type == "pve-lxc" else "vm", str(guest.vmid))
            guest_keys.add(key)
            snapshots = backups_by_guest.get(key, ())
            latest = max(snapshots, key=lambda item: item.timestamp_epoch, default=None)
            stale = latest is None or (now - latest.timestamp_epoch) > self._stale_after_hours * 3600
            guest_statuses.append(GuestBackupStatus(resource_id=guest.id, name=guest.name, vmid=guest.vmid, remote=guest.remote, type=guest.type, last_backup=latest, backup_count=len(snapshots), stale=stale))
        guest_statuses.sort(key=lambda status: (not status.stale, status.name.casefold()))

        orphans = [
            OrphanBackup(remote=snapshots[0].remote, datastore=snapshots[0].datastore, guest_type=key[0], vmid=key[1], last_backup=max(snapshots, key=lambda item: item.timestamp_epoch), backup_count=len(snapshots))
            for key, snapshots in backups_by_guest.items()
            if key not in guest_keys
        ]
        orphans.sort(key=lambda orphan: orphan.last_backup.timestamp_epoch, reverse=True)

        return BackupOverview(guests=tuple(guest_statuses), orphans=tuple(orphans), datastore_errors=tuple(errors), stale_after_hours=self._stale_after_hours)

    @staticmethod
    def _parse_snapshot(remote: str, datastore: str, raw: object) -> BackupSnapshot | None:
        if not isinstance(raw, dict):
            return None
        try:
            guest_type = str(raw["backup-type"])
            vmid = str(raw["backup-id"])
            timestamp_epoch = int(raw["backup-time"])
        except (KeyError, TypeError, ValueError):
            return None
        size = raw.get("size")
        return BackupSnapshot(
            remote=remote,
            datastore=datastore,
            guest_type=guest_type,
            vmid=vmid,
            comment=str(raw.get("comment") or ""),
            timestamp_epoch=timestamp_epoch,
            size_bytes=size if isinstance(size, int) else None,
            protected=bool(raw.get("protected", False)),
        )
