import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const extensionRoot = path.dirname(fileURLToPath(import.meta.url));

const configSource = fs.readFileSync(path.join(extensionRoot, 'src', 'config.ts'), 'utf8');
const packageJson = JSON.parse(fs.readFileSync(path.join(extensionRoot, 'package.json'), 'utf8'));

// The CLI crate ships its binary as `spectralang(.exe)`; the legacy
// `spectra-cli` name is not resolved anymore.
const modernIndex = configSource.indexOf("getExecutableName('spectralang')");
assert.notEqual(modernIndex, -1, "config.ts must search for 'spectralang'");
assert.equal(
  configSource.indexOf("getExecutableName('spectra-cli')"),
  -1,
  "config.ts must not resolve the legacy 'spectra-cli' name",
);

const cliFnStart = configSource.indexOf('export function getCliPath()');
assert.notEqual(cliFnStart, -1, 'getCliPath must remain exported');
const cliFnEnd = configSource.indexOf('\n}\n', cliFnStart);
const cliFnBody = configSource.slice(cliFnStart, cliFnEnd);
assert.match(
  cliFnBody,
  /return executables\[0\]/,
  'PATH fallback must return the modern spectralang executable',
);
assert.doesNotMatch(
  cliFnBody,
  /spectra-cli/,
  'fallback must not reference the legacy binary name',
);

const description =
  packageJson.contributes?.configuration?.properties?.['spectra.cliPath']?.description;

assert.ok(description, 'package.json must document spectra.cliPath');
assert.match(description, /spectralang/, 'description must mention the spectralang binary');
assert.doesNotMatch(description, /spectra-cli/, 'description must not mention the legacy fallback');

console.log('validated Spectra CLI resolution: spectralang(.exe) only');
