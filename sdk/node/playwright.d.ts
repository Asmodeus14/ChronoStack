// SPDX-License-Identifier: MIT OR Apache-2.0
/** The part of a Playwright `Page` or `BrowserContext` that `syncClock` uses. */
export interface HasClock {
  clock: {
    setSystemTime(time: number | string | Date): Promise<void>;
    setFixedTime(time: number | string | Date): Promise<void>;
  };
  once?(event: 'close', listener: () => void): unknown;
}
/**
 * Set the browser clock of a Playwright `Page` or `BrowserContext` to the
 * stack's virtual time and keep it in sync after every advance, freeze or
 * unfreeze. Resolves to a function that stops following. Without a running
 * stack it changes nothing (or throws with CHRONOSTACK_STRICT=1).
 */
export function syncClock(target: HasClock): Promise<() => void>;
