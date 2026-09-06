# Keep Nexus runtime service units in this manifest so package CI tracks their changes.
SERVICES := \
	proxmox-datacenter-api.service \
	proxmox-datacenter-privileged-api.service \
	proxmox-datacenter-manager-banner.service \
	proxmox-datacenter-manager-daily-update.service \
	proxmox-datacenter-manager-daily-update.timer \
	nexus-icmp-collector.service \
	nexus-icmp-ping-policy.service \

HELPERS := \
	nexus-domains-helper \

.PHONY: all
all:

.PHONY: install
install: $(SERVICES) $(HELPERS) nexus-icmp-collector-runtime.yaml prometheus-blackbox-exporter-nexus.conf
	install -d $(DESTDIR)/usr/lib/systemd/system
	install -m 0644 $(SERVICES) $(DESTDIR)/usr/lib/systemd/system
	install -d $(DESTDIR)/usr/lib/proxmox
	install -m 0644 nexus-icmp-collector-runtime.yaml \
		$(DESTDIR)/usr/lib/proxmox/nexus-icmp-collector-runtime.yaml
	install -d $(DESTDIR)/usr/lib/systemd/system/prometheus-blackbox-exporter.service.d
	install -m 0644 prometheus-blackbox-exporter-nexus.conf \
		$(DESTDIR)/usr/lib/systemd/system/prometheus-blackbox-exporter.service.d/nexus-icmp.conf
	install -d $(DESTDIR)/usr/libexec/proxmox
	install -m 0755 $(HELPERS) $(DESTDIR)/usr/libexec/proxmox
