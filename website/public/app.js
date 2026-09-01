const RELEASE_OWNER = "thedinz";
const RELEASE_REPOSITORY = "GridMode";
const RELEASE_API = `https://api.github.com/repos/${RELEASE_OWNER}/${RELEASE_REPOSITORY}/releases/latest`;
const RELEASE_FALLBACK = `https://github.com/${RELEASE_OWNER}/${RELEASE_REPOSITORY}/releases/latest`;

const formatBytes = (bytes) => {
  if (!Number.isFinite(bytes) || bytes <= 0) return "";
  const units = ["B", "KB", "MB", "GB"];
  const unit = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** unit;
  return `${value >= 10 || unit === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
};

const normalizedVersion = (tag) => String(tag || "Latest").replace(/^v/i, "");

const findAsset = (assets, platform) => {
  const candidates = assets.filter((asset) => !/\.(sig|yml|json)$/i.test(asset.name));
  if (platform === "windows") {
    return candidates.find((asset) => /\.exe$/i.test(asset.name) && /setup|installer|x64|nsis/i.test(asset.name))
      || candidates.find((asset) => /\.exe$/i.test(asset.name));
  }
  return candidates.find((asset) => /\.dmg$/i.test(asset.name));
};

const updatePlatform = (platform, asset, releaseUrl) => {
  const button = document.querySelector(`[data-download="${platform}"]`);
  const meta = document.querySelector(`[data-asset-meta="${platform}"]`);
  if (!button || !meta) return;

  if (asset) {
    button.href = asset.browser_download_url;
    meta.textContent = [asset.name, formatBytes(asset.size)].filter(Boolean).join(" · ");
    return;
  }

  button.href = releaseUrl;
  meta.textContent = "Installer available from the latest GitHub release";
};

const applyRelease = (release) => {
  const version = normalizedVersion(release.tag_name);
  const releaseUrl = release.html_url || RELEASE_FALLBACK;
  document.querySelectorAll("[data-latest-version]").forEach((element) => {
    element.textContent = `Version ${version}`;
  });
  document.querySelectorAll("[data-release-link]").forEach((element) => {
    element.href = releaseUrl;
  });
  updatePlatform("windows", findAsset(release.assets || [], "windows"), releaseUrl);
  updatePlatform("macos", findAsset(release.assets || [], "macos"), releaseUrl);
};

const applyReleaseFallback = () => {
  document.querySelectorAll("[data-latest-version]").forEach((element) => {
    element.textContent = "Latest release";
  });
  ["windows", "macos"].forEach((platform) => updatePlatform(platform, null, RELEASE_FALLBACK));
};

document.querySelectorAll("[data-current-year]").forEach((element) => {
  element.textContent = new Date().getFullYear();
});

const preferredPlatform = /Mac|iPhone|iPad/i.test(navigator.platform) ? "macos" : /Win/i.test(navigator.platform) ? "windows" : null;
if (preferredPlatform) {
  document.querySelector(`[data-platform-card="${preferredPlatform}"]`)?.classList.add("recommended");
}

fetch(RELEASE_API, {
  cache: "no-store",
  headers: { Accept: "application/vnd.github+json" }
})
  .then((response) => {
    if (!response.ok) throw new Error(`GitHub returned ${response.status}`);
    return response.json();
  })
  .then(applyRelease)
  .catch(applyReleaseFallback);
