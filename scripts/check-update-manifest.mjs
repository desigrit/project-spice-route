import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, writeFile, rm, readFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createManifest } from "./create-update-manifest.mjs";

test("the release feed requires every architecture and includes immutable URLs and checksums", async () => {
  const directory = await mkdtemp(join(tmpdir(), "spice-release-"));
  try {
    await assert.rejects(createManifest(directory, "1.6.8"));
    for (const name of ["Spice-Route-1.6.8-windows-x64-setup.exe", "Spice-Route-1.6.8-windows-arm64-setup.exe", "Spice-Route-1.6.8-macos-apple-silicon.app.tar.gz"]) {
      await writeFile(join(directory, name), "release fixture");
      await writeFile(join(directory, name + ".sig"), await readFile("src-tauri/core/tests/fixtures/update-signature.txt.sig"));
    }
    const feed = await createManifest(directory, "1.6.8");
    assert.equal(Object.keys(feed.platforms).length, 3);
    assert.match(feed.platforms["windows-aarch64"].url, /\/v1\.6\.8\/Spice-Route-1\.6\.8-windows-arm64-setup\.exe$/);
    assert.equal(feed.platforms["darwin-aarch64"].size, 15);
    assert.match(feed.platforms["windows-x86_64"].sha256, /^[a-f0-9]{64}$/);
    await assert.rejects(createManifest(directory, "1.6.8-beta"));
  } finally { await rm(directory, { recursive: true, force: true }); }
});
