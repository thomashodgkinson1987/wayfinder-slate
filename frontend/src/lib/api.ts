export type DeploymentMode = 'local' | 'lan' | 'hub';

export interface AppConfig {
  mode: DeploymentMode;
  version: string;
  live: boolean;
  save_dir: string | null;
  control_addr: string | null;
  map_assets: boolean;
  /** Save/profile data needs a signed-in user (LAN and hub deployments). */
  auth_required: boolean;
  /** Whether self-service registration is currently available. */
  registration_open: boolean;
}

export type UserRole = 'admin' | 'user';

export interface User {
  id: string;
  username: string;
  display_name: string;
  role: UserRole;
  created_at: number;
}

export type GameMode = 'normal' | 'master';

export interface Profile {
  id: string;
  user_id: string;
  name: string;
  game_mode: GameMode;
  is_public: boolean;
  share_global: boolean;
  created_at: number;
}

export interface ProfileInput {
  name: string;
  game_mode?: GameMode;
}

export interface CompletionCount {
  total: number;
  completed: number;
  percentage: number;
}

export interface CompletionSummary {
  shrines: CompletionCount;
  towers: CompletionCount;
  koroks: CompletionCount;
  memories: CompletionCount;
  divine_beasts: CompletionCount;
  locations: CompletionCount;
  enemies: CompletionCount;
  treasures: CompletionCount;
}

export interface HeartStats {
  current: number;
  max: number;
}

export interface StaminaStats {
  current: number;
  max: number;
}

export interface SaveLocation {
  map: string;
  map_type: string;
  x: number;
  y: number;
  z: number;
}

export interface SaveStats {
  play_time_seconds: number;
  rupees: number;
  hearts: HeartStats;
  stamina: StaminaStats;
  location: SaveLocation;
}

export interface SaveCaption {
  saved_at: number;
  location: string;
  map_area: string;
}

export interface SlotSummary extends SaveCaption {
  slot: number;
  label: string;
  caption_url: string;
}

export interface SlotDetail extends SlotSummary {
  stats: SaveStats;
  completion: CompletionSummary;
}

export type MapObjectType =
  | 'shrine'
  | 'tower'
  | 'korok'
  | 'memory'
  | 'location'
  | 'enemy'
  | 'treasure'
  | 'divineBeast';

export interface MapObject {
  /** Save flag name. Treasure chests are hash-addressed and carry an `id` instead. */
  flag?: string;
  x: number;
  z: number;
  name?: string;
  dlc?: boolean;
  seed?: string;
  loc?: string;
  type?: string;
  match_name?: string;
  icon?: string;
  id?: string;
  group?: string;
  /** CRC32 save-flag hash, present on treasure chests. */
  hash?: number;
}

export interface MapObjectsDataset {
  version: number;
  source: Record<string, string>;
  shrines: MapObject[];
  towers: MapObject[];
  koroks: MapObject[];
  memories: MapObject[];
  locations: MapObject[];
  enemies: MapObject[];
  treasures: MapObject[];
  divineBeasts: MapObject[];
}

/** Dataset identifiers of the objects completed in a save slot. */
export interface CompletedObjects {
  shrines: string[];
  towers: string[];
  memories: string[];
  koroks: string[];
  locations: string[];
  enemies: string[];
  divine_beasts: string[];
  /** CRC32 save-flag hashes of opened treasure chests. */
  treasures: number[];
}

export interface MapCompletion {
  slot: number;
  completed: CompletedObjects;
}

