// SPDX-License-Identifier: Apache-2.0
// Include dependency license texts with the portable archive, not just an SBOM.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const stage = path.resolve(process.argv[2]);
const output = path.join(stage, 'licenses');
fs.mkdirSync(output, { recursive: true });
const inventory = [];
function collect(kind, name, version, dir, license = '') {
  if (!dir || !fs.existsSync(dir)) return;
  const id = `${kind}-${name}-${version}`.replace(/[^a-zA-Z0-9._-]/g, '_');
  const candidates = fs.readdirSync(dir).filter(n => /^(licen[sc]e|copying|copyright|notice)([._-]|$)/i.test(n));
  const files = [];
  for (const name of candidates) {
    const source = path.join(dir, name);
    if (!fs.statSync(source).isFile()) continue;
    const dest = path.join(output, id); fs.mkdirSync(dest, { recursive: true });
    fs.copyFileSync(source, path.join(dest, name)); files.push(`${id}/${name}`);
  }
  inventory.push({ ecosystem: kind, name, version, license, files });
}
const modules = execFileSync('go', ['list','-m','-f','{{.Path}}|{{.Version}}|{{.Dir}}','all'], { cwd: repo, encoding:'utf8', maxBuffer: 32*1024*1024 });
for (const line of modules.trim().split(/\r?\n/)) {
  const [name, version, dir] = line.split('|'); collect('go', name, version || 'checkout', dir);
}
const metadata = JSON.parse(execFileSync('cargo', ['metadata','--locked','--format-version','1','--filter-platform','x86_64-pc-windows-msvc','--manifest-path',path.join(repo,'desktop/src-tauri/Cargo.toml')], { cwd:repo, encoding:'utf8', maxBuffer:32*1024*1024 }));
for (const p of metadata.packages) collect('rust', p.name, p.version, path.dirname(p.manifest_path), p.license || '');
const lock = JSON.parse(fs.readFileSync(path.join(repo,'ui/package-lock.json'),'utf8'));
for (const [relative, entry] of Object.entries(lock.packages)) {
  if (!relative || entry.dev) continue;
  const dir = path.join(repo,'ui',relative);
  if (!fs.existsSync(path.join(dir,'package.json'))) continue;
  const p = JSON.parse(fs.readFileSync(path.join(dir,'package.json'),'utf8'));
  collect('npm',p.name,p.version,dir,p.license || '');
}
fs.writeFileSync(path.join(output,'inventory.json'),JSON.stringify(inventory,null,2)+'\n');
console.log(`Included license inventory for ${inventory.length} dependencies.`);
