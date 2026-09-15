// Read-only metadata inspection. Does not read chat content or modify databases.
import { DatabaseSync } from 'node:sqlite';
import { createHash } from 'node:crypto';
import path from 'node:path';
const home = process.argv[2];
if (!home) throw new Error('Pass the Codex data directory.');
const signatures = [];
const migrations = {};
for (const name of ['state_5.sqlite', 'thread_history_1.sqlite']) {
  const db = new DatabaseSync(path.join(home, name), { readOnly: true });
  try {
    migrations[name] = db.prepare('SELECT max(version) AS version FROM _sqlx_migrations WHERE success = 1').get().version;
    signatures.push(db.prepare("SELECT name, coalesce(sql, '') AS sql FROM sqlite_master WHERE type IN ('table','index') AND name NOT LIKE 'sqlite_%' ORDER BY type, name").all().map(row => `${row.name}:${row.sql}`).join('\n'));
  } finally { db.close(); }
}
console.log(JSON.stringify({ migrations, fingerprint: createHash('sha256').update(signatures.join('\n')).digest('hex') }, null, 2));
