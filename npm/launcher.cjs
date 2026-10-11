#!/usr/bin/env node

const { spawnSync } = require('node:child_process');
const { name, version, optionalDependencies } = require('./package.json');

const target = `${process.platform}-${process.arch}`;
const prefix = `${name}-${target}`;
const platformPackage = Object.keys(optionalDependencies).find(
  (dependency) => dependency === prefix || dependency.startsWith(`${prefix}-`),
);
const unsupported = (detail) =>
  `Unsupported platform: ${detail}. See https://github.com/hakula139/kiln#from-source`;

try {
  if (!platformPackage) {
    throw new Error(unsupported(target));
  }
  const binary = require.resolve(
    `${platformPackage}/bin/kiln${process.platform === 'win32' ? '.exe' : ''}`,
  );
  const result = spawnSync(binary, process.argv.slice(2), { stdio: 'inherit' });
  if (result.error) {
    throw result.error;
  }
  if (result.signal) {
    process.kill(process.pid, result.signal);
  } else {
    process.exitCode = result.status;
  }
} catch (error) {
  let message = error.message;
  if (error.code === 'MODULE_NOT_FOUND') {
    message = isMusl()
      ? unsupported(`${target} with musl libc`)
      : `Missing binary for ${target}. Reinstall ${name}@${version} with optional dependencies enabled (--include=optional).`;
  }
  console.error(`kiln: ${message}`);
  process.exitCode = 1;
}

// npm skips the glibc-linked Linux package on musl because of its `libc` field.
function isMusl() {
  if (process.platform !== 'linux') {
    return false;
  }
  process.report.excludeNetwork = true;
  return !process.report.getReport().header.glibcVersionRuntime;
}
