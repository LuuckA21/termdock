#!/usr/bin/env node
// Updater manifest (latest.json) for the release workflow, plus the checks
// that keep a broken or mis-signed release from reaching users.
//
//   node scripts/updater-manifest.mjs check-config
//       plugins.updater in tauri.conf.json: valid public key, https endpoints,
//       requireSignedVersion on.
//   node scripts/updater-manifest.mjs verify <file> <file.sig> [version]
//       Verifies a minisign signature (as written by `tauri signer sign`)
//       against the public key in tauri.conf.json.
//   node scripts/updater-manifest.mjs build <tag> <dir>
//       <dir> holds the release's updater packages and their .sig files.
//       Verifies every package and writes <dir>/latest.json.
//
// Only Node built-ins: Ed25519 and BLAKE2b come from node:crypto.
import { createHash, createPublicKey, verify as edVerify } from "node:crypto";
import { readFileSync, readdirSync, writeFileSync } from "node:fs";
import { execFileSync } from "node:child_process";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const conf = JSON.parse(readFileSync(join(root, "src-tauri/tauri.conf.json"), "utf8"));
const updater = conf.plugins?.updater ?? {};

// Release asset suffix -> updater targets (`{os}-{arch}[-{bundle}]`) it serves.
// deb and rpm are left out on purpose: those installs are updated by the
// package manager (see src-tauri/src/updates.rs).
const PACKAGES = [
  { suffix: "_x64-setup.exe", targets: ["windows-x86_64", "windows-x86_64-nsis"] },
  { suffix: "_x64_en-US.msi", targets: ["windows-x86_64-msi"] },
  {
    suffix: ".app.tar.gz",
    targets: ["darwin-x86_64", "darwin-aarch64", "darwin-x86_64-app", "darwin-aarch64-app"],
  },
  { suffix: "_amd64.AppImage", targets: ["linux-x86_64", "linux-x86_64-appimage"] },
];

function fail(message) {
  console.error(`error: ${message}`);
  process.exit(1);
}

/** Tauri stores minisign files base64-encoded; returns their text lines. */
function minisignLines(base64, what) {
  const text = Buffer.from(base64.trim(), "base64").toString("utf8");
  const lines = text.split("\n").map((l) => l.replace(/\r$/, ""));
  if (!lines[0]?.startsWith("untrusted comment:")) fail(`${what} is not a minisign file`);
  return lines;
}

function publicKey() {
  if (typeof updater.pubkey !== "string" || !updater.pubkey.trim()) {
    fail("plugins.updater.pubkey is empty in src-tauri/tauri.conf.json");
  }
  const raw = Buffer.from(minisignLines(updater.pubkey, "the updater public key")[1] ?? "", "base64");
  if (raw.length !== 42 || raw.toString("latin1", 0, 2) !== "Ed") fail("the updater public key is not an Ed25519 minisign key");
  const der = Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), raw.subarray(10)]);
  return { keyId: raw.subarray(2, 10), key: createPublicKey({ key: der, format: "der", type: "spki" }) };
}

/** Key id as minisign prints it. */
function keyIdText(id) {
  return Buffer.from(id).reverse().toString("hex").toUpperCase();
}

/** Full minisign verification; returns the signed trusted comment. */
function verifySignature(data, sigBase64, pub, name) {
  const lines = minisignLines(sigBase64, `the signature of ${name}`);
  const sig = Buffer.from(lines[1] ?? "", "base64");
  const trusted = lines[2]?.startsWith("trusted comment: ") ? lines[2].slice("trusted comment: ".length) : null;
  const global = Buffer.from(lines[3] ?? "", "base64");
  if (sig.length !== 74 || trusted === null || global.length !== 64) fail(`malformed signature for ${name}`);
  if (!sig.subarray(2, 10).equals(pub.keyId)) {
    fail(`${name} is signed with key ${keyIdText(sig.subarray(2, 10))}, the app trusts ${keyIdText(pub.keyId)}`);
  }
  const algorithm = sig.toString("latin1", 0, 2);
  let message;
  if (algorithm === "ED") message = createHash("blake2b512").update(data).digest();
  else if (algorithm === "Ed") message = data;
  else fail(`unknown signature algorithm for ${name}`);
  if (!edVerify(null, message, pub.key, sig.subarray(10))) fail(`bad signature for ${name}`);
  const signedComment = Buffer.concat([sig.subarray(10), Buffer.from(trusted, "utf8")]);
  if (!edVerify(null, signedComment, pub.key, global)) fail(`bad trusted comment signature for ${name}`);
  return trusted;
}

