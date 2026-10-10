import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import {
  chmodSync,
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

import { packageRelease } from './package.mjs';

const platforms = [
  { name: 'darwin-arm64', target: 'aarch64-apple-darwin', os: 'darwin', cpu: 'arm64' },
  {
    name: 'linux-x64-gnu',
    target: 'x86_64-unknown-linux-gnu',
    os: 'linux',
    cpu: 'x64',
    libc: ['glibc'],
  },
  { name: 'win32-x64-msvc', target: 'x86_64-pc-windows-msvc', os: 'win32', cpu: 'x64' },
];

test('packages select native binaries and preserve launcher arguments and exit status', (t) => {
  const directory = mkdtempSync(join(tmpdir(), 'kiln-npm-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const archives = join(directory, 'archives');
  const fixture = join(directory, 'fixture');
  mkdirSync(archives);
  mkdirSync(fixture);
  const binary = join(fixture, 'kiln');

  for (const variant of ['kiln', 'kiln-extended']) {
    writeFileSync(
      binary,
      `#!/bin/sh
if [ "$1" = '--signal' ]; then
  kill -TERM $$
fi
printf '%s\\n' '${variant}' "$@"
cat
printf 'stderr data\\n' >&2
exit 7
`,
    );
    chmodSync(binary, 0o755);
    copyFileSync(binary, join(fixture, 'kiln.exe'));
    for (const platform of platforms) {
      const archive = join(archives, `${variant}-${platform.target}`);
      if (platform.os === 'win32') {
        execFileSync('zip', ['-q', `${archive}.zip`, 'kiln.exe'], { cwd: fixture });
      } else {
        execFileSync('tar', ['-czf', `${archive}.tar.gz`, 'kiln'], { cwd: fixture });
      }
    }
  }

  const version = '0.5.0-alpha.1';
  const packages = packageRelease(archives, join(directory, 'packages'), version);
  const names = packages.map(
    (packageDirectory) => JSON.parse(readFileSync(join(packageDirectory, 'package.json'))).name,
  );
  const expectedNames = ['kiln', 'kiln-extended'].flatMap((variant) => [
    `@kiln-ssg/${variant}`,
    ...platforms.map(({ name }) => `@kiln-ssg/${variant}-${name}`),
  ]);
  assert.deepEqual(names.sort(), expectedNames.sort());
  for (const packageDirectory of packages) {
    const manifest = JSON.parse(readFileSync(join(packageDirectory, 'package.json')));
    assert.equal(manifest.version, version);
    if (manifest.optionalDependencies) {
      assert.deepEqual(
        manifest.optionalDependencies,
        Object.fromEntries(platforms.map(({ name }) => [`${manifest.name}-${name}`, version])),
      );
      assert.deepEqual(manifest.bin, { kiln: 'launcher.cjs' });
      const platform = platforms.find(
        ({ os, cpu }) => os === process.platform && cpu === process.arch,
      );
      const nativeDirectory = `${packageDirectory}-${platform.name}`;
      const dependencyDirectory = join(
        packageDirectory,
        'node_modules',
        '@kiln-ssg',
        `${manifest.name.split('/')[1]}-${platform.name}`,
      );
      mkdirSync(join(dependencyDirectory, 'bin'), { recursive: true });
      copyFileSync(join(nativeDirectory, 'bin', 'kiln'), join(dependencyDirectory, 'bin', 'kiln'));
      chmodSync(join(dependencyDirectory, 'bin', 'kiln'), 0o755);
      const result = spawnSync(
        process.execPath,
        [join(packageDirectory, 'launcher.cjs'), 'build', 'a path', '--root=example'],
        { input: 'stdin data\n', encoding: 'utf8' },
      );
      assert.equal(result.status, 7);
      assert.equal(
        result.stdout,
        `${manifest.name.split('/')[1]}\nbuild\na path\n--root=example\nstdin data\n`,
      );
      assert.equal(result.stderr, 'stderr data\n');
      const interrupted = spawnSync(process.execPath, [
        join(packageDirectory, 'launcher.cjs'),
        '--signal',
      ]);
      assert.equal(interrupted.status, null);
      assert.equal(interrupted.signal, 'SIGTERM');
    } else {
      const platform = platforms.find(({ name }) => manifest.name.endsWith(`-${name}`));
      assert.deepEqual(manifest.os, [platform.os]);
      assert.deepEqual(manifest.cpu, [platform.cpu]);
      assert.deepEqual(manifest.libc, platform.libc);
      const executable = readFileSync(
        join(packageDirectory, 'bin', platform.os === 'win32' ? 'kiln.exe' : 'kiln'),
        'utf8',
      );
      assert.ok(
        executable.includes(manifest.name.includes('kiln-extended') ? "'kiln-extended'" : "'kiln'"),
      );
    }
  }
});
