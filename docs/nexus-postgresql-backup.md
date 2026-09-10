# Nexus PostgreSQL Backup Strategy

This document records the operational backup decisions for the shared `postgres-01` PostgreSQL service used by Nexus and other DreamtecLabs workloads.

## Current decision

The current production strategy is **hourly logical backups**, implemented with PostgreSQL-native dump tooling and a systemd timer. We are deliberately **not introducing pgBackRest at this stage**.

The reason is proportionality: the current databases are small enough that a logical backup every hour is operationally simple, transparent and inexpensive. pgBackRest is a strong option for larger PostgreSQL estates, physical backups, WAL archiving and point-in-time recovery (PITR), but those capabilities add configuration and operational complexity that we do not currently need.

This is an explicit architecture decision, not a statement that logical dumps are universally better than pgBackRest.

## Recovery objectives

With the current hourly schedule, the practical database-level **RPO is up to approximately one hour** between successful logical backups. Recovery time depends on database size and restore throughput; for the current small databases, a logical restore is considered acceptable.

The strategy must be revisited if either of these objectives stops being acceptable.

## Scheduling

Logical backups are triggered by systemd using:

- `postgres-logical-backup.service`
- `postgres-logical-backup.timer`

The timer runs every hour at minute `15`:

```ini
[Timer]
OnCalendar=*-*-* *:15:00
Persistent=true
RandomizedDelaySec=0
Unit=postgres-logical-backup.service
```

`Persistent=true` is intentional: if the server is unavailable at a scheduled execution time, systemd should trigger the missed run after the timer becomes active again.

The systemd timer is the scheduler of record. Do not add a parallel cron entry for the same backup job.

## Backup scope

The logical backup job should protect all application databases that live on `postgres-01`, not only the Nexus database. The implementation should remain database-aware so an individual application database can be restored without requiring a full PostgreSQL instance rollback.

Global PostgreSQL objects that are not contained in a per-database dump — especially roles and relevant grants — must also be protected as part of the logical-backup set.

Backup artifacts must not contain application credentials in filenames, logs or repository content.

## Retention and storage

Logical dumps are recovery data, not temporary build artifacts. They must live outside PostgreSQL's live data directory and must not be committed to Git.

Retention should provide enough hourly recovery points to cover operational mistakes that are not discovered immediately. Retention policy and the final secondary/off-host copy location must be treated as operational configuration and documented once verified on `postgres-01` rather than guessed in this repository.

A backup that exists only on the same filesystem as the live database does **not** protect against loss of that filesystem or host. Off-host protection remains a required resilience layer even though pgBackRest is not being adopted now.

## Validation

A successful systemd exit code alone is not sufficient evidence of recoverability. Operations should validate:

1. the timer is active and has a future trigger;
2. the latest backup job completed successfully;
3. expected dump files were created and are non-empty;
4. retention cleanup is operating as intended;
5. restores are tested periodically into a disposable PostgreSQL database or instance.

Useful operational checks include:

```bash
systemctl list-timers postgres-logical-backup.timer --all
systemctl status postgres-logical-backup.timer --no-pager
systemctl status postgres-logical-backup.service --no-pager
journalctl -u postgres-logical-backup.service --since today --no-pager
```

Restore testing must never overwrite a production database merely to prove that a backup can be read.

## Why not pgBackRest now?

pgBackRest remains the preferred upgrade path when the environment needs capabilities that logical dumps do not efficiently provide. We should reconsider it when one or more of the following becomes true:

- the databases become large enough that hourly dumps materially load PostgreSQL or storage;
- restore time from logical dumps becomes unacceptable;
- the required RPO becomes materially lower than one hour;
- point-in-time recovery is required;
- continuous WAL archiving is required;
- physical full/differential/incremental backup becomes operationally preferable;
- backup repositories need stronger built-in retention, integrity and multi-repository management.

At that point, the migration should be designed as a deliberate physical-backup/PITR architecture rather than layering pgBackRest on top of the current process without a recovery requirement.

## Relationship to Proxmox/PBS backups

VM/LXC or storage-level backups are complementary to PostgreSQL logical backups. They protect different failure modes and recovery scopes. Infrastructure snapshots/backups must not be considered a replacement for database-consistent logical recovery points, and logical dumps must not be considered a replacement for host-level disaster recovery.

The target defense-in-depth model is therefore:

`PostgreSQL logical recovery points + off-host copy + infrastructure/PBS backup`

## Monitoring requirement

Backup health should ultimately be visible through the Nexus/SigNoz observability path. At minimum, monitoring should detect stale backups and failed backup executions rather than relying on an operator to inspect systemd manually.

Until that integration is implemented and verified, systemd status, journal output and the age of the latest valid backup artifact remain the operational source of truth.

## Decision log

**2026-09-08:** keep the PostgreSQL backup architecture simple and use hourly logical dumps. Do not deploy pgBackRest yet. Reassess when database size, RPO/RTO or PITR requirements justify physical backups and WAL archiving.