function signedVersion(trusted) {
  return trusted
    .split("\t")
    .find((f) => f.startsWith("version:"))
    ?.slice("version:".length);
}

function checkConfig() {
  const pub = publicKey();
  const endpoints = updater.endpoints ?? [];
  if (!endpoints.length) fail("plugins.updater.endpoints is empty");
  for (const url of endpoints) if (!url.startsWith("https://")) fail(`updater endpoint is not https: ${url}`);
  if (updater.requireSignedVersion !== true) fail("plugins.updater.requireSignedVersion must be true");
  for (const flag of ["dangerousInsecureTransportProtocol", "dangerousAcceptInvalidCerts", "dangerousAcceptInvalidHostnames", "allowDowngrades"]) {
    if (updater[flag]) fail(`plugins.updater.${flag} must not be enabled`);
  }
  if (conf.bundle?.createUpdaterArtifacts !== true) fail("bundle.createUpdaterArtifacts must be true");
  console.log(`updater config ok (key ${keyIdText(pub.keyId)})`);
}

function build(tag, dir) {
  const version = tag.replace(/^v/, "");
  const repo = process.env.GITHUB_REPOSITORY;
  if (!repo) fail("GITHUB_REPOSITORY is not set");
  const pub = publicKey();
  const files = readdirSync(dir);
  const platforms = {};
  for (const { suffix, targets } of PACKAGES) {
    const matches = files.filter((f) => f.endsWith(suffix));
    if (matches.length !== 1) fail(`expected one *${suffix} in the release, found ${matches.length}`);
    const name = matches[0];
    if (!files.includes(`${name}.sig`)) fail(`${name}.sig is missing`);
    const signature = readFileSync(join(dir, `${name}.sig`), "utf8").trim();
    const trusted = verifySignature(readFileSync(join(dir, name)), signature, pub, name);
    const signed = signedVersion(trusted);
    if (signed !== version) fail(`${name} is signed for version ${signed ?? "(none)"}, expected ${version}`);
    const url = `https://github.com/${repo}/releases/download/${encodeURIComponent(tag)}/${encodeURIComponent(name)}`;
    for (const target of targets) platforms[target] = { signature, url };
    console.log(`verified ${name}`);
  }
  const notes = execFileSync(process.execPath, [join(root, "scripts/version.mjs"), "release-notes", version], {
    encoding: "utf8",
  }).trim();
  const manifest = { version, notes, pub_date: new Date().toISOString().replace(/\.\d{3}Z$/, "Z"), platforms };
  writeFileSync(join(dir, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`wrote latest.json for ${tag} (${Object.keys(platforms).length} targets)`);
}

const [command, ...args] = process.argv.slice(2);
switch (command) {
  case "check-config":
    checkConfig();
    break;
  case "verify": {
    const [file, sigFile, version] = args;
    if (!file || !sigFile) fail("usage: verify <file> <file.sig> [version]");
    const trusted = verifySignature(readFileSync(file), readFileSync(sigFile, "utf8"), publicKey(), file);
    if (version && signedVersion(trusted) !== version) fail(`${file} is not signed for version ${version}`);
    console.log(`signature ok: ${trusted}`);
    break;
  }
  case "build":
    if (args.length !== 2) fail("usage: build <tag> <dir>");
    build(...args);
    break;
  default:
    console.error("usage: updater-manifest.mjs check-config | verify <file> <file.sig> [version] | build <tag> <dir>");
    process.exit(2);
}
