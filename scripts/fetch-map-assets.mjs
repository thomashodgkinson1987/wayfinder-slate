#!/usr/bin/env node
/**
 * Fetch the Breath of the Wild map tiles and marker data used by Wayfinder
 * Slate. Assets come from zeldadungeon/maps at a pinned commit; only the BotW
 * subtree is downloaded. They are third-party material, are not committed to
 * this repository, and are verified against scripts/map-assets.sha256.
 *
 * This script deliberately has no npm dependencies and runs on Windows,
 * Linux, and macOS with Node.js 20+.
 */
import { createHash } from 'node:crypto';
import {
  access,
  mkdir,
  mkdtemp,
  readdir,
  readFile,
  rename,
  rm,
  writeFile,
} from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(SCRIPT_DIR, '..');
const DEST = path.join(ROOT, 'frontend', 'static', 'map', 'botw');
const MANIFEST = path.join(ROOT, 'scripts', 'map-assets.sha256');
const UPSTREAM = 'zeldadungeon/maps';
const PINNED_COMMIT = 'd32a85656031d861cef38e32eb927a7d08a983a9';
const COMMIT = (process.env.WAYFINDER_MAPS_COMMIT ?? PINNED_COMMIT).trim();
const RAW_BASE = `https://raw.githubusercontent.com/${UPSTREAM}/${COMMIT}/public/botw`;
const TREE_API = `https://api.github.com/repos/${UPSTREAM}/git/trees/${COMMIT}?recursive=1`;
const USER_AGENT = 'wayfinder-slate-map-fetcher/0.1';
const BOTW_PREFIX = 'public/botw/';
const JOBS = clamp(Number.parseInt(process.env.WAYFINDER_MAP_JOBS ?? '12', 10) || 12, 1, 32);

const HELP = `Usage: node scripts/fetch-map-assets.mjs [options]

Downloads the Breath of the Wild map tiles, icons, and marker JSON from
zeldadungeon/maps into frontend/static/map/botw.

Options:
  --force            Re-download even if the pinned commit is already present.
  --verify-only      Verify existing files against scripts/map-assets.sha256.
  --write-manifest   Rebuild scripts/map-assets.sha256 from the pinned upstream.
  -h, --help         Show this help.

Environment:
  WAYFINDER_MAPS_COMMIT  Override the pinned upstream commit (testing only).
  WAYFINDER_MAP_JOBS     Concurrent downloads, 1-32 (default 12).
  GITHUB_TOKEN           Optional token for --write-manifest tree requests.

Wrappers:
  Linux/macOS: ./scripts/fetch-map-assets.sh
  Windows:     scripts\\fetch-map-assets.cmd
`;

