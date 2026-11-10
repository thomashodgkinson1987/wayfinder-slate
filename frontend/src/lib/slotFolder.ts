/**
 * Resolve the files of one BotW save-slot folder picked with a
 * `webkitdirectory` file input.
 *
 * A slot folder holds exactly three files: `game_data.sav` (the save the
 * server parses), `caption.sav`, and `caption.jpg` (the slot's label and
 * screenshot). Every picked file carries a `webkitRelativePath` such as
 * `save_0/3/game_data.sav`, so the slot directory is the path segment
 * directly before the filename.
 */
export interface SlotFolderSelection {
  gameData: File;
  captionSav: File;
  captionImage: File;
  /** Name of the directory directly containing the files, e.g. `3`. */
  slotDirName: string;
  /** The directory name as a slot index when it is a whole number 0–255. */
  slotNumber: number | null;
}

export type SlotFolderResult =
  | { ok: true; selection: SlotFolderSelection }
  | { ok: false; error: string };

const GAME_DATA = 'game_data.sav';
const CAPTION_SAV = 'caption.sav';
const CAPTION_IMAGE = 'caption.jpg';

/**
 * Pick the three required files out of a folder selection.
 *
 * All three must live in the same directory, and that directory must be the
 * one directly containing `game_data.sav` — a parent folder with several slot
 * directories is rejected rather than guessed at.
 */
export function findSlotFolder(files: File[]): SlotFolderResult {
  const saves = files.filter((file) => file.name.toLowerCase() === GAME_DATA);
  if (saves.length === 0) {
    return {
      ok: false,
      error: `No ${GAME_DATA} found in the selected folder. Choose a slot folder that contains ${GAME_DATA}, ${CAPTION_SAV}, and ${CAPTION_IMAGE}.`,
    };
  }

  const directories = new Set(saves.map(directoryName));
  if (directories.size > 1) {
    return {
      ok: false,
      error: 'The selected folder contains more than one save slot. Choose a single slot folder that directly contains game_data.sav, caption.sav, and caption.jpg.',
    };
  }

  const slotDirName = directoryName(saves[0]);
  const inSlotDirectory = (file: File) => directoryName(file) === slotDirName;

  const captionSav = files.find(
    (file) => inSlotDirectory(file) && file.name.toLowerCase() === CAPTION_SAV,
  );
  if (captionSav === undefined) {
    return { ok: false, error: `The selected slot folder is missing ${CAPTION_SAV}.` };
  }

  const captionImage = files.find(
    (file) => inSlotDirectory(file) && file.name.toLowerCase() === CAPTION_IMAGE,
  );
  if (captionImage === undefined) {
    return { ok: false, error: `The selected slot folder is missing ${CAPTION_IMAGE}.` };
  }

  const dirNumber = /^\d+$/.test(slotDirName) ? Number(slotDirName) : null;
  const slotNumber = dirNumber !== null && dirNumber <= 255 ? dirNumber : null;

  return {
    ok: true,
    selection: {
      gameData: saves[0],
      captionSav,
      captionImage,
      slotDirName,
      slotNumber,
    },
  };
}

/** The name of the directory directly holding `file`, from its relative path. */
function directoryName(file: File): string {
  const segments = file.webkitRelativePath.split('/');
  return segments[segments.length - 2] ?? '';
}
