/**
 * Reads the live history database and prints what is in it.
 *
 * Used by the smoke test, and handy on its own for checking what the app
 * actually captured. Opens read-only so it can never disturb the running app,
 * and does read the WAL — under WAL mode the main file stays tiny until a
 * checkpoint, so file size alone tells you nothing.
 */

import { DatabaseSync } from "node:sqlite";
import { join } from "node:path";

const dataDir =
  process.env.NEXUS_DATA_DIR ??
  join(process.env.APPDATA ?? "", "dev.nexus.clipboard");
const dbPath = join(dataDir, "history.db");

let db;
try {
  db = new DatabaseSync(dbPath, { readOnly: true });
} catch (e) {
  console.error(`cannot open ${dbPath}: ${e.message}`);
  process.exit(2);
}

const one = (sql) => db.prepare(sql).get();
const all = (sql) => db.prepare(sql).all();

const { n: total } = one("SELECT COUNT(*) AS n FROM items WHERE deleted_at IS NULL");
const { v: schema } = one("PRAGMA user_version") ?? { v: "?" };

console.log(`database:  ${dbPath}`);
console.log(`schema:    v${Object.values(one("PRAGMA user_version"))[0]}`);
console.log(`entries:   ${total}`);

if (total > 0) {
  const kinds = all(
    "SELECT kind, COUNT(*) AS n FROM items WHERE deleted_at IS NULL GROUP BY kind ORDER BY n DESC",
  );
  console.log(`kinds:     ${kinds.map((k) => `${k.kind}=${k.n}`).join(", ")}`);

  const encrypted = one(
    "SELECT COUNT(*) AS n FROM items WHERE encrypted = 1 AND deleted_at IS NULL",
  ).n;
  console.log(`encrypted: ${encrypted}`);

  console.log("\nmost recent:");
  for (const row of all(
    `SELECT kind, preview, source_app, encrypted
       FROM items WHERE deleted_at IS NULL
      ORDER BY updated_at DESC LIMIT 8`,
  )) {
    const preview = row.preview.slice(0, 62).replace(/\s+/g, " ");
    const lock = row.encrypted ? " 🔒" : "";
    console.log(`  [${row.kind.padEnd(6)}] ${preview}${lock}   (${row.source_app ?? "?"})`);
  }

  // Prove the search index is actually populated and queryable.
  const indexed = one("SELECT COUNT(*) AS n FROM items_fts").n;
  console.log(`\nfts rows:  ${indexed}`);
}

db.close();

// Exit code carries the count so a shell caller can assert on it.
process.exit(total > 0 ? 0 : 1);
