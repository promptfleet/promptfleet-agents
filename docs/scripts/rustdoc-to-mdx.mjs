#!/usr/bin/env node
/**
 * rustdoc-to-mdx.mjs
 *
 * Converts rustdoc JSON (--output-format json) into Starlight-compatible MDX pages.
 *
 * Usage:
 *   node scripts/rustdoc-to-mdx.mjs <path-to-json> <output-dir> [--crate-slug <slug>]
 *
 * Example:
 *   node scripts/rustdoc-to-mdx.mjs structured_logging.json src/content/docs/api/structured-logging
 */

import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import { join, basename } from "node:path";

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------
const args = process.argv.slice(2);
if (args.length < 2) {
  console.error(
    "Usage: node rustdoc-to-mdx.mjs <json-path> <output-dir> [--crate-slug <slug>]"
  );
  process.exit(1);
}

const jsonPath = args[0];
const outDir = args[1];
const slugIdx = args.indexOf("--crate-slug");
const crateSlug =
  slugIdx !== -1 ? args[slugIdx + 1] : basename(jsonPath, ".json");

const doc = JSON.parse(readFileSync(jsonPath, "utf-8"));
const index = doc.index;
const paths = doc.paths;
mkdirSync(outDir, { recursive: true });

// ---------------------------------------------------------------------------
// Type rendering
// ---------------------------------------------------------------------------
function renderType(t) {
  if (!t) return "()";
  if (typeof t === "string") return t;

  if (t.generic) return t.generic;
  if (t.primitive) return t.primitive;

  if (t.resolved_path) {
    const rp = t.resolved_path;
    let name = shortPath(rp.path);
    if (rp.args?.angle_bracketed) {
      const ab = rp.args.angle_bracketed;
      const parts = ab.args
        .map((a) => {
          if (a.type) return renderType(a.type);
          if (a.lifetime) return a.lifetime;
          if (a.const) return renderConst(a.const);
          return "?";
        })
        .filter(Boolean);
      if (parts.length) name += `<${parts.join(", ")}>`;
    }
    return name;
  }

  if (t.borrowed_ref) {
    const br = t.borrowed_ref;
    const lt = br.lifetime ? `'${br.lifetime} ` : "";
    const m = br.is_mutable ? "mut " : "";
    return `&${lt}${m}${renderType(br.type)}`;
  }

  if (t.tuple) {
    if (t.tuple.length === 0) return "()";
    return `(${t.tuple.map(renderType).join(", ")})`;
  }

  if (t.slice) return `[${renderType(t.slice)}]`;

  if (t.array) return `[${renderType(t.array.type)}; ${t.array.len}]`;

  if (t.raw_pointer) {
    const m = t.raw_pointer.is_mutable ? "*mut " : "*const ";
    return m + renderType(t.raw_pointer.type);
  }

  if (t.qualified_path) {
    const qp = t.qualified_path;
    return `<${renderType(qp.self_type)} as ${renderType(qp.trait)}>::${qp.name}`;
  }

  if (t.dyn_trait) {
    const traits = (t.dyn_trait.traits || [])
      .map((b) => {
        if (b.trait?.path) return shortPath(b.trait.path);
        return "?";
      })
      .join(" + ");
    return `dyn ${traits}`;
  }

  if (t.function_pointer) {
    const fp = t.function_pointer;
    const ins = fp.sig.inputs.map(([n, ty]) => `${n}: ${renderType(ty)}`);
    const out = fp.sig.output ? ` -> ${renderType(fp.sig.output)}` : "";
    return `fn(${ins.join(", ")})${out}`;
  }

  if (t.impl_trait) {
    const bounds = t.impl_trait
      .map((b) => {
        if (b.trait_bound?.trait?.path) return shortPath(b.trait_bound.trait.path);
        return "?";
      })
      .join(" + ");
    return `impl ${bounds}`;
  }

  if (t.infer) return "_";

  return "?";
}

function shortPath(p) {
  if (!p) return "?";
  const parts = p.split("::");
  if (parts.length <= 2) return p;
  const crate_ = parts[0];
  if (crate_ === "crate" || crate_ === crateSlug.replace(/-/g, "_")) {
    return parts.slice(-1)[0];
  }
  if (["std", "core", "alloc"].includes(crate_)) {
    return parts.slice(-1)[0];
  }
  return parts.slice(-2).join("::");
}

