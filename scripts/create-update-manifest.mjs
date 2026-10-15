import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

// All three packages must exist before publishing one stable update feed.
export async function createManifest(directory, version, publishedAt = new Date().toISOString()) {
  if (!/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) throw new Error("Expected a stable semantic version.");
  const names = {
    "windows-x86_64": `Spice-Route-${version}-windows-x64-setup.exe`,
    "windows-aarch64": `Spice-Route-${version}-windows-arm64-setup.exe`,
    "darwin-aarch64": `Spice-Route-${version}-macos-apple-silicon.app.tar.gz`,
  };
  const platforms = {};
  for (const [platform, name] of Object.entries(names)) {
    const bytes = await readFile(join(directory, name));
    const signature = (await readFile(join(directory, `${name}.sig`), "utf8")).trim();
    if (bytes.length === 0 || bytes.length > 512 * 1024 * 1024 || !/^[A-Za-z0-9+/=]+$/.test(signature) || signature.length < 100)
      throw new Error(`Missing or invalid signed package: ${name}`);
    platforms[platform] = {
      url: `https://github.com/desigrit/project-spice-route/releases/download/v${version}/${name}`,
      signature, sha256: createHash("sha256").update(bytes).digest("hex"), size: bytes.length,
    };
  }
  return { version, notes: `Spice Route ${version}`, pub_date: publishedAt, platforms };
}

if (process.argv[1]?.endsWith("create-update-manifest.mjs")) {
  const [, , directory, version] = process.argv;
  const manifest = await createManifest(directory, version);
  await writeFile(join(directory, "latest.json"), JSON.stringify(manifest, null, 2) + "\n");
  console.log(`Prepared signed update feed for ${Object.keys(manifest.platforms).join(", ")}.`);
}