function clamp(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

function sleep(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

function parseArgs(argv) {
  const options = {
    force: false,
    verifyOnly: false,
    writeManifest: false,
    help: false,
  };

  for (const arg of argv) {
    switch (arg) {
      case '--force':
        options.force = true;
        break;
      case '--verify-only':
        options.verifyOnly = true;
        break;
      case '--write-manifest':
        options.writeManifest = true;
        break;
      case '-h':
      case '--help':
        options.help = true;
        break;
      default:
        throw new Error(`unknown option ${arg}\n\n${HELP}`);
    }
  }

  if (options.verifyOnly && (options.force || options.writeManifest)) {
    throw new Error('--verify-only cannot be combined with --force or --write-manifest');
  }
  if (!/^[0-9a-f]{40}$/i.test(COMMIT)) {
    throw new Error(`WAYFINDER_MAPS_COMMIT must be a 40-character commit SHA, got ${COMMIT}`);
  }

  return options;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  if (options.help) {
    console.log(HELP);
    return;
  }

  if (options.verifyOnly) {
    await verifyInstalled();
    return;
  }

  if (options.writeManifest) {
    await rebuildManifest();
    return;
  }

  const manifest = await loadManifest();
  const installed = await installedCommit();

  if (!options.force && installed === COMMIT) {
    console.log(`Map assets for ${COMMIT.slice(0, 12)} are already installed.`);
    await verifyInstalled(manifest);
    return;
  }

  let tempDir = await makeTempDir();
  try {
    console.log(
      `Fetching ${manifest.length} BotW map files from ${UPSTREAM} at ${COMMIT.slice(0, 12)}...`,
    );
    await downloadAll(manifest, tempDir, { verifyHashes: true });
    await writeFile(path.join(tempDir, '.source-commit'), `${COMMIT}\n`);
    await install(tempDir);
    tempDir = null;
  } finally {
    if (tempDir !== null) await rm(tempDir, { recursive: true, force: true });
  }

  console.log(`Installed ${manifest.length} BotW map files into frontend/static/map/botw.`);
  console.log('Rebuild the frontend so rust-embed picks them up:');
  console.log('  npm --prefix frontend run build');
}

async function rebuildManifest() {
  const files = await fetchBotwTree();
  console.log(`Rebuilding manifest from ${files.length} upstream files at ${COMMIT.slice(0, 12)}...`);

  let tempDir = await makeTempDir();
  let manifestContent;
  try {
    await downloadAll(files, tempDir, { verifyHashes: false });
    await writeFile(path.join(tempDir, '.source-commit'), `${COMMIT}\n`);
    manifestContent = await buildManifest(tempDir);
    await install(tempDir);
    tempDir = null;
  } finally {
    if (tempDir !== null) await rm(tempDir, { recursive: true, force: true });
  }

  const temporaryManifest = `${MANIFEST}.tmp-${process.pid}`;
  try {
    await writeFile(temporaryManifest, manifestContent);
    await rename(temporaryManifest, MANIFEST);
  } finally {
    await rm(temporaryManifest, { force: true });
  }

  console.log(`Installed ${files.length} BotW map files into frontend/static/map/botw.`);
  console.log(`Wrote ${path.relative(ROOT, MANIFEST)}. Review and commit it.`);
}

function sha256(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

async function exists(target) {
  try {
    await access(target);
    return true;
  } catch (error) {
    if (error.code === 'ENOENT') return false;
    throw error;
  }
}

async function makeTempDir() {
  await mkdir(path.dirname(DEST), { recursive: true });
  return mkdtemp(path.join(path.dirname(DEST), '.botw-'));
}

async function installedCommit() {
  try {
    return (await readFile(path.join(DEST, '.source-commit'), 'utf8')).trim();
  } catch (error) {
    if (error.code === 'ENOENT') return null;
    throw error;
  }
}

async function loadManifest() {
  let text;
  try {
    text = await readFile(MANIFEST, 'utf8');
  } catch (error) {
    if (error.code === 'ENOENT') {
      throw new Error(
        `checksum manifest not found at ${path.relative(ROOT, MANIFEST)}; run --write-manifest`,
      );
    }
    throw error;
  }

  const entries = [];
  const seen = new Set();
  for (const line of text.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (trimmed === '') continue;

    const match = /^([0-9a-f]{64})\s+(.+)$/i.exec(trimmed);
    if (!match) throw new Error(`invalid manifest line: ${line}`);

    const relative = safeRelativePath(match[2].replace(/^\.\//, ''));
    if (seen.has(relative)) throw new Error(`duplicate manifest path: ${relative}`);
    seen.add(relative);
    entries.push({ path: relative, hash: match[1].toLowerCase() });
  }

  if (entries.length === 0) throw new Error('checksum manifest is empty');
  return entries;
}

function safeRelativePath(relative) {
  const normalized = relative.replaceAll('\\', '/');
  if (normalized === '' || normalized.startsWith('/') || /^[a-zA-Z]:/.test(normalized)) {
    throw new Error(`unsafe asset path: ${relative}`);
  }
  if (normalized.split('/').some((part) => part === '' || part === '..' || part === '.')) {
    throw new Error(`unsafe asset path: ${relative}`);
  }
  return normalized;
}

function rawFileUrl(relative) {
  const encoded = relative.split('/').map(encodeURIComponent).join('/');
  return `${RAW_BASE}/${encoded}`;
}

async function request(url, { json = false, signal } = {}) {
  let lastError;

  for (let attempt = 1; attempt <= 4; attempt += 1) {
    if (signal?.aborted) throw new Error('download aborted');
    try {
      const headers = { 'user-agent': USER_AGENT };
      if (json) {
        headers.accept = 'application/vnd.github+json';
        if (process.env.GITHUB_TOKEN) {
          headers.authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
        }
      }

      const response = await fetch(url, { headers, signal });
      if (response.ok) {
        return json ? await response.json() : Buffer.from(await response.arrayBuffer());
      }

      const message = `HTTP ${response.status} ${response.statusText} for ${url}`;
      const error = new Error(message);
      error.retryable = response.status === 429 || response.status >= 500;
      if (json && (response.status === 401 || response.status === 403)) {
        error.message += '\nIf this is GitHub\'s rate limit, set GITHUB_TOKEN and retry.';
      }
      throw error;
    } catch (error) {
      if (error.name === 'AbortError') throw error;
      if (error.retryable === false) throw error;
      lastError = error;
      if (attempt < 4) {
        await sleep(400 * 2 ** (attempt - 1) + Math.floor(Math.random() * 250));
      }
    }
  }

  throw lastError;
}

async function fetchBotwTree() {
  const tree = await request(TREE_API, { json: true });
  if (tree.truncated) {
    throw new Error('GitHub tree response was truncated; cannot rebuild the manifest');
  }

  const files = tree.tree
    .filter((entry) => entry.type === 'blob' && entry.path.startsWith(BOTW_PREFIX))
    .map((entry) => ({
      path: safeRelativePath(entry.path.slice(BOTW_PREFIX.length)),
      size: entry.size ?? 0,
    }));

  if (files.length === 0) {
    throw new Error(`upstream layout changed; no files found under ${BOTW_PREFIX}`);
  }

  files.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return files;
}

async function downloadAll(entries, targetDir, { verifyHashes }) {
  const total = entries.length;
  let next = 0;
  let completed = 0;
  let firstError = null;
  const controller = new AbortController();

  const workers = Array.from({ length: Math.min(JOBS, Math.max(1, total)) }, async () => {
    while (!firstError) {
      const index = next;
      next += 1;
      if (index >= total) return;

      const entry = entries[index];
      try {
        await downloadOne(entry, targetDir, controller.signal, verifyHashes);
        completed += 1;
        reportProgress(completed, total);
      } catch (error) {
        if (!firstError) firstError = error;
        controller.abort();
        return;
      }
    }
  });

  await Promise.all(workers);
  if (firstError) throw firstError;
}

async function downloadOne(entry, targetDir, signal, verifyHashes) {
  const bytes = await request(rawFileUrl(entry.path), { signal });

  if (verifyHashes && entry.hash) {
    const actual = sha256(bytes);
    if (actual !== entry.hash) {
      throw new Error(
        `${entry.path}: checksum mismatch\n  expected ${entry.hash}\n  actual   ${actual}`,
      );
    }
  }

  const target = path.join(targetDir, ...entry.path.split('/'));
  await mkdir(path.dirname(target), { recursive: true });
  await writeFile(target, bytes);
}

function reportProgress(completed, total) {
  if (completed !== total && completed % 100 !== 0) return;

  const message = `Downloaded ${completed}/${total} files`;
  if (process.stdout.isTTY) {
    process.stdout.write(`\r${message.padEnd(42)}`);
    if (completed === total) process.stdout.write('\n');
  } else {
    console.log(message);
  }
}

async function verifyInstalled(manifest) {
  const entries = manifest ?? (await loadManifest());
  const installed = await installedCommit();
  if (installed && installed !== COMMIT) {
    console.warn(
      `Warning: installed assets are from ${installed.slice(0, 12)}, but the manifest pins ${COMMIT.slice(0, 12)}.`,
    );
  }

  const failures = [];
  let verified = 0;
  await mapLimit(entries, JOBS, async (entry) => {
    try {
      const bytes = await readFile(path.join(DEST, ...entry.path.split('/')));
      if (sha256(bytes) !== entry.hash) {
        failures.push(`${entry.path}: checksum mismatch`);
      } else {
        verified += 1;
      }
    } catch (error) {
      failures.push(`${entry.path}: ${error.code === 'ENOENT' ? 'missing' : error.message}`);
    }
  });

  if (failures.length > 0) {
    const detail = failures.slice(0, 25).join('\n');
    const more = failures.length > 25 ? `\n...and ${failures.length - 25} more` : '';
    throw new Error(
      `map asset verification failed:\n${detail}${more}\n\nRe-run without --verify-only to download again.`,
    );
  }

  console.log(
    `Verified ${verified} BotW map files against ${path.relative(ROOT, MANIFEST)}.`,
  );
}

async function buildManifest(directory) {
  const files = await listFiles(directory);
  files.sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));

  const lines = new Array(files.length);
  await mapLimit(files, JOBS, async (relative, index) => {
    const bytes = await readFile(path.join(directory, ...relative.split('/')));
    lines[index] = `${sha256(bytes)}  ./${relative}`;
  });
  return `${lines.join('\n')}\n`;
}

async function listFiles(directory, prefix = '') {
  const result = [];
  const entries = await readdir(directory, { withFileTypes: true });

  for (const entry of entries) {
    const relative = prefix === '' ? entry.name : `${prefix}/${entry.name}`;
    if (entry.isDirectory()) {
      result.push(...(await listFiles(path.join(directory, entry.name), relative)));
    } else if (entry.isFile() && entry.name !== '.source-commit') {
      result.push(relative);
    }
  }

  return result;
}

async function mapLimit(items, limit, action) {
  let next = 0;
  const workers = Array.from({ length: Math.min(limit, Math.max(1, items.length)) }, async () => {
    while (true) {
      const index = next;
      next += 1;
      if (index >= items.length) return;
      await action(items[index], index);
    }
  });
  await Promise.all(workers);
}

async function install(tempDir) {
  const parent = path.dirname(DEST);
  await mkdir(parent, { recursive: true });

  const backup = `${DEST}.backup-${process.pid}`;
  let hasBackup = false;

  if (await exists(DEST)) {
    await rm(backup, { recursive: true, force: true });
    await rename(DEST, backup);
    hasBackup = true;
  }

  try {
    await rename(tempDir, DEST);
  } catch (error) {
    if (hasBackup) {
      await rm(DEST, { recursive: true, force: true });
      await rename(backup, DEST);
    }
    throw error;
  }

  if (hasBackup) await rm(backup, { recursive: true, force: true });
}

main().catch((error) => {
  console.error(`\nerror: ${error.message}`);
  process.exitCode = 1;
});
