export const DOWNLOAD_ORIGIN = "https://wisp-science.sfl.bio";
export const RELEASES = "https://github.com/xuzhougeng/wisp-science/releases";
export const PACKAGES = [
  { id: "windows-x64-exe", os: "windows", arch: "x64", format: "exe", suffix: "x64-setup.exe" },
  { id: "windows-x64-msi", os: "windows", arch: "x64", format: "msi", suffix: "x64_en-US.msi" },
  { id: "macos-arm64-dmg", os: "macos", arch: "arm64", format: "dmg", suffix: "aarch64.dmg" },
  { id: "macos-x64-dmg", os: "macos", arch: "x64", format: "dmg", suffix: "x64.dmg" },
  { id: "linux-x64-deb", os: "linux", arch: "x64", format: "deb", suffix: "amd64.deb" },
  { id: "linux-arm64-deb", os: "linux", arch: "arm64", format: "deb", suffix: "arm64.deb" },
  { id: "linux-x64-appimage", os: "linux", arch: "x64", format: "appimage", suffix: "amd64.AppImage" },
  { id: "linux-arm64-appimage", os: "linux", arch: "arm64", format: "appimage", suffix: "aarch64.AppImage" },
];

export function validTag(tag) {
  return typeof tag === "string" && /^v\d+\.\d+\.\d+$/.test(tag);
}

export function assetName(tag, pkg) {
  return `wisp-science_${tag.slice(1)}_${pkg.suffix}`;
}

export function downloadKey(tag, name, sha256) {
  return `releases/${tag}/${sha256}/${name}`;
}

export function validDownloadKey(key) {
  const [prefix, tag, digest, name, ...rest] = key.split("/");
  return rest.length === 0 && prefix === "releases" && validTag(tag) &&
    /^[a-f0-9]{64}$/.test(digest) && PACKAGES.some(pkg => assetName(tag, pkg) === name);
}

export function validateManifest(manifest) {
  if (manifest?.schema !== 1 || !validTag(manifest.tag) || !Array.isArray(manifest.packages) ||
      manifest.packages.length !== PACKAGES.length) throw new Error("Invalid download manifest");
  for (const pkg of PACKAGES) {
    const matches = manifest.packages.filter(asset => asset.id === pkg.id);
    const asset = matches[0];
    if (matches.length !== 1 || asset.name !== assetName(manifest.tag, pkg) ||
        !/^[a-f0-9]{64}$/.test(asset.sha256) || !Number.isSafeInteger(asset.size) || asset.size <= 0 ||
        asset.key !== downloadKey(manifest.tag, asset.name, asset.sha256)) {
      throw new Error(`Invalid package: ${pkg.id}`);
    }
  }
  return manifest;
}

// Browser strings cannot reliably distinguish Intel and Apple Silicon Macs.
export function detectOS(platform, userAgent) {
  if (/Android|iPhone|iPad|iPod/i.test(userAgent)) return "";
  if (/Win/i.test(platform)) return "windows";
  if (/Mac/i.test(platform)) return "macos";
  if (/Linux/i.test(platform)) return "linux";
  return "";
}
