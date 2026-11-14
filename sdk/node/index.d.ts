// SPDX-License-Identifier: MIT OR Apache-2.0
export interface ClockChange {
  /** Virtual milliseconds since the epoch, right after the change. */
  virtualMs: number;
  /** Virtual minus real time, in milliseconds. */
  offsetMs: number;
  frozen: boolean;
  generation: number;
}
/** Install the virtual clock (idempotent). Returns true when it is active. */
export function install(): boolean;
/** Called after every clock change. Returns a function that removes the listener. */
export function onAdvance(cb: (change: ClockChange) => void): () => void;
/** Virtual milliseconds since the epoch (real time when not active). */
export function now(): number;
export const active: boolean;
export const offsetMs: number;
export const frozen: boolean;
/** The original, real-time Date constructor. */
export const RealDate: DateConstructor;
