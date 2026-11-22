/**
 * CLI: re-wrap every custodial wallet secret under the current KEK (#944).
 *
 *   npm run rotate-keys -- --dry-run   # verify all rows, write nothing
 *   npm run rotate-keys                # rotate
 *
 * See "Key rotation runbook" in docs/WHATSAPP_BOT.md.
 */
import './bootstrapEnv';
import { rotateWalletKeys } from './keyRotation';

async function main(): Promise<void> {
  const dryRun = process.argv.includes('--dry-run');
  const result = await rotateWalletKeys({ dryRun });

  console.warn(
    `${dryRun ? '[dry-run] ' : ''}key version "${result.targetVersion}": ` +
      `${result.rotated} rotated, ${result.skipped} skipped, ${result.failed} failed`
  );
  if (result.failed > 0) {
    console.error('Some rows could not be re-wrapped; make sure every old key is listed in ENCRYPTION_KEY_RETIRED.');
    process.exitCode = 1;
  } else if (result.skipped > 0) {
    console.warn('Some rows changed during the run; run the command again to finish.');
  }
}

main().then(
  () => process.exit(),
  (error: unknown) => {
    console.error('Key rotation aborted:', error instanceof Error ? error.message : error);
    process.exit(1);
  }
);
