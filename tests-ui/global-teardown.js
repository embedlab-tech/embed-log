import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export default async function globalTeardown() {
  if (process.env.E2E_KEEP_LOGS === '1') return;
  const here = path.dirname(fileURLToPath(import.meta.url));
  fs.rmSync(path.join(here, '.tmp'), { recursive: true, force: true });
}