function renderConst(c) {
  if (c?.expr) return c.expr;
  return "?";
}

// ---------------------------------------------------------------------------
// Signature rendering
// ---------------------------------------------------------------------------
function fnSignature(item) {
  const fn_ = item.inner.function;
  if (!fn_) return "";
  const header = fn_.header || {};
  const parts = [];
  if (header.is_const) parts.push("const ");
  if (header.is_async) parts.push("async ");
  if (header.is_unsafe) parts.push("unsafe ");
  parts.push("fn ");
  parts.push(item.name);

  const generics = renderGenerics(fn_.generics);
  parts.push(generics.params);

  const inputs = fn_.sig.inputs
    .map(([name, type]) => {
      if (name === "self") {
        if (
          type.borrowed_ref &&
          type.borrowed_ref.type?.generic === "Self"
        ) {
          return type.borrowed_ref.is_mutable ? "&mut self" : "&self";
        }
        return "self";
      }
      return `${name}: ${renderType(type)}`;
    })
    .join(", ");
  parts.push(`(${inputs})`);

  if (fn_.sig.output) {
    parts.push(` -> ${renderType(fn_.sig.output)}`);
  }

  parts.push(generics.where_);
  return parts.join("");
}

function renderGenerics(g) {
  if (!g) return { params: "", where_: "" };
  let params = "";
  if (g.params?.length) {
    const ps = g.params
      .map((p) => {
        if (p.name) return p.name;
        if (p.type) return renderType(p.type);
        return "?";
      })
      .filter(Boolean);
    if (ps.length) params = `<${ps.join(", ")}>`;
  }
  let where_ = "";
  if (g.where_predicates?.length) {
    const wps = g.where_predicates
      .map((wp) => {
        if (wp.bound_predicate) {
          const ty = renderType(wp.bound_predicate.type);
          const bounds = (wp.bound_predicate.bounds || [])
            .map((b) => {
              if (b.trait_bound?.trait?.path) return shortPath(b.trait_bound.trait.path);
              if (b.trait_bound?.trait) return renderType(b.trait_bound.trait);
              return "?";
            })
            .join(" + ");
          return `${ty}: ${bounds}`;
        }
        return null;
      })
      .filter(Boolean);
    if (wps.length) where_ = `\nwhere\n    ${wps.join(",\n    ")}`;
  }
  return { params, where_ };
}

// ---------------------------------------------------------------------------
// Item grouping helpers
// ---------------------------------------------------------------------------
function getItem(id) {
  return index[id] || null;
}

function itemKind(item) {
  return Object.keys(item.inner)[0];
}

function isPublic(item) {
  return item.visibility === "public" || item.visibility === "default";
}

function groupItems(itemIds) {
  const groups = {
    struct: [],
    enum: [],
    trait: [],
    function: [],
    type_alias: [],
    constant: [],
    macro: [],
    module: [],
    use: [],
  };
  for (const id of itemIds) {
    const item = getItem(id);
    if (!item || !isPublic(item)) continue;
    const kind = itemKind(item);
    if (kind === "use") {
      const target = item.inner.use;
      if (target?.id) {
        const resolved = getItem(target.id);
        if (resolved && isPublic(resolved)) {
          const rKind = itemKind(resolved);
          if (groups[rKind]) {
            groups[rKind].push({ ...resolved, _reexport_name: item.name });
          }
        }
      }
      continue;
    }
    if (groups[kind]) groups[kind].push(item);
  }
  return groups;
}

