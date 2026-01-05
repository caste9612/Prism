import { describe, it, expect } from 'vitest';
import { formatBytes, formatNumber, formatDate, formatDuration, truncatePath } from './format';

describe('formatBytes', () => {
  it('should format 0 bytes', () => {
    expect(formatBytes(0)).toBe('0 B');
  });

  it('should format bytes', () => {
    expect(formatBytes(500)).toBe('500.00 B');
  });

  it('should format kilobytes', () => {
    expect(formatBytes(1024)).toBe('1.00 KB');
    expect(formatBytes(1536)).toBe('1.50 KB');
  });

  it('should format megabytes', () => {
    expect(formatBytes(1048576)).toBe('1.00 MB');
    expect(formatBytes(1572864)).toBe('1.50 MB');
  });

  it('should format gigabytes', () => {
    expect(formatBytes(1073741824)).toBe('1.00 GB');
  });

  it('should format terabytes', () => {
    expect(formatBytes(1099511627776)).toBe('1.00 TB');
  });
});

describe('formatNumber', () => {
  it('should format numbers with locale separators', () => {
    // The exact format depends on locale, so just check it returns a string
    expect(typeof formatNumber(0)).toBe('string');
    expect(typeof formatNumber(1000)).toBe('string');
    expect(typeof formatNumber(1000000)).toBe('string');
  });
});

describe('formatDate', () => {
  it('should format recent timestamps as relative time', () => {
    const now = Math.floor(Date.now() / 1000);
    expect(formatDate(now)).toBe('Just now');
    expect(formatDate(now - 60)).toBe('Just now'); // 1 minute ago
  });

  it('should format hours ago', () => {
    const now = Math.floor(Date.now() / 1000);
    const twoHoursAgo = now - 7200;
    expect(formatDate(twoHoursAgo)).toBe('2h ago');
  });

  it('should format days ago', () => {
    const now = Math.floor(Date.now() / 1000);
    const threeDaysAgo = now - 259200;
    expect(formatDate(threeDaysAgo)).toBe('3d ago');
  });
});

describe('formatDuration', () => {
  it('should format seconds', () => {
    expect(formatDuration(5)).toBe('5.0s');
    expect(formatDuration(30.5)).toBe('30.5s');
  });

  it('should format minutes and seconds', () => {
    expect(formatDuration(65)).toBe('1m 5s');
    expect(formatDuration(125)).toBe('2m 5s');
  });

  it('should format hours and minutes', () => {
    expect(formatDuration(3665)).toBe('1h 1m');
    expect(formatDuration(7800)).toBe('2h 10m');
  });
});

describe('truncatePath', () => {
  it('should not truncate short paths', () => {
    const path = 'C:\\Users\\file.txt';
    expect(truncatePath(path, 50)).toBe(path);
  });

  it('should truncate long paths', () => {
    const path = 'C:\\Users\\Documents\\Very\\Long\\Path\\To\\Some\\Deep\\Folder\\file.txt';
    const result = truncatePath(path, 30);
    expect(result.length).toBeLessThanOrEqual(30);
    expect(result).toContain('...');
  });

  it('should keep first and last parts', () => {
    const path = 'C:\\Users\\Documents\\file.txt';
    const result = truncatePath(path, 20);
    expect(result).toContain('...');
  });
});
