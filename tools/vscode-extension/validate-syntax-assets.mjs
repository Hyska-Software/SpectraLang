import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const extensionRoot = path.dirname(fileURLToPath(import.meta.url));

function readJson(relativePath) {
  const filePath = path.join(extensionRoot, relativePath);
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

const grammar = readJson('syntaxes/spectra.tmLanguage.json');
const languageConfiguration = readJson('language-configuration.json');
const snippets = readJson('snippets/spectra.code-snippets');
const extensionSource = fs.readFileSync(
  path.join(extensionRoot, 'src', 'extension.ts'),
  'utf8',
);

const grammarText = JSON.stringify(grammar);
const requiredGrammarTerms = [
  'func',
  'returns',
  'record',
  'public',
  'from',
  'when',
  'then',
  'otherwise',
  'and',
  'or',
  'not',
];

for (const term of requiredGrammarTerms) {
  assert.ok(grammarText.includes(term), `grammar is missing canonical term: ${term}`);
}


assert.equal(languageConfiguration.comments.lineComment, '//');
assert.deepEqual(languageConfiguration.comments.blockComment, ['/*', '*/']);

const snippetBodies = Object.entries(snippets).flatMap(([name, snippet]) => {
  assert.ok(Array.isArray(snippet.body), `${name} must define a snippet body`);
  return snippet.body;
});
const snippetText = snippetBodies.join('\n');

assert.doesNotMatch(
  snippetText,
  /\b(?:pub|fn|struct|unless|elif|elseif|of)\b|->|=>|;/,
  'snippets still emit superseded syntax',
);
for (const term of ['public func', 'returns', 'record', 'from ', 'when ', 'otherwise']) {
  assert.ok(snippetText.includes(term), `snippets are missing canonical form: ${term}`);
}

for (const forbidden of ['pub fn', 'pub async fn', ' -> ']) {
  assert.equal(
    extensionSource.includes(forbidden),
    false,
    `extension action still emits superseded syntax: ${forbidden}`,
  );
}
for (const term of ['public func', 'public async func', ' returns ']) {
  assert.ok(extensionSource.includes(term), `extension actions are missing canonical form: ${term}`);
}

console.log(
  `validated Spectra VS Code syntax assets: ${Object.keys(snippets).length} snippets`,
);
