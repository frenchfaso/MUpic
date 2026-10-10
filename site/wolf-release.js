const SHA256 = /^[a-f0-9]{64}$/;
const LIMITS = { "/wolf.prg": 65538, "/wolf.reu": 16777216, "/wolf.cfg": 1048576 };

function validateFiles(files, revision) {
  if (!Array.isArray(files) || files.length !== 3) throw new Error("Invalid WOLF64U file list.");
  const seen = new Set();
  return files.map((file) => {
    if (!file || !Object.hasOwn(LIMITS, file.path) || seen.has(file.path)) throw new Error("Invalid WOLF64U file path.");
    seen.add(file.path);
    const expected = revision ? `wolf/releases/${revision}${file.path}` : `data${file.path}`;
    if (file.url !== expected || !Number.isSafeInteger(file.bytes) || file.bytes <= 0
        || file.bytes > LIMITS[file.path] || typeof file.sha256 !== "string" || !SHA256.test(file.sha256)) throw new Error("Invalid WOLF64U file metadata.");
    return Object.freeze({ path: file.path, url: file.url, bytes: file.bytes, sha256: file.sha256 });
  });
}

// Read once per launch. URLs belong to an immutable release, so a later
// publication cannot mix this launch's PRG, REU and settings.
export async function readWolfRelease(fallback, fetcher = fetch) {
  const response = await fetcher("wolf/latest.json", { cache: "no-store" });
  if (response.status === 404) {
    return Object.freeze({ schema: 1, revision: null, published_at: null, source: "snapshot",
      wolf: Object.freeze(validateFiles(fallback, null)) });
  }
  if (!response.ok) throw new Error(`WOLF64U latest build: HTTP ${response.status}`);
  const release = await response.json();
  if (release?.schema !== 1 || typeof release.revision !== "string" || !SHA256.test(release.revision) || !Number.isSafeInteger(release.published_at)
      || release.published_at < 0) throw new Error("Invalid WOLF64U release manifest.");
  return Object.freeze({ schema: 1, revision: release.revision, published_at: release.published_at, source: "published",
    wolf: Object.freeze(validateFiles(release.wolf, release.revision)) });
}

export async function verifyWolfAsset(entry, bytes, subtle = globalThis.crypto.subtle) {
  if (bytes.byteLength !== entry.bytes) throw new Error(`WOLF64U ${entry.path}: size mismatch.`);
  const digest = new Uint8Array(await subtle.digest("SHA-256", bytes));
  const sha256 = Array.from(digest, (byte) => byte.toString(16).padStart(2, "0")).join("");
  if (sha256 !== entry.sha256) throw new Error(`WOLF64U ${entry.path}: SHA-256 mismatch.`);
}
