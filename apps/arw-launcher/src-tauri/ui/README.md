# Launcher UI Sources

The JavaScript files in this directory (`common.js`, `hub.js`, etc.) are the hand-written source for the Home and workspace pages that Tauri serves. We do not bundle or minify them during build—`scripts/build.{sh,ps1}` copies the files verbatim into the app package.

Editing tips:

- Update these files directly; there is no separate build step or transpiler output to chase.
- Shared helpers live in `common.js`. Page-specific logic stays with its matching `{name}.html` and `{name}.css`.
- When you add or rename a page, keep the triplet (`.html`, `.css`, `.js`) together so the packaging scripts pick it up automatically.

If you ever introduce a build pipeline (TypeScript, Vite, etc.), document the new source-of-truth and relocate generated assets under `apps/arw-launcher/src-tauri/gen/` so assistants do not edit bundled output by mistake.
