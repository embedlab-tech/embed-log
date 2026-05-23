import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export default async function globalTeardown() {
  if (process.env.E2E_KEEP_LOGS === '1') return;

  const here = path.dirname(fileURLToPath(import.meta.url));
  const tmpDir = path.join(here, '.tmp');
  await fs.rm(tmpDir, { recursive: true, force: true });
}
