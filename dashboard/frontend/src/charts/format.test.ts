import { describe, expect, it } from 'vitest';
import { formatBytes } from './format';

/**
 * The unit label has to match the divisor. `formatBytes` scales by 1024, so
 * decimal-SI labels would overstate every figure — 2.4 % at the KiB step, 4.9 %
 * at the MiB one — on the page-set whose own header exists to teach that a
 * budget in MB and a reading in MiB are different numbers.
 */
describe('formatBytes', () => {
  it('labels the 1024-based steps as binary units', () => {
    expect(formatBytes(63_590)).toBe('62.1 KiB');
    expect(formatBytes(1_468_006)).toBe('1.4 MiB');
  });

  it('never prints a decimal-SI unit', () => {
    for (const bytes of [0, 512, 1024, 63_590, 1_468_006, 1_073_741_824]) {
      expect(formatBytes(bytes)).not.toMatch(/\b[KM]B\b/);
    }
  });

  it('keeps raw bytes below the first step, so a block reads zero', () => {
    expect(formatBytes(0)).toBe('0 B');
    expect(formatBytes(1023)).toBe('1023 B');
    expect(formatBytes(1024)).toBe('1 KiB');
  });
});
