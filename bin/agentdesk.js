#!/usr/bin/env node

const fs = require('fs');
const path = require('path');
const os = require('os');
const https = require('https');
const { spawn, execSync } = require('child_process');

const PKG_VERSION = require('../package.json').version;
const REPO = 'vijaygovindBiju/AgentDesk';

function getBinaryName() {
  const platform = process.platform;
  const arch = process.arch;

  if (platform !== 'linux') {
    console.error(`AgentDesk prebuilt binaries are currently available for Linux. Detected platform: ${platform}`);
    console.error(`To run on ${platform}, build from source: cd core && cargo build --workspace`);
    process.exit(1);
  }

  let targetArch;
  if (arch === 'x64') {
    targetArch = 'x86_64';
  } else if (arch === 'arm64') {
    targetArch = 'aarch64';
  } else {
    console.error(`Unsupported architecture: ${arch}. Supported architectures: x64, arm64.`);
    process.exit(1);
  }

  return {
    arch: targetArch,
    assetName: `AgentDesk-v${PKG_VERSION}-linux-${targetArch}.tar.gz`,
  };
}

function getCacheDir() {
  const base = process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache');
  const cacheDir = path.join(base, 'agentdesk', `v${PKG_VERSION}`);
  if (!fs.existsSync(cacheDir)) {
    fs.mkdirSync(cacheDir, { recursive: true });
  }
  return cacheDir;
}

function findLocalBinary() {
  // 1. Check local cache
  const cacheDir = getCacheDir();
  const cachedBin = path.join(cacheDir, 'agentdesk');
  if (fs.existsSync(cachedBin)) {
    return cachedBin;
  }

  // 2. Check ~/.local/lib/agentdesk/agentdesk
  const systemUserBin = path.join(os.homedir(), '.local', 'lib', 'agentdesk', 'agentdesk');
  if (fs.existsSync(systemUserBin)) {
    return systemUserBin;
  }

  // 3. Check cargo target in source tree if run from repo
  const repoBin = path.join(__dirname, '..', 'core', 'target', 'release', 'agentdesk');
  if (fs.existsSync(repoBin)) {
    return repoBin;
  }

  return null;
}

function downloadUrl(url, destPath) {
  return new Promise((resolve, reject) => {
    https.get(url, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return downloadUrl(res.headers.location, destPath).then(resolve, reject);
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`Failed to download: HTTP ${res.statusCode} from ${url}`));
      }

      const fileStream = fs.createWriteStream(destPath);
      res.pipe(fileStream);
      fileStream.on('finish', () => {
        fileStream.close(resolve);
      });
      fileStream.on('error', reject);
    }).on('error', reject);
  });
}

async function ensureBinary() {
  const existing = findLocalBinary();
  if (existing) {
    return existing;
  }

  const { assetName } = getBinaryName();
  const cacheDir = getCacheDir();
  const binaryPath = path.join(cacheDir, 'agentdesk');
  const tarPath = path.join(cacheDir, assetName);
  const downloadEndpoint = `https://github.com/${REPO}/releases/download/v${PKG_VERSION}/${assetName}`;

  console.error(`AgentDesk v${PKG_VERSION} binary not found locally.`);
  console.error(`Downloading ${assetName} from GitHub Releases...`);

  try {
    await downloadUrl(downloadEndpoint, tarPath);
    execSync(`tar -xzf "${tarPath}" -C "${cacheDir}"`);
    fs.unlinkSync(tarPath);
    fs.chmodSync(binaryPath, 0o755);
    console.error(`Successfully installed AgentDesk v${PKG_VERSION} to ${cacheDir}`);
    return binaryPath;
  } catch (err) {
    console.error(`Download failed: ${err.message}`);
    console.error(`You can build manually with: cd core && cargo build --release`);
    process.exit(1);
  }
}

async function main() {
  const binaryPath = await ensureBinary();
  const child = spawn(binaryPath, process.argv.slice(2), {
    stdio: 'inherit',
    env: process.env,
  });

  child.on('exit', (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
    } else {
      process.exit(code ?? 0);
    }
  });

  process.on('SIGINT', () => {
    // Forwarded to child
  });
  process.on('SIGTERM', () => {
    // Forwarded to child
  });
}

main().catch((err) => {
  console.error(err);
  process.exit(1);
});
