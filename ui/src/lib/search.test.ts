import { describe, expect, it } from 'vitest';
import { getFilter, setFilter, splitAt } from './search';

describe('search filters', () => {
  // F07-AC5: the controls write the same text a user would type.
  it('sets, replaces and removes a filter', () => {
    expect(setFilter('report', 'type', 'image')).toBe('report type:image');
    expect(setFilter('report type:image', 'type', 'video')).toBe('report type:video');
    expect(setFilter('type:image report size:>1GB', 'type', null)).toBe('report size:>1GB');
    expect(setFilter('report', 'disk', 'Backup Foto')).toBe('report disk:"Backup Foto"');
    expect(setFilter('report disk:"Backup Foto"', 'disk', 'Data')).toBe('report disk:Data');
    expect(setFilter('', 'is', 'dir')).toBe('is:dir');
  });

  it('reads a filter back', () => {
    expect(getFilter('report disk:"Backup Foto" type:image', 'disk')).toBe('Backup Foto');
    expect(getFilter('report type:image', 'TYPE')).toBe('image');
    expect(getFilter('report', 'type')).toBeNull();
  });

  // F07-AC6: the mistake is shown where it is (positions in characters).
  it('splits the text around an error', () => {
    expect(splitAt('città colour:red', 6, 12)).toEqual(['città ', 'colour', ':red']);
    expect(splitAt('abc', 5, 9)).toEqual(['abc', '', '']);
  });
});
