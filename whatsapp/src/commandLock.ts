/**
 * Per-key exclusive execution queue.
 *
 * WhatsApp commands for a single user (session state, OTP verification,
 * strategy changes, deposit/withdraw transactions) all read and mutate the
 * same in-memory session record. Two messages arriving close together for
 * the same phone number can otherwise race through that state and leave it
 * inconsistent, or produce out-of-order acknowledgements.
 *
 * `runExclusive` serializes tasks sharing the same key so they run strictly
 * one at a time, in arrival order, while tasks for different keys (i.e.
 * different users) continue to run fully in parallel.
 */

type Task<T> = () => Promise<T>;

const tails = new Map<string, Promise<void>>();

export function runExclusive<T>(key: string, task: Task<T>): Promise<T> {
  const previous = tails.get(key) ?? Promise.resolve();

  // Run the next task once the previous one has *settled*, regardless of
  // whether it succeeded or failed - a failed command must never leave a
  // permanent lock blocking the user's next message.
  const run = previous.then(task, task);

  const settled = run.then(
    () => undefined,
    () => undefined
  );
  tails.set(key, settled);

  // Avoid unbounded growth for users who stop messaging: once this is the
  // last queued task for the key and it has settled, drop the entry.
  settled.then(() => {
    if (tails.get(key) === settled) {
      tails.delete(key);
    }
  });

  return run;
}

/** Test/diagnostic helper - not used in production code paths. */
export function pendingLockCount(): number {
  return tails.size;
}
