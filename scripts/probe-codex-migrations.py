"""Build disposable Codex databases for the upstream migration CI check."""

import json
import re
import sqlite3
import sys
from pathlib import Path


def build(root: Path, upstream: Path, name: str, folder: str, baseline: dict):
    migrations = []
    for path in sorted((upstream / folder).glob("*.sql")):
        match = re.fullmatch(r"(\d+)_(.+)\.sql", path.name)
        if not match:
            raise ValueError(f"Unrecognized migration filename {path.name}")
        migrations.append((int(match[1]), path))
    base = baseline["migration"]
    if not any(version == base for version, _ in migrations):
        raise ValueError(f"Upstream {folder} no longer contains baseline {base}")
    db = sqlite3.connect(root / name)
    try:
        for kind in ("table", "index", "view", "trigger"):
            for obj in baseline["objects"]:
                if obj["type"] == kind:
                    db.execute(obj["sql"])
        for version, path in migrations:
            if version > base:
                print(f"{name}: applying {path.name}", flush=True)
                db.executescript(path.read_text(encoding="utf-8"))
            db.execute(
                "INSERT INTO _sqlx_migrations(version,description,success,checksum,execution_time) "
                "VALUES(?,?,1,X'00',0)",
                (version, path.stem),
            )
        db.commit()
        print(f"{name}: {base} -> {migrations[-1][0]}")
    finally:
        db.close()


if __name__ == "__main__":
    upstream, output = map(Path, sys.argv[1:])
    output.mkdir(exist_ok=False, parents=True)
    fixture = json.loads(Path("src-tauri/src/fixtures/schema-64.json").read_text())
    for name, folder in (("state_5.sqlite", "migrations"), ("thread_history_1.sqlite", "thread_history_migrations")):
        build(output, upstream, name, folder, fixture[name])
