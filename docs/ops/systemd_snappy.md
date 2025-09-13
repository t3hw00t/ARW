# Systemd Snappy Overrides

Give the interactive control plane headroom under load via cgroup v2 weights.

Create an override drop‑in:

```
# /etc/systemd/system/agent-hub.service.d/snappy.conf
[Service]
CPUWeight=900
IOWeight=900
MemoryLow=256M
Restart=always
```

Reload and restart:

```
sudo systemctl daemon-reload
sudo systemctl restart agent-hub
```

Adjust per host as needed. See also `docs/ethics/SNAPPY_CHARTER.md`.

