// Refreshes the vendored Public Suffix List (D33): downloads the list and
// its test cases, then records the version and SHA-256 in
// core/pwm-autofill/data/README.md and PSL_VERSION in src/psl.rs. Run with
// `just psl` before a release and review the diff like any other change.
import { createHash } from 'node:crypto';
import fs from 'node:fs';

const DATA = 'core/pwm-autofill/data';
const LIST_URL = 'https://publicsuffix.org/list/public_suffix_list.dat';
const TESTS_URL = 'https://raw.githubusercontent.com/publicsuffix/list/main/tests/test_psl.txt';

async function fetchBytes(url) {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return Buffer.from(await response.arrayBuffer());
}

const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

const list = await fetchBytes(LIST_URL);
const tests = await fetchBytes(TESTS_URL);
const text = list.toString('utf8');
const version = /^\/\/ VERSION: (\S+)$/m.exec(text)?.[1];
const commit = /^\/\/ COMMIT: (\S+)$/m.exec(text)?.[1];
if (!version || !commit || !text.includes('===BEGIN PRIVATE DOMAINS===')) {
  throw new Error('the downloaded list does not look like the Public Suffix List');
}
fs.writeFileSync(`${DATA}/public_suffix_list.dat`, list);
fs.writeFileSync(`${DATA}/test_psl.txt`, tests);

const readme = `${DATA}/README.md`;
let table = fs.readFileSync(readme, 'utf8');
table = table.replace(
  /(\| `public_suffix_list.dat` \|[^|]+\|[^|]+\|) [^|]+ \| `[0-9a-f]{64}` \|/,
  `$1 ${version}, commit ${commit} | \`${sha256(list)}\` |`,
);
table = table.replace(/(\| `test_psl.txt` \|[^|]+\|[^|]+\|[^|]+\|) `[0-9a-f]{64}` \|/, `$1 \`${sha256(tests)}\` |`);
fs.writeFileSync(readme, table);

const source = 'core/pwm-autofill/src/psl.rs';
fs.writeFileSync(
  source,
  fs.readFileSync(source, 'utf8').replace(/pub const PSL_VERSION: &str = "[^"]+";/, `pub const PSL_VERSION: &str = "${version}";`),
);
console.log(`Public Suffix List ${version} (${sha256(list)}). Review the diff, then run just test.`);