// ---------------------------------------------------------------------------
// MDX rendering
// ---------------------------------------------------------------------------
function escapeHtml(s) {
  return s.replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

function renderDocs(item) {
  if (!item.docs) return "";
  let docs = item.docs;
  docs = docs.replace(/```(?:rust,)?(?:ignore|no_run|compile_fail)/g, '```rust');
  return docs + "\n";
}

function renderStruct(item) {
  const lines = [];
  const name = item._reexport_name || item.name;
  lines.push(`### \`${name}\``);
  lines.push("");
  lines.push(renderDocs(item));

  const s = item.inner.struct;
  const fields = s.kind?.plain?.fields || [];
  const publicFields = fields
    .map(getItem)
    .filter((f) => f && isPublic(f));

  if (publicFields.length > 0) {
    lines.push("**Fields**\n");
    lines.push("| Field | Type | Description |");
    lines.push("|-------|------|-------------|");
    for (const f of publicFields) {
      const ty = escapeHtml(renderType(f.inner.struct_field));
      const doc = (f.docs || "").split("\n")[0];
      lines.push(`| \`${f.name}\` | \`${ty}\` | ${doc} |`);
    }
    lines.push("");
  }

  const methods = collectMethods(s.impls || []);
  if (methods.length > 0) {
    lines.push("**Methods**\n");
    for (const m of methods) {
      lines.push(`#### \`${m.name}\``);
      lines.push("");
      lines.push("```rust");
      lines.push(fnSignature(m));
      lines.push("```");
      lines.push("");
      lines.push(renderDocs(m));
    }
  }

  return lines.join("\n");
}

function renderEnum(item) {
  const lines = [];
  const name = item._reexport_name || item.name;
  lines.push(`### \`${name}\``);
  lines.push("");
  lines.push(renderDocs(item));

  const e = item.inner.enum;
  const variants = (e.variants || []).map(getItem).filter(Boolean);

  if (variants.length > 0) {
    lines.push("**Variants**\n");
    lines.push("| Variant | Description |");
    lines.push("|---------|-------------|");
    for (const v of variants) {
      const doc = (v.docs || "").split("\n")[0];
      const vFields = renderVariantFields(v);
      const display = vFields ? `${v.name}${vFields}` : v.name;
      lines.push(`| \`${display}\` | ${doc} |`);
    }
    lines.push("");
  }

  const methods = collectMethods(e.impls || []);
  if (methods.length > 0) {
    lines.push("**Methods**\n");
    for (const m of methods) {
      lines.push(`#### \`${m.name}\``);
      lines.push("");
      lines.push("```rust");
      lines.push(fnSignature(m));
      lines.push("```");
      lines.push("");
      lines.push(renderDocs(m));
    }
  }

  return lines.join("\n");
}

function renderVariantFields(variant) {
  const vInner = variant.inner?.variant;
  if (!vInner || !vInner.kind) return "";
  if (vInner.kind === "plain") return "";
  if (vInner.kind.tuple) {
    const fields = vInner.kind.tuple
      .map((id) => {
        if (!id) return "_";
        const f = getItem(id);
        return f ? renderType(f.inner.struct_field) : "?";
      })
      .join(", ");
    return `(${fields})`;
  }
  if (vInner.kind.struct) {
    return " { ... }";
  }
  return "";
}

function renderTrait(item) {
  const lines = [];
  const name = item._reexport_name || item.name;
  lines.push(`### \`${name}\``);
  lines.push("");
  lines.push(renderDocs(item));

  const t = item.inner.trait;
  const items = (t.items || []).map(getItem).filter(Boolean);
  const fns = items.filter((i) => itemKind(i) === "function");
  const types = items.filter((i) => itemKind(i) === "assoc_type");

  if (types.length > 0) {
    lines.push("**Associated Types**\n");
    for (const at of types) {
      lines.push(`- \`type ${at.name}\` — ${(at.docs || "").split("\n")[0]}`);
    }
    lines.push("");
  }

  if (fns.length > 0) {
    lines.push("**Required / Provided Methods**\n");
    for (const f of fns) {
      lines.push("```rust");
      lines.push(fnSignature(f));
      lines.push("```");
      lines.push("");
      lines.push(renderDocs(f));
    }
  }

  return lines.join("\n");
}

function renderFunction(item) {
  const lines = [];
  const name = item._reexport_name || item.name;
  lines.push(`### \`${name}\``);
  lines.push("");
  lines.push("```rust");
  lines.push(fnSignature(item));
  lines.push("```");
  lines.push("");
  lines.push(renderDocs(item));
  return lines.join("\n");
}

function renderMacro(item) {
  const lines = [];
  const name = item._reexport_name || item.name;
  lines.push(`### \`${name}!\``);
  lines.push("");
  lines.push(renderDocs(item));
  return lines.join("\n");
}

function renderConstant(item) {
  const lines = [];
  const name = item._reexport_name || item.name;
  const c = item.inner.constant;
  const ty = c?.type ? renderType(c.type) : "?";
  const val = c?.value || "";
  lines.push(`### \`${name}\``);
  lines.push("");
  lines.push("```rust");
  lines.push(`const ${name}: ${ty} = ${val};`);
  lines.push("```");
  lines.push("");
  lines.push(renderDocs(item));
  return lines.join("\n");
}

function renderTypeAlias(item) {
  const lines = [];
  const name = item._reexport_name || item.name;
  const ta = item.inner.type_alias;
  const ty = ta?.type ? renderType(ta.type) : "?";
  lines.push(`### \`${name}\``);
  lines.push("");
  lines.push("```rust");
  lines.push(`type ${name} = ${ty};`);
  lines.push("```");
  lines.push("");
  lines.push(renderDocs(item));
  return lines.join("\n");
}

function collectMethods(implIds) {
  const methods = [];
  const seen = new Set();
  for (const implId of implIds) {
    const impl_ = getItem(implId);
    if (!impl_ || itemKind(impl_) !== "impl") continue;
    const implInner = impl_.inner.impl;
    if (implInner.trait) continue; // skip trait impls (Display, Debug, etc.)
    for (const mId of implInner.items || []) {
      const m = getItem(mId);
      if (!m || !isPublic(m)) continue;
      if (itemKind(m) !== "function") continue;
      if (seen.has(m.name)) continue;
      seen.add(m.name);
      methods.push(m);
    }
  }
  return methods;
}

// ---------------------------------------------------------------------------
// Page generation
// ---------------------------------------------------------------------------
function generateModulePage(modItem, slug, parentTitle) {
  const mod_ = modItem.inner.module;
  const items = mod_.items || [];
  const groups = groupItems(items);

  const title = modItem.name;
  const description = (modItem.docs || "")
    .split("\n")
    .find((l) => l.trim().length > 0) || `${title} module`;

  const sections = [];

  sections.push(`---
title: "${title}"
description: "${description.replace(/"/g, '\\"').slice(0, 160)}"
---

import { Aside, Badge } from '@astrojs/starlight/components';

<Aside type="tip">Auto-generated from rustdoc JSON (format v${doc.format_version}). Crate version: ${doc.crate_version}.</Aside>
`);

  if (modItem.docs) {
    sections.push(modItem.docs);
    sections.push("");
  }

  if (groups.constant.length) {
    sections.push("## Constants\n");
    for (const item of groups.constant) sections.push(renderConstant(item));
  }

  if (groups.type_alias.length) {
    sections.push("## Type Aliases\n");
    for (const item of groups.type_alias) sections.push(renderTypeAlias(item));
  }

  if (groups.trait.length) {
    sections.push("## Traits\n");
    for (const item of groups.trait) sections.push(renderTrait(item));
  }

  if (groups.struct.length) {
    sections.push("## Structs\n");
    for (const item of groups.struct) sections.push(renderStruct(item));
  }

  if (groups.enum.length) {
    sections.push("## Enums\n");
    for (const item of groups.enum) sections.push(renderEnum(item));
  }

  if (groups.function.length) {
    sections.push("## Functions\n");
    for (const item of groups.function) sections.push(renderFunction(item));
  }

  if (groups.macro.length) {
    sections.push("## Macros\n");
    for (const item of groups.macro) sections.push(renderMacro(item));
  }

  return sections.join("\n");
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------
const rootItem = index[doc.root];
const rootMod = rootItem.inner.module;

// Generate crate root page (index.mdx)
const indexContent = generateModulePage(rootItem, "", "");
writeFileSync(join(outDir, "index.mdx"), indexContent);
console.log(`  wrote index.mdx (crate root)`);

// Generate per-module pages
for (const childId of rootMod.items) {
  const child = getItem(childId);
  if (!child || itemKind(child) !== "module") continue;
  if (!isPublic(child)) continue;

  const slug = child.name.replace(/_/g, "-");
  const content = generateModulePage(child, slug, rootItem.name);
  writeFileSync(join(outDir, `${slug}.mdx`), content);
  console.log(`  wrote ${slug}.mdx (module: ${child.name})`);
}

console.log(`\nDone! Generated API docs in ${outDir}`);
