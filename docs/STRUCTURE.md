

<!-- ARW_STRUCTURE_ISOLATION -->
### Packaging & Isolation Notes

- Use **process‑mode backends** first (e.g., `llama.cpp` server) for portability and zero native binding friction.
- Prefer **per‑user, self‑contained directories**; state lives in `%LOCALAPPDATA%\arw` or in the app folder (portable).
- **Mandatory CPU fallback** if an accelerator path fails; emit `FallbackApplied` events.
- **WASI plugin host** is Phase 2; ABI kept stable from day one.
