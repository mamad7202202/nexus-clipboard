/**
 * Verifies the central security claim: a detected credential's plaintext must
 * not exist anywhere on disk.
 *
 * Two independent checks, because either alone could pass for the wrong reason:
 *   1. A raw byte scan of every file in the data directory (database, WAL,
 *      shared-memory, blobs). This catches a leak the SQL layer would hide.
 *   2. A full-text search through the app's own index, which catches a leak
 *      that a byte scan could miss if the text were stored tokenised.
 *
 * Run after `smoke.ps1`, which plants a known credential.
 */

import { DatabaseSync } from "node:sqlite";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const dataDir =
  process.env.NEXUS_DATA_DIR ??
  join(process.env.APPDATA ?? "", "dev.nexus.clipboard");

// The exact credential smoke.ps1 copies.
const SECRET = "ghp_smoketest" + "a".repeat(30);
const DISTINCTIVE = "smoketest" + "a".repeat(30);

function walk(dir) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) out.push(...walk(path));
    else out.push(path);
  }
  return out;
}

let failures = 0;

// --- 1. raw byte scan ------------------------------------------------------

console.log(`scanning ${dataDir}\n`);
const files = walk(dataDir);

for (const file of files) {
  const size = statSync(file).size;
  const bytes = readFileSync(file);
  // Latin1 preserves byte values, so a UTF-8 or UTF-16 ASCII run is still found.
  const asLatin1 = bytes.toString("latin1");
  const asUtf16 = bytes.toString("utf16le");

  const leaked = asLatin1.includes(DISTINCTIVE) || asUtf16.includes(DISTINCTIVE);
  const name = file.slice(dataDir.length + 1);

  console.log(
    `  ${leaked ? "LEAK" : "ok  "}  ${name.padEnd(28)} ${String(size).padStart(9)} bytes`,
  );
  if (leaked) failures++;
}

// --- 2. search the app's own index -----------------------------------------

const db = new DatabaseSync(join(dataDir, "history.db"), { readOnly: true });

const ftsHits = db
  .prepare("SELECT COUNT(*) AS n FROM items_fts WHERE items_fts MATCH ?")
  .get(`"${DISTINCTIVE}"`).n;

const rowHits = db
  .prepare("SELECT COUNT(*) AS n FROM items WHERE body LIKE ? OR preview LIKE ?")
  .get(`%${DISTINCTIVE}%`, `%${DISTINCTIVE}%`).n;

const secretRow = db
  .prepare(
    "SELECT preview, body IS NULL AS body_null, cipher IS NOT NULL AS has_cipher, encrypted, sensitive FROM items WHERE kind = 'secret' LIMIT 1",
  )
  .get();

console.log(`\nfts matches for the credential:  ${ftsHits}`);
console.log(`row matches for the credential:  ${rowHits}`);

if (ftsHits > 0) {
  console.log("  LEAK: the credential is searchable");
  failures++;
}
if (rowHits > 0) {
  console.log("  LEAK: the credential is in a plaintext column");
  failures++;
}

if (!secretRow) {
  console.log("\nFAIL: no secret entry was found — did smoke.ps1 run?");
  failures++;
} else {
  console.log("\nstored secret entry:");
  console.log(`  preview:    ${secretRow.preview}`);
  console.log(`  body NULL:  ${secretRow.body_null ? "yes" : "NO — LEAK"}`);
  console.log(`  ciphertext: ${secretRow.has_cipher ? "present" : "MISSING"}`);
  console.log(`  encrypted:  ${secretRow.encrypted}`);
  console.log(`  sensitive:  ${secretRow.sensitive}`);

  if (!secretRow.body_null) failures++;
  if (!secretRow.has_cipher) failures++;
  if (secretRow.preview.includes(DISTINCTIVE)) {
    console.log("  LEAK: the preview contains the credential");
    failures++;
  }
}

db.close();

console.log(
  failures === 0
    ? `\nPASS - the credential (${SECRET.length} chars) exists nowhere in plaintext`
    : `\nFAIL - ${failures} problem(s) found`,
);
process.exit(failures === 0 ? 0 : 1);
