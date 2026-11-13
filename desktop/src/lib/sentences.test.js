import { describe, it, expect } from 'vitest';
import { firstSentences } from './sentences';

describe('firstSentences', () => {
  it('keeps a reply of two sentences or fewer exactly as it was', () => {
    expect(firstSentences('One. Two.')).toBe('One. Two.');
    expect(firstSentences('Just this one.', 2)).toBe('Just this one.');
    expect(firstSentences('Geen punt hier')).toBe('Geen punt hier');
  });

  it('drops everything after the second sentence', () => {
    expect(firstSentences('One. Two. Three. Four.')).toBe('One. Two.');
    expect(firstSentences('Eerste zin. Tweede zin. Derde zin.')).toBe('Eerste zin. Tweede zin.');
  });

  it('counts !, ? and … as sentence endings too', () => {
    expect(firstSentences('Really? Yes! And then some. More.')).toBe('Really? Yes!');
    expect(firstSentences('Wacht… wat? En dan. Nog meer.')).toBe('Wacht… wat?');
  });

  it('honours the requested count', () => {
    expect(firstSentences('One. Two. Three.', 1)).toBe('One.');
    expect(firstSentences('One. Two. Three.', 3)).toBe('One. Two. Three.');
  });

  it('keeps the closing punctuation of the sentence it keeps', () => {
    expect(firstSentences('He said "go." Then he left. Later.')).toBe('He said "go." Then he left.');
    expect(firstSentences('**Done.** Next. Third.')).toBe('**Done.** Next.');
    expect(firstSentences('(Zie dit.) Daarna. Verder.')).toBe('(Zie dit.) Daarna.');
  });

  it('does not end a sentence on decimals or version numbers', () => {
    expect(firstSentences('Pi is 3.14 and that is enough. Next. Third.')).toBe(
      'Pi is 3.14 and that is enough. Next.'
    );
    expect(firstSentences('We run v2.0 now. Then. Later.')).toBe('We run v2.0 now. Then.');
  });

  it('does not end a sentence on initials, initialisms or abbreviations', () => {
    expect(firstSentences('J. Smith agreed. Then left. Later.')).toBe('J. Smith agreed. Then left.');
    expect(firstSentences('The U.S. team won. Then left. Later.')).toBe('The U.S. team won. Then left.');
    expect(firstSentences('Dr. De Vries belde. Daarna. Verder.')).toBe('Dr. De Vries belde. Daarna.');
    expect(firstSentences('Dit werkt, bijv. in het echt. Daarna. Verder.')).toBe(
      'Dit werkt, bijv. in het echt. Daarna.'
    );
    expect(firstSentences('Use this, e.g. in tests. Then stop. Later.')).toBe(
      'Use this, e.g. in tests. Then stop.'
    );
  });

  it('does not end a sentence on a numbered list marker', () => {
    expect(firstSentences('Kies een optie:\n1. Eerst dit\n2. Dan dat\n3. Tot slot')).toBe(
      'Kies een optie:\n1. Eerst dit\n2. Dan dat\n3. Tot slot'
    );
  });

  it('returns empty text and handles a missing value', () => {
    expect(firstSentences('')).toBe('');
    expect(firstSentences(undefined)).toBe('');
    expect(firstSentences(null)).toBe('');
  });

  it('leaves the text whole for an unusable count', () => {
    expect(firstSentences('One. Two. Three.', 0)).toBe('One. Two. Three.');
    expect(firstSentences('One. Two. Three.', -1)).toBe('One. Two. Three.');
    expect(firstSentences('One. Two. Three.', 1.5)).toBe('One. Two. Three.');
  });
});
