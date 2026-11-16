// Restore, lock, and verify reference source repositories listed in
// reference-packages/manifest.json. These are evaluation sources only and are
// git-ignored; they are never part of the application build.
//
//   node scripts/reference-packages.mjs fetch [name...]   clone missing repos (shallow, no submodules)
//                                                         at the recorded SHA if one exists
//   node scripts/reference-packages.mjs lock              record checked-out SHAs into the manifest
//   node scripts/reference-packages.mjs verify            check every repo is present at its recorded SHA
//   node scripts/reference-packages.mjs status            print name / SHA / license-file table

import { spawnSync } from "node:child_process";
import { existsSync, readFileSync, readdirSync, writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const refsDir = join(root, "reference-packages");
const manifestPath = join(refsDir, "manifest.json");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));

const [command = "status", ...names] = process.argv.slice(2);
const selected = manifest.packages.filter((p) => names.length === 0 || names.includes(p.name));

function git(args, cwd) {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  return { ok: result.status === 0, out: `${result.stdout ?? ""}`.trim(), err: `${result.stderr ?? ""}`.trim() };
}

function headSha(dir) {
  const r = git(["rev-parse", "HEAD"], dir);
  return r.ok ? r.out : null;
}

function licenseFiles(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).filter((f) => /^(licen[cs]e|copying|notice)/i.test(f));
}

function fetchPackage(pkg) {
  const dir = join(refsDir, pkg.dir);
  if (existsSync(join(dir, ".git"))) {
    console.log(`= ${pkg.name}: already present at ${headSha(dir)?.slice(0, 12)}`);
    return true;
  }
  mkdirSync(dir, { recursive: true });
  if (pkg.sha) {
    // Fetch exactly the recorded commit, shallow.
    const steps = [
      ["init", "-q"],
      ["remote", "add", "origin", pkg.url],
      ["fetch", "-q", "--depth", "1", "origin", pkg.sha],
      ["checkout", "-q", "FETCH_HEAD"],
    ];
    for (const step of steps) {
      const r = git(step, dir);
      if (!r.ok) {
        console.error(`x ${pkg.name}: git ${step.join(" ")} failed\n  ${r.err}`);
        return false;
      }
    }
  } else {
    const r = git(["clone", "-q", "--depth", "1", "--no-recurse-submodules", pkg.url, dir], refsDir);
    if (!r.ok) {
      console.error(`x ${pkg.name}: clone failed\n  ${r.err}`);
      return false;
    }
  }
  console.log(`+ ${pkg.name}: ${headSha(dir)?.slice(0, 12)}`);
  return true;
}

let failed = 0;
switch (command) {
  case "fetch":
    for (const pkg of selected) if (!fetchPackage(pkg)) failed++;
    break;
  case "lock": {
    for (const pkg of selected) {
      const sha = headSha(join(refsDir, pkg.dir));
      if (sha) pkg.sha = sha;
      else console.warn(`! ${pkg.name}: not present, SHA unchanged`);
    }
    writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
    console.log("manifest.json updated");
    break;
  }
  case "verify":
    for (const pkg of selected) {
      const sha = headSha(join(refsDir, pkg.dir));
      const ok = sha && pkg.sha && sha === pkg.sha;
      if (!ok) failed++;
      console.log(`${ok ? "ok " : "BAD"} ${pkg.name.padEnd(20)} recorded=${pkg.sha?.slice(0, 12) ?? "none"} actual=${sha?.slice(0, 12) ?? "missing"}`);
    }
    break;
  case "status":
    for (const pkg of selected) {
      const dir = join(refsDir, pkg.dir);
      const sha = headSha(dir);
      console.log(`${pkg.name.padEnd(20)} ${(sha ?? "missing").slice(0, 12).padEnd(12)} ${licenseFiles(dir).join(", ")}`);
    }
    break;
  default:
    console.error(`Unknown command "${command}". Use fetch | lock | verify | status.`);
    process.exit(2);
}
if (failed) {
  console.error(`\n${failed} package(s) failed.`);
  process.exitCode = 1;
}
