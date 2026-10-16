#!/usr/bin/env node
// Single entry point for the app version, which lives in four places:
// package.json (+ package-lock.json), Cargo.toml ([workspace.package]),
// Cargo.lock and src-tauri/tauri.conf.json.
//
// The changelog exists in English (CHANGELOG.md) and Italian (CHANGELOG.it.md);
// both are kept in step.
//
//   node scripts/version.mjs check              all files agree, both changelogs have the entry
//   node scripts/version.mjs get                print the current version
//   node scripts/version.mjs set 0.2.0          update every file and roll the "Unreleased"
//                                               section of both changelogs into 0.2.0
//   node scripts/version.mjs changelog [v] [it] print one changelog section (English by default)
//   node scripts/version.mjs release-notes [v]  English section followed by the Italian one
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const file = (p) => join(root, p);
const read = (p) => readFileSync(file(p), "utf8");
const SEMVER = /^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/;
const CARGO_PACKAGES = ["termdock", "termdock-core"];

function versions() {
  const pkg = JSON.parse(read("package.json")).version;
  const lock = JSON.parse(read("package-lock.json"));
  const tauri = JSON.parse(read("src-tauri/tauri.conf.json")).version;
  const cargo = read("Cargo.toml").match(/\[workspace\.package\][^[]*?\nversion = "([^"]+)"/)?.[1];
  const cargoLock = Object.fromEntries(
    CARGO_PACKAGES.map((name) => [
      `Cargo.lock (${name})`,
      read("Cargo.lock").match(new RegExp(`\\[\\[package\\]\\]\\nname = "${name}"\\nversion = "([^"]+)"`))?.[1],
    ]),
  );
  return {
    "package.json": pkg,
    "package-lock.json": lock.version,
    "package-lock.json (root package)": lock.packages?.[""]?.version,
    "Cargo.toml": cargo,
    ...cargoLock,
    "src-tauri/tauri.conf.json": tauri,
  };
}

function current() {
  return JSON.parse(read("src-tauri/tauri.conf.json")).version;
}

const CHANGELOGS = { en: "CHANGELOG.md", it: "CHANGELOG.it.md" };

function changelogSection(version, lang = "en") {
  const lines = read(CHANGELOGS[lang]).split("\n");
  const start = lines.findIndex((l) => l.startsWith(`## [${version}]`));
  if (start < 0) return null;
  const end = lines.findIndex((l, i) => i > start && l.startsWith("## ["));
  return lines
    .slice(start + 1, end < 0 ? undefined : end)
    .filter((l) => !/^\[[^\]]+\]: /.test(l))
    .join("\n")
    .trim();
}

function check() {
  const all = versions();
  const expected = current();
  let ok = true;
  for (const [where, v] of Object.entries(all)) {
    if (v !== expected) {
      console.error(`✗ ${where}: ${v ?? "missing"} (expected ${expected})`);
      ok = false;
    }
  }
  if (!SEMVER.test(expected)) {
    console.error(`✗ ${expected} is not a semantic version`);
    ok = false;
  }
  for (const [lang, name] of Object.entries(CHANGELOGS)) {
    if (!changelogSection(expected, lang)) {
      console.error(`✗ ${name} has no "## [${expected}]" section`);
      ok = false;
    }
  }
  // Both languages must list the same releases.
  const headings = (name) => [...read(name).matchAll(/^## \[([^\]]+)\]/gm)].map((m) => m[1]).join(",");
  if (headings(CHANGELOGS.en) !== headings(CHANGELOGS.it)) {
    console.error(`✗ ${CHANGELOGS.en} and ${CHANGELOGS.it} list different versions`);
    ok = false;
  }
  if (!ok) process.exit(1);
  console.log(`✓ version ${expected} is consistent`);
}

function set(next) {
  if (!SEMVER.test(next ?? "")) {
    console.error("usage: version.mjs set X.Y.Z");
    process.exit(2);
  }
  const prev = current();
  if (next === prev) {
    console.error(`already at ${next}`);
    process.exit(1);
  }
  for (const [lang, name] of Object.entries(CHANGELOGS)) {
    if (!changelogSection("Unreleased", lang)) {
      console.error(`${name}: describe the changes under "## [Unreleased]" before releasing`);
      process.exit(1);
    }
  }

  const tauri = JSON.parse(read("src-tauri/tauri.conf.json"));
  tauri.version = next;
  writeFileSync(file("src-tauri/tauri.conf.json"), `${JSON.stringify(tauri, null, 2)}\n`);

  const cargo = read("Cargo.toml").replace(
    /(\[workspace\.package\][^[]*?\nversion = ")[^"]+(")/,
    `$1${next}$2`,
  );
  writeFileSync(file("Cargo.toml"), cargo);

  // npm and cargo rewrite their own lock files.
  execFileSync("npm", ["version", next, "--no-git-tag-version", "--allow-same-version"], { cwd: root, stdio: "inherit" });
  execFileSync("cargo", ["update", "--workspace", "--offline"], { cwd: root, stdio: "inherit" });

  // Changelogs: "Unreleased" becomes the new version, a fresh empty one goes on top.
  const date = new Date().toISOString().slice(0, 10);
  const repo = "https://github.com/LuuckA21/termdock";
  for (const name of Object.values(CHANGELOGS)) {
    const changelog = read(name)
      .replace(/^## \[Unreleased\]\n/m, `## [Unreleased]\n\n## [${next}] - ${date}\n`)
      .replace(
        /^\[Unreleased\]: .*$/m,
        `[Unreleased]: ${repo}/compare/v${next}...HEAD\n[${next}]: ${repo}/compare/v${prev}...v${next}`,
      );
    writeFileSync(file(name), changelog);
  }

  check();
}

const [cmd, arg] = process.argv.slice(2);
switch (cmd) {
  case "check":
    check();
    break;
  case "get":
    console.log(current());
    break;
  case "set":
    set(arg);
    break;
  case "changelog": {
    const section = changelogSection(arg ?? current(), process.argv[4] === "it" ? "it" : "en");
    if (section === null) process.exit(1);
    console.log(section);
    break;
  }
  case "release-notes": {
    const v = arg ?? current();
    const en = changelogSection(v, "en");
    const it = changelogSection(v, "it");
    if (!en || !it) process.exit(1);
    console.log(`${en}\n\n---\n\n**Italiano**\n\n${it.replace(/^### /gm, "#### ")}`);
    break;
  }
  default:
    console.error("usage: version.mjs check | get | set X.Y.Z | changelog [X.Y.Z] [it] | release-notes [X.Y.Z]");
    process.exit(2);
}
