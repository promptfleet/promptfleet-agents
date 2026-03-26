# PromptFleet Agents — documentation site

Starlight (Astro) site for the [`promptfleet-agents`](https://github.com/promptfleet/promptfleet-agents) repo. Source: `src/content/docs/`.

**Run every Yarn command from this `docs/` directory.** The repo root has **no** `package.json`; if you run `yarn` there, your global **Yarn 1** runs and may create a stray root `yarn.lock` / `node_modules`.

Use **Node 20+** and enable Corepack so `yarn` matches `package.json`’s `packageManager` (Yarn 4):

```bash
cd docs
corepack enable
yarn install
yarn dev
```

Use `yarn install --immutable` only in CI (or when you want to enforce an unchanged lockfile). For day‑to‑day local work, `yarn install` is enough.

If **`yarn -v`** or **`yarn install`** fails with **“Unknown Syntax”** / **“Ambiguous Syntax”** and the log mentions **`While running --non-interactive`**, check **`alias yarn`** — a common pattern is **`yarn: aliased to yarn --non-interactive`**, which breaks Yarn 4. See **Troubleshooting** in [Getting started](src/content/docs/getting-started.mdx). Workaround: **`command yarn install`** or **`unalias yarn`**. Quick check: `node -p "require('./package.json').packageManager"`.

Production build output: `dist/` (configured with `site` + `base` for GitHub Pages).
