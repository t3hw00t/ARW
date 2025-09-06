

<!-- ARW_HW_VIRTUALIZATION -->
### Virtualization & Accelerators

- **Native user‑mode (recommended)** for reliable GPU/NPU access (DirectML/CUDA/ROCm/CoreML/OpenVINO).
- **Windows Sandbox** supported via `sandbox/ARW.wsb` (ephemeral VM). Accelerator access may be limited.
- **WSL2 / full VMs**: accelerator pass‑through depends on vendor/runtime; ARW automatically falls back to CPU.
