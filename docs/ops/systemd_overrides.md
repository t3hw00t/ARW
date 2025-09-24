# Systemd Overrides (Interactive Performance)
Updated: 2025-09-16
Type: How‑to

Give the interactive control plane headroom under load via cgroup v2 weights.

Create an override drop‑in:

```
# /etc/systemd/system/arw-server@.service.d/interactive.conf
[Service]
CPUWeight=900
IOWeight=900
MemoryLow=256M
Restart=always
```

Reload and restart:

```
sudo systemctl daemon-reload
sudo systemctl restart arw-server@<unix-user>
```

Adjust per host as needed. See also [Interactive Performance](../guide/interactive_performance.md).