export async function getConfig(): Promise<AppConfig> {
  const response = await fetch('/api/config', {
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(`Server returned ${response.status}`);
  }
  return (await response.json()) as AppConfig;
}

export async function getMapObjects(): Promise<MapObjectsDataset> {
  const response = await fetch('/api/map-objects', {
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(`Server returned ${response.status}`);
  }
  return (await response.json()) as MapObjectsDataset;
}

export async function getSlots(): Promise<SlotSummary[]> {
  const response = await fetch('/api/slots', {
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(`Save slots could not be loaded (server returned ${response.status}).`);
  }
  return (await response.json()) as SlotSummary[];
}

export async function getSlot(slot: number): Promise<SlotDetail> {
  const response = await fetch(`/api/slots/${slot}`, {
    headers: { accept: 'application/json' },
  });
  if (response.status === 404) {
    throw new Error('Save slot not found.');
  }
  if (response.status === 422) {
    throw new Error('The selected save file could not be parsed.');
  }
  if (!response.ok) {
    throw new Error(`Save slot could not be loaded (server returned ${response.status}).`);
  }
  return (await response.json()) as SlotDetail;
}

/**
 * Load per-object completion for a slot: the dataset identifiers of every
 * completed shrine, tower, memory, Korok, location, enemy, Divine Beast, and
 * treasure (by save-flag hash).
 */
export async function getMapCompletion(slot: number): Promise<MapCompletion> {
  const response = await fetch(`/api/slots/${slot}/map`, {
    headers: { accept: 'application/json' },
  });
  if (response.status === 404) {
    throw new Error('Save slot not found.');
  }
  if (response.status === 422) {
    throw new Error('The selected save file could not be parsed.');
  }
  if (!response.ok) {
    throw new Error(await apiError(response, 'Map completion could not be loaded'));
  }
  return (await response.json()) as MapCompletion;
}

/** Pull the `{ "error": "..." }` message out of a failed API response. */
async function apiError(response: Response, fallback: string): Promise<string> {
  try {
    const body = (await response.json()) as { error?: unknown };
    if (typeof body.error === 'string' && body.error.length > 0) {
      return body.error;
    }
  } catch {
    // Non-JSON error body; fall back to a generic message.
  }
  return `${fallback} (server returned ${response.status}).`;
}

/**
 * Load the signed-in user, or `null` when the request is unauthenticated.
 * Other failures (server unreachable) propagate so the caller can decide.
 */
export async function getMe(): Promise<User | null> {
  const response = await fetch('/api/me', {
    headers: { accept: 'application/json' },
  });
  if (response.status === 401) return null;
  if (!response.ok) {
    throw new Error(`Could not load the current user (server returned ${response.status}).`);
  }
  const body = (await response.json()) as { user: User };
  return body.user;
}

export async function login(username: string, password: string): Promise<User> {
  const response = await fetch('/api/auth/login', {
    method: 'POST',
    headers: { 'content-type': 'application/json', accept: 'application/json' },
    body: JSON.stringify({ username, password }),
  });
  if (!response.ok) {
    throw new Error(await apiError(response, 'Sign in failed'));
  }
  const body = (await response.json()) as { user: User };
  return body.user;
}

export async function register(
  username: string,
  password: string,
  displayName?: string,
): Promise<User> {
  const payload: Record<string, string> = { username, password };
  const trimmedName = displayName?.trim();
  if (trimmedName) payload.display_name = trimmedName;

  const response = await fetch('/api/auth/register', {
    method: 'POST',
    headers: { 'content-type': 'application/json', accept: 'application/json' },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    throw new Error(await apiError(response, 'Registration failed'));
  }
  const body = (await response.json()) as { user: User };
  return body.user;
}

export async function logout(): Promise<void> {
  const response = await fetch('/api/auth/logout', {
    method: 'POST',
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(await apiError(response, 'Sign out failed'));
  }
}

export async function getProfiles(): Promise<Profile[]> {
  const response = await fetch('/api/profiles', {
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(await apiError(response, 'Could not load profiles'));
  }
  return (await response.json()) as Profile[];
}

export async function createProfile(input: ProfileInput): Promise<Profile> {
  const response = await fetch('/api/profiles', {
    method: 'POST',
    headers: { 'content-type': 'application/json', accept: 'application/json' },
    body: JSON.stringify(input),
  });
  if (!response.ok) {
    throw new Error(await apiError(response, 'Could not create the profile'));
  }
  return (await response.json()) as Profile;
}

/** List the stored save slots owned by a profile. */
export async function getProfileSlots(profileId: string): Promise<SlotSummary[]> {
  const response = await fetch(`/api/profiles/${encodeURIComponent(profileId)}/slots`, {
    headers: { accept: 'application/json' },
  });
  if (!response.ok) {
    throw new Error(await apiError(response, 'Could not load the profile saves'));
  }
  return (await response.json()) as SlotSummary[];
}

/** Files accepted by [`uploadSlot`]; `game_data` is required. */
export interface SlotUploadFiles {
  game_data: File;
  caption_sav?: File | null;
  caption_image?: File | null;
}

/**
 * Upload a save into a profile. Pass `slotIndex` to replace a specific slot;
 * omit it to allocate the next free slot. Returns the stored summary.
 */
export async function uploadSlot(
  profileId: string,
  files: SlotUploadFiles,
  slotIndex?: number | null,
): Promise<SlotSummary> {
  const form = new FormData();
  form.append('game_data', files.game_data);
  if (files.caption_sav) form.append('caption_sav', files.caption_sav);
  if (files.caption_image) form.append('caption_image', files.caption_image);
  if (slotIndex !== undefined && slotIndex !== null) {
    form.append('slot_index', String(slotIndex));
  }

  const response = await fetch(`/api/profiles/${encodeURIComponent(profileId)}/saves/upload`, {
    method: 'POST',
    headers: { accept: 'application/json' },
    body: form,
  });
  if (!response.ok) {
    throw new Error(await apiError(response, 'Upload failed'));
  }
  return (await response.json()) as SlotSummary;
}

export interface ImportError {
  slot: number;
  error: string;
}

export interface ImportResult {
  imported: SlotSummary[];
  errors: ImportError[];
}

/**
 * Import a save folder into the machine-local source. Local deployments only;
 * the server rejects this route with `403` on LAN/hub.
 */
export async function importPath(path: string): Promise<ImportResult> {
  const response = await fetch('/api/saves/import-path', {
    method: 'POST',
    headers: { 'content-type': 'application/json', accept: 'application/json' },
    body: JSON.stringify({ path }),
  });
  if (!response.ok) {
    throw new Error(await apiError(response, 'Import failed'));
  }
  return (await response.json()) as ImportResult;
}
