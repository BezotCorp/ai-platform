import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { test } from 'node:test';
import AppUpdaterModule from 'electron-updater/out/AppUpdater.js';
import { fileURLToPath } from 'node:url';
const moduleDir = path.dirname(fileURLToPath(import.meta.url));
const { AppUpdater } = AppUpdaterModule;
import ProviderModule from 'electron-updater/out/providers/Provider.js';
const { parseUpdateInfo } = ProviderModule;

function workspace(t) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'bcaip-manifest-test-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  return directory;
}

function recordBundle(directory, name, appMinimum, deploymentTarget) {
  const appPath = path.join(directory, `${name}.app`);
  fs.mkdirSync(path.join(appPath, 'Contents'), { recursive: true });
  fs.writeFileSync(
    path.join(appPath, 'Contents', 'Info.plist'),
    `<?xml version="1.0"?><plist version="1.0"><dict><key>LSMinimumSystemVersion</key><string>${appMinimum}</string></dict></plist>`
  );
  fs.writeFileSync(path.join(directory, name), 'archive fixture');
  execFileSync(
    process.execPath,
    [
      path.join(moduleDir, 'mac-update-requirements.js'),
      appPath,
      path.join(directory, `${name}.macos.json`),
    ],
    { env: { ...process.env, MACOSX_DEPLOYMENT_TARGET: deploymentTarget }, stdio: 'pipe' }
  );
}

function generateManifest(directory) {
  execFileSync(
    process.execPath,
    [
      path.join(moduleDir, 'generate-mac-update-manifest.js'),
      '--version',
      'v1.51.0',
      '--directory',
      directory,
    ],
    { stdio: 'pipe' }
  );
}

for (const {
  name,
  appMinimum,
  backendMinimum,
  intelMinimum,
  minimumMacOS,
  blockedDarwin,
  allowedDarwin,
} of [
  {
    name: 'still allows macOS 12 for a compatible release',
    appMinimum: '12.0',
    backendMinimum: '12.0',
    intelMinimum: '12.0',
    minimumMacOS: '12.0.0',
    blockedDarwin: '20.6.0',
    allowedDarwin: '21.0.0',
  },
  {
    name: 'blocks macOS 12 when Electron requires macOS 13',
    appMinimum: '13.0',
    backendMinimum: '12.0',
    intelMinimum: '13.0',
    minimumMacOS: '13.0.0',
    blockedDarwin: '21.6.0',
    allowedDarwin: '22.0.0',
  },
  {
    name: 'blocks macOS 12 when the backend requires macOS 13',
    appMinimum: '12.0',
    backendMinimum: '13.0',
    intelMinimum: '12.0',
    minimumMacOS: '13.0.0',
    blockedDarwin: '21.6.0',
    allowedDarwin: '22.0.0',
  },
  {
    name: 'honors the stricter Intel bundle requirement',
    appMinimum: '12.0',
    backendMinimum: '12.0',
    intelMinimum: '13.0',
    minimumMacOS: '13.0.0',
    blockedDarwin: '21.6.0',
    allowedDarwin: '22.0.0',
  },
  {
    name: 'handles the macOS 26 naming change',
    appMinimum: '26.0',
    backendMinimum: '13.0',
    intelMinimum: '26.0',
    minimumMacOS: '26.0.0',
    blockedDarwin: '24.6.0',
    allowedDarwin: '25.0.0',
  },
]) {
  test(name, { skip: process.platform !== 'darwin' }, async (t) => {
    const directory = workspace(t);
    recordBundle(directory, 'BCAIP.zip', appMinimum, backendMinimum);
    recordBundle(directory, 'BCAIP_intel_mac.zip', intelMinimum, backendMinimum);
    generateManifest(directory);

    const updateInfo = parseUpdateInfo(
      fs.readFileSync(path.join(directory, 'latest-mac.yml'), 'utf8'),
      'latest-mac.yml',
      new URL('https://example.invalid/latest-mac.yml')
    );
    const updater = new AppUpdater(undefined, { version: '1.50.0' });
    updater.logger = null;
    t.mock.method(os, 'release', () => blockedDarwin);
    assert.equal(await updater.isUpdateSupported(updateInfo), false);
    os.release.mock.mockImplementation(() => allowedDarwin);
    assert.equal(await updater.isUpdateSupported(updateInfo), true);

    const fallbackMetadata = JSON.parse(
      fs.readFileSync(path.join(directory, 'mac-update-requirements.json'))
    );
    assert.equal(fallbackMetadata.version, updateInfo.version);
    assert.equal(fallbackMetadata.minimumMacOSVersion, minimumMacOS);
  });
}

test('does not publish a manifest if one architecture has no compatibility metadata', (t) => {
  const directory = workspace(t);
  fs.writeFileSync(
    path.join(directory, 'BCAIP.zip.macos.json'),
    JSON.stringify({ minimumMacOSVersion: '12.0.0' })
  );
  for (const name of ['BCAIP.zip', 'BCAIP_intel_mac.zip']) {
    fs.writeFileSync(path.join(directory, name), 'archive fixture');
  }
  assert.throws(() => generateManifest(directory));
  assert.equal(fs.existsSync(path.join(directory, 'latest-mac.yml')), false);
  assert.equal(fs.existsSync(path.join(directory, 'mac-update-requirements.json')), false);
});

test(
  'stops packaging rather than silently rounding down unsupported OS minima',
  { skip: process.platform !== 'darwin' },
  (t) => {
    for (const minimum of ['13.1', '13.0.1', '16.0']) {
      const directory = workspace(t);
      assert.throws(() => recordBundle(directory, 'BCAIP.zip', minimum, '12.0'));
      assert.equal(fs.existsSync(path.join(directory, 'BCAIP.zip.macos.json')), false);
    }
  }
);
