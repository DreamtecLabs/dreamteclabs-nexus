SERVICES := \
	proxmox-datacenter-api.service \
	proxmox-datacenter-privileged-api.service \
	proxmox-datacenter-manager-banner.service \
	proxmox-datacenter-manager-daily-update.service \
	proxmox-datacenter-manager-daily-update.timer \
	nexus-blackbox-exporter.service \
	nexus-icmp-collector.service \

HELPERS := \
	nexus-domains-helper \

.PHONY: all
all:

.PHONY: install
install: $(SERVICES) $(HELPERS)
	install -d $(DESTDIR)/usr/lib/systemd/system
	install -m 0644 $(SERVICES) $(DESTDIR)/usr/lib/systemd/system
	install -d $(DESTDIR)/usr/libexec/proxmox
	install -m 0755 $(HELPERS) $(DESTDIR)/usr/libexec/proxmox
