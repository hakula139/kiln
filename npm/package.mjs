import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';

const root = resolve(import.meta.dirname, '..');

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

const [, , input, output] = process.argv;
const archives = resolve(input);
const destination = resolve(output);
const cargo = readFileSync(join(root, 'Cargo.toml'), 'utf8');
const version = cargo.match(/\[workspace\.package\][\s\S]*?^version = "([^"]+)"/m)[1];

const metadata = {
  version,
  license: 'MIT',
  repository: { type: 'git', url: 'https://github.com/hakula139/kiln.git' },
  homepage: 'https://github.com/hakula139/kiln',
  bugs: { url: 'https://github.com/hakula139/kiln/issues' },
  publishConfig: { access: 'public' },
};

for (const variant of ['kiln', 'kiln-extended']) {
  const optionalDependencies = {};
  for (const platform of platforms) {
    const name = `@kiln-ssg/${variant}-${platform.name}`;
    const directory = join(destination, `${variant}-${platform.name}`);
    const binaryDirectory = join(directory, 'bin');
    mkdirSync(binaryDirectory, { recursive: true });
    const archive = join(archives, `${variant}-${platform.target}`);
    if (platform.os === 'win32') {
      execFileSync('unzip', ['-q', `${archive}.zip`, '-d', binaryDirectory]);
    } else {
      execFileSync('tar', ['-xzf', `${archive}.tar.gz`, '-C', binaryDirectory]);
    }
    packPackage(directory, {
      ...metadata,
      name,
      description: `${variant} binary for ${platform.os} ${platform.cpu}`,
      os: [platform.os],
      cpu: [platform.cpu],
      ...(platform.libc && { libc: platform.libc }),
      files: ['bin'],
    });
    optionalDependencies[name] = version;
  }
  const directory = join(destination, variant);
  mkdirSync(directory, { recursive: true });
  copyFileSync(join(root, 'npm/launcher.cjs'), join(directory, 'launcher.cjs'));
  copyFileSync(join(root, 'README.md'), join(directory, 'README.md'));
  packPackage(directory, {
    ...metadata,
    name: `@kiln-ssg/${variant}`,
    description: `kiln static site generator${variant === 'kiln-extended' ? ' with AVIF placeholders' : ''}`,
    bin: { kiln: 'launcher.cjs' },
    files: ['launcher.cjs'],
    engines: { node: '>=20' },
    optionalDependencies,
  });
}

function packPackage(directory, manifest) {
  copyFileSync(join(root, 'LICENSE'), join(directory, 'LICENSE'));
  writeFileSync(join(directory, 'package.json'), `${JSON.stringify(manifest, null, 2)}\n`);
  execFileSync('npm', ['pack', directory, '--pack-destination', destination], { stdio: 'inherit' });
}
