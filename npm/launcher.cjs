#!/usr/bin/env node

const { spawnSync } = require('node:child_process');
const { name, optionalDependencies } = require('./package.json');

const prefix = `${name}-${process.platform}-${process.arch}`;
const platformPackage = Object.keys(optionalDependencies).find(
  (dependency) => dependency === prefix || dependency.startsWith(`${prefix}-`),
);

try {
  if (!platformPackage) {
    throw new Error(`Unsupported platform: ${process.platform}-${process.arch}`);
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
  console.error(`kiln: ${error.message}`);
  process.exitCode = 1;
}
