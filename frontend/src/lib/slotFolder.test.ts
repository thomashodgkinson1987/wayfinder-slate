import { describe, expect, it } from 'vitest';
import { findSlotFolder } from './slotFolder';

/** A picked folder file: `new File` cannot set `webkitRelativePath` directly. */
function picked(path: string): File {
  const name = path.split('/').pop() ?? path;
  const file = new File([new Uint8Array([1, 2, 3])], name);
  Object.defineProperty(file, 'webkitRelativePath', { value: path });
  return file;
}

/** Build a slot folder from a `game_data.sav` path, with captions beside it. */
function folderFor(gameDataPath: string): File[] {
  return [
    picked(gameDataPath),
    picked(gameDataPath.replace('game_data.sav', 'caption.sav')),
    picked(gameDataPath.replace('game_data.sav', 'caption.jpg')),
  ];
}

/** The resolved slot number for a folder, or `null` when the folder is invalid. */
function slotNumberOf(files: File[]): number | null {
  const result = findSlotFolder(files);
  return result.ok ? result.selection.slotNumber : null;
}

describe('findSlotFolder', () => {
  it('resolves a complete slot folder', () => {
    const result = findSlotFolder(folderFor('save_0/3/game_data.sav'));
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.selection.gameData.name).toBe('game_data.sav');
    expect(result.selection.captionSav.name).toBe('caption.sav');
    expect(result.selection.captionImage.name).toBe('caption.jpg');
    expect(result.selection.slotDirName).toBe('3');
    expect(result.selection.slotNumber).toBe(3);
  });

  it('rejects a folder without game_data.sav', () => {
    expect(findSlotFolder([picked('save_0/3/caption.sav')])).toEqual({
      ok: false,
      error: expect.stringContaining('No game_data.sav'),
    });
  });

  it('requires caption.sav and caption.jpg in the slot directory', () => {
    const withoutSav = folderFor('save_0/3/game_data.sav').filter(
      (file) => file.name !== 'caption.sav',
    );
    expect(findSlotFolder(withoutSav)).toEqual({
      ok: false,
      error: expect.stringContaining('missing caption.sav'),
    });

    const withoutImage = folderFor('save_0/3/game_data.sav').filter(
      (file) => file.name !== 'caption.jpg',
    );
    expect(findSlotFolder(withoutImage)).toEqual({
      ok: false,
      error: expect.stringContaining('missing caption.jpg'),
    });

    // A caption in another directory does not count: the slot directory is
    // where game_data.sav sits.
    const captionElsewhere = [
      picked('save_0/3/game_data.sav'),
      picked('save_0/caption.sav'),
      picked('save_0/3/caption.jpg'),
    ];
    expect(findSlotFolder(captionElsewhere)).toEqual({
      ok: false,
      error: expect.stringContaining('missing caption.sav'),
    });
  });

  it('rejects a parent folder holding several slots', () => {
    const result = findSlotFolder([
      ...folderFor('save_0/3/game_data.sav'),
      ...folderFor('save_0/4/game_data.sav'),
    ]);
    expect(result).toEqual({
      ok: false,
      error: expect.stringContaining('more than one save slot'),
    });
  });

  it('prefills the slot number only from a numeric directory name in range', () => {
    expect(slotNumberOf(folderFor('save_0/3/game_data.sav'))).toBe(3);
    expect(slotNumberOf(folderFor('save_0/0/game_data.sav'))).toBe(0);
    expect(slotNumberOf(folderFor('save_0/255/game_data.sav'))).toBe(255);
    expect(slotNumberOf(folderFor('save_0/256/game_data.sav'))).toBeNull();
    expect(slotNumberOf(folderFor('stable/game_data.sav'))).toBeNull();
  });

  it('matches file names case-insensitively', () => {
    const result = findSlotFolder([
      picked('save_0/3/Game_Data.SAV'),
      picked('save_0/3/Caption.SAV'),
      picked('save_0/3/caption.JPG'),
    ]);
    expect(result.ok).toBe(true);
  });

  it('ignores unrelated files beside the slot files', () => {
    const files = [
      ...folderFor('3/game_data.sav'),
      picked('3/option.sav'),
      picked('3/notes.txt'),
    ];
    const result = findSlotFolder(files);
    expect(result.ok).toBe(true);
    if (!result.ok) return;
    expect(result.selection.slotDirName).toBe('3');
    expect(result.selection.slotNumber).toBe(3);
  });
});
